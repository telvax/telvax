//! The typed, name-resolved High-Level IR produced by the checker.

use crate::ty::Ty;
pub use telvax_ast::{BinOp, Lit, UnOp};

pub type Span = std::ops::Range<usize>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    /// Fully qualified names brought in by `import`.
    pub imports: Vec<Vec<String>>,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub globals: Vec<Global>,
    pub fns: Vec<FnDef>,
    /// The `serve:` block: option names (as the flags, with `_` for `-`) and their literal values.
    pub serve: Vec<(String, FieldDefault)>,
    /// The functions of `on_start: [..]` in the `serve:` block, in the order they run before the
    /// server listens.
    pub on_start: Vec<String>,
    /// The lines of the `serve:` block that read the environment, `env("NAME", "default")`.
    pub serve_env: Vec<EnvOption>,
}

/// An option of `serve:` whose value is read from a variable when the server starts.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvOption {
    pub option: String,
    pub var: String,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    pub fields: Vec<(String, Ty)>,
    /// The fields with a literal default (`language: Text = "en"`), which a JSON body may leave out.
    pub defaults: Vec<(String, FieldDefault)>,
    pub span: Span,
}

/// The literal a field is given when it is left out.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldDefault {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    /// Variant names with their payload types.
    pub variants: Vec<(String, Vec<Ty>)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub name: String,
    pub ty: Ty,
    pub init: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    pub id: LocalId,
    pub name: String,
    pub ty: Ty,
    pub mutable: bool,
    /// Span of the binding's name.
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

/// Validated `@endpoint(...)` metadata; input for the server generator.
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub path: String,
    pub method: HttpMethod,
    /// The names in `{...}` segments of `path`, in order. Each is a parameter of the function.
    pub path_params: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    /// `async fn`: a call gives a `Task`; `ret` is the type the task gives when it is awaited.
    pub is_async: bool,
    pub params: Vec<Local>,
    pub ret: Ty,
    pub body: Block,
    pub endpoint: Option<Endpoint>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { local: Local, init: Expr },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Ty,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Lit(Lit),
    Local(LocalId),
    Global(String),
    Fn(String),
    /// Path into an imported / standard-library item the checker cannot see into.
    Extern(Vec<String>),
    /// Pipelines are desugared: `a |> f(b)` becomes `f(a, b)`.
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: String,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Closure {
        params: Vec<Local>,
        body: Box<Expr>,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    If {
        cond: Box<Expr>,
        then: Block,
        otherwise: Option<Box<Expr>>,
    },
    Block(Block),
    Return(Option<Box<Expr>>),
    Break,
    Continue,
    /// `expr?` (see the AST).
    Try(Box<Expr>),
    /// `(a, b)`
    Tuple(Vec<Expr>),
    /// A method called on a value whose type is a trait: the implementation is chosen at run time
    /// from the type of `args[0]`, the receiver.
    DynCall {
        trait_name: String,
        method: String,
        args: Vec<Expr>,
    },
    /// `Shape::Circle(1.0)`, `Shape::Empty`
    EnumCtor {
        enum_name: String,
        variant: String,
        args: Vec<Expr>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<Arm>,
    },
    FString(Vec<FPart>),
    /// Assignment to a place rooted in a mutable local; evaluates to unit. The value is
    /// evaluated first, then the place's index expressions left to right.
    Assign {
        place: Place,
        value: Box<Expr>,
    },
    While {
        cond: Box<Expr>,
        body: Block,
    },
    For {
        var: Local,
        iter: ForIter,
        body: Block,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Array(Vec<Expr>),
    /// `await task`
    Await(Box<Expr>),
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    pub pat: Pat,
    /// The `if` of `case x if x > 3:`.
    pub guard: Option<Expr>,
    pub body: Expr,
    /// Written by the user, and no arm before it can leave a value for it (`telvax lint` reports it).
    pub unreachable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pat {
    Wild,
    Bind(Local),
    Lit(Lit),
    /// A negative integer literal: the number without its sign.
    NegInt(u64),
    /// `(a, b)`
    Tuple(Vec<Pat>),
    /// `1 | 2`; the alternatives bind no names.
    Or(Vec<Pat>),
    /// `[a, *rest, z]`: `before`, the optional `*rest` (a `Bind` or `Wild`), then `after`.
    List {
        before: Vec<Pat>,
        rest: Option<Box<Pat>>,
        after: Vec<Pat>,
    },
    /// A value of a `type`, by the fields the pattern names.
    Struct {
        name: String,
        fields: Vec<(String, Pat)>,
    },
    /// A variant of a user enum (`enum_name` is `Some`) or `Ok` / `Err` of a `Result` (`None`).
    Variant {
        enum_name: Option<String>,
        variant: String,
        args: Vec<Pat>,
    },
}

/// Assignable location: a `let mut` local (or `mut` parameter) followed by field / index steps.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub local: LocalId,
    pub name: String,
    pub projs: Vec<Proj>,
    /// Type of the whole place (what the assigned value must have).
    pub ty: Ty,
    /// Type of the root local.
    pub root_ty: Ty,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Proj {
    Field { name: String, struct_name: String },
    Index(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForIter {
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    /// Iterates the elements of a `Vec`.
    Vec(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum FPart {
    Text(String),
    Expr(Expr),
    /// A value with a format (`{x:>8}`), or shown as `repr` (`{x!r}`, `{x=}`).
    Fmt {
        expr: Expr,
        repr: bool,
        spec: Option<Expr>,
    },
}

impl FPart {
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            FPart::Text(_) => Vec::new(),
            FPart::Expr(e) => vec![e],
            FPart::Fmt { expr, spec, .. } => std::iter::once(expr).chain(spec.as_ref()).collect(),
        }
    }

    pub fn exprs_mut(&mut self) -> Vec<&mut Expr> {
        match self {
            FPart::Text(_) => Vec::new(),
            FPart::Expr(e) => vec![e],
            FPart::Fmt { expr, spec, .. } => std::iter::once(expr).chain(spec.as_mut()).collect(),
        }
    }
}
