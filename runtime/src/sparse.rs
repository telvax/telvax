//! `std.sparse`: a matrix of `f32` in compressed sparse row (CSR) form, for features where most values are zero (the
//! words of a text). Each row keeps its non-zero values in order of column; nothing else is stored.

use std::sync::Arc;

use crate::RuntimeError;
use crate::value::{Native, TensorVal, Value};

/// A CSR matrix. The non-zeros of row `r` are `indices[indptr[r]..indptr[r + 1]]` (their columns, increasing) and the
/// same range of `data`. It does not change: every operation gives a new matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct SparseVal {
    pub rows: usize,
    pub cols: usize,
    pub indptr: Vec<usize>,
    pub indices: Vec<u32>,
    pub data: Vec<f32>,
}

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

fn want_size(v: &Value, what: &str) -> Result<usize, RuntimeError> {
    let n = want_i64(v)?;
    if n < 0 {
        return err(format!("{what} is {n}; it cannot be negative"));
    }
    Ok(n as usize)
}

fn want_tuple(v: &Value) -> Result<Arc<Vec<Value>>, RuntimeError> {
    match v {
        Value::Tuple(t) => Ok(t.clone()),
        other => err(format!("expected a tuple, found {}", other.type_name())),
    }
}

fn want_tensor(v: &Value) -> Result<Arc<TensorVal>, RuntimeError> {
    match v {
        Value::Tensor(t) => Ok(t.clone()),
        other => err(format!("expected a Tensor, found {}", other.type_name())),
    }
}

fn floats(v: &Value, what: &str, len: usize) -> Result<Vec<f64>, RuntimeError> {
    let xs = want_vec(v)?
        .iter()
        .map(want_f64)
        .collect::<Result<Vec<f64>, _>>()?;
    if xs.len() != len {
        return err(format!(
            "{what} has {} value(s), and the matrix needs {len}",
            xs.len()
        ));
    }
    Ok(xs)
}

fn index(v: &Value, len: usize, what: &str) -> Result<usize, RuntimeError> {
    let i = want_i64(v)?;
    if i < 0 || i as u64 >= len as u64 {
        return err(format!("{what} {i} is out of range: the matrix has {len}"));
    }
    Ok(i as usize)
}

pub(crate) fn value(m: SparseVal) -> Value {
    Value::Native(Native::Sparse(Arc::new(m)))
}

impl SparseVal {
    /// From the triplets `(row, column, value)` in any order; the values of one place are added, and zeros are dropped.
    pub fn from_triplets(
        rows: usize,
        cols: usize,
        mut t: Vec<(usize, usize, f32)>,
    ) -> Result<Self, RuntimeError> {
        if cols > u32::MAX as usize {
            return err(format!(
                "{cols} columns are more than a sparse matrix holds"
            ));
        }
        for &(r, c, _) in &t {
            if r >= rows || c >= cols {
                return err(format!(
                    "the place ({r}, {c}) is outside a matrix of {rows} row(s) and {cols} column(s)"
                ));
            }
        }
        t.sort_by_key(|&(r, c, _)| (r, c));
        let mut m = SparseVal {
            rows,
            cols,
            indptr: vec![0; rows + 1],
            indices: Vec::with_capacity(t.len()),
            data: Vec::with_capacity(t.len()),
        };
        let mut i = 0;
        while i < t.len() {
            let (r, c, mut x) = t[i];
            i += 1;
            while i < t.len() && t[i].0 == r && t[i].1 == c {
                x += t[i].2;
                i += 1;
            }
            if x != 0.0 {
                m.indices.push(c as u32);
                m.data.push(x);
                m.indptr[r + 1] += 1;
            }
        }
        for r in 0..rows {
            m.indptr[r + 1] += m.indptr[r];
        }
        Ok(m)
    }

    fn row(&self, r: usize) -> std::ops::Range<usize> {
        self.indptr[r]..self.indptr[r + 1]
    }

    fn triplets(&self) -> impl Iterator<Item = (usize, usize, f32)> + '_ {
        (0..self.rows).flat_map(move |r| {
            self.row(r)
                .map(move |k| (r, self.indices[k] as usize, self.data[k]))
        })
    }

    /// The same places with each value changed by `f` (a value that becomes zero is dropped).
    fn map(&self, mut f: impl FnMut(usize, usize, f32) -> f32) -> SparseVal {
        let t = self
            .triplets()
            .map(|(r, c, x)| (r, c, f(r, c, x)))
            .collect();
        SparseVal::from_triplets(self.rows, self.cols, t).expect("same shape")
    }

    pub fn transpose(&self) -> SparseVal {
        let t = self.triplets().map(|(r, c, x)| (c, r, x)).collect();
        SparseVal::from_triplets(self.cols, self.rows, t).expect("same places")
    }

    /// The matrix times a vector of `cols` values: `rows` values.
    pub fn matvec(&self, x: &[f32]) -> Vec<f32> {
        (0..self.rows)
            .map(|r| {
                self.row(r)
                    .map(|k| self.data[k] * x[self.indices[k] as usize])
                    .sum()
            })
            .collect()
    }

    /// The matrix times a dense `cols x n` matrix (row-major): `rows x n`, row-major.
    pub fn matmul(&self, b: &[f32], n: usize) -> Vec<f32> {
        let mut out = vec![0f32; self.rows * n];
        for r in 0..self.rows {
            let dst = &mut out[r * n..(r + 1) * n];
            for k in self.row(r) {
                let (c, x) = (self.indices[k] as usize, self.data[k]);
                for (o, b) in dst.iter_mut().zip(&b[c * n..(c + 1) * n]) {
                    *o += x * b;
                }
            }
        }
        out
    }

    pub fn to_dense(&self) -> Vec<f32> {
        let mut out = vec![0f32; self.rows * self.cols];
        for (r, c, x) in self.triplets() {
            out[r * self.cols + c] = x;
        }
        out
    }
}

/// `std.sparse.from_rows`, `from_triplets` and `from_dense`.
pub(crate) fn call(name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
    match name {
        "std::sparse::from_rows" => {
            let rows = want_vec(&args[0])?;
            let cols = want_size(&args[1], "the number of columns")?;
            let mut t = Vec::new();
            for (r, row) in rows.iter().enumerate() {
                for pair in want_vec(row)? {
                    let p = want_tuple(&pair)?;
                    let c = want_i64(&p[0])?;
                    if c < 0 {
                        return err(format!("row {r} has the column {c}; columns start at 0"));
                    }
                    t.push((r, c as usize, want_f64(&p[1])? as f32));
                }
            }
            Ok(value(SparseVal::from_triplets(rows.len(), cols, t)?))
        }
        "std::sparse::from_triplets" => {
            let rows = want_size(&args[0], "the number of rows")?;
            let cols = want_size(&args[1], "the number of columns")?;
            let mut t = Vec::new();
            for item in want_vec(&args[2])? {
                let p = want_tuple(&item)?;
                let (r, c) = (want_i64(&p[0])?, want_i64(&p[1])?);
                if r < 0 || c < 0 {
                    return err(format!("the place ({r}, {c}) has a negative index"));
                }
                t.push((r as usize, c as usize, want_f64(&p[2])? as f32));
            }
            Ok(value(SparseVal::from_triplets(rows, cols, t)?))
        }
        "std::sparse::from_dense" => {
            let t = want_tensor(&args[0])?;
            let [rows, cols] = t.shape[..] else {
                return err(format!(
                    "from_dense needs a tensor of 2 dimensions, found {}",
                    t.shape.len()
                ));
            };
            let data = t.to_f32_vec();
            let trip = data
                .iter()
                .enumerate()
                .filter(|(_, x)| **x != 0.0)
                .map(|(i, x)| (i / cols, i % cols, *x))
                .collect();
            Ok(value(SparseVal::from_triplets(rows, cols, trip)?))
        }
        _ => err(format!("unknown function `{}`", name.replace("::", "."))),
    }
}

/// The methods of a `Sparse` matrix.
pub(crate) fn method(m: &SparseVal, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
    let list_f = |xs: Vec<f64>| Value::vec(xs.into_iter().map(Value::Float).collect());
    let list_i =
        |xs: Vec<usize>| Value::vec(xs.into_iter().map(|x| Value::Int(x as i64)).collect());
    match name {
        "rows" => Ok(Value::Int(m.rows as i64)),
        "cols" => Ok(Value::Int(m.cols as i64)),
        "nnz" => Ok(Value::Int(m.data.len() as i64)),
        "get" => {
            let r = index(&args[0], m.rows, "row")?;
            let c = index(&args[1], m.cols, "column")?;
            let found = m.row(r).find(|&k| m.indices[k] as usize == c);
            Ok(Value::Float(f64::from(found.map_or(0.0, |k| m.data[k]))))
        }
        "row" => {
            let r = index(&args[0], m.rows, "row")?;
            Ok(Value::vec(
                m.row(r)
                    .map(|k| {
                        Value::Tuple(Arc::new(vec![
                            Value::Int(i64::from(m.indices[k])),
                            Value::Float(f64::from(m.data[k])),
                        ]))
                    })
                    .collect(),
            ))
        }
        "select_rows" => {
            let picks = want_vec(&args[0])?
                .iter()
                .map(|v| index(v, m.rows, "row"))
                .collect::<Result<Vec<_>, _>>()?;
            let mut out = SparseVal {
                rows: picks.len(),
                cols: m.cols,
                indptr: vec![0],
                indices: vec![],
                data: vec![],
            };
            for r in picks {
                out.indices.extend_from_slice(&m.indices[m.row(r)]);
                out.data.extend_from_slice(&m.data[m.row(r)]);
                out.indptr.push(out.data.len());
            }
            Ok(value(out))
        }
        "select_cols" => {
            // Column `picks[j]` becomes column `j`; a column picked twice appears twice.
            let picks = want_vec(&args[0])?
                .iter()
                .map(|v| index(v, m.cols, "column"))
                .collect::<Result<Vec<_>, _>>()?;
            let mut to: Vec<Vec<usize>> = vec![Vec::new(); m.cols];
            for (j, &c) in picks.iter().enumerate() {
                to[c].push(j);
            }
            let t = m
                .triplets()
                .flat_map(|(r, c, x)| to[c].iter().map(move |&j| (r, j, x)))
                .collect();
            Ok(value(SparseVal::from_triplets(m.rows, picks.len(), t)?))
        }
        "row_sums" => Ok(list_f(
            (0..m.rows)
                .map(|r| m.row(r).map(|k| f64::from(m.data[k])).sum())
                .collect(),
        )),
        "col_sums" => {
            let mut s = vec![0f64; m.cols];
            for (_, c, x) in m.triplets() {
                s[c] += f64::from(x);
            }
            Ok(list_f(s))
        }
        "row_counts" => Ok(list_i((0..m.rows).map(|r| m.row(r).len()).collect())),
        "col_counts" => {
            let mut n = vec![0usize; m.cols];
            for &c in &m.indices {
                n[c as usize] += 1;
            }
            Ok(list_i(n))
        }
        "normalize_rows" => {
            let norms: Vec<f32> = (0..m.rows)
                .map(|r| {
                    m.row(r)
                        .map(|k| f64::from(m.data[k]).powi(2))
                        .sum::<f64>()
                        .sqrt() as f32
                })
                .collect();
            Ok(value(m.map(|r, _, x| x / norms[r])))
        }
        "scale" => {
            let k = want_f64(&args[0])? as f32;
            Ok(value(m.map(|_, _, x| x * k)))
        }
        "scale_cols" => {
            let w = floats(&args[0], "the list of weights", m.cols)?;
            Ok(value(m.map(|_, c, x| x * w[c] as f32)))
        }
        "scale_rows" => {
            let w = floats(&args[0], "the list of weights", m.rows)?;
            Ok(value(m.map(|r, _, x| x * w[r] as f32)))
        }
        "transpose" => Ok(value(m.transpose())),
        "matvec" => {
            let x = want_tensor(&args[0])?;
            if x.shape != [m.cols] {
                return err(format!(
                    "matvec: the matrix has {} column(s), and the vector has the shape {:?}",
                    m.cols, x.shape
                ));
            }
            Ok(Value::Tensor(Arc::new(TensorVal::from_f32(
                vec![m.rows],
                m.matvec(&x.to_f32_vec()),
            ))))
        }
        "matmul" => {
            let b = want_tensor(&args[0])?;
            let [k, n] = b.shape[..] else {
                return err(format!(
                    "matmul needs a tensor of 2 dimensions, found {}",
                    b.shape.len()
                ));
            };
            if k != m.cols {
                return err(format!(
                    "matmul: the matrix has {} column(s), and the tensor has {k} row(s)",
                    m.cols
                ));
            }
            Ok(Value::Tensor(Arc::new(TensorVal::from_f32(
                vec![m.rows, n],
                m.matmul(&b.to_f32_vec(), n),
            ))))
        }
        "to_dense" => Ok(Value::Tensor(Arc::new(TensorVal::from_f32(
            vec![m.rows, m.cols],
            m.to_dense(),
        )))),
        _ => err(format!("Sparse has no method `{name}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> SparseVal {
        // [[1, 0, 2],
        //  [0, 0, 0],
        //  [0, 3, 0],
        //  [4, 0, 5]]
        SparseVal::from_triplets(
            4,
            3,
            vec![
                (3, 2, 5.0),
                (0, 0, 1.0),
                (2, 1, 3.0),
                (0, 2, 2.0),
                (3, 0, 4.0),
            ],
        )
        .unwrap()
    }

    #[test]
    fn csr_layout_and_products_match_the_dense_matrix() {
        let a = m();
        assert_eq!(a.indptr, vec![0, 2, 2, 3, 5]);
        assert_eq!(a.indices, vec![0, 2, 1, 0, 2]);
        assert_eq!(a.data, vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        let d = a.to_dense();
        assert_eq!(d, vec![1., 0., 2., 0., 0., 0., 0., 3., 0., 4., 0., 5.]);
        assert_eq!(a.matvec(&[1.0, 2.0, 3.0]), vec![7.0, 0.0, 6.0, 19.0]);
        // Against a dense product computed by hand.
        let b = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]; // 3 x 2
        let mut want = vec![0f32; 8];
        for r in 0..4 {
            for j in 0..2 {
                want[r * 2 + j] = (0..3).map(|c| d[r * 3 + c] * b[c * 2 + j]).sum();
            }
        }
        assert_eq!(a.matmul(&b, 2), want);
        let t = a.transpose();
        assert_eq!((t.rows, t.cols), (3, 4));
        assert_eq!(t.transpose(), a);
    }

    #[test]
    fn duplicates_add_up_and_zeros_are_dropped() {
        let a = SparseVal::from_triplets(
            2,
            2,
            vec![
                (0, 1, 1.5),
                (0, 1, 2.5),
                (1, 0, 0.0),
                (1, 1, 1.0),
                (1, 1, -1.0),
            ],
        )
        .unwrap();
        assert_eq!(
            (a.data.clone(), a.indptr.clone()),
            (vec![4.0], vec![0, 1, 1])
        );
        assert!(SparseVal::from_triplets(2, 2, vec![(2, 0, 1.0)]).is_err());
    }
}
