//! Abstract syntax tree definitions for Telvax.

use std::ops::Range;

pub mod stack;
pub mod tail;

pub type Span = Range<usize>;

/// A node together with its byte span in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

pub type Ident = Spanned<String>;
pub type Expr = Spanned<ExprKind>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Import(Import),
    /// Top-level `let` (global resource setup).
    Let(Let),
    Struct(Struct),
    Enum(Enum),
    /// `type Id = Int`: another name for a type.
    Alias(Alias),
    Impl(Impl),
    Trait(Trait),
    Fn(Fn),
    /// `serve:` block: the options of the server.
    Serve(Serve),
}

/// The lines of a `serve:` block: `name: value`, each value a literal.
#[derive(Debug, Clone, PartialEq)]
pub struct Serve {
    pub options: Vec<(Ident, Expr)>,
    pub span: Span,
}

/// `import a.b.C` or `from std.http import Request, Response`, each name optionally `as other`.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub path: Vec<Ident>,
    /// Empty for a plain import; the names after `import` in `from a import b, c`.
    pub group: Vec<Ident>,
    /// The `as` names, one per imported name (`group`, or the last part of `path` for a plain
    /// import). May be empty or shorter than that list: a missing entry means no `as`.
    pub aliases: Vec<Option<Ident>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Let {
    pub mutable: bool,
    pub name: Ident,
    pub ty: Option<Type>,
    pub init: Expr,
    pub span: Span,
}

/// `type Id = Int` or `type Pair[T] = (T, T)`: a name that stands for a type.
#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: Ident,
    pub ty: Type,
    /// `top_k: Int = 5`: the value used when a construction leaves the field out.
    pub default: Option<Expr>,
}

/// `impl Point { fn norm(self) -> f64 { .. } }`: methods and associated functions of a type. A
/// function whose first parameter is `self` is a method; the others are called as `Point::new(..)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Impl {
    pub generics: Vec<GenericParam>,
    pub ty: Ident,
    /// The type arguments written after the name: the `T` of `impl<T> Stack<T>`.
    pub ty_args: Vec<Type>,
    /// `impl Shape for Circle`: the trait being implemented.
    pub trait_name: Option<Ident>,
    pub fns: Vec<Fn>,
    pub span: Span,
}

/// `trait Shape { fn area(self) -> f64; fn describe(self) -> String { .. } }`: an interface. A
/// function without a body is required of every implementation; one with a body is a default.
#[derive(Debug, Clone, PartialEq)]
pub struct Trait {
    pub is_pub: bool,
    pub name: Ident,
    pub fns: Vec<Fn>,
    pub span: Span,
}

/// `enum Shape { Circle(f64), Rect(f64, f64), Empty }`
#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub variants: Vec<Variant>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: Ident,
    /// Payload types, in order (empty for a unit variant).
    pub fields: Vec<Type>,
    /// `Circle(r: Float)`: the name of each value, or empty when the values have no names.
    pub names: Vec<Ident>,
    /// `from Parse(Text)`: `?` turns an error of the type this variant holds into this variant.
    pub from: bool,
}

/// One `pattern => body` arm of a `match`.
#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    pub pat: Pattern,
    /// `case x if x > 3:`: the arm is taken only when this is true.
    pub guard: Option<Expr>,
    pub body: Expr,
}

pub type Pattern = Spanned<PatKind>;

#[derive(Debug, Clone, PartialEq)]
pub enum PatKind {
    /// `_`
    Wild,
    /// A lowercase name that binds the matched value.
    Bind(String),
    Lit(Lit),
    /// `-1`, `-2.5`: a literal with a minus sign.
    NegLit(Lit),
    /// `(a, b)`
    Tuple(Vec<Pattern>),
    /// `1 | 2`: any of the alternatives. They cannot bind names.
    Or(Vec<Pattern>),
    /// `[a, b]`, `[first, *rest]`, `[*_, last]`: a list. `rest` is the `*name` (or `*_`) part.
    List {
        before: Vec<Pattern>,
        rest: Option<Box<Pattern>>,
        after: Vec<Pattern>,
    },
    /// `Point(x: a, y: 0)`: a value of a `type`, naming some of its fields.
    Struct {
        path: Vec<Ident>,
        fields: Vec<(Ident, Pattern)>,
    },
    /// `Shape::Circle(r)`, `Shape::Empty`, `Ok(v)`, `Err(e)`
    Variant {
        path: Vec<Ident>,
        args: Vec<Pattern>,
    },
}

/// A type parameter, `T` or `T: Shape + Other`.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericParam {
    pub name: Ident,
    pub bounds: Vec<Ident>,
}

/// `@endpoint(path = "/x", method = "POST")`
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: Ident,
    pub args: Vec<(Ident, Expr)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fn {
    pub attrs: Vec<Attribute>,
    pub is_pub: bool,
    pub name: Ident,
    /// `async fn`: calling it gives a `Task` that `await` runs.
    pub is_async: bool,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub body: Block,
    /// Declared with `;` instead of a body (only allowed in a `trait`).
    pub required: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// `mut x: T` allows assigning to the parameter inside the function.
    pub mutable: bool,
    pub name: Ident,
    pub ty: Type,
    /// `limit: Int = 5`: the value used when a call leaves the argument out.
    pub default: Option<Expr>,
}

/// `Name`, `a::B`, `Vec<T>`, `Tensor<f32, [768]>`
#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub path: Vec<Ident>,
    pub args: Vec<TypeArg>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeArg {
    Type(Type),
    /// Tensor shape such as `[768]` or `[N, 4]`.
    Shape(Vec<Dim>),
    /// An inline row type: `Table<{ region: String, revenue: f64 }>`.
    Row(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dim {
    Lit(u64),
    Named(Ident),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// Trailing expression without `;`, the block's value.
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let(Let),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Lit(Lit),
    /// `x` or `std.text.trim`
    Path(Vec<Ident>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `limit: 5` inside the arguments of a call. The checker moves it to the right place.
    Named {
        name: Ident,
        value: Box<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: Ident,
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
    /// `lhs |> rhs`; the lowest-precedence, left-associative operator.
    Pipe {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Closure {
        params: Vec<Ident>,
        /// The annotation of each parameter (`|x: i64| ..`), `None` when it is inferred.
        types: Vec<Option<Type>>,
        body: Box<Expr>,
    },
    StructLit {
        path: Vec<Ident>,
        fields: Vec<(Ident, Expr)>,
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
    /// `await task`: runs a `Task` (what an `async fn` returns) and gives its value.
    Await(Box<Expr>),
    /// `expr?`: the value of an `Ok` / `Some`, or return the `Err` / `None` from the function.
    Try(Box<Expr>),
    /// `(a, b, c)`: two or more values.
    Tuple(Vec<Expr>),
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<Arm>,
    },
    FString(Vec<FPart>),
    /// `target = value`; compound forms (`+=`, ...) are desugared by the parser.
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    While {
        cond: Box<Expr>,
        body: Block,
    },
    For {
        var: Ident,
        iter: ForIter,
        body: Block,
    },
    /// `base[index]`
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// `[a, b, c]`
    Array(Vec<Expr>),
    /// Placeholder left by parser error recovery.
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForIter {
    /// `start..end` or `start..=end`
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    /// Any expression producing a `Vec`.
    Expr(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FPart {
    Text(String),
    Expr(Expr),
    /// `{x:>8}`, `{x!r}` or `{x=}`: the value with a format, or as `repr` (a `Text` in quotes).
    /// The format is itself text and may hold `{...}` (`{n:>{width}}`).
    Fmt {
        expr: Expr,
        repr: bool,
        spec: Option<Expr>,
    },
}

impl FPart {
    /// The expressions this part holds.
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

#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Int(u64),
    Float(f64),
    Str(String),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl Import {
    /// The name `i` (an index into `group`, or 0 for a plain import) is known by in the importing
    /// file: its `as` name, or itself.
    pub fn local_name(&self, i: usize, name: &Ident) -> Ident {
        match self.aliases.get(i) {
            Some(Some(alias)) => alias.clone(),
            _ => name.clone(),
        }
    }
}

/// The names of the types as written (`Int`, `Text`, `List`) and as the compiler knows them.
pub fn canonical_type_name(name: &str) -> &str {
    match name {
        "Int" => "i64",
        "Float" => "f64",
        "Text" => "String",
        "Bool" => "bool",
        "List" => "Vec",
        other => other,
    }
}
