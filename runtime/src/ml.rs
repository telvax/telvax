//! `std.ml`: what goes around a model on a table: splits, metrics, and preprocessing steps that are fitted on the
//! training rows only and then applied as values (`Scaler`, `LabelEncoder`, `Features`).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use arrow::array::{Array, ArrayRef, AsArray, UInt64Array};
use arrow::compute::{cast, take_record_batch};
use arrow::datatypes::{DataType, Int32Type, Int64Type, TimeUnit};

use crate::RuntimeError;
use crate::value::{GeneratorVal, Native, TableVal, TensorVal, Value};

fn err<T>(msg: impl Into<String>) -> Result<T, RuntimeError> {
    Err(RuntimeError::new(msg))
}

fn want_i64(v: &Value) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(i) => Ok(*i),
        other => err(format!("expected an Int, found {}", other.type_name())),
    }
}

fn want_f64(v: &Value) -> Result<f64, RuntimeError> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(x) => Ok(*x),
        other => err(format!("expected a number, found {}", other.type_name())),
    }
}

fn want_vec(v: &Value) -> Result<Vec<Value>, RuntimeError> {
    match v {
        Value::Vec(items) => Ok(items.snapshot()),
        other => err(format!("expected a List, found {}", other.type_name())),
    }
}

fn want_str(v: &Value) -> Result<String, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => err(format!("expected a Text, found {}", other.type_name())),
    }
}

/// Numbers from a `List` or a tensor of one dimension.
fn want_floats(v: &Value) -> Result<Vec<f64>, RuntimeError> {
    match v {
        Value::Tensor(t) => Ok(t.to_f64_vec()),
        other => want_vec(other)?.iter().map(want_f64).collect(),
    }
}

/// The items of a `List` or of a tensor of one dimension.
fn want_items(v: &Value) -> Result<Vec<Value>, RuntimeError> {
    match v {
        Value::Tensor(t) => (0..t.len()).map(|i| t.element(i, &[])).collect(),
        other => want_vec(other),
    }
}

fn want_texts(v: &Value) -> Result<Vec<String>, RuntimeError> {
    want_vec(v)?.iter().map(want_str).collect()
}

fn want_table(v: &Value) -> Result<Arc<TableVal>, RuntimeError> {
    match v {
        Value::Table(t) => Ok(t.clone()),
        other => err(format!("expected a Table, found {}", other.type_name())),
    }
}

fn floats(xs: Vec<f64>) -> Value {
    Value::vec(xs.into_iter().map(Value::Float).collect())
}

fn texts(xs: &[String]) -> Value {
    Value::vec(xs.iter().map(|s| Value::str(s.as_str())).collect())
}

fn pair(a: Value, b: Value) -> Value {
    Value::Tuple(Arc::new(vec![a, b]))
}

/// A label as text, for the classes of a metric or an encoder: a `Text` as it is, anything else as it prints.
fn label(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        other => other.to_string(),
    }
}

/// The distinct labels in order: as numbers when they all are, otherwise as text (the order of `numpy.unique`).
fn sorted_labels<'a>(all: impl Iterator<Item = &'a String>) -> Vec<String> {
    let set: BTreeSet<&String> = all.collect();
    let mut out: Vec<String> = set.into_iter().cloned().collect();
    if out.iter().all(|s| s.parse::<f64>().is_ok()) {
        out.sort_by(|a, b| {
            a.parse::<f64>()
                .unwrap()
                .total_cmp(&b.parse::<f64>().unwrap())
        });
    }
    out
}

// ---- splits ----------------------------------------------------------------------------------------------------

fn test_size(v: &Value, what: &str) -> Result<f64, RuntimeError> {
    let f = want_f64(v)?;
    if !(f > 0.0 && f < 1.0) {
        return err(format!(
            "{what}: test_size is {f}; it is the share of rows for the test, above 0 and below 1"
        ));
    }
    Ok(f)
}

/// Fisher-Yates over all the items, drawn from the generator.
fn shuffle(g: &GeneratorVal, xs: &mut [u64]) {
    for i in 0..xs.len().saturating_sub(1) {
        let j = i + g.below((xs.len() - i) as u64) as usize;
        xs.swap(i, j);
    }
}

fn take(t: &TableVal, rows: Vec<u64>) -> Result<Value, RuntimeError> {
    let batch = take_record_batch(&t.batch, &UInt64Array::from(rows))
        .map_err(|e| RuntimeError::new(format!("taking rows: {e}")))?;
    Ok(Value::Table(Arc::new(TableVal {
        schema: t.schema.clone(),
        batch,
    })))
}

fn split_pair(
    t: &TableVal,
    train: Vec<u64>,
    test: Vec<u64>,
    what: &str,
) -> Result<Value, RuntimeError> {
    if train.is_empty() || test.is_empty() {
        return err(format!(
            "{what}: {} row(s) give {} for training and {} for the test; both need at least one",
            t.rows(),
            train.len(),
            test.len()
        ));
    }
    Ok(pair(take(t, train)?, take(t, test)?))
}

fn column(
    batch: &arrow::record_batch::RecordBatch,
    name: &str,
    what: &str,
) -> Result<ArrayRef, RuntimeError> {
    match batch.schema().index_of(name) {
        Ok(i) => Ok(batch.column(i).clone()),
        Err(_) => {
            let names: Vec<String> = batch
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();
            err(format!(
                "{what}: no column `{name}`; the columns are {}",
                names.join(", ")
            ))
        }
    }
}

fn labels_of(col: &ArrayRef) -> Result<Vec<String>, RuntimeError> {
    (0..col.len())
        .map(|i| {
            if col.is_null(i) {
                Ok(String::new())
            } else {
                crate::table::cell(col, i).map(|v| label(&v))
            }
        })
        .collect()
}

/// The values of a column as numbers: integers, floats and `Bool` (1 or 0) as they are, a `Date` and a `Timestamp`
/// as seconds since 1970-01-01 UTC. A missing value is an error.
fn numbers(col: &ArrayRef, name: &str) -> Result<Vec<f64>, RuntimeError> {
    if col.null_count() > 0 {
        return err(format!(
            "the column `{name}` has {} missing value(s); remove them first (`dropna`) or fill them",
            col.null_count()
        ));
    }
    let bad = |e| RuntimeError::new(format!("the column `{name}` is not a number: {e}"));
    Ok(match col.data_type() {
        DataType::Date32 => cast(col, &DataType::Int32)
            .map_err(bad)?
            .as_primitive::<Int32Type>()
            .values()
            .iter()
            .map(|d| f64::from(*d) * 86_400.0)
            .collect(),
        DataType::Timestamp(unit, _) => {
            let per_second = match unit {
                TimeUnit::Second => 1.0,
                TimeUnit::Millisecond => 1e3,
                TimeUnit::Microsecond => 1e6,
                TimeUnit::Nanosecond => 1e9,
            };
            cast(col, &DataType::Int64)
                .map_err(bad)?
                .as_primitive::<Int64Type>()
                .values()
                .iter()
                .map(|t| *t as f64 / per_second)
                .collect()
        }
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {
            return err(format!(
                "the column `{name}` is text; give it as a categorical feature, or turn it into a number first"
            ));
        }
        _ => {
            let f = cast(col, &DataType::Float64).map_err(bad)?;
            f.as_primitive::<arrow::datatypes::Float64Type>()
                .values()
                .to_vec()
        }
    })
}

// ---- metrics ---------------------------------------------------------------------------------------------------

fn same_length(a: usize, b: usize, what: &str) -> Result<(), RuntimeError> {
    if a != b {
        return err(format!(
            "{what}: the true values are {a} and the predictions {b}; they must be as many"
        ));
    }
    if a == 0 {
        return err(format!("{what}: the lists are empty"));
    }
    Ok(())
}

fn regression(name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
    let (y, p) = (want_floats(&args[0])?, want_floats(&args[1])?);
    same_length(y.len(), p.len(), name)?;
    let n = y.len() as f64;
    let mse = y.iter().zip(&p).map(|(a, b)| (a - b).powi(2)).sum::<f64>() / n;
    Ok(Value::Float(match name {
        "mae" => y.iter().zip(&p).map(|(a, b)| (a - b).abs()).sum::<f64>() / n,
        "mse" => mse,
        "rmse" => mse.sqrt(),
        _ => {
            // r2, with scikit-learn's answer for a constant target: 1 when it is predicted exactly, 0 otherwise.
            let mean = y.iter().sum::<f64>() / n;
            let ss_tot: f64 = y.iter().map(|a| (a - mean).powi(2)).sum();
            let ss_res = mse * n;
            if ss_tot == 0.0 {
                if ss_res == 0.0 { 1.0 } else { 0.0 }
            } else {
                1.0 - ss_res / ss_tot
            }
        }
    }))
}

/// Per class: the labels, and for each its true positives, the count predicted and the count true (the support).
struct Counts {
    labels: Vec<String>,
    tp: Vec<f64>,
    predicted: Vec<f64>,
    support: Vec<f64>,
    matrix: Vec<Vec<i64>>,
    n: f64,
}

fn counts(args: &[Value], what: &str) -> Result<Counts, RuntimeError> {
    let y: Vec<String> = want_items(&args[0])?.iter().map(label).collect();
    let p: Vec<String> = want_items(&args[1])?.iter().map(label).collect();
    same_length(y.len(), p.len(), what)?;
    let labels = sorted_labels(y.iter().chain(&p));
    let at: HashMap<&String, usize> = labels.iter().enumerate().map(|(i, l)| (l, i)).collect();
    let k = labels.len();
    let mut c = Counts {
        tp: vec![0.0; k],
        predicted: vec![0.0; k],
        support: vec![0.0; k],
        matrix: vec![vec![0; k]; k],
        n: y.len() as f64,
        labels: Vec::new(),
    };
    for (a, b) in y.iter().zip(&p) {
        let (i, j) = (at[a], at[b]);
        c.matrix[i][j] += 1;
        c.support[i] += 1.0;
        c.predicted[j] += 1.0;
        if i == j {
            c.tp[i] += 1.0;
        }
    }
    c.labels = labels;
    Ok(c)
}

fn ratio(a: f64, b: f64) -> f64 {
    if b == 0.0 { 0.0 } else { a / b }
}

impl Counts {
    fn precision(&self, i: usize) -> f64 {
        ratio(self.tp[i], self.predicted[i])
    }
    fn recall(&self, i: usize) -> f64 {
        ratio(self.tp[i], self.support[i])
    }
    fn f1(&self, i: usize) -> f64 {
        let (p, r) = (self.precision(i), self.recall(i));
        ratio(2.0 * p * r, p + r)
    }
    fn accuracy(&self) -> f64 {
        self.tp.iter().sum::<f64>() / self.n
    }
    /// A per-class score averaged as `macro` (each class the same), `weighted` (by support) or `micro` (over all the
    /// rows, which for one label per row is the accuracy).
    fn average(&self, per: impl Fn(&Self, usize) -> f64, how: &str) -> Result<f64, RuntimeError> {
        let k = self.labels.len();
        match how {
            "macro" => Ok((0..k).map(|i| per(self, i)).sum::<f64>() / k as f64),
            "weighted" => Ok((0..k).map(|i| per(self, i) * self.support[i]).sum::<f64>() / self.n),
            "micro" => Ok(self.accuracy()),
            other => err(format!(
                "the average is `{other}`; it is `macro`, `micro` or `weighted`"
            )),
        }
    }

    /// The text of scikit-learn's `classification_report`, with 2 decimals.
    fn report(&self) -> String {
        let width = self
            .labels
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
            .max("weighted avg".len());
        let mut s = format!("{:>width$} ", "");
        for h in ["precision", "recall", "f1-score", "support"] {
            s += &format!(" {h:>9}");
        }
        s += "\n\n";
        let row = |name: &str, p: f64, r: f64, f: f64, n: f64| {
            format!(
                "{name:>width$}  {p:>9.2} {r:>9.2} {f:>9.2} {:>9}\n",
                n as i64
            )
        };
        for i in 0..self.labels.len() {
            s += &row(
                &self.labels[i],
                self.precision(i),
                self.recall(i),
                self.f1(i),
                self.support[i],
            );
        }
        s += "\n";
        s += &format!(
            "{:>width$}  {:>9} {:>9} {:>9.2} {:>9}\n",
            "accuracy",
            "",
            "",
            self.accuracy(),
            self.n as i64
        );
        for (name, how) in [("macro avg", "macro"), ("weighted avg", "weighted")] {
            s += &row(
                name,
                self.average(Self::precision, how).unwrap(),
                self.average(Self::recall, how).unwrap(),
                self.average(Self::f1, how).unwrap(),
                self.n,
            );
        }
        s
    }
}

/// The area under the ROC curve: the chance that a positive row scores above a negative one, ties counting half
/// (the Mann-Whitney statistic, which is what the trapezoids of the curve add up to).
fn roc_auc(args: &[Value]) -> Result<Value, RuntimeError> {
    let y: Vec<bool> = want_items(&args[0])?
        .iter()
        .map(|v| match v {
            Value::Bool(b) => Ok(*b),
            other => err(format!(
                "roc_auc: the true values are Bool, found {}",
                other.type_name()
            )),
        })
        .collect::<Result<_, _>>()?;
    let s = want_floats(&args[1])?;
    same_length(y.len(), s.len(), "roc_auc")?;
    let pos = y.iter().filter(|b| **b).count() as f64;
    let neg = y.len() as f64 - pos;
    if pos == 0.0 || neg == 0.0 {
        return err("roc_auc: the true values need both True and False rows");
    }
    let mut order: Vec<usize> = (0..s.len()).collect();
    order.sort_by(|a, b| s[*a].total_cmp(&s[*b]));
    let mut ranks = vec![0f64; s.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && s[order[j + 1]] == s[order[i]] {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0 + 1.0;
        for k in i..=j {
            ranks[order[k]] = rank;
        }
        i = j + 1;
    }
    let rank_sum: f64 = (0..y.len()).filter(|k| y[*k]).map(|k| ranks[k]).sum();
    Ok(Value::Float(
        (rank_sum - pos * (pos + 1.0) / 2.0) / (pos * neg),
    ))
}

// ---- fitted steps ----------------------------------------------------------------------------------------------

/// `std.ml.Scaler`: the mean and standard deviation of the training values (the population one, as scikit-learn's
/// `StandardScaler`); a deviation of 0 scales by 1.
#[derive(Debug, Clone, PartialEq)]
pub struct ScalerVal {
    pub mean: f64,
    pub std: f64,
}

/// `std.ml.LabelEncoder`: the classes of the training values in order; a class is its position.
#[derive(Debug, Clone, PartialEq)]
pub struct EncoderVal {
    pub classes: Vec<String>,
}

/// `std.ml.Features`: how the rows of a table become a matrix for a model, learned from the training rows: each
/// numeric column standardized with its mean and deviation, then each categorical column one-hot with its classes.
#[derive(Debug, Clone, PartialEq)]
pub struct FeaturesVal {
    /// The struct of the rows it was fitted on.
    pub schema: String,
    pub target: String,
    pub numeric: Vec<(String, ScalerVal)>,
    pub categorical: Vec<(String, Vec<String>)>,
}

fn fit_scaler(xs: &[f64]) -> Result<ScalerVal, RuntimeError> {
    if xs.is_empty() {
        return err("Scaler.fit: there are no values to learn from");
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let std = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    Ok(ScalerVal {
        mean,
        std: if std == 0.0 { 1.0 } else { std },
    })
}

impl ScalerVal {
    fn apply(&self, x: f64) -> f64 {
        (x - self.mean) / self.std
    }
}

impl FeaturesVal {
    pub fn width(&self) -> usize {
        self.numeric.len() + self.categorical.iter().map(|(_, c)| c.len()).sum::<usize>()
    }

    pub fn names(&self) -> Vec<String> {
        let mut out: Vec<String> = self.numeric.iter().map(|(n, _)| n.clone()).collect();
        for (n, classes) in &self.categorical {
            out.extend(classes.iter().map(|c| format!("{n}={c}")));
        }
        out
    }

    fn fit(
        t: &TableVal,
        target: String,
        numeric: Vec<String>,
        categorical: Vec<String>,
    ) -> Result<Self, RuntimeError> {
        let what = "Features.fit";
        column(&t.batch, &target, what)?;
        let mut seen = BTreeSet::new();
        for name in numeric.iter().chain(&categorical) {
            if *name == target {
                return err(format!(
                    "{what}: the target `{target}` is also given as a feature; a model would read the answer it has to predict"
                ));
            }
            if !seen.insert(name.clone()) {
                return err(format!("{what}: the column `{name}` is given twice"));
            }
        }
        if t.rows() == 0 {
            return err(format!("{what}: the training table has no rows"));
        }
        let numeric = numeric
            .into_iter()
            .map(|n| {
                let xs = numbers(&column(&t.batch, &n, what)?, &n)?;
                Ok((n, fit_scaler(&xs)?))
            })
            .collect::<Result<_, RuntimeError>>()?;
        let categorical = categorical
            .into_iter()
            .map(|n| {
                let col = column(&t.batch, &n, what)?;
                let values = labels_of(&col)?;
                Ok((n, sorted_labels(values.iter())))
            })
            .collect::<Result<_, RuntimeError>>()?;
        Ok(FeaturesVal {
            schema: t.schema.clone(),
            target,
            numeric,
            categorical,
        })
    }

    /// The rows of `t` as a `rows x width` matrix. A class not seen in training gives zeros in its column's places.
    fn transform(&self, t: &TableVal) -> Result<TensorVal, RuntimeError> {
        let (rows, width) = (t.rows(), self.width());
        let mut out = vec![0f32; rows * width];
        for (j, (name, s)) in self.numeric.iter().enumerate() {
            let xs = numbers(&column(&t.batch, name, "Features.transform")?, name)?;
            for (r, x) in xs.iter().enumerate() {
                out[r * width + j] = s.apply(*x) as f32;
            }
        }
        let mut base = self.numeric.len();
        for (name, classes) in &self.categorical {
            let values = labels_of(&column(&t.batch, name, "Features.transform")?)?;
            let at: HashMap<&String, usize> =
                classes.iter().enumerate().map(|(i, c)| (c, i)).collect();
            for (r, v) in values.iter().enumerate() {
                if let Some(i) = at.get(v) {
                    out[r * width + base + i] = 1.0;
                }
            }
            base += classes.len();
        }
        Ok(TensorVal::from_f32(vec![rows, width], out))
    }
}

// ---- the inner loops of the models of `learn` (#366) -------------------------------------------------------------

fn want_sparse(v: &Value) -> Result<Arc<crate::sparse::SparseVal>, RuntimeError> {
    match v {
        Value::Native(Native::Sparse(m)) => Ok(m.clone()),
        other => err(format!(
            "expected a Sparse matrix, found {}",
            other.type_name()
        )),
    }
}

fn want_count(v: &Value, what: &str) -> Result<usize, RuntimeError> {
    let n = want_i64(v)?;
    if n < 1 {
        return err(format!("{what} is {n}; it must be at least 1"));
    }
    Ok(n as usize)
}

fn objective_value(loss: f64, grad: Vec<f64>) -> Value {
    pair(Value::Float(loss), floats(grad))
}

/// The rows of a CSR matrix as (column, value) pairs, in f64.
fn row_of(m: &crate::sparse::SparseVal, r: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
    (m.indptr[r]..m.indptr[r + 1]).map(move |p| (m.indices[p] as usize, f64::from(m.data[p])))
}

/// The mean multinomial log loss plus `|W|^2 / (2 c n)` and its gradient. `theta` is the weights feature by feature
/// (`features * classes`), then the `classes` intercepts, which are not penalized.
fn logistic_objective(args: &[Value]) -> Result<Value, RuntimeError> {
    let x = want_sparse(&args[0])?;
    let y: Vec<i64> = want_vec(&args[1])?
        .iter()
        .map(want_i64)
        .collect::<Result<_, _>>()?;
    let theta = want_floats(&args[2])?;
    let k = want_count(&args[3], "the number of classes")?;
    let c = want_f64(&args[4])?;
    let (n, f) = (x.rows, x.cols);
    if y.len() != n || theta.len() != f * k + k {
        return err(format!(
            "logistic_objective: {} labels and {} parameters for {n} rows, {f} features and {k} classes; it needs {n} and {}",
            y.len(),
            theta.len(),
            f * k + k
        ));
    }
    let nf = n as f64;
    let mut grad = vec![0f64; f * k + k];
    let mut loss = 0.0;
    let mut z = vec![0f64; k];
    for (r, &t) in y.iter().enumerate() {
        if t < 0 || t as usize >= k {
            return err(format!(
                "logistic_objective: the label of row {r} is {t}; it is from 0 to {}",
                k - 1
            ));
        }
        z.copy_from_slice(&theta[f * k..]);
        for (j, v) in row_of(&x, r) {
            for (cl, zc) in z.iter_mut().enumerate() {
                *zc += v * theta[j * k + cl];
            }
        }
        let top = z.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let total: f64 = z.iter().map(|v| (v - top).exp()).sum();
        for (cl, zc) in z.iter().enumerate() {
            let p = (zc - top).exp() / total;
            if cl == t as usize {
                loss -= p.max(1e-300).ln();
            }
            let d = (p - if cl == t as usize { 1.0 } else { 0.0 }) / nf;
            grad[f * k + cl] += d;
            for (j, v) in row_of(&x, r) {
                grad[j * k + cl] += v * d;
            }
        }
    }
    let mut penalty = 0.0;
    for i in 0..f * k {
        grad[i] += theta[i] / (c * nf);
        penalty += theta[i] * theta[i];
    }
    Ok(objective_value(loss / nf + penalty / (2.0 * c * nf), grad))
}

/// `|w|^2 / 2 + c * sum of max(0, 1 - y (w x + b))^2` and its gradient, for `y` in {-1, 1}. `w` ends with the intercept,
/// penalized like the weights (LinearSVC's intercept_scaling=1).
fn squared_hinge_objective(args: &[Value]) -> Result<Value, RuntimeError> {
    let x = want_sparse(&args[0])?;
    let y = want_floats(&args[1])?;
    let w = want_floats(&args[2])?;
    let c = want_f64(&args[3])?;
    let (n, f) = (x.rows, x.cols);
    if y.len() != n || w.len() != f + 1 {
        return err(format!(
            "squared_hinge_objective: {} labels and {} weights for {n} rows and {f} features; it needs {n} and {}",
            y.len(),
            w.len(),
            f + 1
        ));
    }
    let mut grad = w.clone();
    let mut loss = 0.0;
    for (r, &yr) in y.iter().enumerate() {
        let margin: f64 = w[f] + row_of(&x, r).map(|(j, v)| v * w[j]).sum::<f64>();
        let gap = 1.0 - yr * margin;
        if gap > 0.0 {
            loss += gap * gap;
            let coef = -2.0 * c * yr * gap;
            for (j, v) in row_of(&x, r) {
                grad[j] += coef * v;
            }
            grad[f] += coef;
        }
    }
    let norm: f64 = w.iter().map(|v| v * v).sum();
    Ok(objective_value(norm / 2.0 + c * loss, grad))
}

/// exp(E[log theta]) of a Dirichlet: exp(digamma(a) - digamma(sum a)).
fn expected(alpha: &[f64], out: &mut [f64]) {
    let total = crate::std_impl::digamma(alpha.iter().sum());
    for (o, a) in out.iter_mut().zip(alpha) {
        *o = (crate::std_impl::digamma(*a) - total).exp();
    }
}

/// The variational E-step of LDA (scikit-learn's `_update_doc_distribution`), every document from its `start` shares:
/// at most 100 updates, until the shares change by less than 1e-3 on average. Gives the shares of each document and the
/// statistics of each topic (to be multiplied by `exp_topic_word` in the M-step).
fn lda_e_step(args: &[Value]) -> Result<Value, RuntimeError> {
    let x = want_sparse(&args[0])?;
    let ew = want_floats(&args[1])?;
    let k = want_count(&args[2], "the number of topics")?;
    let prior = want_f64(&args[3])?;
    let start = want_floats(&args[4])?;
    let (n, v) = (x.rows, x.cols);
    if ew.len() != k * v || start.len() != n * k {
        return err(format!(
            "lda_e_step: {} topic weights and {} start values for {k} topics, {v} words and {n} documents; it needs {} and {}",
            ew.len(),
            start.len(),
            k * v,
            n * k
        ));
    }
    let mut shares = vec![0f64; n * k];
    let mut stats = vec![0f64; k * v];
    let (mut et, mut next, mut ratio) = (vec![0f64; k], vec![0f64; k], Vec::new());
    for d in 0..n {
        let row: Vec<(usize, f64)> = row_of(&x, d).collect();
        let mut theta = start[d * k..(d + 1) * k].to_vec();
        expected(&theta, &mut et);
        for _ in 0..100 {
            ratio.clear();
            for &(w, cnt) in &row {
                let phi: f64 = f64::EPSILON + (0..k).map(|t| et[t] * ew[t * v + w]).sum::<f64>();
                ratio.push(cnt / phi);
            }
            for t in 0..k {
                let s: f64 = row
                    .iter()
                    .zip(&ratio)
                    .map(|(&(w, _), r)| r * ew[t * v + w])
                    .sum();
                next[t] = et[t] * s + prior;
            }
            let change: f64 = (0..k).map(|t| (next[t] - theta[t]).abs()).sum::<f64>() / k as f64;
            theta.copy_from_slice(&next);
            expected(&theta, &mut et);
            if change < 1e-3 {
                break;
            }
        }
        // The statistics with norm_phi of the final shares, as scikit-learn computes them after the loop.
        ratio.clear();
        for &(w, cnt) in &row {
            let phi: f64 = f64::EPSILON + (0..k).map(|t| et[t] * ew[t * v + w]).sum::<f64>();
            ratio.push(cnt / phi);
        }
        for t in 0..k {
            for (&(w, _), r) in row.iter().zip(&ratio) {
                stats[t * v + w] += et[t] * r;
            }
        }
        shares[d * k..(d + 1) * k].copy_from_slice(&theta);
    }
    Ok(pair(floats(shares), floats(stats)))
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The search direction of L-BFGS (the two-loop recursion of Nocedal) for the gradient `g` and the last corrections
/// `s` (steps) and `y` (changes of the gradient), oldest first. With no correction it is minus the gradient scaled to at
/// most 1 per coordinate, the first step of scipy's L-BFGS.
fn lbfgs_direction(args: &[Value]) -> Result<Value, RuntimeError> {
    let g = want_floats(&args[0])?;
    let ss: Vec<Vec<f64>> = want_vec(&args[1])?
        .iter()
        .map(want_floats)
        .collect::<Result<_, _>>()?;
    let ys: Vec<Vec<f64>> = want_vec(&args[2])?
        .iter()
        .map(want_floats)
        .collect::<Result<_, _>>()?;
    if ss.len() != ys.len() || ss.iter().chain(&ys).any(|v| v.len() != g.len()) {
        return err(
            "lbfgs_direction: the steps and the gradient changes must be as many, each as long as the gradient",
        );
    }
    let h = ss.len();
    let mut q = g.clone();
    let mut alphas = vec![0f64; h];
    for i in (0..h).rev() {
        let a = dot(&ss[i], &q) / dot(&ys[i], &ss[i]);
        alphas[i] = a;
        for (qj, yj) in q.iter_mut().zip(&ys[i]) {
            *qj -= a * yj;
        }
    }
    let gamma = if h > 0 {
        dot(&ss[h - 1], &ys[h - 1]) / dot(&ys[h - 1], &ys[h - 1])
    } else {
        1.0 / g.iter().fold(1.0f64, |m, v| m.max(v.abs()))
    };
    for v in q.iter_mut() {
        *v *= gamma;
    }
    for i in 0..h {
        let b = dot(&ys[i], &q) / dot(&ys[i], &ss[i]);
        for (qj, sj) in q.iter_mut().zip(&ss[i]) {
            *qj += (alphas[i] - b) * sj;
        }
    }
    Ok(floats(q.into_iter().map(|v| -v).collect()))
}

pub(crate) fn call(name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
    let f = name.strip_prefix("std::ml::").unwrap_or(name);
    match f {
        "lbfgs_direction" => lbfgs_direction(args),
        "logistic_objective" => logistic_objective(args),
        "squared_hinge_objective" => squared_hinge_objective(args),
        "lda_e_step" => lda_e_step(args),
        "split" => {
            let t = want_table(&args[0])?;
            let share = test_size(&args[1], "split")?;
            let g = GeneratorVal::new(want_i64(&args[2])?);
            let mut order: Vec<u64> = (0..t.rows() as u64).collect();
            shuffle(&g, &mut order);
            let n_test = (share * t.rows() as f64).ceil() as usize;
            let train = order.split_off(n_test.min(order.len()));
            split_pair(&t, train, order, "split")
        }
        "split_stratified" => {
            let t = want_table(&args[0])?;
            let col = column(&t.batch, &want_str(&args[1])?, "split_stratified")?;
            let share = test_size(&args[2], "split_stratified")?;
            let g = GeneratorVal::new(want_i64(&args[3])?);
            let values = labels_of(&col)?;
            let mut classes: Vec<(String, Vec<u64>)> = Vec::new();
            for (i, v) in values.iter().enumerate() {
                match classes.iter_mut().find(|(c, _)| c == v) {
                    Some((_, rows)) => rows.push(i as u64),
                    None => classes.push((v.clone(), vec![i as u64])),
                }
            }
            let (mut train, mut test) = (Vec::new(), Vec::new());
            for (_, mut rows) in classes {
                shuffle(&g, &mut rows);
                let n_test = (share * rows.len() as f64).round() as usize;
                test.extend_from_slice(&rows[..n_test]);
                train.extend_from_slice(&rows[n_test..]);
            }
            shuffle(&g, &mut train);
            shuffle(&g, &mut test);
            split_pair(&t, train, test, "split_stratified")
        }
        "split_by_time" => {
            let t = want_table(&args[0])?;
            let share = test_size(&args[1], "split_by_time")?;
            let n = t.rows() as u64;
            let n_test = (share * n as f64).ceil() as u64;
            let cut = n.saturating_sub(n_test);
            split_pair(&t, (0..cut).collect(), (cut..n).collect(), "split_by_time")
        }
        "k_fold" => {
            let (n, k) = (want_i64(&args[0])?, want_i64(&args[1])?);
            if k < 2 || n < k {
                return err(format!(
                    "k_fold: {k} folds of {n} row(s); it needs at least 2 folds and a row for each"
                ));
            }
            let g = GeneratorVal::new(want_i64(&args[2])?);
            let mut order: Vec<u64> = (0..n as u64).collect();
            shuffle(&g, &mut order);
            // The first `n % k` folds have one row more, as scikit-learn's `KFold`.
            let (size, extra) = ((n / k) as usize, (n % k) as usize);
            let mut start = 0;
            let mut out = Vec::new();
            for fold in 0..k as usize {
                let len = size + usize::from(fold < extra);
                let ints = |xs: &[u64]| {
                    let mut v: Vec<u64> = xs.to_vec();
                    v.sort_unstable();
                    Value::vec(v.into_iter().map(|i| Value::Int(i as i64)).collect())
                };
                let test = &order[start..start + len];
                let train: Vec<u64> = order[..start]
                    .iter()
                    .chain(&order[start + len..])
                    .copied()
                    .collect();
                out.push(pair(ints(&train), ints(test)));
                start += len;
            }
            Ok(Value::vec(out))
        }
        "mae" | "mse" | "rmse" | "r2" => regression(f, args),
        "accuracy" => Ok(Value::Float(counts(args, f)?.accuracy())),
        "precision" | "recall" | "f1" => {
            let c = counts(args, f)?;
            let how = want_str(&args[2])?;
            let per = match f {
                "precision" => Counts::precision,
                "recall" => Counts::recall,
                _ => Counts::f1,
            };
            Ok(Value::Float(c.average(per, &how)?))
        }
        "confusion_matrix" => {
            let c = counts(args, f)?;
            let rows = c
                .matrix
                .iter()
                .map(|r| Value::vec(r.iter().map(|x| Value::Int(*x)).collect()))
                .collect();
            Ok(pair(texts(&c.labels), Value::vec(rows)))
        }
        "classification_report" => Ok(Value::str(counts(args, f)?.report())),
        "roc_auc" => roc_auc(args),
        "Scaler::fit" => Ok(Value::Native(Native::Scaler(Arc::new(fit_scaler(
            &want_floats(&args[0])?,
        )?)))),
        "LabelEncoder::fit" => {
            let xs = want_texts(&args[0])?;
            if xs.is_empty() {
                return err("LabelEncoder.fit: there are no values to learn from");
            }
            Ok(Value::Native(Native::LabelEncoder(Arc::new(EncoderVal {
                classes: sorted_labels(xs.iter()),
            }))))
        }
        "Features::fit" => {
            let t = want_table(&args[0])?;
            let fitted = FeaturesVal::fit(
                &t,
                want_str(&args[1])?,
                want_texts(&args[2])?,
                want_texts(&args[3])?,
            )?;
            Ok(Value::Native(Native::Features(Arc::new(fitted))))
        }
        _ => err(format!("unknown function `{}`", name.replace("::", "."))),
    }
}

pub(crate) fn scaler_method(
    s: &ScalerVal,
    name: &str,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    match name {
        "apply" => Ok(floats(
            want_floats(&args[0])?
                .into_iter()
                .map(|x| s.apply(x))
                .collect(),
        )),
        "inverse" => Ok(floats(
            want_floats(&args[0])?
                .into_iter()
                .map(|x| x * s.std + s.mean)
                .collect(),
        )),
        "mean" => Ok(Value::Float(s.mean)),
        "std" => Ok(Value::Float(s.std)),
        _ => err(format!("Scaler has no method `{name}`")),
    }
}

pub(crate) fn encoder_method(
    e: &EncoderVal,
    name: &str,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    match name {
        "encode" => {
            let at: HashMap<&String, usize> =
                e.classes.iter().enumerate().map(|(i, c)| (c, i)).collect();
            let out = want_texts(&args[0])?
                .iter()
                .map(|x| match at.get(x) {
                    Some(i) => Ok(Value::Int(*i as i64)),
                    None => err(format!(
                        "LabelEncoder.encode: `{x}` was not in the training values; the classes are {}",
                        e.classes.join(", ")
                    )),
                })
                .collect::<Result<_, _>>()?;
            Ok(Value::vec(out))
        }
        "decode" => {
            let out = want_vec(&args[0])?
                .iter()
                .map(|v| {
                    let i = want_i64(v)?;
                    match usize::try_from(i).ok().and_then(|i| e.classes.get(i)) {
                        Some(c) => Ok(Value::str(c.as_str())),
                        None => err(format!(
                            "LabelEncoder.decode: {i} is not a class; there are {}",
                            e.classes.len()
                        )),
                    }
                })
                .collect::<Result<_, _>>()?;
            Ok(Value::vec(out))
        }
        "classes" => Ok(texts(&e.classes)),
        _ => err(format!("LabelEncoder has no method `{name}`")),
    }
}

pub(crate) fn features_method(
    f: &FeaturesVal,
    name: &str,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    match name {
        "transform" => Ok(Value::Tensor(Arc::new(
            f.transform(&*want_table(&args[0])?)?,
        ))),
        "names" => Ok(texts(&f.names())),
        "width" => Ok(Value::Int(f.width() as i64)),
        "target" => Ok(Value::str(f.target.as_str())),
        _ => err(format!("Features has no method `{name}`")),
    }
}
