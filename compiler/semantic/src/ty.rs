//! Semantic types.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prim {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Str,
    /// A calendar day, written `"2026-10-17"`. Text in the language, an Arrow `Date32` in a table.
    Date,
    /// A UTC instant, written `"2026-10-17T16:53:03Z"`. Text in the language, an Arrow
    /// `Timestamp(µs, UTC)` in a table.
    Timestamp,
}

impl Prim {
    pub fn from_name(name: &str) -> Option<Prim> {
        Some(match name {
            "i8" => Prim::I8,
            "i16" => Prim::I16,
            "i32" => Prim::I32,
            "i64" => Prim::I64,
            "u8" => Prim::U8,
            "u16" => Prim::U16,
            "u32" => Prim::U32,
            "u64" => Prim::U64,
            "f32" => Prim::F32,
            "f64" => Prim::F64,
            "bool" => Prim::Bool,
            "String" => Prim::Str,
            "Date" => Prim::Date,
            "Timestamp" => Prim::Timestamp,
            _ => return None,
        })
    }

    pub fn is_int(self) -> bool {
        matches!(
            self,
            Prim::I8
                | Prim::I16
                | Prim::I32
                | Prim::I64
                | Prim::U8
                | Prim::U16
                | Prim::U32
                | Prim::U64
        )
    }

    pub fn is_float(self) -> bool {
        matches!(self, Prim::F32 | Prim::F64)
    }

    /// Values are strings: `Text`, `Date` and `Timestamp`.
    pub fn is_text(self) -> bool {
        matches!(self, Prim::Str | Prim::Date | Prim::Timestamp)
    }

    pub fn is_numeric(self) -> bool {
        self.is_int() || self.is_float()
    }

    /// The name as a program writes it (`Int`, `Float`, `Text`, `i32`, ...).
    pub fn name(self) -> &'static str {
        match self {
            Prim::I8 => "i8",
            Prim::I16 => "i16",
            Prim::I32 => "i32",
            Prim::I64 => "Int",
            Prim::U8 => "u8",
            Prim::U16 => "u16",
            Prim::U32 => "u32",
            Prim::U64 => "u64",
            Prim::F32 => "f32",
            Prim::F64 => "Float",
            Prim::Bool => "Bool",
            Prim::Str => "Text",
            Prim::Date => "Date",
            Prim::Timestamp => "Timestamp",
        }
    }
}

/// One dimension of a tensor shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dim {
    Lit(u64),
    /// A symbolic dimension (e.g. `N`); compatible with any size.
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Ty {
    Unit,
    Prim(Prim),
    /// A user-declared struct.
    Struct(String),
    /// A user-declared enum.
    Enum(String),
    /// A user-declared trait, used as a type: any value whose type implements it.
    Trait(String),
    /// A type parameter (`T`) while a generic signature is read to infer its arguments.
    Param(String),
    /// Columnar table whose schema is the named struct (one column per field).
    Table(String),
    /// An imported type whose definition is not known to the checker.
    Named(String),
    Vec(Box<Ty>),
    Tensor {
        elem: Box<Ty>,
        shape: Vec<Dim>,
    },
    Result(Box<Ty>, Box<Ty>),
    /// `(A, B, ..)`: two or more values of different types.
    Tuple(Vec<Ty>),
    /// `Option<T>`: `Some(value)` or `None`.
    Option(Box<Ty>),
    /// `Map<K, V>`: keys are strings, integers or booleans.
    Map(Box<Ty>, Box<Ty>),
    /// `Task[T]`: what an `async fn` returns; `await` runs it and gives the `T`.
    Task(Box<Ty>),
    /// `Cache[Text, V]`: a `std.cache` whose values are `V` (keys are always `Text`).
    Cache(Box<Ty>),
    Fn(Vec<Ty>, Box<Ty>),
    /// Type of expressions that never produce a value (`return`).
    Never,
    /// Type of an unsuffixed integer literal that is still waiting to be pinned down by its
    /// uses; defaults to `i32`. Only exists while a function is being checked.
    IntVar(u32),
    /// Like `IntVar`, for float literals; defaults to `f64`.
    FloatVar(u32),
    /// Not known (opaque std items) or already reported as an error; compatible with everything.
    #[default]
    Unknown,
}

impl Ty {
    pub fn str() -> Ty {
        Ty::Prim(Prim::Str)
    }

    pub fn bool() -> Ty {
        Ty::Prim(Prim::Bool)
    }

    /// Whether a value of type `actual` can be used where `expected` is required.
    pub fn accepts(&self, actual: &Ty) -> bool {
        use Ty::*;
        match (self, actual) {
            (Unknown, _) | (_, Unknown) | (_, Never) | (Never, _) => true,
            (IntVar(_), IntVar(_)) | (FloatVar(_), FloatVar(_)) => true,
            (IntVar(_), Prim(p)) | (Prim(p), IntVar(_)) => p.is_int(),
            (FloatVar(_), Prim(p)) | (Prim(p), FloatVar(_)) => p.is_float(),
            (Unit, Unit) => true,
            // A `Date` or `Timestamp` is a string that is checked when it enters a table.
            (Prim(a), Prim(b)) => {
                a == b
                    || (a.is_text()
                        && b.is_text()
                        && (*a == crate::ty::Prim::Str || *b == crate::ty::Prim::Str))
            }
            (Struct(a), Struct(b))
            | (Enum(a), Enum(b))
            | (Trait(a), Trait(b))
            | (Param(a), Param(b))
            | (Named(a), Named(b))
            | (Table(a), Table(b)) => a == b,
            (Vec(a), Vec(b)) => a.accepts(b),
            (
                Tensor {
                    elem: e1,
                    shape: s1,
                },
                Tensor {
                    elem: e2,
                    shape: s2,
                },
            ) => {
                e1.accepts(e2)
                    && s1.len() == s2.len()
                    && s1.iter().zip(s2).all(|(a, b)| match (a, b) {
                        (Dim::Lit(x), Dim::Lit(y)) => x == y,
                        _ => true,
                    })
            }
            (Result(a, b), Result(c, d)) => a.accepts(c) && b.accepts(d),
            (Option(a), Option(b)) => a.accepts(b),
            (Task(a), Task(b)) | (Cache(a), Cache(b)) => a.accepts(b),
            // `std.cache.new` with no annotation gives the untyped `Cache`, which a typed one accepts.
            (Cache(_), Named(n)) | (Named(n), Cache(_)) => n == "Cache",
            (Tuple(a), Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.accepts(y))
            }
            (Map(k1, v1), Map(k2, v2)) => k1.accepts(k2) && v1.accepts(v2),
            (Fn(p1, r1), Fn(p2, r2)) => {
                p1.len() == p2.len()
                    && p1.iter().zip(p2).all(|(a, b)| a.accepts(b))
                    && r1.accepts(r2)
            }
            _ => false,
        }
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Dim::Lit(n) => write!(f, "{n}"),
            Dim::Named(n) => f.write_str(n),
        }
    }
}

/// Types are shown the way a program writes them: `List[Int]`, `Int?`, `Map[Text, Float]`.
impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Unit => f.write_str("None"),
            Ty::Prim(p) => f.write_str(p.name()),
            Ty::Struct(n) | Ty::Enum(n) | Ty::Trait(n) | Ty::Param(n) | Ty::Named(n) => {
                f.write_str(n)
            }
            Ty::Table(n) => write!(f, "Table[{n}]"),
            Ty::Vec(t) => write!(f, "List[{t}]"),
            Ty::Tensor { elem, shape } => {
                let dims: Vec<_> = shape.iter().map(|d| d.to_string()).collect();
                write!(f, "Tensor[{elem}, {}]", dims.join(", "))
            }
            Ty::Result(a, b) => write!(f, "Result[{a}, {b}]"),
            Ty::Option(t) => write!(f, "{t}?"),
            Ty::Task(t) => write!(f, "Task[{t}]"),
            Ty::Cache(t) => write!(f, "Cache[Text, {t}]"),
            Ty::Tuple(items) => {
                let shown: Vec<_> = items.iter().map(|t| t.to_string()).collect();
                write!(f, "({})", shown.join(", "))
            }
            Ty::Map(k, v) => write!(f, "Map[{k}, {v}]"),
            Ty::Fn(params, ret) => {
                let ps: Vec<_> = params.iter().map(|p| p.to_string()).collect();
                write!(f, "({}) -> {ret}", ps.join(", "))
            }
            Ty::IntVar(_) => f.write_str("{integer}"),
            Ty::FloatVar(_) => f.write_str("{float}"),
            Ty::Never => f.write_str("Never"),
            Ty::Unknown => f.write_str("_"),
        }
    }
}
