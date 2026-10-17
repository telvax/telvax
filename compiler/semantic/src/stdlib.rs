//! Typing rules for the Telvax standard library.
//!
//! Std functions are polymorphic (`map`, `filter`, ...) and the checker has no generics yet, so
//! each item carries a small typing rule instead of a fixed signature. The interpreter in
//! `telvax_runtime` implements the same set of items; keep the two in sync.

use crate::ty::{Dim, Prim, Ty};

type Check = fn(&[Ty]) -> Result<Ty, String>;
type Hint = fn(usize, &[Ty]) -> Option<Ty>;

/// Typing rule of one std function or method. For methods, `args` excludes the receiver.
#[derive(Clone, Copy)]
pub struct Sig {
    /// Validates the argument types and returns the result type.
    pub check: Check,
    /// Expected type for argument `i` given the types of the previous ones (guides closures).
    pub hint: Hint,
    /// For functions whose result type cannot be derived from the arguments (`zeros`,
    /// `read_csv`, ...): maps the argument types and the type the context expects to the result.
    pub from_expected: Option<FromExpected>,
}

type FromExpected = fn(&[Ty], &Ty) -> Result<Ty, String>;

fn no_hint(_: usize, _: &[Ty]) -> Option<Ty> {
    None
}

impl std::fmt::Debug for Sig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sig")
    }
}

const fn sig(check: Check) -> Sig {
    Sig {
        check,
        hint: no_hint,
        from_expected: None,
    }
}

const fn sig_expected(check: Check, from_expected: FromExpected) -> Sig {
    Sig {
        check,
        hint: no_hint,
        from_expected: Some(from_expected),
    }
}

fn arity(args: &[Ty], n: usize) -> Result<(), String> {
    if args.len() == n {
        Ok(())
    } else {
        Err(format!("expected {n} argument(s), found {}", args.len()))
    }
}

fn want(args: &[Ty], i: usize, expected: &Ty) -> Result<(), String> {
    if expected.accepts(&args[i]) {
        Ok(())
    } else {
        Err(format!(
            "argument {} must be `{expected}`, found `{}`",
            i + 1,
            args[i]
        ))
    }
}

fn want_int(args: &[Ty], i: usize) -> Result<(), String> {
    match &args[i] {
        Ty::Prim(p) if p.is_int() => Ok(()),
        Ty::IntVar(_) | Ty::Unknown | Ty::Never => Ok(()),
        other => Err(format!(
            "argument {} must be an integer, found `{other}`",
            i + 1
        )),
    }
}

fn s() -> Ty {
    Ty::str()
}

fn vec_of(t: Ty) -> Ty {
    Ty::Vec(Box::new(t))
}

fn named(n: &str) -> Ty {
    Ty::Named(n.to_string())
}

/// Element type of a `Vec` argument; `Unknown` when the argument is not statically known.
fn elem(args: &[Ty], i: usize) -> Result<Ty, String> {
    match &args[i] {
        Ty::Vec(t) => Ok((**t).clone()),
        Ty::Unknown | Ty::Never => Ok(Ty::Unknown),
        other => Err(format!(
            "argument {} must be a `List`, found `{other}`",
            i + 1
        )),
    }
}

fn str_to_str(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(s())
}

fn truncate(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &s())?;
    want_int(args, 1)?;
    Ok(s())
}

fn split(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &s())?;
    want(args, 1, &s())?;
    Ok(vec_of(s()))
}

fn join(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    elem(args, 0)?;
    want(args, 1, &s())?;
    Ok(s())
}

fn map_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    if i == 1 {
        let t = elem(prev, 0).ok()?;
        Some(Ty::Fn(vec![t], Box::new(Ty::Unknown)))
    } else {
        None
    }
}

fn map(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let item = elem(args, 0)?;
    match &args[1] {
        Ty::Fn(p, _) if p.len() == 1 && !p[0].accepts(&item) => Err(format!(
            "the function takes `{}` but the vector holds `{item}`: mismatched element type",
            p[0]
        )),
        Ty::Fn(p, r) if p.len() == 1 => Ok(vec_of((**r).clone())),
        Ty::Unknown => Ok(vec_of(Ty::Unknown)),
        other => Err(format!(
            "argument 2 must be a one-argument function, found `{other}`"
        )),
    }
}

fn filter_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    if i == 1 {
        let t = elem(prev, 0).ok()?;
        Some(Ty::Fn(vec![t], Box::new(Ty::bool())))
    } else {
        None
    }
}

fn filter(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let t = elem(args, 0)?;
    match &args[1] {
        Ty::Fn(p, _) if p.len() == 1 && !p[0].accepts(&t) => Err(format!(
            "the predicate takes `{}` but the vector holds `{t}`: mismatched element type",
            p[0]
        )),
        Ty::Fn(p, r) if p.len() == 1 && Ty::bool().accepts(r) => Ok(vec_of(t)),
        Ty::Unknown => Ok(vec_of(t)),
        other => Err(format!("argument 2 must be `(_) -> Bool`, found `{other}`")),
    }
}

fn take(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let t = elem(args, 0)?;
    want_int(args, 1)?;
    Ok(vec_of(t))
}

fn bool_fn_over_vec(args: &[Ty], what: &str) -> Result<Ty, String> {
    arity(args, 2)?;
    let t = elem(args, 0)?;
    match &args[1] {
        Ty::Fn(p, _) if p.len() == 1 && !p[0].accepts(&t) => Err(format!(
            "the predicate takes `{}` but the vector holds `{t}`: mismatched element type",
            p[0]
        )),
        Ty::Fn(p, r) if p.len() == 1 && Ty::bool().accepts(r) => Ok(t),
        Ty::Unknown => Ok(t),
        other => Err(format!(
            "argument 2 of `{what}` must be `(_) -> Bool`, found `{other}`"
        )),
    }
}

fn any_all(args: &[Ty]) -> Result<Ty, String> {
    bool_fn_over_vec(args, "any / all")?;
    Ok(Ty::bool())
}

fn find(args: &[Ty]) -> Result<Ty, String> {
    let t = bool_fn_over_vec(args, "find")?;
    Ok(Ty::Option(Box::new(t)))
}

fn for_each_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    if i == 1 {
        let t = elem(prev, 0).ok()?;
        Some(Ty::Fn(vec![t], Box::new(Ty::Unknown)))
    } else {
        None
    }
}

fn for_each(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    elem(args, 0)?;
    Ok(Ty::Unit)
}

fn enumerate(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    let t = elem(args, 0)?;
    Ok(vec_of(Ty::Tuple(vec![Ty::Prim(Prim::I64), t])))
}

fn zip(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (a, b) = (elem(args, 0)?, elem(args, 1)?);
    Ok(vec_of(Ty::Tuple(vec![a, b])))
}

fn numeric_elem(args: &[Ty], what: &str) -> Result<Ty, String> {
    arity(args, 1)?;
    let t = elem(args, 0)?;
    match &t {
        Ty::Prim(p) if p.is_numeric() => Ok(t),
        Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Unknown => Ok(t),
        other => Err(format!(
            "`{what}` needs a vector of numbers, found a vector of `{other}`"
        )),
    }
}

fn sum(args: &[Ty]) -> Result<Ty, String> {
    numeric_elem(args, "sum")
}

fn min_max(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    let t = elem(args, 0)?;
    match &t {
        Ty::Prim(p) if p.is_numeric() || p.is_text() => Ok(Ty::Option(Box::new(t))),
        Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Unknown => Ok(Ty::Option(Box::new(t))),
        other => Err(format!(
            "`min` and `max` need numbers or strings, found `{other}`"
        )),
    }
}

fn fold_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    if i == 2 {
        let t = elem(prev, 0).ok()?;
        let acc = prev.get(1)?.clone();
        Some(Ty::Fn(vec![acc.clone(), t], Box::new(acc)))
    } else {
        None
    }
}

fn fold(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 3)?;
    let t = elem(args, 0)?;
    let acc = args[1].clone();
    match &args[2] {
        Ty::Fn(p, r) if p.len() == 2 => {
            if !p[0].accepts(&acc) || !p[1].accepts(&t) {
                return Err(format!(
                    "the function takes (`{}`, `{}`) but fold passes (`{acc}`, `{t}`)",
                    p[0], p[1]
                ));
            }
            if !acc.accepts(r) {
                return Err(format!(
                    "the function returns `{r}` but the accumulator is `{acc}`"
                ));
            }
            Ok(acc)
        }
        Ty::Unknown => Ok(acc),
        other => Err(format!(
            "argument 3 must be a function of two arguments, found `{other}`"
        )),
    }
}

fn skip(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want_int(args, 1)?;
    Ok(vec_of(elem(args, 0)?))
}

fn count(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    elem(args, 0)?;
    Ok(Ty::Prim(Prim::U64))
}

fn collect(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(vec_of(elem(args, 0)?))
}

fn print(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(Ty::Unit)
}

fn status(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(named("Status"))
}

fn new_embedder(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(named("OllamaEmbedder"))
}

fn new_llm(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(named("OpenAIClient"))
}

fn connect(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(named("PgVectorClient"))
}

/// Dimension produced by the embedding models supported in the alpha (`nomic-embed-text`).
pub const EMBEDDING_DIM: u64 = 768;

fn embedding() -> Ty {
    Ty::Tensor {
        elem: Box::new(Ty::Prim(Prim::F32)),
        shape: vec![Dim::Lit(EMBEDDING_DIM)],
    }
}

fn embed(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(embedding())
}

fn complete(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(s())
}

fn search_collection(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(named("VectorQuery"))
}

fn with_vector(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &named("VectorQuery"))?;
    want(args, 1, &embedding())?;
    Ok(named("VectorQuery"))
}

fn limit(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &named("VectorQuery"))?;
    want_int(args, 1)?;
    Ok(named("VectorQuery"))
}

fn execute(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &named("VectorQuery"))?;
    // Rows are objects with `id`, `text` and `score` fields.
    Ok(vec_of(Ty::Unknown))
}

fn insert(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 4)?;
    want(args, 0, &s())?;
    want(args, 1, &s())?;
    want(args, 2, &s())?;
    want(args, 3, &embedding())?;
    Ok(Ty::Unit)
}

// ---- numbers, slices and conversions -----------------------------------------------------

fn is_float_ty(t: &Ty) -> bool {
    matches!(t, Ty::Prim(p) if p.is_float()) || matches!(t, Ty::FloatVar(_))
}

fn want_number(args: &[Ty], i: usize) -> Result<(), String> {
    match &args[i] {
        Ty::Prim(p) if p.is_numeric() => Ok(()),
        Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Unknown | Ty::Never => Ok(()),
        other => Err(format!(
            "argument {} must be a number, found `{other}`",
            i + 1
        )),
    }
}

/// The type of an operation on two numbers: a float if either is one, else the first.
fn number_result(args: &[Ty]) -> Ty {
    if is_float_ty(&args[0]) || args[1] == Ty::Unknown {
        args[0].clone()
    } else if is_float_ty(&args[1]) || args[0] == Ty::Unknown {
        args[1].clone()
    } else {
        args[0].clone()
    }
}

fn same_as_first_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    match (i, prev.first()) {
        (1, Some(t)) if !matches!(t, Ty::Unknown | Ty::Never) => Some(t.clone()),
        _ => None,
    }
}

fn math_pow(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want_number(args, 0)?;
    want_number(args, 1)?;
    Ok(number_result(args))
}

fn math_binary(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want_number(args, 0)?;
    want_number(args, 1)?;
    Ok(number_result(args))
}

fn math_min_max(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    if args[0] == s() && args[1] == s() {
        return Ok(s());
    }
    math_binary(args)
}

fn math_abs(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_number(args, 0)?;
    Ok(args[0].clone())
}

/// `sqrt`, `floor`, `ceil` and `round` give a float; an integer argument gives an `f64`.
fn math_float_fn(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_number(args, 0)?;
    Ok(if is_float_ty(&args[0]) || args[0] == Ty::Unknown {
        args[0].clone()
    } else {
        Ty::Prim(Prim::F64)
    })
}

fn math_to_float(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_number(args, 0)?;
    Ok(Ty::Prim(Prim::F64))
}

fn math_to_int(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_number(args, 0)?;
    Ok(Ty::Prim(Prim::I64))
}

fn text_to_text(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(s())
}

fn text_format(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 1, &s())?;
    Ok(s())
}

fn string_slice(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 3)?;
    want(args, 0, &s())?;
    want_int(args, 1)?;
    want_int(args, 2)?;
    Ok(s())
}

fn string_repeat(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &s())?;
    want_int(args, 1)?;
    Ok(s())
}

fn iter_slice(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 3)?;
    want_int(args, 1)?;
    want_int(args, 2)?;
    Ok(vec_of(elem(args, 0)?))
}

fn iter_concat(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (a, b) = (elem(args, 0)?, elem(args, 1)?);
    if a == Ty::Unknown {
        return Ok(vec_of(b));
    }
    if !a.accepts(&b) {
        return Err(format!("cannot join a `List[{a}]` and a `List[{b}]`"));
    }
    Ok(vec_of(a))
}

fn iter_range(args: &[Ty]) -> Result<Ty, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!("expected 1 or 2 arguments, found {}", args.len()));
    }
    for i in 0..args.len() {
        want_int(args, i)?;
    }
    let ty = args
        .iter()
        .find(|t| !matches!(t, Ty::IntVar(_)))
        .unwrap_or(&args[0]);
    Ok(vec_of(ty.clone()))
}

fn task_gather(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    match elem(args, 0)? {
        Ty::Task(t) => Ok(Ty::Task(Box::new(vec_of(*t)))),
        Ty::Unknown => Ok(Ty::Task(Box::new(vec_of(Ty::Unknown)))),
        other => Err(format!(
            "`gather` needs a list of tasks, the values of calls of `async fn`s, found a list of `{other}`"
        )),
    }
}

fn task_run(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    match &args[0] {
        Ty::Task(t) => Ok((**t).clone()),
        t if is_dynamic(t) => Ok(Ty::Unknown),
        other => Err(format!("argument 1 must be a task, found `{other}`")),
    }
}

fn task_sleep(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_int(args, 0)?;
    Ok(Ty::Task(Box::new(Ty::Unit)))
}

fn time_sleep(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_int(args, 0)?;
    Ok(Ty::Unit)
}

fn time_now(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 0)?;
    Ok(Ty::Prim(Prim::F64))
}

fn map_key_ok(t: &Ty) -> bool {
    match t {
        Ty::Prim(p) => !p.is_float(),
        // The variants of an enum, when none of them holds a value (checked when the map is used).
        Ty::Enum(_) | Ty::IntVar(_) | Ty::Unknown | Ty::Never => true,
        _ => false,
    }
}

fn map_from_entries(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    match elem(args, 0)? {
        Ty::Tuple(parts) if parts.len() == 2 && !map_key_ok(&parts[0]) => Err(format!(
            "a `Map` key must be a `Text`, an integer, a `Bool` or an enum, found `{}`",
            parts[0]
        )),
        Ty::Tuple(parts) if parts.len() == 2 => Ok(Ty::Map(
            Box::new(parts[0].clone()),
            Box::new(parts[1].clone()),
        )),
        Ty::Unknown => Ok(Ty::Unknown),
        other => Err(format!(
            "a `Map` is built from `(key, value)` pairs, found `{other}`"
        )),
    }
}

// ---- tensors -----------------------------------------------------------------------------

fn tensor_of(t: &Ty) -> Option<(Ty, Vec<Dim>)> {
    match t {
        Ty::Tensor { elem, shape } => Some(((**elem).clone(), shape.clone())),
        _ => None,
    }
}

fn is_dynamic(t: &Ty) -> bool {
    matches!(t, Ty::Unknown | Ty::Never)
}

fn want_tensor(args: &[Ty], i: usize) -> Result<Option<(Ty, Vec<Dim>)>, String> {
    if is_dynamic(&args[i]) {
        return Ok(None);
    }
    tensor_of(&args[i])
        .map(Some)
        .ok_or_else(|| format!("argument {} must be a `Tensor`, found `{}`", i + 1, args[i]))
}

fn dims_match(a: &Dim, b: &Dim) -> bool {
    match (a, b) {
        (Dim::Lit(x), Dim::Lit(y)) => x == y,
        _ => true,
    }
}

/// Dimension to keep when two compatible dims meet: the concrete one wins.
fn merge_dim(a: &Dim, b: &Dim) -> Dim {
    match (a, b) {
        (Dim::Named(_), other) => other.clone(),
        (other, _) => other.clone(),
    }
}

fn shape_str(shape: &[Dim]) -> String {
    format!(
        "[{}]",
        shape
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Result of an element-wise operation on two tensors of compatible shape.
fn elementwise(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (Some((e1, s1)), Some((e2, s2))) = (want_tensor(args, 0)?, want_tensor(args, 1)?) else {
        return Ok(if is_dynamic(&args[0]) {
            args[1].clone()
        } else {
            args[0].clone()
        });
    };
    if !e1.accepts(&e2) {
        return Err(format!("tensor element types differ: `{e1}` and `{e2}`"));
    }
    if s1.len() != s2.len() || !s1.iter().zip(&s2).all(|(a, b)| dims_match(a, b)) {
        return Err(format!(
            "tensor shapes differ: {} and {}",
            shape_str(&s1),
            shape_str(&s2)
        ));
    }
    let shape = s1.iter().zip(&s2).map(|(a, b)| merge_dim(a, b)).collect();
    Ok(Ty::Tensor {
        elem: Box::new(e1),
        shape,
    })
}

fn tensor_unary(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_tensor(args, 0)?;
    Ok(args[0].clone())
}

fn tensor_scale(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want_tensor(args, 0)?;
    match &args[1] {
        Ty::Prim(p) if p.is_float() => {}
        Ty::FloatVar(_) | Ty::IntVar(_) | Ty::Unknown | Ty::Never => {}
        other => return Err(format!("argument 2 must be a float, found `{other}`")),
    }
    Ok(args[0].clone())
}

fn tensor_reduce(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(match want_tensor(args, 0)? {
        Some((elem, _)) => elem,
        None => Ty::Unknown,
    })
}

fn tensor_argmax(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_tensor(args, 0)?;
    Ok(Ty::Prim(Prim::U64))
}

fn tensor_dot(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (Some((e1, s1)), Some((e2, s2))) = (want_tensor(args, 0)?, want_tensor(args, 1)?) else {
        return Ok(Ty::Unknown);
    };
    if s1.len() != 1 || s2.len() != 1 {
        return Err("`dot` needs two rank-1 tensors; use `matmul` for matrices".into());
    }
    if !dims_match(&s1[0], &s2[0]) {
        return Err(format!(
            "`dot` operands differ in length: {} and {}",
            s1[0], s2[0]
        ));
    }
    if !e1.accepts(&e2) {
        return Err(format!("tensor element types differ: `{e1}` and `{e2}`"));
    }
    Ok(e1)
}

fn tensor_matmul(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (Some((e1, s1)), Some((e2, s2))) = (want_tensor(args, 0)?, want_tensor(args, 1)?) else {
        return Ok(Ty::Unknown);
    };
    if !e1.accepts(&e2) {
        return Err(format!("tensor element types differ: `{e1}` and `{e2}`"));
    }
    let inner_ok = |k1: &Dim, k2: &Dim| -> Result<(), String> {
        if dims_match(k1, k2) {
            Ok(())
        } else {
            Err(format!("`matmul` inner dimensions differ: {k1} and {k2}"))
        }
    };
    match (s1.as_slice(), s2.as_slice()) {
        ([m, k1], [k2, n]) => {
            inner_ok(k1, k2)?;
            Ok(Ty::Tensor {
                elem: Box::new(e1),
                shape: vec![m.clone(), n.clone()],
            })
        }
        ([m, k1], [k2]) => {
            inner_ok(k1, k2)?;
            Ok(Ty::Tensor {
                elem: Box::new(e1),
                shape: vec![m.clone()],
            })
        }
        _ => Err(format!(
            "`matmul` needs shapes [M, K] x [K, N] or [M, K] x [K], found {} and {}",
            shape_str(&s1),
            shape_str(&s2)
        )),
    }
}

fn tensor_transpose(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    match want_tensor(args, 0)? {
        None => Ok(Ty::Unknown),
        Some((elem, s)) if s.len() == 2 => Ok(Ty::Tensor {
            elem: Box::new(elem),
            shape: vec![s[1].clone(), s[0].clone()],
        }),
        Some((_, s)) => Err(format!(
            "`transpose` needs a rank-2 tensor, found {}",
            shape_str(&s)
        )),
    }
}

fn static_count(shape: &[Dim]) -> Option<u64> {
    shape.iter().try_fold(1u64, |acc, d| match d {
        Dim::Lit(n) => acc.checked_mul(*n),
        Dim::Named(_) => None,
    })
}

fn expect_tensor(expected: &Ty, what: &str) -> Result<(Ty, Vec<Dim>), String> {
    tensor_of(expected).ok_or_else(|| {
        format!("cannot infer the tensor type of `{what}`; annotate it, e.g. `let t: Tensor[f32, 3] = ...`")
    })
}

fn zeros_check(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 0)?;
    Ok(Ty::Unknown)
}

fn zeros_result(_: &[Ty], expected: &Ty) -> Result<Ty, String> {
    expect_tensor(expected, "std::tensor::zeros")?;
    Ok(expected.clone())
}

fn map_new_result(_: &[Ty], expected: &Ty) -> Result<Ty, String> {
    match expected {
        Ty::Map(..) | Ty::Unknown => Ok(expected.clone()),
        other => Err(format!(
            "`std.map.new()` and `{{}}` make a `Map[K, V]` but the context expects `{other}`; \
             write `let m: Map[Text, Int] = {{}}`"
        )),
    }
}

fn from_vec_check(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    elem(args, 0)?;
    Ok(Ty::Unknown)
}

fn from_vec_result(_: &[Ty], expected: &Ty) -> Result<Ty, String> {
    expect_tensor(expected, "std::tensor::from_vec")?;
    Ok(expected.clone())
}

fn reshape_check(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_tensor(args, 0)?;
    Ok(Ty::Unknown)
}

fn reshape_result(args: &[Ty], expected: &Ty) -> Result<Ty, String> {
    let (_, to) = expect_tensor(expected, "std::tensor::reshape")?;
    if let Some((_, from)) = tensor_of(&args[0])
        && let (Some(a), Some(b)) = (static_count(&from), static_count(&to))
        && a != b
    {
        return Err(format!(
            "cannot reshape {} ({a} elements) into {} ({b} elements)",
            shape_str(&from),
            shape_str(&to)
        ));
    }
    Ok(expected.clone())
}

fn to_vec(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(match want_tensor(args, 0)? {
        Some((elem, _)) => vec_of(elem),
        None => vec_of(Ty::Unknown),
    })
}

// ---- tables ------------------------------------------------------------------------------

fn want_table(args: &[Ty], i: usize) -> Result<Option<String>, String> {
    match &args[i] {
        Ty::Table(s) => Ok(Some(s.clone())),
        t if is_dynamic(t) => Ok(None),
        other => Err(format!(
            "argument {} must be a `Table`, found `{other}`",
            i + 1
        )),
    }
}

fn expect_table(expected: &Ty, what: &str) -> Result<Ty, String> {
    match expected {
        Ty::Table(_) => Ok(expected.clone()),
        _ => Err(format!(
            "cannot infer the schema of `{what}`; annotate the result, e.g. `let t: Table[Row] = ...`"
        )),
    }
}

fn read_check(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(Ty::Unknown)
}

fn read_result(_: &[Ty], expected: &Ty) -> Result<Ty, String> {
    expect_table(expected, "a table reader")
}

fn flight_read_check(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &s())?;
    want(args, 1, &s())?;
    Ok(Ty::Unknown)
}

fn flight_write_check(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 3)?;
    want_table(args, 0)?;
    want(args, 1, &s())?;
    want(args, 2, &s())?;
    Ok(Ty::Unit)
}

fn batches_check(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &s())?;
    want_int(args, 1)?;
    Ok(Ty::Unknown)
}

fn batches_result(_: &[Ty], expected: &Ty) -> Result<Ty, String> {
    match expected {
        Ty::Vec(row) if matches!(**row, Ty::Table(_)) => Ok(expected.clone()),
        _ => Err(
            "cannot infer the schema of a batch reader; annotate the result, e.g. `let parts: List[Table[Row]] = ...`"
                .to_string(),
        ),
    }
}

fn table_split(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let schema = want_table(args, 0)?;
    want_int(args, 1)?;
    Ok(Ty::Vec(Box::new(schema.map_or(Ty::Unknown, Ty::Table))))
}

fn test_assert(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &Ty::bool())?;
    Ok(Ty::Unit)
}

fn test_assert_msg(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &Ty::bool())?;
    want(args, 1, &s())?;
    Ok(Ty::Unit)
}

/// What a test names a function by: a std path as text, or the function itself.
fn mock_target(args: &[Ty]) -> Result<(), String> {
    match &args[0] {
        Ty::Fn(..) => Ok(()),
        t if s().accepts(t) => Ok(()),
        t => Err(format!(
            "argument 1 must be a function or the path of a std function as `Text`, found `{t}`"
        )),
    }
}

fn test_mock(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    mock_target(args)?;
    let Ty::Fn(..) = &args[1] else {
        return Err(format!(
            "argument 2 must be the function that takes its place, found `{}`",
            args[1]
        ));
    };
    if let Ty::Fn(..) = &args[0]
        && !args[0].accepts(&args[1])
    {
        return Err(format!(
            "the replacement must have the type of the function it replaces: `{}`, found `{}`",
            args[0], args[1]
        ));
    }
    Ok(Ty::Unit)
}

fn test_calls(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    mock_target(args)?;
    Ok(Ty::Prim(Prim::I64))
}

fn test_reset_mocks(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 0)?;
    Ok(Ty::Unit)
}

fn test_fail(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(Ty::Never)
}

fn debug_dbg(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    Ok(args[0].clone())
}

fn test_assert_eq(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    if args[0].accepts(&args[1]) || args[1].accepts(&args[0]) {
        Ok(Ty::Unit)
    } else {
        Err(format!(
            "cannot compare `{}` with `{}`: they are different types",
            args[0], args[1]
        ))
    }
}

fn env_get(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(Ty::Option(Box::new(s())))
}

fn fs_read(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(Ty::Result(Box::new(s()), Box::new(s())))
}

fn fs_write(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 0, &s())?;
    want(args, 1, &s())?;
    Ok(Ty::Result(Box::new(Ty::Unit), Box::new(s())))
}

fn fs_exists(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(Ty::bool())
}

fn json_stringify(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(s())
}

fn json_parse(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want(args, 0, &s())?;
    Ok(Ty::Unknown)
}

/// `std.json.parse(text)` gives a `Result[T, Text]`; `T` is the type the context asks for.
fn json_parse_result(_: &[Ty], expected: &Ty) -> Result<Ty, String> {
    match expected {
        Ty::Result(_, e) if e.accepts(&s()) => Ok(expected.clone()),
        Ty::Unknown => Ok(Ty::Unknown),
        _ => Err(
            "cannot infer the type of `std.json.parse`; annotate the result, e.g. \
                  `let q: Result[Query, Text] = std.json.parse(body)`"
                .to_string(),
        ),
    }
}

fn sql_check(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want_table(args, 0)?;
    want(args, 1, &s())?;
    Ok(Ty::Unknown)
}

fn sql_result(_: &[Ty], expected: &Ty) -> Result<Ty, String> {
    expect_table(expected, "std::table::sql")
}

fn from_rows(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    match &args[0] {
        Ty::Vec(t) => match &**t {
            Ty::Struct(s) => Ok(Ty::Table(s.clone())),
            Ty::Unknown => Ok(Ty::Unknown),
            other => Err(format!(
                "`from_rows` needs a `List` of types, found `List[{other}]`"
            )),
        },
        t if is_dynamic(t) => Ok(Ty::Unknown),
        other => Err(format!(
            "argument 1 must be a `List` of types, found `{other}`"
        )),
    }
}

fn to_rows(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(match want_table(args, 0)? {
        Some(s) => vec_of(Ty::Struct(s)),
        None => vec_of(Ty::Unknown),
    })
}

fn table_len(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    want_table(args, 0)?;
    Ok(Ty::Prim(Prim::U64))
}

fn write_table(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want_table(args, 0)?;
    want(args, 1, &s())?;
    Ok(Ty::Unit)
}

fn table_head(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let schema = want_table(args, 0)?;
    want(args, 1, &Ty::Prim(Prim::I64))?;
    Ok(schema.map_or(Ty::Unknown, Ty::Table))
}

fn table_sort(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let schema = want_table(args, 0)?;
    want(args, 1, &s())?;
    Ok(schema.map_or(Ty::Unknown, Ty::Table))
}

/// `select(t, ["a", "b"])`: the checker works out the result row from the column names.
fn table_select(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let schema = want_table(args, 0)?;
    want(args, 1, &vec_of(s()))?;
    Ok(schema.map_or(Ty::Unknown, Ty::Table))
}
fn table_concat(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let a = want_table(args, 0)?;
    let b = want_table(args, 1)?;
    match (a, b) {
        (Some(a), Some(b)) if a != b => Err(format!(
            "`concat` needs two tables with the same rows, found `Table[{a}]` and `Table[{b}]`"
        )),
        (Some(a), _) => Ok(Ty::Table(a)),
        (None, b) => Ok(b.map_or(Ty::Unknown, Ty::Table)),
    }
}

fn filter_rows_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    match (i, prev.first()) {
        (1, Some(Ty::Table(s))) => Some(Ty::Fn(vec![Ty::Struct(s.clone())], Box::new(Ty::bool()))),
        _ => None,
    }
}

fn filter_rows(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let schema = want_table(args, 0)?;
    match &args[1] {
        Ty::Fn(p, r) if p.len() == 1 && Ty::bool().accepts(r) => {}
        Ty::Unknown => {}
        other => {
            return Err(format!(
                "argument 2 must be `(row) -> Bool`, found `{other}`"
            ));
        }
    }
    Ok(schema.map_or(Ty::Unknown, Ty::Table))
}

// ---- Option and Result -------------------------------------------------------------------

fn opt_inner(args: &[Ty]) -> Result<Ty, String> {
    match &args[0] {
        Ty::Option(t) => Ok((**t).clone()),
        t if is_dynamic(t) => Ok(Ty::Unknown),
        other => Err(format!("the receiver must be an `Option`, found `{other}`")),
    }
}

fn res_parts(args: &[Ty]) -> Result<(Ty, Ty), String> {
    match &args[0] {
        Ty::Result(t, e) => Ok(((**t).clone(), (**e).clone())),
        t if is_dynamic(t) => Ok((Ty::Unknown, Ty::Unknown)),
        other => Err(format!("the receiver must be a `Result`, found `{other}`")),
    }
}

fn option_of(t: Ty) -> Ty {
    Ty::Option(Box::new(t))
}

fn result_of(t: Ty, e: Ty) -> Ty {
    Ty::Result(Box::new(t), Box::new(e))
}

/// The return type of a function argument that takes `params`; `Unknown` when not yet known.
fn fn_returning(args: &[Ty], i: usize, params: &[Ty], what: &str) -> Result<Ty, String> {
    match &args[i] {
        Ty::Fn(p, r) if p.len() == params.len() => {
            for (given, want) in p.iter().zip(params) {
                if !given.accepts(want) {
                    return Err(format!(
                        "the function takes `{given}` but `{what}` passes `{want}`"
                    ));
                }
            }
            Ok((**r).clone())
        }
        t if is_dynamic(t) => Ok(Ty::Unknown),
        other => Err(format!(
            "argument {} must be a function of {} argument(s), found `{other}`",
            i + 1,
            params.len()
        )),
    }
}

fn option_map_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    match (i, prev.first()) {
        (1, Some(Ty::Option(t))) => Some(Ty::Fn(vec![(**t).clone()], Box::new(Ty::Unknown))),
        _ => None,
    }
}

fn option_filter_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    match (i, prev.first()) {
        (1, Some(Ty::Option(t))) => Some(Ty::Fn(vec![(**t).clone()], Box::new(Ty::bool()))),
        _ => None,
    }
}

fn option_else_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    match (i, prev.first()) {
        (1, Some(Ty::Option(t))) => Some(Ty::Fn(vec![], t.clone())),
        _ => None,
    }
}

fn option_map(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let t = opt_inner(args)?;
    Ok(option_of(fn_returning(args, 1, &[t], "map")?))
}

fn option_and_then(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let t = opt_inner(args)?;
    match fn_returning(args, 1, &[t], "and_then")? {
        r @ Ty::Option(_) => Ok(r),
        r if is_dynamic(&r) => Ok(option_of(Ty::Unknown)),
        other => Err(format!(
            "the function must return an `Option`, found `{other}`; use `map` to wrap a plain value"
        )),
    }
}

fn option_filter(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let t = opt_inner(args)?;
    let r = fn_returning(args, 1, std::slice::from_ref(&t), "filter")?;
    if !Ty::bool().accepts(&r) {
        return Err(format!("the predicate must return `Bool`, found `{r}`"));
    }
    Ok(option_of(t))
}

fn option_ok_or(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    Ok(result_of(opt_inner(args)?, args[1].clone()))
}

fn option_unwrap_or_else(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let t = opt_inner(args)?;
    let r = fn_returning(args, 1, &[], "unwrap_or_else")?;
    if !t.accepts(&r) {
        return Err(format!(
            "the function returns `{r}` but the option holds `{t}`"
        ));
    }
    Ok(t)
}

fn option_expect(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 1, &s())?;
    opt_inner(args)
}

fn result_is(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    res_parts(args)?;
    Ok(Ty::bool())
}

fn result_unwrap(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(res_parts(args)?.0)
}

fn result_expect(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    want(args, 1, &s())?;
    Ok(res_parts(args)?.0)
}

fn result_unwrap_or(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (t, _) = res_parts(args)?;
    want(args, 1, &t)?;
    Ok(t)
}

fn result_ok(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(option_of(res_parts(args)?.0))
}

fn result_err(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 1)?;
    Ok(option_of(res_parts(args)?.1))
}

fn result_ok_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    match (i, prev.first()) {
        (1, Some(Ty::Result(t, _))) => Some(Ty::Fn(vec![(**t).clone()], Box::new(Ty::Unknown))),
        _ => None,
    }
}

fn result_err_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    match (i, prev.first()) {
        (1, Some(Ty::Result(_, e))) => Some(Ty::Fn(vec![(**e).clone()], Box::new(Ty::Unknown))),
        _ => None,
    }
}

fn result_else_hint(i: usize, prev: &[Ty]) -> Option<Ty> {
    match (i, prev.first()) {
        (1, Some(Ty::Result(t, e))) => Some(Ty::Fn(vec![(**e).clone()], t.clone())),
        _ => None,
    }
}

fn result_map(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (t, e) = res_parts(args)?;
    Ok(result_of(fn_returning(args, 1, &[t], "map")?, e))
}

fn result_map_err(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (t, e) = res_parts(args)?;
    Ok(result_of(t, fn_returning(args, 1, &[e], "map_err")?))
}

fn result_and_then(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (t, e) = res_parts(args)?;
    match fn_returning(args, 1, &[t], "and_then")? {
        Ty::Result(u, e2) => {
            if !e.accepts(&e2) {
                return Err(format!(
                    "the function fails with `{e2}` but this `Result` fails with `{e}`; use `map_err` to convert one"
                ));
            }
            Ok(result_of(*u, e))
        }
        r if is_dynamic(&r) => Ok(result_of(Ty::Unknown, e)),
        other => Err(format!(
            "the function must return a `Result`, found `{other}`; use `map` to wrap a plain value"
        )),
    }
}

fn result_unwrap_or_else(args: &[Ty]) -> Result<Ty, String> {
    arity(args, 2)?;
    let (t, e) = res_parts(args)?;
    let r = fn_returning(args, 1, &[e], "unwrap_or_else")?;
    if !t.accepts(&r) {
        return Err(format!("the function returns `{r}` but the value is `{t}`"));
    }
    Ok(t)
}

const fn hinted(check: Check, hint: Hint) -> Sig {
    Sig {
        check,
        hint,
        from_expected: None,
    }
}

const FUNCTIONS: &[(&str, Sig)] = &[
    ("std::text::clean_whitespace", sig(str_to_str)),
    ("std::text::trim", sig(str_to_str)),
    ("std::text::lowercase", sig(str_to_str)),
    ("std::text::uppercase", sig(str_to_str)),
    ("std::text::truncate", sig(truncate)),
    ("std::text::split", sig(split)),
    ("std::string::join", sig(join)),
    (
        "std::iter::map",
        Sig {
            check: map,
            hint: map_hint,
            from_expected: None,
        },
    ),
    (
        "std::iter::filter",
        Sig {
            check: filter,
            hint: filter_hint,
            from_expected: None,
        },
    ),
    (
        "std::dist::map",
        Sig {
            check: map,
            hint: map_hint,
            from_expected: None,
        },
    ),
    (
        "std::iter::any",
        Sig {
            check: any_all,
            hint: filter_hint,
            from_expected: None,
        },
    ),
    (
        "std::iter::all",
        Sig {
            check: any_all,
            hint: filter_hint,
            from_expected: None,
        },
    ),
    (
        "std::iter::find",
        Sig {
            check: find,
            hint: filter_hint,
            from_expected: None,
        },
    ),
    (
        "std::iter::for_each",
        Sig {
            check: for_each,
            hint: for_each_hint,
            from_expected: None,
        },
    ),
    (
        "std::iter::fold",
        Sig {
            check: fold,
            hint: fold_hint,
            from_expected: None,
        },
    ),
    ("std::option::map", hinted(option_map, option_map_hint)),
    (
        "std::option::and_then",
        hinted(option_and_then, option_map_hint),
    ),
    (
        "std::option::filter",
        hinted(option_filter, option_filter_hint),
    ),
    ("std::option::ok_or", sig(option_ok_or)),
    (
        "std::option::unwrap_or_else",
        hinted(option_unwrap_or_else, option_else_hint),
    ),
    ("std::option::expect", sig(option_expect)),
    ("std::result::is_ok", sig(result_is)),
    ("std::result::is_err", sig(result_is)),
    ("std::result::unwrap", sig(result_unwrap)),
    ("std::result::expect", sig(result_expect)),
    ("std::result::unwrap_or", sig(result_unwrap_or)),
    (
        "std::result::unwrap_or_else",
        hinted(result_unwrap_or_else, result_else_hint),
    ),
    ("std::result::map", hinted(result_map, result_ok_hint)),
    (
        "std::result::map_err",
        hinted(result_map_err, result_err_hint),
    ),
    (
        "std::result::and_then",
        hinted(result_and_then, result_ok_hint),
    ),
    ("std::result::ok", sig(result_ok)),
    ("std::result::err", sig(result_err)),
    ("std::iter::enumerate", sig(enumerate)),
    ("std::iter::zip", sig(zip)),
    ("std::iter::sum", sig(sum)),
    ("std::iter::min", sig(min_max)),
    ("std::iter::max", sig(min_max)),
    ("std::iter::skip", sig(skip)),
    ("std::iter::take", sig(take)),
    ("std::iter::count", sig(count)),
    ("std::task::gather", sig(task_gather)),
    ("std::task::run", sig(task_run)),
    ("std::task::sleep", sig(task_sleep)),
    ("std::time::sleep", sig(time_sleep)),
    ("std::time::now", sig(time_now)),
    ("std::iter::slice", sig(iter_slice)),
    ("std::iter::concat", sig(iter_concat)),
    ("std::iter::range", sig(iter_range)),
    ("std::string::slice", sig(string_slice)),
    ("std::string::repeat", sig(string_repeat)),
    ("std::text::to_text", sig(text_to_text)),
    ("std::text::format", sig(text_format)),
    ("std::math::pow", sig(math_pow)),
    (
        "std::math::floor_div",
        hinted(math_binary, same_as_first_hint),
    ),
    ("std::math::min", hinted(math_min_max, same_as_first_hint)),
    ("std::math::max", hinted(math_min_max, same_as_first_hint)),
    ("std::math::abs", sig(math_abs)),
    ("std::math::sqrt", sig(math_float_fn)),
    ("std::math::floor", sig(math_float_fn)),
    ("std::math::ceil", sig(math_float_fn)),
    ("std::math::round", sig(math_float_fn)),
    ("std::math::to_float", sig(math_to_float)),
    ("std::math::to_int", sig(math_to_int)),
    ("std::map::from_entries", sig(map_from_entries)),
    ("std::collect", sig(collect)),
    ("std::map::new", sig_expected(zeros_check, map_new_result)),
    (
        "std::tensor::zeros",
        sig_expected(zeros_check, zeros_result),
    ),
    (
        "std::tensor::from_vec",
        sig_expected(from_vec_check, from_vec_result),
    ),
    (
        "std::tensor::reshape",
        sig_expected(reshape_check, reshape_result),
    ),
    ("std::tensor::to_vec", sig(to_vec)),
    ("std::tensor::add", sig(elementwise)),
    ("std::tensor::sub", sig(elementwise)),
    ("std::tensor::mul", sig(elementwise)),
    ("std::tensor::div", sig(elementwise)),
    ("std::tensor::scale", sig(tensor_scale)),
    ("std::tensor::relu", sig(tensor_unary)),
    ("std::tensor::sigmoid", sig(tensor_unary)),
    ("std::tensor::softmax", sig(tensor_unary)),
    ("std::tensor::sum", sig(tensor_reduce)),
    ("std::tensor::mean", sig(tensor_reduce)),
    ("std::tensor::argmax", sig(tensor_argmax)),
    ("std::tensor::dot", sig(tensor_dot)),
    ("std::tensor::matmul", sig(tensor_matmul)),
    ("std::tensor::transpose", sig(tensor_transpose)),
    (
        "std::table::read_csv",
        sig_expected(read_check, read_result),
    ),
    (
        "std::table::read_parquet",
        sig_expected(read_check, read_result),
    ),
    (
        "std::table::read_ipc",
        sig_expected(read_check, read_result),
    ),
    (
        "std::table::read_flight",
        sig_expected(flight_read_check, read_result),
    ),
    (
        "std::table::read_csv_batches",
        sig_expected(batches_check, batches_result),
    ),
    (
        "std::table::read_parquet_batches",
        sig_expected(batches_check, batches_result),
    ),
    (
        "std::table::read_ipc_batches",
        sig_expected(batches_check, batches_result),
    ),
    ("std::table::sql", sig_expected(sql_check, sql_result)),
    ("std::test::assert", sig(test_assert)),
    ("std::test::assert_eq", sig(test_assert_eq)),
    ("std::test::assert_msg", sig(test_assert_msg)),
    ("std::test::fail", sig(test_fail)),
    ("std::test::mock", sig(test_mock)),
    ("std::test::calls", sig(test_calls)),
    ("std::test::reset_mocks", sig(test_reset_mocks)),
    ("std::debug::dbg", sig(debug_dbg)),
    ("std::env::get", sig(env_get)),
    ("std::fs::read", sig(fs_read)),
    ("std::fs::write", sig(fs_write)),
    ("std::fs::exists", sig(fs_exists)),
    ("std::json::stringify", sig(json_stringify)),
    (
        "std::json::parse",
        sig_expected(json_parse, json_parse_result),
    ),
    ("std::table::from_rows", sig(from_rows)),
    ("std::table::to_rows", sig(to_rows)),
    ("std::table::len", sig(table_len)),
    ("std::table::write_csv", sig(write_table)),
    ("std::table::write_parquet", sig(write_table)),
    ("std::table::write_ipc", sig(write_table)),
    ("std::table::write_flight", sig(flight_write_check)),
    ("std::table::head", sig(table_head)),
    ("std::table::tail", sig(table_head)),
    ("std::table::sort_by", sig(table_sort)),
    ("std::table::sort_by_desc", sig(table_sort)),
    ("std::table::concat", sig(table_concat)),
    ("std::table::select", sig(table_select)),
    ("std::table::split", sig(table_split)),
    (
        "std::table::filter",
        Sig {
            check: filter_rows,
            hint: filter_rows_hint,
            from_expected: None,
        },
    ),
    ("std::io::print", sig(print)),
    ("std::io::println", sig(print)),
    ("std::http::Status::BadRequest", sig(status)),
    ("std::http::Status::Unauthorized", sig(status)),
    ("std::http::Status::Forbidden", sig(status)),
    ("std::http::Status::NotFound", sig(status)),
    ("std::http::Status::Unprocessable", sig(status)),
    ("std::http::Status::Internal", sig(status)),
    (
        "std::ai::embeddings::OllamaEmbedder::new",
        sig(new_embedder),
    ),
    ("std::ai::llm::OpenAIClient::new", sig(new_llm)),
    ("std::db::pgvector::PgVectorClient::connect", sig(connect)),
];

const METHODS: &[(&str, &str, Sig)] = &[
    ("OllamaEmbedder", "embed", sig(embed)),
    ("OpenAIClient", "complete", sig(complete)),
    (
        "PgVectorClient",
        "search_collection",
        sig(search_collection),
    ),
    ("PgVectorClient", "with_vector", sig(with_vector)),
    ("PgVectorClient", "limit", sig(limit)),
    ("PgVectorClient", "execute", sig(execute)),
    ("PgVectorClient", "insert", sig(insert)),
];

/// Importable std types: full path -> short name.
const TYPES: &[&str] = &[
    "std::http::Request",
    "std::http::Response",
    "std::http::Status",
    "std::ai::embeddings::OllamaEmbedder",
    "std::ai::llm::OpenAIClient",
    "std::db::pgvector::PgVectorClient",
    "std::db::pgvector::VectorQuery",
];

pub fn function(path: &str) -> Option<Sig> {
    FUNCTIONS.iter().find(|(p, _)| *p == path).map(|(_, s)| *s)
}

pub fn method(ty: &str, name: &str) -> Option<Sig> {
    METHODS
        .iter()
        .find(|(t, n, _)| *t == ty && *n == name)
        .map(|(_, _, s)| *s)
}

/// Whether `short_name` is a std type that has methods (so unknown methods are errors).
pub fn is_std_type(short_name: &str) -> bool {
    TYPES
        .iter()
        .any(|p| p.rsplit("::").next() == Some(short_name))
}

pub fn is_type(path: &str) -> bool {
    TYPES.contains(&path)
}

/// Whether `path` names a std module (a proper prefix of some item).
pub fn is_module(path: &str) -> bool {
    let prefix = format!("{path}::");
    path == "std"
        || FUNCTIONS.iter().any(|(p, _)| p.starts_with(&prefix))
        || TYPES.iter().any(|p| p.starts_with(&prefix))
}

/// Every std function path, for runtimes that must implement them all.
pub fn function_paths() -> impl Iterator<Item = &'static str> {
    FUNCTIONS.iter().map(|(p, _)| *p)
}

pub fn method_names() -> impl Iterator<Item = (&'static str, &'static str)> {
    METHODS.iter().map(|(t, n, _)| (*t, *n))
}

pub fn type_paths() -> impl Iterator<Item = &'static str> {
    TYPES.iter().copied()
}

/// What a name inside a std module refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    Module,
    Function,
    Type,
}

/// The direct children of the std path `prefix` (e.g. `std.text`), sorted, without duplicates.
pub fn children(prefix: &str) -> Vec<(String, ItemKind)> {
    let start = format!("{prefix}::");
    let mut out: Vec<(String, ItemKind)> = Vec::new();
    let mut add = |path: &str, leaf: ItemKind| {
        let Some(rest) = path.strip_prefix(&start) else {
            return;
        };
        let (name, kind) = match rest.split_once("::") {
            Some((head, _)) => (head, ItemKind::Module),
            None => (rest, leaf),
        };
        if !out.iter().any(|(n, _)| n == name) {
            out.push((name.to_string(), kind));
        }
    };
    for p in function_paths() {
        add(p, ItemKind::Function);
    }
    for p in type_paths() {
        add(p, ItemKind::Type);
    }
    out.sort();
    out
}

/// Methods available on the std type `ty`, with their names.
pub fn methods_of(ty: &str) -> Vec<&'static str> {
    METHODS
        .iter()
        .filter(|(t, _, _)| *t == ty)
        .map(|(_, n, _)| *n)
        .collect()
}

/// A method built into the language: its name and how it reads.
pub type BuiltinMethod = (&'static str, &'static str);

const TEXT_METHODS: &[BuiltinMethod] = &[
    ("is_empty", "() -> Bool"),
    ("len", "() -> u64"),
    ("trim", "() -> Text"),
    ("to_lowercase", "() -> Text"),
    ("to_uppercase", "() -> Text"),
    ("contains", "(part: Text) -> Bool"),
    ("starts_with", "(prefix: Text) -> Bool"),
    ("ends_with", "(suffix: Text) -> Bool"),
    ("replace", "(from: Text, to: Text) -> Text"),
    ("split", "(sep: Text) -> List[Text]"),
    ("find", "(part: Text) -> u64?"),
    ("to_int", "() -> i64?"),
    ("to_float", "() -> f64?"),
    ("chars", "() -> List[Text]"),
    ("lines", "() -> List[Text]"),
    ("slice", "(start, end) -> Text"),
];

const LIST_METHODS: &[BuiltinMethod] = &[
    ("is_empty", "() -> Bool"),
    ("len", "() -> u64"),
    ("push", "(x: T)"),
    ("contains", "(x: T) -> Bool"),
    ("first", "() -> T?"),
    ("last", "() -> T?"),
    ("pop", "() -> T?"),
    ("get", "(i) -> T?"),
    ("sort", "()"),
    ("reverse", "()"),
    ("map", "(f: (T) -> U) -> List[U]"),
    ("filter", "(f: (T) -> Bool) -> List[T]"),
    ("take", "(n) -> List[T]"),
    ("skip", "(n) -> List[T]"),
    ("count", "(f: (T) -> Bool) -> u64"),
    ("any", "(f: (T) -> Bool) -> Bool"),
    ("all", "(f: (T) -> Bool) -> Bool"),
    ("find", "(f: (T) -> Bool) -> T?"),
    ("for_each", "(f: (T) -> ())"),
    ("fold", "(init: A, f: (A, T) -> A) -> A"),
    ("enumerate", "() -> List[(u64, T)]"),
    ("zip", "(other: List[U]) -> List[(T, U)]"),
    ("sum", "() -> T"),
    ("min", "() -> T?"),
    ("max", "() -> T?"),
    ("join", "(sep: Text) -> Text"),
    ("slice", "(start, end) -> List[T]"),
];

const OPTION_METHODS: &[BuiltinMethod] = &[
    ("is_some", "() -> Bool"),
    ("is_none", "() -> Bool"),
    ("unwrap", "() -> T"),
    ("unwrap_or", "(default: T) -> T"),
    ("unwrap_or_else", "(f: () -> T) -> T"),
    ("expect", "(message: Text) -> T"),
    ("map", "(f: (T) -> U) -> U?"),
    ("and_then", "(f: (T) -> U?) -> U?"),
    ("filter", "(f: (T) -> Bool) -> T?"),
    ("ok_or", "(error: E) -> Result[T, E]"),
];

const RESULT_METHODS: &[BuiltinMethod] = &[
    ("is_ok", "() -> Bool"),
    ("is_err", "() -> Bool"),
    ("unwrap", "() -> T"),
    ("expect", "(message: Text) -> T"),
    ("unwrap_or", "(default: T) -> T"),
    ("unwrap_or_else", "(f: (E) -> T) -> T"),
    ("map", "(f: (T) -> U) -> Result[U, E]"),
    ("map_err", "(f: (E) -> F) -> Result[T, F]"),
    ("and_then", "(f: (T) -> Result[U, E]) -> Result[U, E]"),
    ("ok", "() -> T?"),
    ("err", "() -> E?"),
];

const MAP_METHODS: &[BuiltinMethod] = &[
    ("insert", "(key: K, value: V)"),
    ("get", "(key: K) -> V?"),
    ("remove", "(key: K) -> V?"),
    ("contains_key", "(key: K) -> Bool"),
    ("len", "() -> u64"),
    ("is_empty", "() -> Bool"),
    ("keys", "() -> List[K]"),
    ("values", "() -> List[V]"),
    ("entries", "() -> List[(K, V)]"),
];

const TABLE_METHODS: &[BuiltinMethod] = &[
    ("len", "() -> u64"),
    ("is_empty", "() -> Bool"),
    ("sql", "(query: Text) -> Table"),
    ("filter", "(keep: (row) -> Bool) -> Table"),
    ("head", "(n: Int) -> Table"),
    ("tail", "(n: Int) -> Table"),
    ("sort_by", "(column: Text) -> Table"),
    ("sort_by_desc", "(column: Text) -> Table"),
    ("concat", "(other: Table) -> Table"),
    ("split", "(rows: Int) -> List[Table]"),
    ("select", "(columns: List[Text]) -> Table"),
];

const TENSOR_METHODS: &[BuiltinMethod] = &[
    ("add", "(other) -> Tensor"),
    ("sub", "(other) -> Tensor"),
    ("mul", "(other) -> Tensor"),
    ("div", "(other) -> Tensor"),
    ("scale", "(k) -> Tensor"),
    ("relu", "() -> Tensor"),
    ("sigmoid", "() -> Tensor"),
    ("softmax", "() -> Tensor"),
    ("sum", "() -> number"),
    ("mean", "() -> number"),
    ("argmax", "() -> u64"),
    ("dot", "(other) -> number"),
    ("matmul", "(other) -> Tensor"),
    ("transpose", "() -> Tensor"),
    ("reshape", "(shape) -> Tensor"),
    ("to_vec", "() -> List"),
];

/// The methods the language gives every value of `ty` (not those the user declares), in the order
/// the type checker lists them when a name is misspelled.
pub fn builtin_methods(ty: &Ty) -> &'static [BuiltinMethod] {
    match ty {
        Ty::Prim(p) if p.is_text() => TEXT_METHODS,
        Ty::Vec(_) => LIST_METHODS,
        Ty::Option(_) => OPTION_METHODS,
        Ty::Result(..) => RESULT_METHODS,
        Ty::Map(..) => MAP_METHODS,
        Ty::Table(_) => TABLE_METHODS,
        Ty::Tensor { .. } => TENSOR_METHODS,
        _ => &[],
    }
}

/// Just the names of [`builtin_methods`].
pub fn builtin_method_names(ty: &Ty) -> Vec<&'static str> {
    builtin_methods(ty).iter().map(|(n, _)| *n).collect()
}
