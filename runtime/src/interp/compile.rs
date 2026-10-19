//! Function bodies compiled to trees of closures.
//!
//! The tree walker looks at the kind of every node each time it runs it, looks a function up by its
//! name at every call and checks its hooks at every statement. Here a body is turned once, the
//! first time it is called, into closures that already know what they are going to do: the callee
//! of a call is an index, a std path is a string made once, a number literal is a value. The
//! closures work on the same `Ctx` as the tree walker, so a node that is not compiled here runs
//! through `Interp::eval` and the two mix freely.
//!
//! Compiled code is used only when nothing is watching the run (no profiler, debugger, coverage or
//! step limit): those hooks live in the tree walker.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use telvax_hir::Ty;
use telvax_hir::hir::{self, BinOp, ExprKind, Lit, LocalId, UnOp};

use super::{Ctrl, Ctx, Eval, Interp, Step, fail, float_op, int_op, match_pattern, overflow};
use crate::interp::RuntimeError;
use crate::std_impl;
use crate::value::{Args, Closure, Env, MapKey, Native, StructVal, Value};

/// A compiled expression.
pub(super) type Code = Box<dyn Fn(&Interp, &mut Ctx) -> Eval + Send + Sync>;

/// Compiles the body of the function called `name`.
pub(super) fn function(it: &Interp, name: &str) -> Code {
    let Some(f) = it.fns.get(name) else {
        let name = name.to_string();
        return Box::new(move |_, _| fail(format!("no function `{name}`")));
    };
    block(it, &f.body)
}

fn unbound() -> Ctrl {
    Ctrl::Fail(RuntimeError::new("unbound local"))
}

/// Runs the tree walker on a node that is not compiled.
fn fallback(e: &hir::Expr) -> Code {
    let e = e.clone();
    Box::new(move |it, ctx| it.eval(&e, ctx))
}

fn block(it: &Interp, b: &hir::Block) -> Code {
    enum Step {
        Let(LocalId, Code),
        Expr(Code),
    }
    let steps: Vec<Step> = b
        .stmts
        .iter()
        .map(|s| match s {
            hir::Stmt::Let { local, init } => Step::Let(local.id, expr(it, init)),
            hir::Stmt::Expr(e) => Step::Expr(expr(it, e)),
        })
        .collect();
    let tail = b.tail.as_ref().map(|t| expr(it, t));
    Box::new(move |it, ctx| {
        for s in &steps {
            match s {
                Step::Let(id, init) => {
                    let v = init(it, ctx)?;
                    ctx.env.insert(*id, v);
                }
                Step::Expr(e) => {
                    e(it, ctx)?;
                }
            }
        }
        match &tail {
            Some(t) => t(it, ctx),
            None => Ok(Value::Unit),
        }
    })
}

/// A name, or a field of one: something that can be looked at without running anything.
enum Place {
    Local(LocalId),
    Global(String),
    Field(Box<Place>, String),
}

impl Place {
    /// The value it holds, without copying it.
    #[inline]
    fn get<'a>(&'a self, it: &'a Interp, ctx: &'a Ctx) -> Option<&'a Value> {
        match self {
            Place::Local(id) => ctx.env.get(id),
            Place::Global(name) => it.frozen_globals.get().and_then(|all| all.get(name)),
            Place::Field(base, name) => match base.get(it, ctx)? {
                Value::Struct(s) => s.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
                _ => None,
            },
        }
    }
}

fn peek(e: &hir::Expr) -> Option<Place> {
    match &e.kind {
        ExprKind::Local(id) => Some(Place::Local(*id)),
        ExprKind::Global(name) => Some(Place::Global(name.clone())),
        ExprKind::Field { base, name } => Some(Place::Field(Box::new(peek(base)?), name.clone())),
        _ => None,
    }
}

/// The field `name` of a value.
fn field_of(v: &Value, name: &str) -> Eval {
    match v {
        Value::Table(t) => Ok(t.column(name)?),
        Value::Struct(s) => s
            .fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| {
                Ctrl::Fail(RuntimeError::new(format!(
                    "{} has no field `{name}`",
                    s.name
                )))
            }),
        Value::Tuple(items) => name
            .parse::<usize>()
            .ok()
            .and_then(|i| items.get(i).cloned())
            .ok_or_else(|| Ctrl::Fail(RuntimeError::new(format!("tuple has no part `{name}`")))),
        other => fail(format!("{} has no field `{name}`", other.type_name())),
    }
}

fn args(it: &Interp, list: &[hir::Expr]) -> Vec<Code> {
    list.iter().map(|a| expr(it, a)).collect()
}

fn eval_args(codes: &[Code], it: &Interp, ctx: &mut Ctx) -> Result<Args, Ctrl> {
    let mut values = Args::new();
    for c in codes {
        values.push(c(it, ctx)?);
    }
    Ok(values)
}

pub(super) fn expr(it: &Interp, e: &hir::Expr) -> Code {
    match &e.kind {
        ExprKind::Lit(l) => literal(e, l),
        ExprKind::Local(id) => {
            let id = *id;
            Box::new(move |_, ctx| ctx.env.get(&id).cloned().ok_or_else(unbound))
        }
        ExprKind::Global(name) => {
            let name = name.clone();
            Box::new(move |it, _| {
                match it.frozen_globals.get() {
                    Some(all) => all.get(&name).cloned(),
                    None => it
                        .globals
                        .read()
                        .unwrap_or_else(|e| e.into_inner())
                        .get(&name)
                        .cloned(),
                }
                .ok_or_else(|| {
                    Ctrl::Fail(RuntimeError::new(format!(
                        "global `{name}` is not initialised yet"
                    )))
                })
            })
        }
        ExprKind::Field { base, name } => {
            let name = name.clone();
            let base_code = expr(it, base);
            match peek(base) {
                Some(place) => Box::new(move |it, ctx| match place.get(it, ctx) {
                    Some(v) => field_of(v, &name),
                    None => field_of(&base_code(it, ctx)?, &name),
                }),
                None => Box::new(move |it, ctx| field_of(&base_code(it, ctx)?, &name)),
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let (op, inner) = (*op, expr(it, inner));
            Box::new(move |it, ctx| match (op, inner(it, ctx)?) {
                (UnOp::Neg, Value::Int(i)) => i.checked_neg().map(Value::Int).ok_or_else(overflow),
                (UnOp::Neg, Value::Float(x)) => Ok(Value::Float(-x)),
                (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                (op, v) => fail(format!("cannot apply {op:?} to {}", v.type_name())),
            })
        }
        ExprKind::Binary { op, lhs, rhs } => binary(it, e, *op, lhs, rhs),
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            let cond = expr(it, cond);
            let then = block(it, then);
            let otherwise = otherwise.as_ref().map(|o| expr(it, o));
            Box::new(move |it, ctx| match cond(it, ctx)? {
                Value::Bool(true) => then(it, ctx),
                Value::Bool(false) => match &otherwise {
                    Some(o) => o(it, ctx),
                    None => Ok(Value::Unit),
                },
                other => fail(format!("`if` condition is {}, not bool", other.type_name())),
            })
        }
        ExprKind::Block(b) => block(it, b),
        ExprKind::Return(v) => {
            let v = v.as_ref().map(|v| expr(it, v));
            Box::new(move |it, ctx| {
                let v = match &v {
                    Some(v) => v(it, ctx)?,
                    None => Value::Unit,
                };
                Err(Ctrl::Return(v))
            })
        }
        ExprKind::Break => Box::new(|_, _| Err(Ctrl::Break)),
        ExprKind::Continue => Box::new(|_, _| Err(Ctrl::Continue)),
        ExprKind::Try(inner) => {
            let inner = expr(it, inner);
            Box::new(move |it, ctx| match inner(it, ctx)? {
                Value::Ok(v) => Ok(*v),
                Value::Err(e) => Err(Ctrl::Return(Value::Err(e))),
                Value::Enum(e) if e.name == "Option" => match e.fields.first() {
                    Some(v) => Ok(v.clone()),
                    None => Err(Ctrl::Return(Value::none())),
                },
                other => fail(format!(
                    "`?` needs a Result or an Option, found {}",
                    other.type_name()
                )),
            })
        }
        ExprKind::Tuple(items) => {
            let items = args(it, items);
            Box::new(move |it, ctx| {
                let mut values = Vec::with_capacity(items.len());
                for item in &items {
                    values.push(item(it, ctx)?);
                }
                Ok(Value::Tuple(Arc::new(values)))
            })
        }
        ExprKind::Array(items) => {
            let items = args(it, items);
            Box::new(move |it, ctx| {
                let mut out = Vec::with_capacity(items.len());
                for item in &items {
                    out.push(item(it, ctx)?);
                }
                Ok(Value::vec(out))
            })
        }
        ExprKind::Match { scrutinee, arms } => matching(it, scrutinee, arms),
        ExprKind::Assign { place, value } => assign(it, place, value),
        ExprKind::While { cond, body } => {
            let cond = expr(it, cond);
            let body = block(it, body);
            Box::new(move |it, ctx| {
                loop {
                    match cond(it, ctx)? {
                        Value::Bool(true) => match body(it, ctx) {
                            Ok(_) | Err(Ctrl::Continue) => {}
                            Err(Ctrl::Break) => break,
                            Err(other) => return Err(other),
                        },
                        Value::Bool(false) => break,
                        other => {
                            return fail(format!(
                                "`while` condition is {}, not bool",
                                other.type_name()
                            ));
                        }
                    }
                }
                Ok(Value::Unit)
            })
        }
        ExprKind::For { var, iter, body } => looping(it, var.id, iter, body),
        ExprKind::Index { base, index } => indexing(it, base, index),
        ExprKind::Call { callee, args } => call(it, e, callee, args),
        ExprKind::Closure { params, body } => {
            let ids: Arc<[LocalId]> = params.iter().map(|p| p.id).collect();
            let body: Arc<hir::Expr> = Arc::new((**body).clone());
            let code = super::ClosureCode(Arc::new(expr(it, &body)));
            Box::new(move |_, ctx| {
                Ok(Value::Closure(Arc::new(Closure {
                    params: ids.clone(),
                    body: body.clone(),
                    env: ctx.env.clone(),
                    code: Some(code.clone()),
                })))
            })
        }
        ExprKind::StructLit { name, fields } => structure(it, e, name, fields),
        ExprKind::EnumCtor {
            enum_name,
            variant,
            args: list,
        } => {
            let index = it
                .enums
                .get(enum_name)
                .and_then(|vs| vs.iter().position(|(v, _)| v == variant))
                .unwrap_or(0) as u32;
            let (codes, enum_name, variant) = (args(it, list), enum_name.clone(), variant.clone());
            Box::new(move |it, ctx| {
                let mut fields = Vec::with_capacity(codes.len());
                for c in &codes {
                    fields.push(c(it, ctx)?);
                }
                Ok(Value::Enum(Arc::new(crate::value::EnumVal {
                    name: enum_name.clone().into(),
                    variant: variant.clone().into(),
                    index,
                    fields: fields.into(),
                })))
            })
        }
        ExprKind::FString(parts) => formatted(it, parts),
        _ => fallback(e),
    }
}

fn literal(e: &hir::Expr, l: &Lit) -> Code {
    let value = match l {
        Lit::Int(n) => match &e.ty {
            Ty::Prim(p) if p.is_float() => Some(super::round_to(&e.ty, Value::Float(*n as f64))),
            _ => i64::try_from(*n).ok().map(Value::Int),
        },
        Lit::Float(x) => Some(super::round_to(&e.ty, Value::Float(*x))),
        Lit::Bool(b) => Some(Value::Bool(*b)),
        Lit::Str(s) => {
            // A new `Text` every time: a shared one would make every thread write to its reference count.
            let s = s.clone();
            return Box::new(move |_, _| Ok(Value::str(s.as_str())));
        }
    };
    match value {
        Some(v) => Box::new(move |_, _| Ok(v.clone())),
        None => fallback(e),
    }
}

fn binary(it: &Interp, e: &hir::Expr, op: BinOp, lhs: &hir::Expr, rhs: &hir::Expr) -> Code {
    let f32_result = matches!(&e.ty, Ty::Prim(telvax_hir::Prim::F32));
    let round = move |v: Value| {
        if f32_result {
            super::round_to(&Ty::Prim(telvax_hir::Prim::F32), v)
        } else {
            v
        }
    };
    // A text literal on the right of `==`, `!=` or `+` is used as it is.
    if let ExprKind::Lit(Lit::Str(lit)) = &rhs.kind
        && matches!(op, BinOp::Eq | BinOp::NotEq | BinOp::Add)
    {
        let lit = lit.clone();
        let place = peek(lhs);
        let left = expr(it, lhs);
        return Box::new(move |it, ctx| {
            if matches!(op, BinOp::Eq | BinOp::NotEq)
                && let Some(place) = &place
                && let Some(Value::Str(a)) = place.get(it, ctx)
            {
                return Ok(Value::Bool((**a == *lit) == (op == BinOp::Eq)));
            }
            let l = left(it, ctx)?;
            if let Value::Str(a) = &l {
                return Ok(match op {
                    BinOp::Eq => Value::Bool(**a == *lit),
                    BinOp::NotEq => Value::Bool(**a != *lit),
                    _ => Value::concat(a, &lit),
                });
            }
            it.binary_values(op, l, Value::str(lit.as_str())).map(round)
        });
    }
    // Both sides are names, or one is a number written in the program: they are looked at in place,
    // with no copy of the value and no drop of it after.
    let (lplace, rplace) = (peek(lhs), peek(rhs));
    let rconst = match &rhs.kind {
        ExprKind::Lit(Lit::Int(n)) if !matches!(&rhs.ty, Ty::Prim(p) if p.is_float()) => {
            i64::try_from(*n).ok().map(Value::Int)
        }
        ExprKind::Lit(Lit::Float(x)) => Some(Value::Float(*x)),
        _ => None,
    };
    let (left, right) = (expr(it, lhs), expr(it, rhs));
    Box::new(move |it, ctx| {
        if let Some(lp) = &lplace
            && let Some(a) = lp.get(it, ctx)
        {
            let quick = match (&rconst, &rplace) {
                (Some(b), _) => in_place(op, a, b),
                (None, Some(rp)) => rp.get(it, ctx).and_then(|b| in_place(op, a, b)),
                _ => None,
            };
            if let Some(v) = quick {
                return v.map(round);
            }
        }
        let l = left(it, ctx)?;
        match (op, &l) {
            (BinOp::And, Value::Bool(false)) => return Ok(Value::Bool(false)),
            (BinOp::Or, Value::Bool(true)) => return Ok(Value::Bool(true)),
            _ => {}
        }
        let r = right(it, ctx)?;
        // The common cases, with no further matching.
        match (&l, &r, op) {
            (Value::Int(a), Value::Int(b), BinOp::Eq) => return Ok(Value::Bool(a == b)),
            (Value::Int(a), Value::Int(b), BinOp::NotEq) => return Ok(Value::Bool(a != b)),
            (Value::Float(a), Value::Float(b), BinOp::Eq) => return Ok(Value::Bool(a == b)),
            (Value::Float(a), Value::Float(b), BinOp::NotEq) => return Ok(Value::Bool(a != b)),
            (Value::Int(a), Value::Int(b), op) if !matches!(op, BinOp::And | BinOp::Or) => {
                return int_op(op, *a, *b).map(round);
            }
            (Value::Float(a), Value::Float(b), op) if !matches!(op, BinOp::And | BinOp::Or) => {
                return float_op(op, *a, *b).map(round);
            }
            _ => {}
        }
        it.binary_values(op, l, r).map(round)
    })
}

fn matching(it: &Interp, scrutinee: &hir::Expr, arms: &[hir::Arm]) -> Code {
    struct Arm {
        pat: hir::Pat,
        guard: Option<Code>,
        body: Code,
    }
    let scrutinee = expr(it, scrutinee);
    let arms: Vec<Arm> = arms
        .iter()
        .map(|a| Arm {
            pat: a.pat.clone(),
            guard: a.guard.as_ref().map(|g| expr(it, g)),
            body: expr(it, &a.body),
        })
        .collect();
    Box::new(move |it, ctx| {
        let value = scrutinee(it, ctx)?;
        for arm in &arms {
            if !match_pattern(&arm.pat, &value, &mut ctx.env) {
                continue;
            }
            if let Some(guard) = &arm.guard {
                match guard(it, ctx)? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => continue,
                    other => {
                        return fail(format!(
                            "the `if` of a `case` gave {}, not a Bool",
                            other.type_name()
                        ));
                    }
                }
            }
            return (arm.body)(it, ctx);
        }
        fail("no `match` arm matched")
    })
}

fn assign(it: &Interp, place: &hir::Place, value: &hir::Expr) -> Code {
    let value = expr(it, value);
    let (local, name) = (place.local, place.name.clone());
    if place.projs.is_empty() {
        return Box::new(move |it, ctx| {
            let v = value(it, ctx)?;
            // The place must exist, as for a place with steps.
            if ctx.env.get(&local).is_none() {
                return Err(Ctrl::Fail(RuntimeError::new(format!(
                    "unbound variable `{name}`"
                ))));
            }
            ctx.env.insert(local, v);
            Ok(Value::Unit)
        });
    }
    enum Proj {
        Field(String),
        Index(Code),
    }
    let projs: Vec<Proj> = place
        .projs
        .iter()
        .map(|p| match p {
            hir::Proj::Field { name, .. } => Proj::Field(name.clone()),
            hir::Proj::Index(i) => Proj::Index(expr(it, i)),
        })
        .collect();
    Box::new(move |it, ctx| {
        // Order: the value, then the place's index expressions left to right.
        let v = value(it, ctx)?;
        let mut steps = Vec::with_capacity(projs.len());
        for p in &projs {
            steps.push(match p {
                Proj::Field(name) => Step::Field(name.as_str()),
                Proj::Index(i) => match i(it, ctx)? {
                    Value::Int(i) => Step::Index(i),
                    other => Step::Key(
                        MapKey::from_value(&other).map_err(|e| Ctrl::Fail(RuntimeError::new(e)))?,
                    ),
                },
            });
        }
        let root =
            ctx.env.get(&local).cloned().ok_or_else(|| {
                Ctrl::Fail(RuntimeError::new(format!("unbound variable `{name}`")))
            })?;
        let updated = super::update_place(root, &steps, v)?;
        ctx.env.insert(local, updated);
        Ok(Value::Unit)
    })
}

/// One run of a loop body: `Ok(false)` when it executed `break`.
fn loop_step(body: &Code, it: &Interp, ctx: &mut Ctx) -> Result<bool, Ctrl> {
    match body(it, ctx) {
        Ok(_) | Err(Ctrl::Continue) => Ok(true),
        Err(Ctrl::Break) => Ok(false),
        Err(other) => Err(other),
    }
}

fn looping(it: &Interp, var: LocalId, iter: &hir::ForIter, body: &hir::Block) -> Code {
    let body = block(it, body);
    match iter {
        hir::ForIter::Range {
            start,
            end,
            inclusive,
        } => {
            let (start, end, inclusive) = (expr(it, start), expr(it, end), *inclusive);
            Box::new(move |it, ctx| {
                let (Value::Int(a), Value::Int(b)) = (start(it, ctx)?, end(it, ctx)?) else {
                    return fail("range bounds must be integers");
                };
                // i128 so that `..=i64::MAX` cannot overflow the counter.
                let (mut i, last) = (i128::from(a), i128::from(b));
                while i < last || (inclusive && i == last) {
                    ctx.env.insert(var, Value::Int(i as i64));
                    if !loop_step(&body, it, ctx)? {
                        break;
                    }
                    i += 1;
                }
                Ok(Value::Unit)
            })
        }
        hir::ForIter::Vec(v) => {
            let v = expr(it, v);
            Box::new(move |it, ctx| {
                let items = match v(it, ctx)? {
                    Value::Vec(items) => items,
                    Value::Table(t) => {
                        for i in 0..t.rows() {
                            ctx.env.insert(var, t.row(i)?);
                            if !loop_step(&body, it, ctx)? {
                                break;
                            }
                        }
                        return Ok(Value::Unit);
                    }
                    _ => return fail("`for` needs a Vec or a Table"),
                };
                // Live view: element assignments inside the loop are visible to later iterations.
                // The length is fixed when the loop starts.
                for i in 0..items.len() {
                    let Some(item) = items.get(i) else { break };
                    ctx.env.insert(var, item);
                    if !loop_step(&body, it, ctx)? {
                        break;
                    }
                }
                Ok(Value::Unit)
            })
        }
    }
}

fn indexing(it: &Interp, base: &hir::Expr, index: &hir::Expr) -> Code {
    let place = peek(base);
    let base = expr(it, base);
    let index = expr(it, index);
    Box::new(move |it, ctx| {
        // A name or a field of one is read in place: cloning a shared list or map to look at one
        // item makes every thread that reads it write to the same reference count.
        if let Some(place) = &place
            && place.get(it, ctx).is_some()
        {
            let i = index(it, ctx)?;
            let b = place.get(it, ctx).ok_or_else(unbound)?;
            return super::index_value(b, i);
        }
        let b = base(it, ctx)?;
        let i = index(it, ctx)?;
        super::index_value(&b, i)
    })
}

fn call(it: &Interp, e: &hir::Expr, callee: &hir::Expr, list: &[hir::Expr]) -> Code {
    let result_ty = e.ty.clone();
    match &callee.kind {
        ExprKind::Fn(name) => {
            let (Some(&id), Some(def)) = (it.fn_ids.get(name), it.fns.get(name)) else {
                return fallback(e);
            };
            if def.is_async || def.params.len() != list.len() {
                return fallback(e);
            }
            let params: Vec<LocalId> = def.params.iter().map(|p| p.id).collect();
            let codes = args(it, list);
            let name = name.clone();
            Box::new(move |it, ctx| {
                if it.mocked.load(Relaxed) {
                    let values = eval_args(&codes, it, ctx)?;
                    if let Some(mock) = it.mock_for(&name) {
                        return it.apply_at(&mock, values, ctx.depth);
                    }
                    let mut env = Env::with_capacity(params.len() + 8);
                    for (id, v) in params.iter().zip(values) {
                        env.push(*id, v);
                    }
                    return it.run_function(id, &name, env, ctx.depth);
                }
                let mut env = Env::with_capacity(params.len() + 8);
                for (id, code) in params.iter().zip(&codes) {
                    env.push(*id, code(it, ctx)?);
                }
                it.run_function(id, &name, env, ctx.depth)
            })
        }
        #[cfg(feature = "gpu")]
        ExprKind::Extern(_) => fallback(e),
        #[cfg(not(feature = "gpu"))]
        ExprKind::Extern(path) => {
            let codes = args(it, list);
            if let [n] = path.as_slice()
                && (n == "Ok" || n == "Err")
            {
                let ok = n == "Ok";
                return Box::new(move |it, ctx| {
                    let mut values = eval_args(&codes, it, ctx)?;
                    let v = Box::new(values.remove(0));
                    Ok(if ok { Value::Ok(v) } else { Value::Err(v) })
                });
            }
            let path = path.joined().to_string();
            Box::new(move |it, ctx| {
                let values = eval_args(&codes, it, ctx)?;
                if it.mocked.load(Relaxed)
                    && let Some(mock) = it.mock_for(&path)
                {
                    return it.apply_at(&mock, values, ctx.depth);
                }
                Ok(std_impl::call_function(it, &path, values, &result_ty)?)
            })
        }
        ExprKind::Field { base, name } => {
            let codes = args(it, list);
            let place = peek(base);
            let base = expr(it, base);
            let name = name.clone();
            Box::new(move |it, ctx| {
                // The receiver is a name or a field of one: it is read in place, after the
                // arguments, which cannot change what a name stands for.
                if let Some(place) = &place
                    && place.get(it, ctx).is_some()
                {
                    let values = eval_args(&codes, it, ctx)?;
                    let receiver = place.get(it, ctx).ok_or_else(unbound)?;
                    return method(it, receiver, &name, values, &result_ty, ctx.depth);
                }
                let receiver = base(it, ctx)?;
                let values = eval_args(&codes, it, ctx)?;
                method(it, &receiver, &name, values, &result_ty, ctx.depth)
            })
        }
        _ => {
            let f = expr(it, callee);
            let codes = args(it, list);
            Box::new(move |it, ctx| {
                let f = f(it, ctx)?;
                let values = eval_args(&codes, it, ctx)?;
                it.apply_at(&f, values, ctx.depth)
            })
        }
    }
}

/// `receiver.name(args)`: a function held in a field, a method of a client, or a method of a value.
fn method(
    it: &Interp,
    receiver: &Value,
    name: &str,
    args: Args,
    result: &Ty,
    depth: usize,
) -> Eval {
    if let Value::Struct(s) = receiver
        && let Some((_, f)) = s.fields.iter().find(|(n, _)| n == name)
    {
        let f = f.clone();
        return it.apply_at(&f, args, depth);
    }
    if let Value::Native(Native::Postgres(pg)) = receiver {
        return Ok(std_impl::postgres_method(it, pg, name, &args, result)?);
    }
    Ok(std_impl::call_method(receiver, name, args)?)
}

fn structure(it: &Interp, e: &hir::Expr, name: &str, fields: &[(String, hir::Expr)]) -> Code {
    let Some(decl) = it.structs.get(name) else {
        return fallback(e);
    };
    // Where each declared field comes from among the written ones; the written ones run in the order
    // they are written.
    let sources: Vec<(String, Option<usize>)> = decl
        .iter()
        .map(|(n, _)| (n.clone(), fields.iter().position(|(f, _)| f == n)))
        .collect();
    let codes: Vec<Code> = fields.iter().map(|(_, fe)| expr(it, fe)).collect();
    let name = name.to_string();
    Box::new(move |it, ctx| {
        let mut values: smallvec::SmallVec<[Option<Value>; 8]> = smallvec::SmallVec::new();
        for c in &codes {
            values.push(Some(c(it, ctx)?));
        }
        let mut ordered = Vec::with_capacity(sources.len());
        for (n, from) in &sources {
            let v = from
                .and_then(|i| values[i].take())
                .ok_or_else(|| Ctrl::Fail(RuntimeError::new(format!("missing field `{n}`"))))?;
            ordered.push((n.clone(), v));
        }
        Ok(Value::Struct(Arc::new(StructVal {
            name: name.clone(),
            fields: ordered,
        })))
    })
}

fn formatted(it: &Interp, parts: &[hir::FPart]) -> Code {
    enum Part {
        Text(String),
        Expr(Code, Ty),
        Fmt {
            expr: Code,
            ty: Ty,
            repr: bool,
            spec: Option<Code>,
        },
    }
    let parts: Vec<Part> = parts
        .iter()
        .map(|p| match p {
            hir::FPart::Text(t) => Part::Text(t.clone()),
            hir::FPart::Expr(x) => Part::Expr(expr(it, x), x.ty.clone()),
            hir::FPart::Fmt {
                expr: x,
                repr,
                spec,
            } => Part::Fmt {
                expr: expr(it, x),
                ty: x.ty.clone(),
                repr: *repr,
                spec: spec.as_ref().map(|s| expr(it, s)),
            },
        })
        .collect();
    Box::new(move |it, ctx| {
        let mut out = String::new();
        for p in &parts {
            match p {
                Part::Text(t) => out.push_str(t),
                Part::Expr(x, ty) => {
                    let v = x(it, ctx)?;
                    out.push_str(&it.show(&v, ty, false));
                }
                Part::Fmt {
                    expr: x,
                    ty,
                    repr,
                    spec,
                } => {
                    let v = x(it, ctx)?;
                    let shown = it.show(&v, ty, *repr);
                    match spec {
                        None => out.push_str(&shown),
                        Some(spec) => {
                            let Value::Str(spec) = spec(it, ctx)? else {
                                return fail("a format is a Text");
                            };
                            out.push_str(&std_impl::format_value(&v, &shown, &spec)?);
                        }
                    }
                }
            }
        }
        Ok(Value::str(out))
    })
}

/// An operator on two numbers or two texts that are looked at where they are; `None` when it needs
/// the general path.
fn in_place(op: BinOp, a: &Value, b: &Value) -> Option<Eval> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => Some(match op {
            BinOp::Eq => Ok(Value::Bool(a == b)),
            BinOp::NotEq => Ok(Value::Bool(a != b)),
            BinOp::And | BinOp::Or => return None,
            op => int_op(op, *a, *b),
        }),
        (Value::Float(a), Value::Float(b)) => Some(match op {
            BinOp::Eq => Ok(Value::Bool(a == b)),
            BinOp::NotEq => Ok(Value::Bool(a != b)),
            BinOp::And | BinOp::Or => return None,
            op => float_op(op, *a, *b),
        }),
        (Value::Str(a), Value::Str(b)) => match op {
            BinOp::Eq => Some(Ok(Value::Bool(a == b))),
            BinOp::NotEq => Some(Ok(Value::Bool(a != b))),
            _ => None,
        },
        _ => None,
    }
}
