//! How values are written as text: in an f-string, by `str(x)`, and when a value is printed.
//!
//! A `Float` always shows a decimal point (`3.0`), switches to the exponent form beyond what
//! Python does (`1e+16`, `1e-05`), and an `f32` shows the fewest digits that read back as the
//! same `f32` (`0.1`, not `0.10000000149011612`). A `Text` inside a list, a map, a tuple or a
//! value is quoted, so `["a, b", "c"]` is not mistaken for three items.

use std::collections::HashMap;

use telvax_hir::{Prim, Ty};

use crate::value::Value;

/// The variants of each enum and the types of their values.
pub type EnumTypes = HashMap<String, Vec<(String, Vec<Ty>)>>;

/// The types of the fields of the programs' own `type`s and enums, to show `f32` values inside
/// them with their own digits.
#[derive(Default)]
pub struct TypeEnv<'a> {
    pub structs: Option<&'a HashMap<String, Vec<(String, Ty)>>>,
    pub enums: Option<&'a EnumTypes>,
}

/// `x` as Python writes a float: the fewest digits that read back as `x`, with a `.0` when it is
/// a whole number and an exponent when it is very large or very small.
pub fn float_repr(x: f64) -> String {
    repr_digits(x, &format!("{x:e}"), &x.to_string())
}

/// The same for an `f32`, whose value is kept in an `f64`.
pub fn float32_repr(x: f64) -> String {
    let y = x as f32;
    repr_digits(f64::from(y), &format!("{y:e}"), &y.to_string())
}

fn repr_digits(x: f64, scientific: &str, plain: &str) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if (-4..16).contains(&exponent) {
        return if plain.contains('.') {
            plain.to_string()
        } else {
            format!("{plain}.0")
        };
    }
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.abs())
}

/// `s` between double quotes, with the escapes a Telvax text can hold.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `v` as text. `ty` is its type when it is known; `quoted` writes a `Text` with quotes (what a
/// `Text` looks like inside something else, or under `!r`).
pub fn show(v: &Value, ty: Option<&Ty>, env: &TypeEnv, quoted: bool) -> String {
    let mut out = String::new();
    write(&mut out, v, ty, env, quoted);
    out
}

fn known(ty: Option<&Ty>) -> Option<&Ty> {
    ty.filter(|t| !matches!(t, Ty::Unknown | Ty::Never))
}

fn write(out: &mut String, v: &Value, ty: Option<&Ty>, env: &TypeEnv, quoted: bool) {
    let ty = known(ty);
    match v {
        Value::Unit => out.push_str("None"),
        Value::Bool(b) => out.push_str(if *b { "True" } else { "False" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(x) => out.push_str(&if matches!(ty, Some(Ty::Prim(Prim::F32))) {
            float32_repr(*x)
        } else {
            float_repr(*x)
        }),
        Value::Str(s) => {
            if quoted {
                out.push_str(&quote(s));
            } else {
                out.push_str(s);
            }
        }
        Value::Vec(items) => {
            let item_ty = match ty {
                Some(Ty::Vec(t)) => Some(&**t),
                _ => None,
            };
            out.push('[');
            for (i, item) in items.snapshot().iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write(out, item, item_ty, env, true);
            }
            out.push(']');
        }
        Value::Tuple(items) => {
            let tys = match ty {
                Some(Ty::Tuple(ts)) if ts.len() == items.len() => Some(ts),
                _ => None,
            };
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write(out, item, tys.map(|t| &t[i]), env, true);
            }
            out.push(')');
        }
        Value::Map(m) => {
            let (key_ty, value_ty) = match ty {
                Some(Ty::Map(k, v)) => (Some(&**k), Some(&**v)),
                _ => (None, None),
            };
            out.push('{');
            for (i, (k, value)) in m.entries().iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write(out, &k.to_value(), key_ty, env, true);
                out.push_str(": ");
                write(out, value, value_ty, env, true);
            }
            out.push('}');
        }
        Value::Struct(s) => {
            let declared = match ty {
                Some(Ty::Struct(name)) => env.structs.and_then(|m| m.get(name)),
                _ => env.structs.and_then(|m| m.get(&s.name)),
            };
            out.push_str(&s.name);
            out.push('(');
            for (i, (name, value)) in s.fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let field_ty = declared
                    .and_then(|d| d.iter().find(|(n, _)| n == name))
                    .map(|(_, t)| t);
                out.push_str(name);
                out.push('=');
                write(out, value, field_ty, env, true);
            }
            out.push(')');
        }
        Value::Enum(e) if e.name == "Option" => match e.fields.first() {
            Some(inner) => {
                let inner_ty = match ty {
                    Some(Ty::Option(t)) => Some(&**t),
                    _ => None,
                };
                out.push_str("Some(");
                write(out, inner, inner_ty, env, true);
                out.push(')');
            }
            None => out.push_str("None"),
        },
        Value::Enum(e) => {
            out.push_str(&e.name);
            out.push('.');
            out.push_str(&e.variant);
            if !e.fields.is_empty() {
                let tys = env
                    .enums
                    .and_then(|m| m.get(&*e.name))
                    .and_then(|vs| vs.iter().find(|(n, _)| *n == e.variant))
                    .map(|(_, t)| t);
                out.push('(');
                for (i, field) in e.fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write(out, field, tys.and_then(|t| t.get(i)), env, true);
                }
                out.push(')');
            }
        }
        Value::Ok(inner) => {
            let inner_ty = match ty {
                Some(Ty::Result(ok, _)) => Some(&**ok),
                _ => None,
            };
            out.push_str("Ok(");
            write(out, inner, inner_ty, env, true);
            out.push(')');
        }
        Value::Err(inner) => {
            let inner_ty = match ty {
                Some(Ty::Result(_, err)) => Some(&**err),
                _ => None,
            };
            out.push_str("Err(");
            write(out, inner, inner_ty, env, true);
            out.push(')');
        }
        other => out.push_str(&other.fallback_text()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_read_like_python() {
        for (x, want) in [
            (3.0, "3.0"),
            (0.1, "0.1"),
            (1.5, "1.5"),
            (-2.0, "-2.0"),
            (100.0, "100.0"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1.5e20, "1.5e+20"),
            (1e300, "1e+300"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (-1e-9, "-1e-09"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (f64::NAN, "nan"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
        ] {
            assert_eq!(float_repr(x), want, "{x:e}");
        }
    }

    #[test]
    fn an_f32_shows_its_own_digits() {
        for (x, want) in [
            (0.1f32, "0.1"),
            (0.3f32, "0.3"),
            (16777216.0f32, "16777216.0"),
            (1e-7f32, "1e-07"),
            (3.4e38f32, "3.4e+38"),
            (2.5f32, "2.5"),
        ] {
            assert_eq!(float32_repr(f64::from(x)), want);
        }
        // The same number as an `f64` has more digits.
        assert_eq!(float_repr(f64::from(0.1f32)), "0.10000000149011612");
    }

    #[test]
    fn text_is_quoted_inside_something_else() {
        assert_eq!(quote("a\"b\\c\nd"), r#""a\"b\\c\nd""#);
        let list = Value::vec(vec![Value::str("a, b"), Value::str("c")]);
        assert_eq!(list.to_string(), r#"["a, b", "c"]"#);
        assert_eq!(Value::str("plain").to_string(), "plain");
        let pair = Value::Tuple(std::sync::Arc::new(vec![
            Value::str("x"),
            Value::Float(2.0),
        ]));
        assert_eq!(pair.to_string(), r#"("x", 2.0)"#);
        assert_eq!(Value::some(Value::str("y")).to_string(), r#"Some("y")"#);
    }
}
