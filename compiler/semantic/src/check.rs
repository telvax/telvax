//! Name resolution and type checking: AST -> typed HIR.

use std::collections::{HashMap, HashSet};

use telvax_ast as ast;
use telvax_diagnostics::Diagnostic;

use crate::hir::{self, Endpoint, HttpMethod, LocalId};
use crate::stdlib::{self, Sig};
use crate::ty::{Dim, Prim, Ty};

type Span = std::ops::Range<usize>;

/// A variant (or `Ok` / `Err`) with its enum name and payload types, for exhaustiveness checks.
type Constructor = (Option<String>, String, Vec<Ty>);
/// The patterns of one arm, a column for each value that is matched (`None` matches anything).
type Row<'p> = Vec<Option<&'p hir::Pat>>;

/// The rows with each `a | b` in the first column replaced by one row for each alternative.
fn expand_or<'p>(rows: &[Row<'p>]) -> Vec<Row<'p>> {
    fn push<'p>(row: &Row<'p>, out: &mut Vec<Row<'p>>) {
        match row[0] {
            Some(hir::Pat::Or(alts)) => {
                for alt in alts {
                    let mut r = row.clone();
                    r[0] = Some(alt);
                    push(&r, out);
                }
            }
            _ => out.push(row.clone()),
        }
    }
    let mut out = Vec::new();
    for row in rows {
        push(row, &mut out);
    }
    out
}

/// The fewest items a list that matches the pattern in this column has.
fn list_min_len(p: Option<&hir::Pat>) -> usize {
    match p {
        Some(hir::Pat::List { before, after, .. }) => before.len() + after.len(),
        _ => 0,
    }
}

/// The patterns for the `n` items of a list, when a pattern with these parts matches a list of
/// that length (`None` when it does not). The items `*rest` stands for match anything.
fn list_items<'p>(
    before: &'p [hir::Pat],
    has_rest: bool,
    after: &'p [hir::Pat],
    n: usize,
) -> Option<Vec<Option<&'p hir::Pat>>> {
    let fixed = before.len() + after.len();
    if (has_rest && n < fixed) || (!has_rest && n != fixed) {
        return None;
    }
    let mut out: Vec<Option<&hir::Pat>> = before.iter().map(Some).collect();
    out.extend(std::iter::repeat_n(None, n - fixed));
    out.extend(after.iter().map(Some));
    Some(out)
}

/// Prefix of the name given to a call of a generic function whose instance is not chosen yet.
const OPEN_CALL: &str = "\u{0}open";

/// A call of a generic function that still has a literal of unknown type among its arguments.
/// The instance is chosen when the enclosing function is finished (see `close_generic_call`).
#[derive(Clone)]
struct OpenCall {
    name: String,
    decl: ast::Fn,
    generics: Vec<ast::GenericParam>,
    params: Vec<Ty>,
    ret: Ty,
    bind: HashMap<String, Ty>,
    span: Span,
}

/// A type written where an expression is parsed: `Int`, `List[Text]`, `Map[Text, Int]`.
fn expr_as_type(e: &ast::Expr) -> Option<ast::Type> {
    let (path, args) = match &e.node {
        ast::ExprKind::Path(p) => (p.clone(), Vec::new()),
        ast::ExprKind::Index { base, index } => {
            let ast::ExprKind::Path(p) = &base.node else {
                return None;
            };
            let parts = match &index.node {
                ast::ExprKind::Tuple(items) => items.clone(),
                _ => vec![(**index).clone()],
            };
            let args = parts
                .iter()
                .map(|a| expr_as_type(a).map(ast::TypeArg::Type))
                .collect::<Option<Vec<_>>>()?;
            (p.clone(), args)
        }
        _ => return None,
    };
    let mut path = path;
    if path.len() == 1 {
        path[0].node = ast::canonical_type_name(&path[0].node).to_string();
    }
    Some(ast::Type {
        path,
        args,
        span: e.span.clone(),
    })
}

/// Bounds every program can use without declaring a trait: `fn sum[T: Num](xs: List[T]) -> T`.
#[derive(Clone, Copy)]
enum Builtin {
    /// `Int`, `Float` and the sized number types.
    Num,
    /// Values that `<` and `>` compare: numbers and `Text`.
    Ord,
    /// Values that `==` compares: everything but functions.
    Eq,
}

impl Builtin {
    const NAMES: [&'static str; 3] = ["Num", "Ord", "Eq"];

    fn from_name(name: &str) -> Option<Builtin> {
        match name {
            "Num" => Some(Builtin::Num),
            "Ord" => Some(Builtin::Ord),
            "Eq" => Some(Builtin::Eq),
            _ => None,
        }
    }

    fn holds(self, ty: &Ty) -> bool {
        match (self, ty) {
            (_, Ty::Unknown | Ty::Never | Ty::Param(_)) => true,
            (Builtin::Num, Ty::Prim(p)) => p.is_numeric(),
            (Builtin::Num, Ty::IntVar(_) | Ty::FloatVar(_)) => true,
            (Builtin::Ord, Ty::Prim(p)) => p.is_numeric() || p.is_text(),
            (Builtin::Ord, Ty::IntVar(_) | Ty::FloatVar(_)) => true,
            (Builtin::Eq, Ty::Fn(..)) => false,
            (Builtin::Eq, _) => true,
            _ => false,
        }
    }

    fn meaning(self) -> &'static str {
        match self {
            Builtin::Num => {
                "`Num` is for Int, Float and the sized number types (`i32`, `u8`, `f32`, ...)"
            }
            Builtin::Ord => "`Ord` is for numbers and Text, the values `<` and `>` compare",
            Builtin::Eq => "`Eq` is for every type but functions, the values `==` compares",
        }
    }
}

/// How big, in parts, the types of one instance may be. A generic function that calls itself
/// with a bigger type (`f([x])`, `f((x, x))`) would otherwise be instantiated forever.
const MAX_TYPE_SIZE: usize = 200;

/// A generic function (or method) instance waiting to have its body checked.
#[derive(Clone)]
struct Pending {
    f: ast::Fn,
    impl_ty: Option<String>,
    subst: HashMap<String, Ty>,
}

#[derive(Default, Clone)]
pub(crate) struct Checker {
    diags: Vec<Diagnostic>,
    /// Imported name -> its full path.
    imports: HashMap<String, Vec<String>>,
    struct_fields: HashMap<String, Vec<(String, Ty)>>,
    /// Enum name -> its variants and their payload types.
    enums: HashMap<String, Vec<(String, Vec<Ty>)>>,
    /// Row types written inline (`Table<{ a: i64 }>`), turned into structs named by their text.
    anon_structs: Vec<hir::StructDef>,
    /// While checking the functions of an `impl`, the type it is for (what `Self` means).
    impl_ty: Option<String>,
    /// Declared traits: the names of their methods and whether each is required (no body).
    traits: HashMap<String, Vec<(String, bool)>>,
    /// `impl Trait for Type` pairs.
    impls: HashSet<(String, String)>,
    impl_spans: Vec<(String, String, Span)>,
    /// For a method defined in `impl Trait for Type`, the trait it belongs to.
    impl_of_fn: HashMap<String, String>,
    /// Generic functions, structs, enums and `impl` blocks, instantiated for the types that use them.
    generic_fns: HashMap<String, ast::Fn>,
    generic_structs: HashMap<String, ast::Struct>,
    generic_enums: HashMap<String, ast::Enum>,
    generic_impls: HashMap<String, Vec<ast::Impl>>,
    /// Type parameters in scope and the types they stand for.
    type_params: HashMap<String, Ty>,
    /// For an instance of a generic type (`Stack<i64>`): the generic it came from and its arguments.
    instance_args: HashMap<String, (String, Vec<Ty>)>,
    /// Instances whose bodies still have to be checked.
    pending: Vec<Pending>,
    open_calls: Vec<OpenCall>,
    anon_enums: Vec<hir::EnumDef>,
    /// The names of the values of the variants that have them (`Circle(r: Float)`), by enum as
    /// declared (the generic one, for an instance) and variant.
    enum_labels: HashMap<(String, String), Vec<String>>,
    /// `type Id = Int`, by name, and the aliases being expanded right now (an alias that uses
    /// itself would never end).
    aliases: HashMap<String, ast::Alias>,
    /// The variants marked `from`, by enum and variant: where `?` wraps an error.
    enum_from: HashSet<(String, String)>,
    /// What each `async fn` gives when its task is awaited (its signature says `Task[T]`).
    async_inner: HashMap<String, Ty>,
    /// Whether the code being checked is the body of an `async fn`, where `await` is allowed.
    in_async: bool,
    alias_stack: Vec<String>,
    /// Functions of `impl` blocks, by their full name (`Point::norm`), with the type they belong to.
    methods: HashMap<String, String>,
    fn_sigs: HashMap<String, (Vec<Ty>, Ty)>,
    globals: HashMap<String, Ty>,
    scopes: Vec<HashMap<String, (LocalId, Ty, bool)>>,
    next_local: u32,
    ret: Ty,
    /// How many loops enclose the expression being checked (reset inside closures).
    loop_depth: u32,
    /// For each closure being checked, how many scopes existed before it: a variable found in a
    /// scope below that floor was captured by value.
    closure_floors: Vec<usize>,
    endpoints: HashSet<(HttpMethod, String)>,
    /// Bindings of literal type variables (`Ty::IntVar`/`Ty::FloatVar`), indexed by id.
    subst: Vec<Option<Ty>>,
    /// Set once every struct's fields are known (table schemas are validated from then on).
    structs_resolved: bool,
}

pub(crate) fn lower(program: &ast::Program) -> (hir::Program, Vec<Diagnostic>) {
    let mut c = Checker::default();
    let out = c.run(program);
    (out, c.diags)
}

/// The std module whose functions are available as methods on a value of type `ty`.
fn method_module(ty: &Ty, name: &str) -> Option<&'static str> {
    match ty {
        Ty::Tensor { .. }
            if matches!(
                name,
                "add"
                    | "sub"
                    | "mul"
                    | "div"
                    | "scale"
                    | "relu"
                    | "sigmoid"
                    | "softmax"
                    | "sum"
                    | "mean"
                    | "argmax"
                    | "dot"
                    | "matmul"
                    | "transpose"
                    | "reshape"
                    | "to_vec"
            ) =>
        {
            Some("tensor")
        }
        Ty::Table(_)
            if matches!(
                name,
                "sql"
                    | "filter"
                    | "head"
                    | "tail"
                    | "sort_by"
                    | "sort_by_desc"
                    | "concat"
                    | "split"
                    | "select"
                    | "group_by"
            ) =>
        {
            Some("table")
        }
        Ty::Vec(_)
            if matches!(
                name,
                "map"
                    | "filter"
                    | "take"
                    | "count"
                    | "any"
                    | "all"
                    | "find"
                    | "for_each"
                    | "fold"
                    | "enumerate"
                    | "zip"
                    | "sum"
                    | "min"
                    | "max"
                    | "skip"
            ) =>
        {
            Some("iter")
        }
        Ty::Vec(_) if name == "join" => Some("string"),
        Ty::Vec(_) if name == "slice" => Some("iter"),
        Ty::Prim(p) if p.is_text() && name == "slice" => Some("string"),
        Ty::Option(_)
            if matches!(
                name,
                "map" | "and_then" | "filter" | "ok_or" | "unwrap_or_else" | "expect"
            ) =>
        {
            Some("option")
        }
        Ty::Result(..)
            if matches!(
                name,
                "is_ok"
                    | "is_err"
                    | "unwrap"
                    | "expect"
                    | "unwrap_or"
                    | "unwrap_or_else"
                    | "map"
                    | "map_err"
                    | "and_then"
                    | "ok"
                    | "err"
            ) =>
        {
            Some("result")
        }
        _ => None,
    }
}

/// Names usable without `std::`: the ones nearly every program needs.
fn prelude(name: &str) -> Option<Vec<String>> {
    let path: &[&str] = match name {
        // `print` ends the line, as in Python. `std.io.print` writes without one.
        "print" => &["std", "io", "println"],
        "assert" => &["std", "test", "assert"],
        "assert_eq" => &["std", "test", "assert_eq"],
        "read_csv" => &["std", "table", "read_csv"],
        "read_parquet" => &["std", "table", "read_parquet"],
        "read_ipc" => &["std", "table", "read_ipc"],
        "abs" => &["std", "math", "abs"],
        "min" => &["std", "math", "min"],
        "max" => &["std", "math", "max"],
        "float" => &["std", "math", "to_float"],
        "int" => &["std", "math", "to_int"],
        "str" => &["std", "text", "to_text"],
        "range" => &["std", "iter", "range"],
        _ => return None,
    };
    Some(path.iter().map(|s| (*s).to_string()).collect())
}

/// The expression a `for` loop goes over (the loop is known to have one).
fn iter_source(iter: &ast::ForIter) -> ast::Expr {
    match iter {
        ast::ForIter::Expr(e) => (**e).clone(),
        ast::ForIter::Range { start, .. } => (**start).clone(),
    }
}

/// The candidate closest to `name` by edit distance (at most 2, and well under the name's length).
fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (name.chars().count() / 3).clamp(1, 2);
    candidates
        .into_iter()
        .filter(|c| *c != name && !c.starts_with('_'))
        .map(|c| (edit_distance(name, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

/// Edit distance where swapping two neighbouring letters counts once (`widht` -> `width`).
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

impl Checker {
    /// Like `err`, with a "did you mean" note when `hint` names a close match.
    fn err_hint(&mut self, span: Span, msg: impl Into<String>, hint: Option<&str>) {
        let mut d = Diagnostic::error(span, msg);
        if let Some(h) = hint {
            d = d.with_note(format!("did you mean `{h}`?"));
        }
        self.diags.push(d);
    }

    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.diags.push(Diagnostic::error(span, msg));
    }

    fn run(&mut self, program: &ast::Program) -> hir::Program {
        let mut out = hir::Program::default();
        self.reject_generic_async(program);

        for item in &program.items {
            if let ast::Item::Import(i) = item {
                let base: Vec<String> = i.path.iter().map(|s| s.node.clone()).collect();
                let mut bring_in =
                    |this: &mut Self, name: &ast::Ident, local: &ast::Ident, full: Vec<String>| {
                        let joined = full.join("::");
                        let is_std = full.first().is_some_and(|f| f == "std");
                        let known = stdlib::function(&joined).is_some()
                            || stdlib::is_type(&joined)
                            || stdlib::is_module(&joined);
                        if is_std && !known {
                            this.err(name.span.clone(), format!("unresolved import `{joined}`"));
                        }
                        this.imports.insert(local.node.clone(), full.clone());
                        out.imports.push(full);
                    };
                if i.group.is_empty() {
                    if let Some(last) = i.path.last() {
                        bring_in(self, last, &i.local_name(0, last), base);
                    }
                } else {
                    for (n, name) in i.group.iter().enumerate() {
                        let mut full = base.clone();
                        full.push(name.node.clone());
                        bring_in(self, name, &i.local_name(n, name), full);
                    }
                }
            }
        }

        // Declare struct names first so fields and signatures may refer to any struct.
        let mut declared = HashSet::new();
        // Generic definitions are kept as written and instantiated where they are used.
        for item in &program.items {
            match item {
                ast::Item::Struct(s) if !s.generics.is_empty() => {
                    if !declared.insert(s.name.node.clone()) {
                        self.err(
                            s.name.span.clone(),
                            format!("type `{}` is defined twice", s.name.node),
                        );
                    }
                    self.generic_structs.insert(s.name.node.clone(), s.clone());
                }
                ast::Item::Enum(en) if !en.generics.is_empty() => {
                    if !declared.insert(en.name.node.clone()) {
                        self.err(
                            en.name.span.clone(),
                            format!("`{}` is defined twice", en.name.node),
                        );
                    }
                    self.generic_enums.insert(en.name.node.clone(), en.clone());
                    self.record_labels(en);
                }
                ast::Item::Alias(a) => {
                    if !declared.insert(a.name.node.clone()) {
                        self.err(
                            a.name.span.clone(),
                            format!("`{}` is defined twice", a.name.node),
                        );
                    } else {
                        self.aliases.insert(a.name.node.clone(), a.clone());
                    }
                }
                _ => {}
            }
        }
        for item in &program.items {
            if let ast::Item::Struct(s) = item
                && s.generics.is_empty()
            {
                if !declared.insert(s.name.node.clone()) {
                    self.err(
                        s.name.span.clone(),
                        format!("type `{}` is defined twice", s.name.node),
                    );
                } else {
                    self.struct_fields.insert(s.name.node.clone(), Vec::new());
                }
            }
        }
        for item in &program.items {
            if let ast::Item::Enum(en) = item
                && en.generics.is_empty()
            {
                if !declared.insert(en.name.node.clone()) {
                    self.err(
                        en.name.span.clone(),
                        format!("`{}` is defined twice", en.name.node),
                    );
                } else {
                    self.enums.insert(en.name.node.clone(), Vec::new());
                }
            }
        }
        for item in &program.items {
            if let ast::Item::Trait(t) = item {
                if !declared.insert(t.name.node.clone()) {
                    self.err(
                        t.name.span.clone(),
                        format!("`{}` is defined twice", t.name.node),
                    );
                } else {
                    let methods = t
                        .fns
                        .iter()
                        .map(|f| (f.name.node.clone(), f.required))
                        .collect();
                    self.traits.insert(t.name.node.clone(), methods);
                }
            }
        }
        let mut done = HashSet::new();
        for item in &program.items {
            if let ast::Item::Struct(s) = item
                && s.generics.is_empty()
            {
                if !done.insert(s.name.node.clone()) {
                    continue;
                }
                let mut fields: Vec<(String, Ty)> = Vec::new();
                for f in &s.fields {
                    if fields.iter().any(|(n, _)| *n == f.name.node) {
                        self.err(
                            f.name.span.clone(),
                            format!("field `{}` is declared twice", f.name.node),
                        );
                        continue;
                    }
                    let ty = self.resolve_type(&f.ty);
                    fields.push((f.name.node.clone(), ty));
                }
                self.struct_fields
                    .insert(s.name.node.clone(), fields.clone());
                out.structs.push(hir::StructDef {
                    name: s.name.node.clone(),
                    name_span: s.name.span.clone(),
                    is_pub: s.is_pub,
                    fields,
                    span: s.span.clone(),
                });
            }
        }

        let mut enum_done = HashSet::new();
        for item in &program.items {
            let ast::Item::Enum(en) = item else { continue };
            if !en.generics.is_empty() {
                continue;
            }
            if !enum_done.insert(en.name.node.clone()) {
                continue;
            }
            self.record_labels(en);
            let mut variants: Vec<(String, Vec<Ty>)> = Vec::new();
            for v in &en.variants {
                if variants.iter().any(|(n, _)| *n == v.name.node) {
                    self.err(
                        v.name.span.clone(),
                        format!("variant `{}` is declared twice", v.name.node),
                    );
                    continue;
                }
                let tys = v.fields.iter().map(|t| self.resolve_type(t)).collect();
                variants.push((v.name.node.clone(), tys));
            }
            // `?` has to know which variant takes an error of a type.
            let marked: Vec<(&ast::Variant, &Ty)> = en
                .variants
                .iter()
                .filter(|v| v.from && v.fields.len() == 1)
                .filter_map(|v| {
                    variants
                        .iter()
                        .find(|(n, _)| *n == v.name.node)
                        .and_then(|(_, tys)| tys.first())
                        .map(|t| (v, t))
                })
                .collect();
            for (i, (v, t)) in marked.iter().enumerate() {
                if marked[..i]
                    .iter()
                    .any(|(_, u)| u.accepts(t) && t.accepts(u))
                {
                    self.err(
                        v.name.span.clone(),
                        format!("two variants marked `from` hold a `{t}`; `?` could not tell which one to use"),
                    );
                }
            }
            self.enums.insert(en.name.node.clone(), variants.clone());
            out.enums.push(hir::EnumDef {
                name: en.name.node.clone(),
                name_span: en.name.span.clone(),
                is_pub: en.is_pub,
                variants,
                span: en.span.clone(),
            });
        }

        self.structs_resolved = true;

        // Every function: the free ones, and the methods of `impl` blocks under the name
        // `Type::method`.
        let mut fns: Vec<(ast::Fn, Option<String>)> = Vec::new();
        for item in &program.items {
            match item {
                ast::Item::Fn(f) if !f.generics.is_empty() => {
                    if self
                        .generic_fns
                        .insert(f.name.node.clone(), f.clone())
                        .is_some()
                    {
                        self.err(
                            f.name.span.clone(),
                            format!("function `{}` is defined twice", f.name.node),
                        );
                    }
                }
                ast::Item::Impl(imp) if !imp.generics.is_empty() => {
                    if self.generic_structs.contains_key(&imp.ty.node)
                        || self.generic_enums.contains_key(&imp.ty.node)
                    {
                        self.generic_impls
                            .entry(imp.ty.node.clone())
                            .or_default()
                            .push(imp.clone());
                    } else {
                        self.err(
                            imp.ty.span.clone(),
                            format!("cannot find the generic type or enum `{}`", imp.ty.node),
                        );
                    }
                }
                ast::Item::Fn(f) => fns.push((f.clone(), None)),
                ast::Item::Trait(t) => {
                    for f in &t.fns {
                        let mut f = f.clone();
                        f.name.node = format!("{}::{}", t.name.node, f.name.node);
                        self.methods
                            .insert(f.name.node.clone(), t.name.node.clone());
                        fns.push((f, Some(t.name.node.clone())));
                    }
                }
                ast::Item::Impl(imp) => {
                    let ty = imp.ty.node.clone();
                    if !self.struct_fields.contains_key(&ty) && !self.enums.contains_key(&ty) {
                        let message = if self.generic_structs.contains_key(&ty)
                            || self.generic_enums.contains_key(&ty)
                        {
                            let params = self
                                .generic_structs
                                .get(&ty)
                                .map(|s| &s.generics)
                                .or_else(|| self.generic_enums.get(&ty).map(|e| &e.generics))
                                .map(|g| {
                                    g.iter()
                                        .map(|p| p.name.node.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                })
                                .unwrap_or_default();
                            format!("`{ty}` has type parameters; write `extend {ty}[{params}]:`")
                        } else {
                            format!("cannot find the type or enum `{ty}` to extend")
                        };
                        self.err(imp.ty.span.clone(), message);
                        continue;
                    }
                    if let Some(tr) = &imp.trait_name {
                        if self.traits.contains_key(&tr.node) {
                            self.impls.insert((tr.node.clone(), ty.clone()));
                            self.impl_spans
                                .push((tr.node.clone(), ty.clone(), imp.span.clone()));
                        } else {
                            self.err(
                                tr.span.clone(),
                                format!("cannot find the trait `{}`", tr.node),
                            );
                            continue;
                        }
                    }
                    // Methods with type parameters of their own wait for a call that picks them.
                    let (own, plain): (Vec<&ast::Fn>, Vec<&ast::Fn>) =
                        imp.fns.iter().partition(|f| !f.generics.is_empty());
                    if !own.is_empty() {
                        if imp.trait_name.is_some() {
                            for f in &own {
                                self.err(
                                    f.name.span.clone(),
                                    "a method of a trait cannot have type parameters of its own",
                                );
                            }
                        } else {
                            let mut only = imp.clone();
                            only.fns = own.into_iter().cloned().collect();
                            self.generic_impls.entry(ty.clone()).or_default().push(only);
                        }
                    }
                    for f in plain {
                        let mut f = f.clone();
                        f.name.node = format!("{ty}::{}", f.name.node);
                        if let Some(tr) = &imp.trait_name {
                            self.impl_of_fn.insert(f.name.node.clone(), tr.node.clone());
                        }
                        self.methods.insert(f.name.node.clone(), ty.clone());
                        fns.push((f, Some(ty.clone())));
                    }
                }
                _ => {}
            }
        }

        // Function signatures.
        let mut sig_done = HashSet::new();
        for (f, impl_ty) in &fns {
            if !sig_done.insert(f.name.node.clone()) {
                self.err(
                    f.name.span.clone(),
                    format!("function `{}` is defined twice", f.name.node),
                );
                continue;
            }
            let in_trait = impl_ty
                .as_ref()
                .is_some_and(|t| self.traits.contains_key(t));
            if f.required && !in_trait {
                self.err(
                    f.name.span.clone(),
                    format!("function `{}` needs a body", f.name.node),
                );
            }
            self.impl_ty = impl_ty.clone();
            let params = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
            let mut ret = f.ret.as_ref().map_or(Ty::Unit, |t| self.resolve_type(t));
            self.impl_ty = None;
            // Calling an `async fn` gives a task; its body is checked against what it gives.
            if f.is_async {
                self.async_inner.insert(f.name.node.clone(), ret.clone());
                ret = Ty::Task(Box::new(ret));
            }
            self.fn_sigs.insert(f.name.node.clone(), (params, ret));
        }
        self.adopt_trait_returns(&mut fns);
        self.check_trait_impls(&fns);

        self.infer_returns(&mut fns);

        // Globals, in source order.
        for item in &program.items {
            if let ast::Item::Let(l) = item {
                let annotated = l.ty.as_ref().map(|t| self.resolve_type(t));
                self.ret = Ty::Unknown;
                self.scopes.clear();
                let init = self.expr(&l.init, annotated.as_ref());
                let init = match &annotated {
                    Some(t) => self.coerce(init, t),
                    None => init,
                };
                let ty = annotated.unwrap_or_else(|| init.ty.clone());
                let mut init = init;
                self.finalize_expr(&mut init);
                let ty = self.finalize_ty(&ty);
                if self
                    .globals
                    .insert(l.name.node.clone(), ty.clone())
                    .is_some()
                {
                    self.err(
                        l.name.span.clone(),
                        format!("global `{}` is defined twice", l.name.node),
                    );
                }
                out.globals.push(hir::Global {
                    name: l.name.node.clone(),
                    ty,
                    init,
                    span: l.span.clone(),
                });
            }
        }

        self.infer_returns(&mut fns);

        let mut lowered = HashSet::new();
        for (f, impl_ty) in &fns {
            if f.required {
                continue;
            }
            if lowered.insert(f.name.node.clone()) {
                self.impl_ty = impl_ty.clone();
                out.fns.push(self.lower_fn(f));
                self.impl_ty = None;
            }
        }
        // Bodies of the instances created along the way (which may create more).
        while let Some(p) = self.pending.pop() {
            self.type_params = p.subst;
            self.impl_ty = p.impl_ty;
            out.fns.push(self.lower_fn(&p.f));
            self.type_params.clear();
            self.impl_ty = None;
        }
        out.structs.append(&mut self.anon_structs);
        out.enums.append(&mut self.anon_enums);
        out
    }

    /// A method of a trait implementation that leaves out `->` returns what the trait declares for
    /// it: `fn area(self):` in `type Circle(Shape)` is `fn area(self) -> Float:` when `Shape` says so.
    /// The body is then checked against that type like any other.
    fn adopt_trait_returns(&mut self, fns: &mut [(ast::Fn, Option<String>)]) {
        for (f, impl_ty) in fns.iter_mut() {
            let Some(ty) = impl_ty.as_deref() else {
                continue;
            };
            let Some(trait_name) = self.impl_of_fn.get(&f.name.node).cloned() else {
                continue;
            };
            let Some(method) = f.name.node.strip_prefix(&format!("{ty}::")) else {
                continue;
            };
            let declared = self
                .fn_sigs
                .get(&format!("{trait_name}::{method}"))
                .map(|(_, ret)| ret.clone());
            let concrete = if self.enums.contains_key(ty) {
                Ty::Enum(ty.to_string())
            } else {
                Ty::Struct(ty.to_string())
            };
            if f.ret.is_none()
                && !f.required
                && let Some(ret) = declared.filter(|r| *r != Ty::Unit)
                && let Some(sig) = self.fn_sigs.get_mut(&f.name.node)
            {
                sig.1 = Self::with_self(&ret, &trait_name, &concrete);
                telvax_ast::tail::tailify_block(&mut f.body);
            }
        }
    }

    /// Gives a function that has no `->` the type of its final expression, so `fn double(x: Int):
    /// x * 2` returns an `Int`. Each body is checked in a copy of the checker; a function that
    /// calls another one without `->` waits for it, so this repeats until nothing changes.
    /// Functions whose signature is fixed elsewhere or must be written (`main`, tests, endpoints,
    /// generics, and methods that implement a trait) and bodies with errors keep returning
    /// nothing. It runs before the globals, which may call these functions, and again after them,
    /// for the functions that read a global.
    fn infer_returns(&mut self, fns: &mut [(ast::Fn, Option<String>)]) {
        let mut open: Vec<usize> = (0..fns.len())
            .filter(|&i| {
                let (f, impl_ty) = &fns[i];
                let own_method = impl_ty.as_ref().is_none_or(|t| {
                    !self.traits.contains_key(t) && !self.impl_of_fn.contains_key(&f.name.node)
                });
                own_method
                    && f.ret.is_none()
                    && !f.required
                    && f.generics.is_empty()
                    && f.attrs.is_empty()
                    && f.name.node != "main"
                    && !f.name.node.starts_with("test_")
                    && self
                        .fn_sigs
                        .get(&f.name.node)
                        .is_some_and(|(_, ret)| *ret == Ty::Unit)
            })
            .collect();
        loop {
            let before = open.len();
            open.retain(|&i| {
                let mut f = fns[i].0.clone();
                telvax_ast::tail::tailify_block(&mut f.body);
                let mut copy = self.clone();
                let known = copy.diags.len();
                if let Some(sig) = copy.fn_sigs.get_mut(&f.name.node) {
                    sig.1 = Ty::Unknown;
                }
                copy.impl_ty = fns[i].1.clone();
                let ty = copy.lower_fn(&f).body.ty;
                if copy.diags.len() > known || matches!(ty, Ty::Unit | Ty::Never | Ty::Unknown) {
                    return true;
                }
                if let Some(sig) = self.fn_sigs.get_mut(&f.name.node) {
                    sig.1 = ty;
                }
                fns[i].0 = f;
                false
            });
            if open.len() == before {
                break;
            }
        }
    }

    // ---- types -------------------------------------------------------------------------

    fn resolve_type(&mut self, t: &ast::Type) -> Ty {
        let names: Vec<&str> = t.path.iter().map(|i| i.node.as_str()).collect();
        if names.len() > 1 {
            if names[0] == "std" || self.imports.contains_key(names[0]) {
                return Ty::Named(names.join("::"));
            }
            self.err(
                t.span.clone(),
                format!("cannot find type `{}`", names.join(".")),
            );
            return Ty::Unknown;
        }
        let name = names[0];
        if let Some(t) = self.type_params.get(name) {
            return t.clone();
        }
        let type_args: Vec<&ast::Type> = t
            .args
            .iter()
            .filter_map(|a| match a {
                ast::TypeArg::Type(t) => Some(t),
                ast::TypeArg::Shape(_) | ast::TypeArg::Row(_) => None,
            })
            .collect();
        let no_args = |this: &mut Self| {
            if !t.args.is_empty() {
                this.err(
                    t.span.clone(),
                    format!("type `{name}` takes no type arguments"),
                );
            }
        };

        if let Some(alias) = self.aliases.get(name).cloned() {
            if self.alias_stack.iter().any(|a| a == name) {
                self.err(
                    t.span.clone(),
                    format!("`type {name}` is defined in terms of itself"),
                );
                return Ty::Unknown;
            }
            let args: Vec<Ty> = type_args.iter().map(|a| self.resolve_type(a)).collect();
            if args.len() != alias.generics.len() {
                self.err(
                    t.span.clone(),
                    format!(
                        "`{name}` takes {} type argument(s), found {}",
                        alias.generics.len(),
                        args.len()
                    ),
                );
                return Ty::Unknown;
            }
            let bind = alias
                .generics
                .iter()
                .map(|g| g.name.node.clone())
                .zip(args)
                .collect();
            let saved = std::mem::replace(&mut self.type_params, bind);
            self.alias_stack.push(name.to_string());
            let out = self.resolve_type(&alias.ty);
            self.alias_stack.pop();
            self.type_params = saved;
            return out;
        }
        if let Some(p) = Prim::from_name(name) {
            no_args(self);
            return Ty::Prim(p);
        }
        match name {
            "Tensor" => self.resolve_tensor(t),
            "Table" if matches!(t.args.as_slice(), [ast::TypeArg::Row(_)]) => {
                let ast::TypeArg::Row(fields) = &t.args[0] else {
                    unreachable!("matched above")
                };
                self.row_type(fields, &t.span)
            }
            "Table" => match type_args.as_slice() {
                [inner] if t.args.len() == 1 => match self.resolve_type(inner) {
                    Ty::Struct(sname) => {
                        self.check_table_schema(&sname, &t.span);
                        Ty::Table(sname)
                    }
                    Ty::Unknown => Ty::Unknown,
                    other => {
                        self.err(
                            inner.span.clone(),
                            format!("a table's schema must be a type, found `{other}`"),
                        );
                        Ty::Unknown
                    }
                },
                _ => {
                    self.err(
                        t.span.clone(),
                        "`Table` takes one type argument: `Table<Row>`",
                    );
                    Ty::Unknown
                }
            },
            "Vec" | "Vector" => match type_args.as_slice() {
                [] if t.args.is_empty() => Ty::Vec(Box::new(Ty::Unknown)),
                [inner] if t.args.len() == 1 => Ty::Vec(Box::new(self.resolve_type(inner))),
                _ => {
                    self.err(
                        t.span.clone(),
                        format!("`{name}` takes at most one type argument"),
                    );
                    Ty::Vec(Box::new(Ty::Unknown))
                }
            },
            "Self" => match self.impl_ty.clone() {
                Some(t) if self.traits.contains_key(&t) => Ty::Trait(t),
                Some(t) if self.enums.contains_key(&t) => Ty::Enum(t),
                Some(t)
                    if self
                        .instance_args
                        .get(&t)
                        .is_some_and(|(base, _)| self.generic_enums.contains_key(base)) =>
                {
                    Ty::Enum(t)
                }
                Some(t) => Ty::Struct(t),
                None => {
                    self.err(t.span.clone(), "`Self` can only be used inside a `type`");
                    Ty::Unknown
                }
            },
            "(unit)" => Ty::Unit,
            "(fn)" => {
                let mut tys: Vec<Ty> = type_args.iter().map(|a| self.resolve_type(a)).collect();
                let ret = tys.pop().unwrap_or(Ty::Unit);
                Ty::Fn(tys, Box::new(ret))
            }
            "(tuple)" => Ty::Tuple(type_args.iter().map(|a| self.resolve_type(a)).collect()),
            "Task" => match type_args.as_slice() {
                [inner] if t.args.len() == 1 => Ty::Task(Box::new(self.resolve_type(inner))),
                _ => {
                    self.err(
                        t.span.clone(),
                        "`Task` takes one type argument: `Task[Int]`",
                    );
                    Ty::Unknown
                }
            },
            "Option" => match type_args.as_slice() {
                [inner] if t.args.len() == 1 => Ty::Option(Box::new(self.resolve_type(inner))),
                _ => {
                    self.err(t.span.clone(), "`Option` takes one type argument: `T?`");
                    Ty::Unknown
                }
            },
            "Map" => match type_args.as_slice() {
                [k, v] if t.args.len() == 2 => {
                    let key = self.resolve_type(k);
                    if !matches!(&key, Ty::Prim(p) if p.is_int() || p.is_text() || *p == Prim::Bool)
                        && !matches!(key, Ty::Unknown)
                    {
                        self.err(
                            k.span.clone(),
                            format!("`Map` keys must be a Text, an Int or a Bool, found `{key}`"),
                        );
                    }
                    Ty::Map(Box::new(key), Box::new(self.resolve_type(v)))
                }
                _ => {
                    self.err(
                        t.span.clone(),
                        "`Map` takes two type arguments: `Map[K, V]`",
                    );
                    Ty::Unknown
                }
            },
            "Result" => match type_args.as_slice() {
                [ok, er] if t.args.len() == 2 => Ty::Result(
                    Box::new(self.resolve_type(ok)),
                    Box::new(self.resolve_type(er)),
                ),
                _ => {
                    self.err(
                        t.span.clone(),
                        "`Result` takes two type arguments: `Result<T, E>`",
                    );
                    Ty::Unknown
                }
            },
            _ if self.generic_structs.contains_key(name)
                || self.generic_enums.contains_key(name) =>
            {
                let args: Vec<Ty> = type_args.iter().map(|a| self.resolve_type(a)).collect();
                self.instantiate_adt(name, args, &t.span)
            }
            _ if self.struct_fields.contains_key(name) => {
                no_args(self);
                Ty::Struct(name.to_string())
            }
            _ if self.enums.contains_key(name) => {
                no_args(self);
                Ty::Enum(name.to_string())
            }
            _ if self.traits.contains_key(name) => {
                no_args(self);
                Ty::Trait(name.to_string())
            }
            _ if self.imports.contains_key(name) => Ty::Named(name.to_string()),
            _ => {
                let mut known: Vec<&str> = vec![
                    "Int", "Float", "Text", "Bool", "i8", "i16", "i32", "i64", "u8", "u16", "u32",
                    "u64", "f32", "f64", "List", "Map", "Tensor", "Table", "Result",
                ];
                known.extend(self.struct_fields.keys().map(String::as_str));
                known.extend(self.enums.keys().map(String::as_str));
                known.extend(self.imports.keys().map(String::as_str));
                // The spellings other languages use for the types Telvax capitalizes.
                let hint = match name {
                    "int" | "integer" => Some("Int".to_string()),
                    "float" | "double" => Some("Float".to_string()),
                    "str" | "string" | "String" => Some("Text".to_string()),
                    "bool" | "boolean" => Some("Bool".to_string()),
                    "list" | "array" => Some("List".to_string()),
                    "dict" | "map" | "hashmap" => Some("Map".to_string()),
                    _ => closest(name, known).map(str::to_string),
                };
                self.err_hint(
                    t.span.clone(),
                    format!("cannot find type `{name}` in this scope"),
                    hint.as_deref(),
                );
                Ty::Unknown
            }
        }
    }

    /// `Table<{ region: String, revenue: f64 }>`: the row is a struct named by its own text, so
    /// two tables written with the same columns share one type.
    fn row_type(&mut self, fields: &[ast::Field], span: &Span) -> Ty {
        if fields.is_empty() {
            self.err(span.clone(), "a row type needs at least one column");
            return Ty::Unknown;
        }
        let mut columns: Vec<(String, Ty)> = Vec::new();
        for f in fields {
            if columns.iter().any(|(n, _)| *n == f.name.node) {
                self.err(
                    f.name.span.clone(),
                    format!("column `{}` is declared twice", f.name.node),
                );
                continue;
            }
            let ty = self.resolve_type(&f.ty);
            columns.push((f.name.node.clone(), ty));
        }
        self.anon_table(columns, span)
    }

    /// The table type whose row has exactly these columns, registering the row on first use.
    fn anon_table(&mut self, columns: Vec<(String, Ty)>, span: &Span) -> Ty {
        let name = format!(
            "{{ {} }}",
            columns
                .iter()
                .map(|(n, t)| format!("{n}: {t}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        if !self.struct_fields.contains_key(&name) {
            self.struct_fields.insert(name.clone(), columns.clone());
            self.anon_structs.push(hir::StructDef {
                name: name.clone(),
                name_span: span.clone(),
                is_pub: false,
                fields: columns,
                span: span.clone(),
            });
        }
        self.check_table_schema(&name, span);
        Ty::Table(name)
    }

    /// The names of a list of text written in place (`["a", "b"]`), or an error on `what`.
    fn literal_names(
        &mut self,
        e: &hir::Expr,
        what: &str,
        example: &str,
    ) -> Option<Vec<(String, Span)>> {
        let hir::ExprKind::Array(items) = &e.kind else {
            self.err(
                e.span.clone(),
                format!("write {what} as a list in place, e.g. `{example}`"),
            );
            return None;
        };
        let mut out = Vec::with_capacity(items.len());
        for a in items {
            let hir::ExprKind::Lit(hir::Lit::Str(name)) = &a.kind else {
                self.err(
                    a.span.clone(),
                    format!("write {what} as text in place, e.g. `{example}`"),
                );
                return None;
            };
            out.push((name.clone(), a.span.clone()));
        }
        Some(out)
    }

    /// `t.group_by(["region"], ["units:sum", "price:mean"])`: one row per distinct value of the
    /// key columns, then one column `<column>_<function>` for each aggregation. `sum` of an
    /// integer column is an `Int`, of a decimal one a `Float`; `mean` is a `Float`; `min` and
    /// `max` keep the column type; `count` is an `Int` and takes any column.
    fn group_by_type(&mut self, args: &[hir::Expr], span: &Span) -> Ty {
        let [table, keys, aggs] = args else {
            return Ty::Unknown;
        };
        let Ty::Table(schema) = &table.ty else {
            return Ty::Unknown;
        };
        let Some(fields) = self.struct_fields.get(schema).cloned() else {
            return Ty::Unknown;
        };
        let example = "t.group_by([\"region\"], [\"units:sum\"])";
        let Some(keys) = self.literal_names(keys, "the key columns", example) else {
            return Ty::Unknown;
        };
        let Some(aggs) = self.literal_names(aggs, "the aggregations", example) else {
            return Ty::Unknown;
        };
        if keys.is_empty() || aggs.is_empty() {
            self.err(
                span.clone(),
                "`group_by` needs at least one key column and one aggregation",
            );
            return Ty::Unknown;
        }
        let known: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
        let find = |this: &mut Self, column: &str, at: &Span| -> Option<Prim> {
            let Some((_, ty)) = fields.iter().find(|(n, _)| n == column) else {
                let hint = closest(column, known.iter().copied())
                    .map(|c| format!("did you mean `{c}`?"))
                    .unwrap_or_else(|| format!("the columns are {}", known.join(", ")));
                this.err(
                    at.clone(),
                    format!("`Table[{schema}]` has no column `{column}`; {hint}"),
                );
                return None;
            };
            match ty {
                Ty::Prim(p) => Some(*p),
                other => {
                    this.err(
                        at.clone(),
                        format!(
                            "column `{column}` is `{other}`; `group_by` needs a column without `?`"
                        ),
                    );
                    None
                }
            }
        };
        let mut columns: Vec<(String, Ty)> = Vec::new();
        let mut push = |this: &mut Self, name: String, ty: Ty, at: &Span| -> bool {
            if columns.iter().any(|(n, _)| *n == name) {
                this.err(
                    at.clone(),
                    format!("the result would have column `{name}` twice"),
                );
                return false;
            }
            columns.push((name, ty));
            true
        };
        for (key, at) in &keys {
            let Some(p) = find(self, key, at) else {
                return Ty::Unknown;
            };
            if !push(self, key.clone(), Ty::Prim(p), at) {
                return Ty::Unknown;
            }
        }
        for (spec, at) in &aggs {
            let Some((column, func)) = spec.split_once(':') else {
                self.err(
                    at.clone(),
                    format!("write the aggregation as `column:function`, e.g. `units:sum`, found `{spec}`"),
                );
                return Ty::Unknown;
            };
            let Some(p) = find(self, column, at) else {
                return Ty::Unknown;
            };
            let numeric = p.is_numeric();
            let out = match func {
                "count" => Prim::I64,
                "sum" | "mean" | "min" | "max" if !numeric => {
                    self.err(
                        at.clone(),
                        format!(
                            "`{func}` needs a number column, and `{column}` is `{}`",
                            p.name()
                        ),
                    );
                    return Ty::Unknown;
                }
                "sum" if p.is_int() => Prim::I64,
                "sum" | "mean" => Prim::F64,
                "min" | "max" => p,
                other => {
                    let hint = closest(other, ["sum", "mean", "min", "max", "count"])
                        .map_or("use sum, mean, min, max or count".to_string(), |c| {
                            format!("did you mean `{c}`?")
                        });
                    self.err(at.clone(), format!("unknown aggregation `{other}`; {hint}"));
                    return Ty::Unknown;
                }
            };
            if !push(self, format!("{column}_{func}"), Ty::Prim(out), at) {
                return Ty::Unknown;
            }
        }
        self.anon_table(columns, span)
    }

    /// `t.select(["a", "b"])`: the table with only those columns, in that order. The names must
    /// be text written in place so the row type of the result is known before the program runs.
    fn select_type(&mut self, args: &[hir::Expr], span: &Span) -> Ty {
        let [table, columns] = args else {
            return Ty::Unknown;
        };
        let Ty::Table(schema) = &table.ty else {
            return Ty::Unknown;
        };
        let Some(fields) = self.struct_fields.get(schema).cloned() else {
            return Ty::Unknown;
        };
        let hir::ExprKind::Array(names) = &columns.kind else {
            self.err(
                columns.span.clone(),
                "write the column names as a list in place in `select`, e.g. `t.select([\"id\", \"name\"])`",
            );
            return Ty::Unknown;
        };
        let mut picked: Vec<(String, Ty)> = Vec::new();
        for a in names {
            let hir::ExprKind::Lit(hir::Lit::Str(column)) = &a.kind else {
                self.err(
                    a.span.clone(),
                    "write the column name as text in `select`, e.g. `t.select([\"id\", \"name\"])`",
                );
                return Ty::Unknown;
            };
            let Some((_, ty)) = fields.iter().find(|(n, _)| n == column) else {
                let known: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                let hint = closest(column, known.iter().copied())
                    .map(|c| format!("did you mean `{c}`?"))
                    .unwrap_or_else(|| format!("the columns are {}", known.join(", ")));
                self.err(
                    a.span.clone(),
                    format!("`Table[{schema}]` has no column `{column}`; {hint}"),
                );
                return Ty::Unknown;
            };
            if picked.iter().any(|(n, _)| n == column) {
                self.err(
                    a.span.clone(),
                    format!("column `{column}` is selected twice"),
                );
                return Ty::Unknown;
            }
            picked.push((column.clone(), ty.clone()));
        }
        if picked.is_empty() {
            self.err(columns.span.clone(), "`select` needs at least one column");
            return Ty::Unknown;
        }
        self.anon_table(picked, span)
    }

    /// Every column of a table must be a primitive (number, bool or string).
    fn check_table_schema(&mut self, sname: &str, span: &Span) {
        let Some(fields) = self.struct_fields.get(sname).cloned() else {
            return;
        };
        if self.structs_resolved && fields.is_empty() {
            self.err(
                span.clone(),
                format!("`{sname}` has no fields, so it cannot be a table schema"),
            );
        }
        for (name, ty) in fields {
            let base = match &ty {
                Ty::Option(inner) => &**inner,
                other => other,
            };
            if !matches!(base, Ty::Prim(_) | Ty::Unknown) {
                self.err(
                    span.clone(),
                    format!("column `{name}` of `{sname}` has type `{ty}`; table columns must be numbers, bools or strings, optionally followed by `?`"),
                );
            }
        }
    }

    fn resolve_tensor(&mut self, t: &ast::Type) -> Ty {
        let (elem, shape) = match t.args.as_slice() {
            [ast::TypeArg::Type(e), ast::TypeArg::Shape(s)] => (e, s),
            _ => {
                self.err(
                    t.span.clone(),
                    "`Tensor` expects an element type and a shape: `Tensor[f32, N, M]`",
                );
                return Ty::Unknown;
            }
        };
        let elem_ty = self.resolve_type(elem);
        if !matches!(&elem_ty, Ty::Prim(p) if p.is_numeric() || *p == Prim::Bool)
            && elem_ty != Ty::Unknown
        {
            self.err(
                elem.span.clone(),
                format!("tensor element type must be a numeric primitive, found `{elem_ty}`"),
            );
        }
        let mut dims = Vec::new();
        for d in shape {
            match d {
                ast::Dim::Lit(0) => {
                    self.err(
                        t.span.clone(),
                        "tensor dimensions must be greater than zero",
                    );
                    dims.push(Dim::Lit(0));
                }
                ast::Dim::Lit(n) => dims.push(Dim::Lit(*n)),
                ast::Dim::Named(n) => dims.push(Dim::Named(n.node.clone())),
            }
        }
        if dims.is_empty() {
            self.err(
                t.span.clone(),
                "tensor shape must have at least one dimension",
            );
        }
        Ty::Tensor {
            elem: Box::new(elem_ty),
            shape: dims,
        }
    }

    // ---- literal type variables ----------------------------------------------------------

    fn fresh(&mut self, float: bool) -> Ty {
        self.subst.push(None);
        let id = (self.subst.len() - 1) as u32;
        if float {
            Ty::FloatVar(id)
        } else {
            Ty::IntVar(id)
        }
    }

    /// Replaces bound variables (recursively) by what they stand for.
    fn resolve(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::IntVar(v) | Ty::FloatVar(v) => match &self.subst[*v as usize] {
                Some(t) => self.resolve(t),
                None => ty.clone(),
            },
            Ty::Vec(t) => Ty::Vec(Box::new(self.resolve(t))),
            Ty::Result(a, b) => Ty::Result(Box::new(self.resolve(a)), Box::new(self.resolve(b))),
            Ty::Option(t) => Ty::Option(Box::new(self.resolve(t))),
            Ty::Task(t) => Ty::Task(Box::new(self.resolve(t))),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(|t| self.resolve(t)).collect()),
            Ty::Map(k, v) => Ty::Map(Box::new(self.resolve(k)), Box::new(self.resolve(v))),
            Ty::Fn(ps, r) => Ty::Fn(
                ps.iter().map(|p| self.resolve(p)).collect(),
                Box::new(self.resolve(r)),
            ),
            other => other.clone(),
        }
    }

    fn finalize_pat(&mut self, p: &mut hir::Pat) {
        match p {
            hir::Pat::Bind(l) => l.ty = self.finalize_ty(&l.ty),
            hir::Pat::Variant { args, .. } => {
                for a in args {
                    self.finalize_pat(a);
                }
            }
            hir::Pat::Tuple(items) | hir::Pat::Or(items) => {
                items.iter_mut().for_each(|a| self.finalize_pat(a))
            }
            hir::Pat::List {
                before,
                rest,
                after,
            } => {
                before
                    .iter_mut()
                    .chain(rest.as_deref_mut())
                    .chain(after.iter_mut())
                    .for_each(|a| self.finalize_pat(a));
            }
            hir::Pat::Struct { fields, .. } => {
                fields.iter_mut().for_each(|(_, a)| self.finalize_pat(a))
            }
            hir::Pat::Wild | hir::Pat::Lit(_) | hir::Pat::NegInt(_) => {}
        }
    }

    /// Like `resolve`, but also pins still-free literal variables to their defaults.
    fn finalize_ty(&mut self, ty: &Ty) -> Ty {
        match self.resolve(ty) {
            Ty::IntVar(v) => {
                self.subst[v as usize] = Some(Ty::Prim(Prim::I32));
                Ty::Prim(Prim::I32)
            }
            Ty::FloatVar(v) => {
                self.subst[v as usize] = Some(Ty::Prim(Prim::F64));
                Ty::Prim(Prim::F64)
            }
            Ty::Vec(t) => Ty::Vec(Box::new(self.finalize_ty(&t))),
            Ty::Option(t) => Ty::Option(Box::new(self.finalize_ty(&t))),
            Ty::Task(t) => Ty::Task(Box::new(self.finalize_ty(&t))),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(|t| self.finalize_ty(t)).collect()),
            Ty::Map(k, v) => Ty::Map(
                Box::new(self.finalize_ty(&k)),
                Box::new(self.finalize_ty(&v)),
            ),
            Ty::Result(a, b) => Ty::Result(
                Box::new(self.finalize_ty(&a)),
                Box::new(self.finalize_ty(&b)),
            ),
            Ty::Fn(ps, r) => {
                let ps = ps.iter().map(|p| self.finalize_ty(p)).collect();
                Ty::Fn(ps, Box::new(self.finalize_ty(&r)))
            }
            other => other,
        }
    }

    /// Whether `actual` can be used where `expected` is required, binding literal variables.
    fn unify(&mut self, expected: &Ty, actual: &Ty) -> bool {
        let (a, b) = (self.resolve(expected), self.resolve(actual));
        match (&a, &b) {
            (Ty::IntVar(x), Ty::IntVar(y)) | (Ty::FloatVar(x), Ty::FloatVar(y)) => {
                if x != y {
                    self.subst[*x as usize] = Some(b.clone());
                }
                true
            }
            (Ty::IntVar(x), Ty::Prim(p)) | (Ty::Prim(p), Ty::IntVar(x)) if p.is_int() => {
                self.subst[*x as usize] = Some(Ty::Prim(*p));
                true
            }
            (Ty::FloatVar(x), Ty::Prim(p)) | (Ty::Prim(p), Ty::FloatVar(x)) if p.is_float() => {
                self.subst[*x as usize] = Some(Ty::Prim(*p));
                true
            }
            (Ty::Vec(x), Ty::Vec(y)) => self.unify(x, y),
            (Ty::Option(x), Ty::Option(y)) | (Ty::Task(x), Ty::Task(y)) => self.unify(x, y),
            // A value of a type that implements the trait can be used where the trait is expected.
            (Ty::Trait(t), Ty::Struct(s) | Ty::Enum(s)) => {
                self.impls.contains(&(t.clone(), s.clone()))
            }
            (Ty::Tuple(xs), Ty::Tuple(ys)) if xs.len() == ys.len() => {
                let mut ok = true;
                for (x, y) in xs.iter().zip(ys) {
                    ok &= self.unify(x, y);
                }
                ok
            }
            (Ty::Map(k1, v1), Ty::Map(k2, v2)) => {
                let ok = self.unify(k1, k2);
                self.unify(v1, v2) && ok
            }
            (Ty::Result(a1, b1), Ty::Result(a2, b2)) => {
                let ok = self.unify(a1, a2);
                self.unify(b1, b2) && ok
            }
            (Ty::Fn(p1, r1), Ty::Fn(p2, r2)) if p1.len() == p2.len() => {
                let mut ok = true;
                for (x, y) in p1.iter().zip(p2) {
                    ok &= self.unify(x, y);
                }
                self.unify(r1, r2) && ok
            }
            _ => a.accepts(&b),
        }
    }

    /// Rewrites every type in `e` to its final form, defaulting unconstrained literals.
    fn finalize_expr(&mut self, e: &mut hir::Expr) {
        e.ty = self.finalize_ty(&e.ty);
        use hir::ExprKind as K;
        match &mut e.kind {
            K::Fn(name) if name.starts_with(OPEN_CALL) => *name = self.close_generic_call(name),
            K::Lit(_) | K::Local(_) | K::Global(_) | K::Fn(_) | K::Extern(_) | K::Error => {}
            K::Call { callee, args } => {
                self.finalize_expr(callee);
                args.iter_mut().for_each(|a| self.finalize_expr(a));
            }
            K::Field { base, .. } => self.finalize_expr(base),
            K::Unary { expr, .. } => self.finalize_expr(expr),
            K::Binary { lhs, rhs, .. } => {
                self.finalize_expr(lhs);
                self.finalize_expr(rhs);
            }
            K::Closure { params, body } => {
                for p in params {
                    p.ty = self.finalize_ty(&p.ty);
                }
                self.finalize_expr(body);
            }
            K::StructLit { fields, .. } => {
                fields.iter_mut().for_each(|(_, v)| self.finalize_expr(v))
            }
            K::If {
                cond,
                then,
                otherwise,
            } => {
                self.finalize_expr(cond);
                self.finalize_block(then);
                if let Some(o) = otherwise {
                    self.finalize_expr(o);
                }
            }
            K::Block(b) => self.finalize_block(b),
            K::Return(v) => {
                if let Some(v) = v {
                    self.finalize_expr(v);
                }
            }
            K::Break | K::Continue => {}
            K::Try(inner) | K::Await(inner) => self.finalize_expr(inner),
            K::Tuple(items) => items.iter_mut().for_each(|i| self.finalize_expr(i)),
            K::DynCall { args, .. } => args.iter_mut().for_each(|a| self.finalize_expr(a)),
            K::EnumCtor { args, .. } => {
                for a in args {
                    self.finalize_expr(a);
                }
            }
            K::Match { scrutinee, arms } => {
                self.finalize_expr(scrutinee);
                for arm in arms {
                    self.finalize_pat(&mut arm.pat);
                    if let Some(g) = &mut arm.guard {
                        self.finalize_expr(g);
                    }
                    self.finalize_expr(&mut arm.body);
                }
            }
            K::FString(parts) => {
                for p in parts {
                    for x in p.exprs_mut() {
                        self.finalize_expr(x);
                    }
                }
            }
            K::Assign { place, value } => {
                place.ty = self.finalize_ty(&place.ty);
                place.root_ty = self.finalize_ty(&place.root_ty);
                for pr in &mut place.projs {
                    if let hir::Proj::Index(i) = pr {
                        self.finalize_expr(i);
                    }
                }
                self.finalize_expr(value);
            }
            K::While { cond, body } => {
                self.finalize_expr(cond);
                self.finalize_block(body);
            }
            K::For { var, iter, body } => {
                var.ty = self.finalize_ty(&var.ty);
                match iter {
                    hir::ForIter::Range { start, end, .. } => {
                        self.finalize_expr(start);
                        self.finalize_expr(end);
                    }
                    hir::ForIter::Vec(v) => self.finalize_expr(v),
                }
                self.finalize_block(body);
            }
            K::Index { base, index } => {
                self.finalize_expr(base);
                self.finalize_expr(index);
            }
            K::Array(items) => items.iter_mut().for_each(|i| self.finalize_expr(i)),
        }
    }

    fn finalize_block(&mut self, b: &mut hir::Block) {
        b.ty = self.finalize_ty(&b.ty);
        for s in &mut b.stmts {
            match s {
                hir::Stmt::Let { local, init } => {
                    local.ty = self.finalize_ty(&local.ty);
                    self.finalize_expr(init);
                }
                hir::Stmt::Expr(e) => self.finalize_expr(e),
            }
        }
        if let Some(t) = &mut b.tail {
            self.finalize_expr(t);
        }
    }

    // ---- scopes ------------------------------------------------------------------------

    fn declare(&mut self, name: &ast::Ident, ty: Ty) -> hir::Local {
        self.declare_mut(name, ty, false)
    }

    fn declare_mut(&mut self, name: &ast::Ident, ty: Ty, mutable: bool) -> hir::Local {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.node.clone(), (id, ty.clone(), mutable));
        }
        hir::Local {
            id,
            name: name.node.clone(),
            ty,
            mutable,
            span: name.span.clone(),
        }
    }

    fn lookup_local(&self, name: &str) -> Option<(LocalId, Ty)> {
        self.lookup_local_full(name).map(|(id, ty, _)| (id, ty))
    }

    fn lookup_local_full(&self, name: &str) -> Option<(LocalId, Ty, bool)> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    // ---- functions ---------------------------------------------------------------------

    fn lower_fn(&mut self, f: &ast::Fn) -> hir::FnDef {
        let (param_tys, signature_ret) = self.fn_sigs[&f.name.node].clone();
        let ret = self
            .async_inner
            .get(&f.name.node)
            .cloned()
            .unwrap_or(signature_ret);
        self.in_async = f.is_async;
        self.scopes = vec![HashMap::new()];
        self.ret = ret.clone();
        let mut params = Vec::new();
        for (p, ty) in f.params.iter().zip(param_tys) {
            if params.iter().any(|q: &hir::Local| q.name == p.name.node) {
                self.err(
                    p.name.span.clone(),
                    format!("parameter `{}` is declared twice", p.name.node),
                );
            }
            params.push(self.declare_mut(&p.name, ty, p.mutable));
        }

        let (mut body, body_ty) = self.block(&f.body, Some(&ret));
        if !self.unify(&ret, &body_ty) {
            let span = body
                .tail
                .as_ref()
                .map_or(f.body.span.clone(), |t| t.span.clone());
            let mut d = Diagnostic::error(
                span,
                format!("mismatched types: expected `{ret}`, found `{body_ty}`"),
            );
            if body.tail.is_none() {
                d = d.with_note(
                    "the function body has no final expression and does not end in `return`",
                );
            }
            self.diags.push(d);
        }

        self.finalize_block(&mut body);
        let endpoint = self.endpoint(f, &params, &ret);
        self.scopes.clear();
        self.in_async = false;
        hir::FnDef {
            name: f.name.node.clone(),
            name_span: f.name.span.clone(),
            is_pub: f.is_pub,
            is_async: f.is_async,
            params,
            ret,
            body,
            endpoint,
            span: f.span.clone(),
        }
    }

    fn endpoint(&mut self, f: &ast::Fn, params: &[hir::Local], ret: &Ty) -> Option<Endpoint> {
        let mut found = None;
        for attr in &f.attrs {
            if attr.name.node != "endpoint" {
                self.err(
                    attr.name.span.clone(),
                    format!("unknown attribute `@{}`", attr.name.node),
                );
                continue;
            }
            if found.is_some() {
                self.err(
                    attr.span.clone(),
                    "a function can have only one `@endpoint`",
                );
                continue;
            }
            let mut path = None;
            let mut method = None;
            for (key, value) in &attr.args {
                let text = match &value.node {
                    ast::ExprKind::Lit(ast::Lit::Str(s)) => s.clone(),
                    _ => {
                        self.err(
                            value.span.clone(),
                            format!("`{}` must be a string literal", key.node),
                        );
                        continue;
                    }
                };
                match key.node.as_str() {
                    "path" if path.is_none() => path = Some((text, value.span.clone())),
                    "method" if method.is_none() => method = Some((text, value.span.clone())),
                    "path" | "method" => {
                        self.err(key.span.clone(), format!("`{}` given twice", key.node))
                    }
                    other => self.err(
                        key.span.clone(),
                        format!("unknown `@endpoint` argument `{other}`"),
                    ),
                }
            }
            let Some((path, path_span)) = path else {
                self.err(attr.span.clone(), "`@endpoint` requires a `path` argument");
                continue;
            };
            if !path.starts_with('/') {
                self.err(path_span, "endpoint path must start with `/`");
            }
            let method = match method {
                None => HttpMethod::Get,
                Some((m, span)) => match m.to_ascii_uppercase().as_str() {
                    "GET" => HttpMethod::Get,
                    "POST" => HttpMethod::Post,
                    "PUT" => HttpMethod::Put,
                    "PATCH" => HttpMethod::Patch,
                    "DELETE" => HttpMethod::Delete,
                    _ => {
                        self.err(span, format!("unsupported HTTP method `{m}`"));
                        HttpMethod::Get
                    }
                },
            };
            if !self.endpoints.insert((method, path.clone())) {
                self.err(
                    attr.span.clone(),
                    format!("duplicate endpoint {method:?} `{path}`"),
                );
            }

            let wants_body = matches!(
                method,
                HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch
            );
            match params {
                [] if !wants_body => {}
                [p] if matches!(p.ty, Ty::Struct(_) | Ty::Unknown) => {}
                _ => self.err(
                    f.name.span.clone(),
                    "an endpoint takes at most one parameter, which must be a `type` (the request payload)",
                ),
            }
            if wants_body && params.is_empty() {
                self.err(
                    f.name.span.clone(),
                    format!("{method:?} endpoints need a payload parameter"),
                );
            }
            if !matches!(ret, Ty::Result(..) | Ty::Unknown) {
                self.err(
                    f.ret
                        .as_ref()
                        .map_or(f.name.span.clone(), |t| t.span.clone()),
                    "an endpoint must return `Result<T, E>`",
                );
            }
            found = Some(Endpoint { path, method });
        }
        found
    }

    // ---- blocks and statements --------------------------------------------------------

    fn block(&mut self, b: &ast::Block, expected: Option<&Ty>) -> (hir::Block, Ty) {
        self.scopes.push(HashMap::new());
        let mut stmts = Vec::new();
        let mut diverges = false;
        for s in &b.stmts {
            match s {
                ast::Stmt::Let(l) => {
                    let annotated = l.ty.as_ref().map(|t| self.resolve_type(t));
                    let init = self.expr(&l.init, annotated.as_ref());
                    let init = match &annotated {
                        Some(t) => self.coerce(init, t),
                        None => init,
                    };
                    let ty = annotated.unwrap_or_else(|| init.ty.clone());
                    // The binding is only visible after its initializer.
                    let local = self.declare_mut(&l.name, ty, l.mutable);
                    stmts.push(hir::Stmt::Let { local, init });
                }
                ast::Stmt::Expr(e) => {
                    let e = self.expr(e, None);
                    diverges |= e.ty == Ty::Never;
                    stmts.push(hir::Stmt::Expr(e));
                }
            }
        }
        let tail = b.tail.as_ref().map(|e| Box::new(self.expr(e, expected)));
        self.scopes.pop();
        let ty = match &tail {
            Some(t) => t.ty.clone(),
            None if diverges => Ty::Never,
            None => Ty::Unit,
        };
        (
            hir::Block {
                stmts,
                tail,
                ty: ty.clone(),
            },
            ty,
        )
    }

    // ---- expressions -------------------------------------------------------------------

    /// Reports a mismatch if `e` cannot be used as `expected`.
    fn coerce(&mut self, e: hir::Expr, expected: &Ty) -> hir::Expr {
        if !self.unify(expected, &e.ty) {
            let (want, found) = (self.resolve(expected), self.resolve(&e.ty));
            let mut diag = Diagnostic::error(
                e.span.clone(),
                format!("mismatched types: expected `{want}`, found `{found}`"),
            );
            // A call of an `async fn` where its value is wanted: the `await` was left out.
            if let Ty::Task(inner) = &found
                && want.accepts(inner)
            {
                diag =
                    diag.with_note("a call of an `async fn` gives a task; write `await` to run it");
            }
            self.diags.push(diag);
        }
        e
    }

    fn mk(kind: hir::ExprKind, ty: Ty, span: &Span) -> hir::Expr {
        hir::Expr {
            kind,
            ty,
            span: span.clone(),
        }
    }

    fn expr(&mut self, e: &ast::Expr, expected: Option<&Ty>) -> hir::Expr {
        use ast::ExprKind as K;
        let span = &e.span;
        match &e.node {
            K::Error => Self::mk(hir::ExprKind::Error, Ty::Unknown, span),
            K::Named { name, .. } => {
                self.err(
                    span.clone(),
                    format!(
                        "`{}: ...` can only be written in the arguments of a call",
                        name.node
                    ),
                );
                Self::mk(hir::ExprKind::Error, Ty::Unknown, span)
            }
            K::Assign { target, value } => self.assign(target, value, span),
            K::While { cond, body } => {
                let cond = self.expr(cond, Some(&Ty::bool()));
                let cond = self.coerce(cond, &Ty::bool());
                self.loop_depth += 1;
                let (body, _) = self.block(body, None);
                self.loop_depth -= 1;
                Self::mk(
                    hir::ExprKind::While {
                        cond: Box::new(cond),
                        body,
                    },
                    Ty::Unit,
                    span,
                )
            }
            K::For { var, iter, body } => self.for_loop(var, iter, body, span),
            K::Index { base, index } => {
                let base = self.expr(base, None);
                // A `Map` is indexed by its key type; everything else by an integer.
                let key_ty = match self.resolve(&base.ty) {
                    Ty::Map(k, _) => Some(*k),
                    _ => None,
                };
                let index = self.expr(index, key_ty.as_ref());
                match &key_ty {
                    Some(k) => {
                        if !self.unify(k, &index.ty) {
                            let found = self.resolve(&index.ty);
                            self.err(
                                index.span.clone(),
                                format!("this `Map` has `{k}` keys, found `{found}`"),
                            );
                        }
                    }
                    None => {
                        let index_ok = matches!(self.resolve(&index.ty), Ty::Prim(p) if p.is_int())
                            || matches!(
                                self.resolve(&index.ty),
                                Ty::IntVar(_) | Ty::Unknown | Ty::Never
                            );
                        if !index_ok {
                            self.err(
                                index.span.clone(),
                                format!("indices must be integers, found `{}`", index.ty),
                            );
                        }
                    }
                }
                let ty = match &base.ty {
                    Ty::Vec(t) => (**t).clone(),
                    Ty::Map(_, v) => (**v).clone(),
                    Ty::Prim(p) if p.is_text() => Ty::str(),
                    Ty::Table(s) => Ty::Struct(s.clone()),
                    // Indexing peels the leading dimension: a row of a matrix is a vector.
                    Ty::Tensor { elem, shape } if shape.len() == 1 => (**elem).clone(),
                    Ty::Tensor { elem, shape } => Ty::Tensor {
                        elem: elem.clone(),
                        shape: shape[1..].to_vec(),
                    },
                    Ty::Unknown | Ty::Never => Ty::Unknown,
                    other => {
                        let other = other.clone();
                        self.err(
                            base.span.clone(),
                            format!("cannot index into a value of type `{other}`"),
                        );
                        Ty::Unknown
                    }
                };
                Self::mk(
                    hir::ExprKind::Index {
                        base: Box::new(base),
                        index: Box::new(index),
                    },
                    ty,
                    span,
                )
            }
            K::Array(items) => {
                let want = match expected {
                    Some(Ty::Vec(t)) => Some((**t).clone()),
                    _ => None,
                };
                let mut out: Vec<hir::Expr> = Vec::with_capacity(items.len());
                for it in items {
                    let hint = want.clone().or_else(|| out.first().map(|f| f.ty.clone()));
                    let it = self.expr(it, hint.as_ref());
                    let it = match &hint {
                        Some(h) => self.coerce(it, h),
                        None => it,
                    };
                    out.push(it);
                }
                let elem = want
                    .or_else(|| out.first().map(|f| f.ty.clone()))
                    .unwrap_or(Ty::Unknown);
                Self::mk(hir::ExprKind::Array(out), Ty::Vec(Box::new(elem)), span)
            }
            K::Lit(l) => {
                let want = expected.map(|t| self.resolve(t));
                let ty = match l {
                    ast::Lit::Int(_) => match want {
                        Some(Ty::Prim(p)) if p.is_numeric() => Ty::Prim(p),
                        Some(v @ (Ty::IntVar(_) | Ty::FloatVar(_))) => v,
                        _ => self.fresh(false),
                    },
                    ast::Lit::Float(_) => match want {
                        Some(Ty::Prim(p)) if p.is_float() => Ty::Prim(p),
                        Some(v @ Ty::FloatVar(_)) => v,
                        _ => self.fresh(true),
                    },
                    ast::Lit::Str(_) => Ty::str(),
                    ast::Lit::Bool(_) => Ty::bool(),
                };
                Self::mk(hir::ExprKind::Lit(l.clone()), ty, span)
            }
            K::Path(p) if self.is_option_ctor(p, "None") => {
                let inner = match expected {
                    Some(Ty::Option(t)) => (**t).clone(),
                    _ => Ty::Unknown,
                };
                Self::mk(
                    hir::ExprKind::EnumCtor {
                        enum_name: "Option".into(),
                        variant: "None".into(),
                        args: vec![],
                    },
                    Ty::Option(Box::new(inner)),
                    span,
                )
            }
            K::Path(p) => match self.variant_then_fields(p, span) {
                Some(chain) => self.expr(&chain, expected),
                None => self.path(p, expected, span),
            },
            K::FString(parts) => {
                let parts = parts
                    .iter()
                    .map(|p| match p {
                        ast::FPart::Text(t) => hir::FPart::Text(t.clone()),
                        ast::FPart::Expr(e) => hir::FPart::Expr(self.expr(e, None)),
                        ast::FPart::Fmt { expr, repr, spec } => {
                            let expr = self.expr(expr, None);
                            let spec = spec.as_ref().map(|s| {
                                let s = self.expr(s, Some(&Ty::str()));
                                self.coerce(s, &Ty::str())
                            });
                            hir::FPart::Fmt {
                                expr,
                                repr: *repr,
                                spec,
                            }
                        }
                    })
                    .collect();
                Self::mk(hir::ExprKind::FString(parts), Ty::str(), span)
            }
            K::Field { base, name } => {
                let base = self.expr(base, None);
                let ty = self.field_ty(&base, name);
                Self::mk(
                    hir::ExprKind::Field {
                        base: Box::new(base),
                        name: name.node.clone(),
                    },
                    ty,
                    span,
                )
            }
            K::Call { callee, args } => self.call(callee, args, expected, span),
            K::Pipe { lhs, rhs } => self.pipe(lhs, rhs, span),
            K::Unary { op, expr } => {
                let inner = self.expr(expr, expected);
                let shown = self.resolve(&inner.ty);
                let ok = match (op, &shown) {
                    (_, Ty::Unknown) => true,
                    (ast::UnOp::Neg, Ty::IntVar(_) | Ty::FloatVar(_)) => true,
                    (ast::UnOp::Neg, Ty::Prim(p)) => p.is_numeric(),
                    (ast::UnOp::Not, Ty::Prim(Prim::Bool)) => true,
                    _ => false,
                };
                if !ok {
                    self.err(span.clone(), format!("cannot apply `{op:?}` to `{shown}`"));
                }
                let ty = inner.ty.clone();
                Self::mk(
                    hir::ExprKind::Unary {
                        op: *op,
                        expr: Box::new(inner),
                    },
                    ty,
                    span,
                )
            }
            K::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, span),
            K::Closure {
                params,
                types,
                body,
            } => {
                let want: Option<(&Vec<Ty>, &Ty)> = match expected {
                    Some(Ty::Fn(p, r)) if p.len() == params.len() => Some((p, r)),
                    _ => None,
                };
                self.closure_floors.push(self.scopes.len());
                self.scopes.push(HashMap::new());
                let saved = std::mem::take(&mut self.ret);
                let saved_depth = std::mem::take(&mut self.loop_depth);
                // A closure is not an `async fn`: `await` is not allowed in it.
                let saved_async = std::mem::take(&mut self.in_async);
                let locals: Vec<_> = params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let ty = match &types[i] {
                            Some(annotation) => self.resolve_type(annotation),
                            None => want.map_or(Ty::Unknown, |(ps, _)| ps[i].clone()),
                        };
                        self.declare(p, ty)
                    })
                    .collect();
                let body = self.expr(body, want.map(|(_, r)| r));
                self.ret = saved;
                self.loop_depth = saved_depth;
                self.in_async = saved_async;
                self.scopes.pop();
                self.closure_floors.pop();
                let ty = Ty::Fn(
                    locals.iter().map(|l| l.ty.clone()).collect(),
                    Box::new(body.ty.clone()),
                );
                Self::mk(
                    hir::ExprKind::Closure {
                        params: locals,
                        body: Box::new(body),
                    },
                    ty,
                    span,
                )
            }
            K::StructLit { path, fields } => self.struct_lit(path, fields, expected, span),
            K::If {
                cond,
                then,
                otherwise,
            } => {
                let cond = self.expr(cond, Some(&Ty::bool()));
                let cond = self.coerce(cond, &Ty::bool());
                let (then, then_ty) = self.block(then, expected);
                let (otherwise, ty) = match otherwise {
                    None => (None, Ty::Unit),
                    Some(o) => {
                        let o = self.expr(o, expected);
                        let ty = if self.unify(&then_ty, &o.ty) {
                            if then_ty == Ty::Never {
                                o.ty.clone()
                            } else {
                                then_ty.clone()
                            }
                        } else {
                            self.diags.push(Diagnostic::error(
                                o.span.clone(),
                                format!(
                                    "`if` and `else` have incompatible types: `{then_ty}` and `{}`",
                                    o.ty
                                ),
                            ));
                            Ty::Unknown
                        };
                        (Some(Box::new(o)), ty)
                    }
                };
                Self::mk(
                    hir::ExprKind::If {
                        cond: Box::new(cond),
                        then,
                        otherwise,
                    },
                    ty,
                    span,
                )
            }
            K::Block(b) => {
                let (block, ty) = self.block(b, expected);
                Self::mk(hir::ExprKind::Block(block), ty, span)
            }
            K::Match { scrutinee, arms } => self.match_expr(scrutinee, arms, expected, span),
            K::Tuple(items) => {
                let hints: Vec<Option<Ty>> = match expected.map(|e| self.resolve(e)) {
                    Some(Ty::Tuple(ts)) if ts.len() == items.len() => {
                        ts.into_iter().map(Some).collect()
                    }
                    _ => vec![None; items.len()],
                };
                let parts: Vec<hir::Expr> = items
                    .iter()
                    .zip(&hints)
                    .map(|(item, hint)| self.expr(item, hint.as_ref()))
                    .collect();
                let ty = Ty::Tuple(parts.iter().map(|p| p.ty.clone()).collect());
                Self::mk(hir::ExprKind::Tuple(parts), ty, span)
            }
            K::Await(inner) => {
                if !self.in_async {
                    self.err(
                        span.clone(),
                        "`await` can only be used inside an `async fn`",
                    );
                }
                let task = self.expr(inner, None);
                let ty = match self.resolve(&task.ty) {
                    Ty::Task(t) => *t,
                    Ty::Unknown | Ty::Never => Ty::Unknown,
                    other => {
                        self.err(
                            task.span.clone(),
                            format!(
                                "`await` needs a task, found `{other}`; a task is what a call of an `async fn` gives"
                            ),
                        );
                        Ty::Unknown
                    }
                };
                Self::mk(hir::ExprKind::Await(Box::new(task)), ty, span)
            }
            K::Try(inner) => {
                let value = self.expr(inner, None);
                let operand = self.resolve(&value.ty);
                let ret = self.resolve(&self.ret.clone());
                let out = match &operand {
                    Ty::Result(ok, err) => {
                        match &ret {
                            Ty::Result(_, ret_err) => {
                                if !ret_err.accepts(err)
                                    && let Some(variant) = self.error_conversion(ret_err, err)
                                {
                                    let converted =
                                        Self::convert_error(inner, ret_err, &variant, span);
                                    return self.expr(&converted, expected);
                                }
                                if !ret_err.accepts(err) {
                                    let how = if matches!(**ret_err, Ty::Enum(_)) {
                                        format!(
                                            "; mark a variant of `{ret_err}` that holds a `{err}` with `from` (`from Name({err})`), or write `match` and `Err(...)`"
                                        )
                                    } else {
                                        String::new()
                                    };
                                    self.err(
                                        span.clone(),
                                        format!(
                                            "`?` cannot turn the error `{err}` into `{ret_err}`, the error type of this function{how}"
                                        ),
                                    );
                                }
                            }
                            Ty::Unknown | Ty::Never => {}
                            other => self.err(
                                span.clone(),
                                format!(
                                    "`?` can only be used in a function that returns `Result`, this one returns `{other}`"
                                ),
                            ),
                        }
                        (**ok).clone()
                    }
                    Ty::Option(inner_ty) => {
                        if !matches!(ret, Ty::Option(_) | Ty::Unknown | Ty::Never) {
                            self.err(
                                span.clone(),
                                format!(
                                    "`?` on an `Option` can only be used in a function that returns `Option`, this one returns `{ret}`"
                                ),
                            );
                        }
                        (**inner_ty).clone()
                    }
                    Ty::Unknown | Ty::Never => Ty::Unknown,
                    other => {
                        self.err(
                            value.span.clone(),
                            format!("`?` needs a `Result` or an `Option`, found `{other}`"),
                        );
                        Ty::Unknown
                    }
                };
                Self::mk(hir::ExprKind::Try(Box::new(value)), out, span)
            }
            K::Break | K::Continue => {
                let (word, kind) = if matches!(e.node, K::Break) {
                    ("break", hir::ExprKind::Break)
                } else {
                    ("continue", hir::ExprKind::Continue)
                };
                if self.loop_depth == 0 {
                    self.err(span.clone(), format!("`{word}` outside of a loop"));
                }
                Self::mk(kind, Ty::Never, span)
            }
            K::Return(value) => {
                let ret = self.ret.clone();
                let value = match value {
                    Some(v) => {
                        let v = self.expr(v, Some(&ret));
                        Some(Box::new(self.coerce(v, &ret)))
                    }
                    None => {
                        if !ret.accepts(&Ty::Unit) {
                            self.err(
                                span.clone(),
                                format!("`return` needs a value of type `{ret}`"),
                            );
                        }
                        None
                    }
                };
                Self::mk(hir::ExprKind::Return(value), Ty::Never, span)
            }
        }
    }

    /// Resolves an assignment target (`x`, `x.f`, `x[i]`, and combinations) to a [`hir::Place`].
    fn place(&mut self, target: &ast::Expr) -> Option<hir::Place> {
        match &target.node {
            ast::ExprKind::Path(p) if p.len() == 1 => {
                let name = p[0].node.clone();
                let Some((id, ty, mutable)) = self.lookup_local_full(&name) else {
                    if self.globals.contains_key(&name) {
                        self.err(
                            target.span.clone(),
                            format!("cannot assign to the global `{name}`"),
                        );
                    } else {
                        self.diags.push(
                            Diagnostic::error(
                                target.span.clone(),
                                format!("cannot find `{name}` in this scope"),
                            )
                            .with_note(format!(
                                "declare it first with `let {name} = ...`, or `var {name} = ...` if it changes"
                            )),
                        );
                    }
                    return None;
                };
                let captured = self.closure_floors.last().is_some_and(|floor| {
                    self.scopes
                        .iter()
                        .rposition(|s| s.contains_key(&name))
                        .is_some_and(|i| i < *floor)
                });
                // `Vec` and `Map` are shared, so changing their elements from a closure is seen
                // outside; any other captured value is a copy.
                if captured && !matches!(self.resolve(&ty), Ty::Vec(_) | Ty::Map(..) | Ty::Unknown)
                {
                    self.diags.push(
                        Diagnostic::error(
                            target.span.clone(),
                            format!("cannot assign to `{name}` from inside a closure"),
                        )
                        .with_note(
                            "closures capture variables by value, so the change would be lost; \
                             return the new value from the closure instead",
                        ),
                    );
                }
                if !mutable {
                    self.diags.push(
                        Diagnostic::error(
                            target.span.clone(),
                            format!("cannot assign to the immutable variable `{name}`"),
                        )
                        .with_note(format!(
                            "declare it with `var {name}` instead of `let {name}` (loop variables \
                             and pattern names cannot change: copy them into a `var`)"
                        )),
                    );
                }
                Some(hir::Place {
                    local: id,
                    name,
                    projs: vec![],
                    ty: ty.clone(),
                    root_ty: ty,
                    span: target.span.clone(),
                })
            }
            ast::ExprKind::Field { base, name } => {
                let mut place = self.place(base)?;
                match self.resolve(&place.ty) {
                    Ty::Struct(sname) => {
                        let found = self
                            .struct_fields
                            .get(&sname)
                            .and_then(|f| f.iter().find(|(n, _)| *n == name.node))
                            .map(|(_, t)| t.clone());
                        match found {
                            Some(t) => {
                                place.projs.push(hir::Proj::Field {
                                    name: name.node.clone(),
                                    struct_name: sname,
                                });
                                place.ty = t;
                                place.span = target.span.clone();
                                Some(place)
                            }
                            None => {
                                self.err(
                                    name.span.clone(),
                                    format!("no field `{}` on type `{sname}`", name.node),
                                );
                                None
                            }
                        }
                    }
                    other => {
                        self.err(
                            target.span.clone(),
                            format!("cannot assign to a field of `{other}`"),
                        );
                        None
                    }
                }
            }
            ast::ExprKind::Index { base, index } => {
                let mut place = self.place(base)?;
                let key_ty = match self.resolve(&place.ty) {
                    Ty::Map(k, _) => Some(*k),
                    _ => None,
                };
                let index = self.expr(index, key_ty.as_ref());
                let shown = self.resolve(&index.ty);
                match &key_ty {
                    Some(k) => {
                        if !self.unify(k, &index.ty) {
                            self.err(
                                index.span.clone(),
                                format!("this `Map` has `{k}` keys, found `{shown}`"),
                            );
                        }
                    }
                    None => {
                        if !(matches!(shown, Ty::Prim(p) if p.is_int())
                            || matches!(shown, Ty::IntVar(_) | Ty::Unknown | Ty::Never))
                        {
                            self.err(
                                index.span.clone(),
                                format!("indices must be integers, found `{shown}`"),
                            );
                        }
                    }
                }
                match self.resolve(&place.ty) {
                    Ty::Vec(elem) | Ty::Map(_, elem) => {
                        place.projs.push(hir::Proj::Index(Box::new(index)));
                        place.ty = *elem;
                        place.span = target.span.clone();
                        Some(place)
                    }
                    other => {
                        self.err(
                            target.span.clone(),
                            format!("cannot index into a value of type `{other}`"),
                        );
                        None
                    }
                }
            }
            _ => {
                self.err(
                    target.span.clone(),
                    "the left side of `=` must be a variable, a field or an index",
                );
                None
            }
        }
    }

    fn assign(&mut self, target: &ast::Expr, value: &ast::Expr, span: &Span) -> hir::Expr {
        let Some(place) = self.place(target) else {
            let _ = self.expr(value, None);
            return Self::mk(hir::ExprKind::Error, Ty::Unit, span);
        };
        let ty = place.ty.clone();
        let v = self.expr(value, Some(&ty));
        let v = self.coerce(v, &ty);
        Self::mk(
            hir::ExprKind::Assign {
                place,
                value: Box::new(v),
            },
            Ty::Unit,
            span,
        )
    }

    fn for_loop(
        &mut self,
        var: &ast::Ident,
        iter: &ast::ForIter,
        body: &ast::Block,
        span: &Span,
    ) -> hir::Expr {
        let is_int = |t: &Ty| {
            matches!(t, Ty::Prim(p) if p.is_int())
                || matches!(t, Ty::IntVar(_) | Ty::Unknown | Ty::Never)
        };
        let (iter, var_ty) = match iter {
            ast::ForIter::Range {
                start,
                end,
                inclusive,
            } => {
                let mut s = self.expr(start, None);
                let e = self.expr(end, Some(&s.ty));
                // An untyped literal start adopts the type of the end bound.
                if matches!(start.node, ast::ExprKind::Lit(_))
                    && s.ty != e.ty
                    && e.ty != Ty::Unknown
                {
                    s = self.expr(start, Some(&e.ty));
                }
                for b in [&s, &e] {
                    if !is_int(&self.resolve(&b.ty)) {
                        self.err(
                            b.span.clone(),
                            format!("range bounds must be integers, found `{}`", b.ty),
                        );
                    }
                }
                if !self.unify(&s.ty, &e.ty) {
                    self.err(
                        e.span.clone(),
                        format!(
                            "range bounds have different types: `{}` and `{}`",
                            s.ty, e.ty
                        ),
                    );
                }
                let ty = if s.ty == Ty::Unknown {
                    e.ty.clone()
                } else {
                    s.ty.clone()
                };
                (
                    hir::ForIter::Range {
                        start: Box::new(s),
                        end: Box::new(e),
                        inclusive: *inclusive,
                    },
                    ty,
                )
            }
            ast::ForIter::Expr(v) => {
                let diags_before = self.diags.len();
                let mut v = self.expr(v, None);
                // `for key in m` goes over the keys, as in Python.
                if matches!(self.resolve(&v.ty), Ty::Map(..)) && self.diags.len() == diags_before {
                    let keys = ast::Expr {
                        node: ast::ExprKind::Call {
                            callee: Box::new(ast::Expr {
                                node: ast::ExprKind::Field {
                                    base: Box::new(iter_source(iter)),
                                    name: ast::Ident {
                                        node: "keys".into(),
                                        span: v.span.clone(),
                                    },
                                },
                                span: v.span.clone(),
                            }),
                            args: Vec::new(),
                        },
                        span: v.span.clone(),
                    };
                    v = self.expr(&keys, None);
                }
                let ty = match &v.ty {
                    Ty::Vec(t) => (**t).clone(),
                    Ty::Table(s) => Ty::Struct(s.clone()),
                    Ty::Unknown | Ty::Never => Ty::Unknown,
                    other => {
                        let other = other.clone();
                        self.err(
                            v.span.clone(),
                            format!("cannot iterate over a value of type `{other}`"),
                        );
                        Ty::Unknown
                    }
                };
                (hir::ForIter::Vec(Box::new(v)), ty)
            }
        };
        self.scopes.push(HashMap::new());
        let var = self.declare(var, var_ty);
        self.loop_depth += 1;
        let (body, _) = self.block(body, None);
        self.loop_depth -= 1;
        self.scopes.pop();
        Self::mk(hir::ExprKind::For { var, iter, body }, Ty::Unit, span)
    }

    // ---- generics: functions, structs, enums and impls instantiated per use ------------------

    /// Whether `ty` mentions a type parameter (also through the arguments of a generic type).
    fn has_param(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Param(_) => true,
            Ty::Vec(t) | Ty::Option(t) | Ty::Task(t) => self.has_param(t),
            Ty::Result(a, b) | Ty::Map(a, b) => self.has_param(a) || self.has_param(b),
            Ty::Tuple(items) => items.iter().any(|t| self.has_param(t)),
            Ty::Fn(ps, r) => ps.iter().any(|p| self.has_param(p)) || self.has_param(r),
            Ty::Tensor { elem, .. } => self.has_param(elem),
            Ty::Struct(k) | Ty::Enum(k) => self
                .instance_args
                .get(k)
                .is_some_and(|(_, args)| args.iter().any(|a| self.has_param(a))),
            _ => false,
        }
    }

    /// Replaces the type parameters in `ty` by what `bind` says (`Unknown` when unbound).
    fn subst_params(&mut self, ty: &Ty, bind: &HashMap<String, Ty>) -> Ty {
        match ty {
            Ty::Param(p) => bind.get(p).cloned().unwrap_or(Ty::Unknown),
            Ty::Vec(t) => Ty::Vec(Box::new(self.subst_params(t, bind))),
            Ty::Option(t) => Ty::Option(Box::new(self.subst_params(t, bind))),
            Ty::Task(t) => Ty::Task(Box::new(self.subst_params(t, bind))),
            Ty::Result(a, b) => Ty::Result(
                Box::new(self.subst_params(a, bind)),
                Box::new(self.subst_params(b, bind)),
            ),
            Ty::Map(a, b) => Ty::Map(
                Box::new(self.subst_params(a, bind)),
                Box::new(self.subst_params(b, bind)),
            ),
            Ty::Tuple(items) => {
                Ty::Tuple(items.iter().map(|t| self.subst_params(t, bind)).collect())
            }
            Ty::Fn(ps, r) => Ty::Fn(
                ps.iter().map(|p| self.subst_params(p, bind)).collect(),
                Box::new(self.subst_params(r, bind)),
            ),
            Ty::Tensor { elem, shape } => Ty::Tensor {
                elem: Box::new(self.subst_params(elem, bind)),
                shape: shape.clone(),
            },
            Ty::Struct(k) | Ty::Enum(k) if self.has_param(ty) => {
                let (base, args) = self.instance_args[k].clone();
                let args: Vec<Ty> = args.iter().map(|a| self.subst_params(a, bind)).collect();
                self.instantiate_adt(&base, args, &(0..0))
            }
            other => other.clone(),
        }
    }

    /// Binds the type parameters in `declared` by matching it against `actual`.
    fn infer_params(&self, declared: &Ty, actual: &Ty, bind: &mut HashMap<String, Ty>) {
        let actual = self.resolve(actual);
        match (declared, &actual) {
            (Ty::Param(p), a) => {
                if !matches!(a, Ty::Unknown | Ty::Never) {
                    bind.entry(p.clone()).or_insert_with(|| a.clone());
                }
            }
            (Ty::Vec(d), Ty::Vec(a))
            | (Ty::Option(d), Ty::Option(a))
            | (Ty::Task(d), Ty::Task(a)) => {
                self.infer_params(d, a, bind);
            }
            (Ty::Result(d1, d2), Ty::Result(a1, a2)) | (Ty::Map(d1, d2), Ty::Map(a1, a2)) => {
                self.infer_params(d1, a1, bind);
                self.infer_params(d2, a2, bind);
            }
            (Ty::Tuple(ds), Ty::Tuple(a)) if ds.len() == a.len() => {
                for (d, a) in ds.iter().zip(a) {
                    self.infer_params(d, a, bind);
                }
            }
            (Ty::Fn(dp, dr), Ty::Fn(ap, ar)) if dp.len() == ap.len() => {
                for (d, a) in dp.iter().zip(ap) {
                    self.infer_params(d, a, bind);
                }
                self.infer_params(dr, ar, bind);
            }
            (Ty::Tensor { elem: d, .. }, Ty::Tensor { elem: a, .. }) => {
                self.infer_params(d, a, bind)
            }
            (Ty::Struct(dk) | Ty::Enum(dk), Ty::Struct(ak) | Ty::Enum(ak)) => {
                if let (Some((db, dargs)), Some((ab, aargs))) =
                    (self.instance_args.get(dk), self.instance_args.get(ak))
                    && db == ab
                    && dargs.len() == aargs.len()
                {
                    for (d, a) in dargs.iter().zip(aargs) {
                        self.infer_params(d, a, bind);
                    }
                }
            }
            _ => {}
        }
    }

    /// Reports a type argument that does not implement a trait its parameter is bounded by.
    /// Reports each bound `ty` does not meet; whether it met them all.
    fn check_bounds(&mut self, param: &ast::GenericParam, ty: &Ty, span: &Span) -> bool {
        let mut all = true;
        for bound in &param.bounds {
            let ty = self.resolve(ty);
            // A trait of the program wins over a built-in bound of the same name.
            if !self.traits.contains_key(&bound.node) {
                let Some(builtin) = Builtin::from_name(&bound.node) else {
                    let known: Vec<&str> = self
                        .traits
                        .keys()
                        .map(String::as_str)
                        .chain(Builtin::NAMES)
                        .collect();
                    self.err_hint(
                        bound.span.clone(),
                        format!("cannot find the trait `{}`", bound.node),
                        closest(&bound.node, known)
                            .map(|c| format!("did you mean `{c}`?"))
                            .as_deref(),
                    );
                    all = false;
                    continue;
                };
                if !builtin.holds(&ty) {
                    all = false;
                    self.diags.push(
                        Diagnostic::error(
                            span.clone(),
                            format!(
                                "`{ty}` does not implement `{}`, which `{}` requires",
                                bound.node, param.name.node
                            ),
                        )
                        .with_note(builtin.meaning()),
                    );
                }
                continue;
            }
            let ok = match ty.clone() {
                Ty::Struct(n) | Ty::Enum(n) => self.impls.contains(&(bound.node.clone(), n)),
                Ty::Trait(t) => t == bound.node,
                Ty::Unknown | Ty::Never | Ty::Param(_) => true,
                _ => false,
            };
            if !ok {
                all = false;
                self.err(
                    span.clone(),
                    format!(
                        "`{ty}` does not implement `{}`, which `{}` requires",
                        bound.node, param.name.node
                    ),
                );
            }
        }
        all
    }

    /// `Stack<i64>`: the generic struct or enum `name` for these arguments, created the first time.
    /// With a type parameter among the arguments it only names the type (used to infer them).
    fn instantiate_adt(&mut self, name: &str, args: Vec<Ty>, span: &Span) -> Ty {
        let is_struct = self.generic_structs.contains_key(name);
        let generics = if is_struct {
            self.generic_structs[name].generics.clone()
        } else {
            self.generic_enums[name].generics.clone()
        };
        if args.len() != generics.len() {
            self.err(
                span.clone(),
                format!(
                    "`{name}` takes {} type argument(s), found {}",
                    generics.len(),
                    args.len()
                ),
            );
            return Ty::Unknown;
        }
        let args: Vec<Ty> = args.iter().map(|a| self.finalize_ty(a)).collect();
        if self.endless_instance(name, &args.iter().collect::<Vec<_>>(), span) {
            return Ty::Unknown;
        }
        let key = format!(
            "{name}<{}>",
            args.iter()
                .map(Ty::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        self.instance_args
            .entry(key.clone())
            .or_insert((name.to_string(), args.clone()));
        let ty = if is_struct {
            Ty::Struct(key.clone())
        } else {
            Ty::Enum(key.clone())
        };
        if args.iter().any(|a| self.has_param(a))
            || self.struct_fields.contains_key(&key)
            || self.enums.contains_key(&key)
        {
            return ty;
        }
        for (g, a) in generics.iter().zip(&args) {
            self.check_bounds(g, a, span);
        }
        let bind: HashMap<String, Ty> = generics
            .iter()
            .map(|g| g.name.node.clone())
            .zip(args.iter().cloned())
            .collect();
        let saved_subst = std::mem::replace(&mut self.type_params, bind);
        let saved_impl = self.impl_ty.take();
        if is_struct {
            self.struct_fields.insert(key.clone(), Vec::new());
            let decl = self.generic_structs[name].clone();
            let mut fields = Vec::new();
            for f in &decl.fields {
                let t = self.resolve_type(&f.ty);
                fields.push((f.name.node.clone(), t));
            }
            self.struct_fields.insert(key.clone(), fields.clone());
            self.anon_structs.push(hir::StructDef {
                name: key.clone(),
                name_span: decl.name.span.clone(),
                is_pub: decl.is_pub,
                fields,
                span: decl.span.clone(),
            });
        } else {
            self.enums.insert(key.clone(), Vec::new());
            let decl = self.generic_enums[name].clone();
            let mut variants = Vec::new();
            for v in &decl.variants {
                let tys: Vec<Ty> = v.fields.iter().map(|t| self.resolve_type(t)).collect();
                variants.push((v.name.node.clone(), tys));
            }
            self.enums.insert(key.clone(), variants.clone());
            self.anon_enums.push(hir::EnumDef {
                name: key.clone(),
                name_span: decl.name.span.clone(),
                is_pub: decl.is_pub,
                variants,
                span: decl.span.clone(),
            });
        }
        self.type_params = saved_subst;
        self.impl_ty = saved_impl;
        self.materialize_impls(name, &key, &args);
        ty
    }

    /// The methods of every `impl<T> Name<T>` for one instance: their signatures now, their bodies
    /// when the pending instances are checked.
    fn materialize_impls(&mut self, name: &str, key: &str, args: &[Ty]) {
        let impls = self.generic_impls.get(name).cloned().unwrap_or_default();
        for imp in impls {
            let mut bind: HashMap<String, Ty> = HashMap::new();
            for (written, actual) in imp.ty_args.iter().zip(args) {
                if written.path.len() == 1
                    && imp
                        .generics
                        .iter()
                        .any(|g| g.name.node == written.path[0].node)
                {
                    bind.insert(written.path[0].node.clone(), actual.clone());
                }
            }
            if let Some(tr) = &imp.trait_name
                && self.traits.contains_key(&tr.node)
            {
                self.impls.insert((tr.node.clone(), key.to_string()));
            }
            for f in &imp.fns {
                // A method with type parameters of its own is instantiated at each call that uses
                // it (see `generic_method_call`).
                if !f.generics.is_empty() {
                    if imp.trait_name.is_some() {
                        self.err(
                            f.name.span.clone(),
                            "a method of a trait cannot have type parameters of its own",
                        );
                    }
                    continue;
                }
                let full = format!("{key}::{}", f.name.node);
                self.methods.insert(full.clone(), key.to_string());
                if let Some(tr) = &imp.trait_name {
                    self.impl_of_fn.insert(full.clone(), tr.node.clone());
                }
                let saved_subst = std::mem::replace(&mut self.type_params, bind.clone());
                let saved_impl = self.impl_ty.replace(key.to_string());
                let params: Vec<Ty> = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
                let ret = f.ret.as_ref().map_or(Ty::Unit, |t| self.resolve_type(t));
                self.type_params = saved_subst;
                self.impl_ty = saved_impl;
                self.fn_sigs.insert(full.clone(), (params, ret));
                let mut renamed = f.clone();
                renamed.name.node = full;
                self.pending.push(Pending {
                    f: renamed,
                    impl_ty: Some(key.to_string()),
                    subst: bind.clone(),
                });
            }
        }
    }

    /// The type (`Box`, `Plain`) whose `impl` has a method `name` with type parameters of its own,
    /// when `ty` is one of its values.
    fn generic_method_owner(&self, ty: &Ty, name: &str) -> Option<String> {
        let key = match self.resolve(ty) {
            Ty::Struct(k) | Ty::Enum(k) => k,
            _ => return None,
        };
        let base = self
            .instance_args
            .get(&key)
            .map_or(key, |(base, _)| base.clone());
        self.generic_impls
            .get(&base)?
            .iter()
            .any(|i| {
                i.fns
                    .iter()
                    .any(|f| f.name.node == name && !f.generics.is_empty())
            })
            .then_some(base)
    }

    /// `recv.m(args)` where `m` has type parameters of its own (`fn map[U](self, f: (T) -> U)`):
    /// infer them from the arguments, create the instance for them, call it.
    fn generic_method_call(
        &mut self,
        receiver: hir::Expr,
        owner: &str,
        name: &str,
        args: &[ast::Expr],
        expected: Option<&Ty>,
        span: &Span,
    ) -> hir::Expr {
        let bad = |this: &mut Self, msg: String| {
            this.err(span.clone(), msg);
            Self::mk(hir::ExprKind::Error, Ty::Unknown, span)
        };
        let Some((decl, imp)) = self.generic_impls.get(owner).and_then(|imps| {
            imps.iter().find_map(|imp| {
                imp.fns
                    .iter()
                    .find(|f| f.name.node == name && !f.generics.is_empty())
                    .map(|f| (f.clone(), imp.clone()))
            })
        }) else {
            return bad(self, format!("`{owner}` has no method `{name}`"));
        };
        let (Ty::Struct(key) | Ty::Enum(key)) = self.resolve(&receiver.ty) else {
            return bad(self, format!("no method `{name}` on `{}`", receiver.ty));
        };
        // What the type's own parameters stand for in this receiver (`T` is `Int` in `Box<Int>`).
        let mut impl_bind: HashMap<String, Ty> = HashMap::new();
        if let Some((_, type_args)) = self.instance_args.get(&key).cloned() {
            for (written, actual) in imp.ty_args.iter().zip(&type_args) {
                if written.path.len() == 1
                    && imp
                        .generics
                        .iter()
                        .any(|g| g.name.node == written.path[0].node)
                {
                    impl_bind.insert(written.path[0].node.clone(), actual.clone());
                }
            }
        }
        // The signature, with the method's own parameters left as `Ty::Param`.
        let mut sig_params = impl_bind.clone();
        for g in &decl.generics {
            sig_params.insert(g.name.node.clone(), Ty::Param(g.name.node.clone()));
        }
        let saved_subst = std::mem::replace(&mut self.type_params, sig_params);
        let saved_impl = self.impl_ty.replace(key.clone());
        let decl_params: Vec<Ty> = decl
            .params
            .iter()
            .map(|p| self.resolve_type(&p.ty))
            .collect();
        let decl_ret = decl.ret.as_ref().map_or(Ty::Unit, |t| self.resolve_type(t));
        self.type_params = saved_subst;
        self.impl_ty = saved_impl;
        if decl_params.len() != args.len() + 1 {
            return bad(
                self,
                format!(
                    "expected {} argument(s), found {}",
                    decl_params.len().saturating_sub(1),
                    args.len()
                ),
            );
        }
        let mut bind: HashMap<String, Ty> = HashMap::new();
        let mut checked: Vec<Option<hir::Expr>> = Vec::new();
        for (a, declared) in args.iter().zip(&decl_params[1..]) {
            if matches!(a.node, ast::ExprKind::Closure { .. }) {
                checked.push(None);
            } else {
                let e = self.expr(a, None);
                self.infer_params(declared, &e.ty, &mut bind);
                checked.push(Some(e));
            }
        }
        if let Some(want) = expected {
            let want = self.resolve(want);
            self.infer_params(&decl_ret, &want, &mut bind);
        }
        for (i, a) in args.iter().enumerate() {
            if checked[i].is_none() {
                let hint = self.subst_params(&decl_params[i + 1], &bind);
                let e = self.expr(a, Some(&hint));
                self.infer_params(&decl_params[i + 1], &e.ty, &mut bind);
                checked[i] = Some(e);
            }
        }
        // An argument that is not a function where one is declared is the mistake to report,
        // not the type parameters it leaves open.
        for (declared, arg) in decl_params[1..].iter().zip(&checked) {
            if let (Ty::Fn(..), Some(arg)) = (declared, arg) {
                let shown = self.resolve(&arg.ty);
                if !matches!(shown, Ty::Fn(..) | Ty::Unknown | Ty::Never) {
                    return bad(
                        self,
                        format!("mismatched types: expected `{declared}`, found `{shown}`"),
                    );
                }
            }
        }
        for g in &decl.generics {
            if !bind.contains_key(&g.name.node) {
                return bad(
                    self,
                    format!(
                        "cannot infer the type parameter `{}` of `{name}`: add a type annotation",
                        g.name.node
                    ),
                );
            }
        }
        // A literal argument takes the type the call is used as (`let n: Int = s.fold(0, f)`)
        // before it is pinned to its default.
        if let Some(want) = expected {
            let ret = self.subst_params(&decl_ret, &bind);
            self.unify(want, &ret);
        }
        let bind: HashMap<String, Ty> = bind
            .into_iter()
            .map(|(k, v)| {
                let v = self.finalize_ty(&v);
                (k, v)
            })
            .collect();
        let mut bounds_ok = true;
        for g in &decl.generics {
            let ty = bind[&g.name.node].clone();
            bounds_ok &= self.check_bounds(g, &ty, span);
        }
        let params: Vec<Ty> = decl_params
            .iter()
            .map(|p| self.subst_params(p, &bind))
            .collect();
        let ret = self.subst_params(&decl_ret, &bind);
        let shown: Vec<String> = decl
            .generics
            .iter()
            .map(|g| bind[&g.name.node].to_string())
            .collect();
        let full = format!("{key}::{name}<{}>", shown.join(", "));
        let growing = {
            let tys: Vec<&Ty> = bind.values().collect();
            self.endless_instance(name, &tys, span)
        };
        if bounds_ok && !growing && !self.fn_sigs.contains_key(&full) {
            self.fn_sigs
                .insert(full.clone(), (params.clone(), ret.clone()));
            self.methods.insert(full.clone(), key.clone());
            let mut renamed = decl.clone();
            renamed.name.node = full.clone();
            renamed.generics = Vec::new();
            let mut subst = impl_bind;
            subst.extend(bind.clone());
            self.pending.push(Pending {
                f: renamed,
                impl_ty: Some(key.clone()),
                subst,
            });
        }
        let mut all = vec![receiver];
        for (e, p) in checked.into_iter().zip(&params[1..]) {
            let e = e.expect("every argument was checked");
            all.push(self.coerce(e, p));
        }
        let callee = Self::mk(
            hir::ExprKind::Fn(full),
            Ty::Fn(params, Box::new(ret.clone())),
            span,
        );
        Self::mk(
            hir::ExprKind::Call {
                callee: Box::new(callee),
                args: all,
            },
            ret,
            span,
        )
    }

    /// A call of a generic function, or of an associated function of a generic type
    /// (`Stack::new()`): infer the type arguments, create the instance, call it.
    fn generic_call(
        &mut self,
        name: &str,
        type_name: Option<&str>,
        explicit: &[ast::Type],
        args: &[ast::Expr],
        expected: Option<&Ty>,
        span: &Span,
    ) -> hir::Expr {
        let bad = |this: &mut Self, msg: String| {
            this.err(span.clone(), msg);
            Self::mk(hir::ExprKind::Error, Ty::Unknown, span)
        };
        // The declaration and the type parameters in play.
        let (decl, generics, owner) = match type_name {
            Some(t) => {
                let found = self.generic_impls.get(t).and_then(|imps| {
                    imps.iter().find_map(|imp| {
                        imp.fns
                            .iter()
                            .find(|f| f.name.node == name)
                            .map(|f| (f.clone(), imp.clone()))
                    })
                });
                let Some((f, imp)) = found else {
                    return bad(self, format!("`{t}` has no associated function `{name}`"));
                };
                if !f.generics.is_empty() {
                    return bad(
                        self,
                        "methods with their own type parameters are not supported yet".into(),
                    );
                }
                (f, imp.generics.clone(), Some(imp))
            }
            None => {
                let f = self.generic_fns[name].clone();
                let g = f.generics.clone();
                (f, g, None)
            }
        };
        // The signature with type parameters left as `Ty::Param`.
        let param_types: HashMap<String, Ty> = generics
            .iter()
            .map(|g| (g.name.node.clone(), Ty::Param(g.name.node.clone())))
            .collect();
        let saved_subst = std::mem::replace(&mut self.type_params, param_types);
        let saved_impl = self.impl_ty.take();
        if let (Some(imp), Some(t)) = (&owner, type_name) {
            let pattern: Vec<Ty> = imp.ty_args.iter().map(|a| self.resolve_type(a)).collect();
            if let Ty::Struct(k) | Ty::Enum(k) = self.instantiate_adt(t, pattern, span) {
                self.impl_ty = Some(k);
            }
        }
        let decl_params: Vec<Ty> = decl
            .params
            .iter()
            .map(|p| self.resolve_type(&p.ty))
            .collect();
        let decl_ret = decl.ret.as_ref().map_or(Ty::Unit, |t| self.resolve_type(t));
        self.type_params = saved_subst;
        self.impl_ty = saved_impl;
        if decl_params.len() != args.len() {
            return bad(
                self,
                format!(
                    "expected {} argument(s), found {}",
                    decl_params.len(),
                    args.len()
                ),
            );
        }
        // Arguments first (closures wait until the types they need are known).
        let mut bind: HashMap<String, Ty> = HashMap::new();
        if !explicit.is_empty() {
            if explicit.len() != generics.len() {
                return bad(
                    self,
                    format!(
                        "`{name}` takes {} type argument(s), found {}",
                        generics.len(),
                        explicit.len()
                    ),
                );
            }
            for (g, t) in generics.iter().zip(explicit) {
                let ty = self.resolve_type(t);
                bind.insert(g.name.node.clone(), ty);
            }
        }
        let mut checked: Vec<Option<hir::Expr>> = Vec::new();
        for (a, declared) in args.iter().zip(&decl_params) {
            if matches!(a.node, ast::ExprKind::Closure { .. }) {
                checked.push(None);
            } else {
                let e = self.expr(a, None);
                self.infer_params(declared, &e.ty, &mut bind);
                checked.push(Some(e));
            }
        }
        if let Some(want) = expected {
            let want = self.resolve(want);
            self.infer_params(&decl_ret, &want, &mut bind);
        }
        for (i, a) in args.iter().enumerate() {
            if checked[i].is_none() {
                let hint = self.subst_params(&decl_params[i], &bind);
                let e = self.expr(a, Some(&hint));
                self.infer_params(&decl_params[i], &e.ty, &mut bind);
                checked[i] = Some(e);
            }
        }
        for g in &generics {
            if !bind.contains_key(&g.name.node) {
                return bad(
                    self,
                    format!(
                        "cannot infer the type parameter `{}` of `{name}`: add a type annotation, or write the type in the call: `{name}[Type](..)`",
                        g.name.node
                    ),
                );
            }
        }
        // A literal argument must take the type the call is used as (`let n: Int = id(7)`)
        // before it is pinned to its default.
        if let Some(want) = expected {
            let ret = self.subst_params(&decl_ret, &bind);
            self.unify(want, &ret);
        }
        // A literal that nothing has pinned yet keeps a free function's call open: the instance
        // is chosen when the enclosing function is finished, so `let a = id(3)` can still be
        // used as an `Int`.
        let open = owner.is_none() && bind.values().any(|t| self.has_literal_var(t));
        let bind: HashMap<String, Ty> = bind
            .into_iter()
            .map(|(k, v)| {
                let v = if open {
                    self.resolve(&v)
                } else {
                    self.finalize_ty(&v)
                };
                (k, v)
            })
            .collect();
        let mut bounds_ok = true;
        if !open {
            for g in &generics {
                let ty = bind[&g.name.node].clone();
                bounds_ok &= self.check_bounds(g, &ty, span);
            }
        }
        let params: Vec<Ty> = decl_params
            .iter()
            .map(|p| self.subst_params(p, &bind))
            .collect();
        let ret = self.subst_params(&decl_ret, &bind);
        // The instance to call.
        let full = match (&owner, type_name) {
            (Some(imp), Some(t)) => {
                let type_args: Vec<Ty> = imp
                    .ty_args
                    .iter()
                    .map(|a| {
                        if a.path.len() == 1 {
                            bind.get(&a.path[0].node).cloned().unwrap_or(Ty::Unknown)
                        } else {
                            Ty::Unknown
                        }
                    })
                    .collect();
                match self.instantiate_adt(t, type_args, span) {
                    Ty::Struct(k) | Ty::Enum(k) => format!("{k}::{name}"),
                    _ => return Self::mk(hir::ExprKind::Error, Ty::Unknown, span),
                }
            }
            _ if open => {
                self.open_calls.push(OpenCall {
                    name: name.to_string(),
                    decl: decl.clone(),
                    generics: generics.clone(),
                    params: decl_params.clone(),
                    ret: decl_ret.clone(),
                    bind: bind.clone(),
                    span: span.clone(),
                });
                format!("{OPEN_CALL}{}", self.open_calls.len() - 1)
            }
            // The body is not checked for types that do not meet the bounds: it would only
            // repeat the mistake.
            _ if !bounds_ok => format!("{name}<invalid>"),
            _ => self.fn_instance(name, &decl, &bind, &params, &ret, span),
        };
        let args: Vec<hir::Expr> = checked
            .into_iter()
            .zip(&params)
            .map(|(e, p)| {
                let e = e.expect("every argument was checked");
                self.coerce(e, p)
            })
            .collect();
        let callee = Self::mk(
            hir::ExprKind::Fn(full),
            Ty::Fn(params, Box::new(ret.clone())),
            span,
        );
        Self::mk(
            hir::ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            ret,
            span,
        )
    }

    /// Whether a literal variable (`{integer}`, `{float}`) is still free somewhere in `ty`.
    fn has_literal_var(&self, ty: &Ty) -> bool {
        match self.resolve(ty) {
            Ty::IntVar(_) | Ty::FloatVar(_) => true,
            Ty::Vec(t) | Ty::Option(t) | Ty::Task(t) => self.has_literal_var(&t),
            Ty::Result(a, b) | Ty::Map(a, b) => {
                self.has_literal_var(&a) || self.has_literal_var(&b)
            }
            Ty::Tuple(items) => items.iter().any(|t| self.has_literal_var(t)),
            Ty::Fn(ps, r) => ps.iter().any(|p| self.has_literal_var(p)) || self.has_literal_var(&r),
            _ => false,
        }
    }

    /// How many parts make up `ty`: `List[(Int, Text)]` has four. It stays small for every type a
    /// person writes, and grows without bound for one that keeps being wrapped or doubled.
    fn type_size(&self, ty: &Ty) -> usize {
        let sum = |tys: &[&Ty]| tys.iter().map(|t| self.type_size(t)).sum::<usize>();
        1 + match ty {
            Ty::Vec(t) | Ty::Option(t) | Ty::Task(t) => self.type_size(t),
            Ty::Tensor { elem, .. } => self.type_size(elem),
            Ty::Result(a, b) | Ty::Map(a, b) => sum(&[a, b]),
            Ty::Tuple(items) => items.iter().map(|t| self.type_size(t)).sum(),
            Ty::Fn(params, ret) => {
                params.iter().map(|t| self.type_size(t)).sum::<usize>() + self.type_size(ret)
            }
            Ty::Struct(key) | Ty::Enum(key) => self
                .instance_args
                .get(key)
                .map_or(0, |(_, args)| args.iter().map(|t| self.type_size(t)).sum()),
            _ => 0,
        }
    }

    /// Reports (and says so) when the types an instance of `name` is made for are too big:
    /// that is a generic function or type that keeps asking for a bigger version of itself.
    fn endless_instance(&mut self, name: &str, tys: &[&Ty], span: &Span) -> bool {
        let Some(deep) = tys.iter().find(|t| self.type_size(t) > MAX_TYPE_SIZE) else {
            return false;
        };
        let mut shown = deep.to_string();
        if shown.len() > 60 {
            shown.truncate(57);
            shown.push_str("...");
        }
        self.err_hint(
            span.clone(),
            format!("`{name}` is instantiated with types that keep growing: `{shown}`"),
            Some("a generic function that calls itself with a bigger type never ends"),
        );
        true
    }

    /// The instance of the generic function `name` for `bind`, queued for checking the first time.
    fn fn_instance(
        &mut self,
        name: &str,
        decl: &ast::Fn,
        bind: &HashMap<String, Ty>,
        params: &[Ty],
        ret: &Ty,
        span: &Span,
    ) -> String {
        let generics = &decl.generics;
        let tys: Vec<&Ty> = bind.values().collect();
        if self.endless_instance(name, &tys, span) {
            return format!("{name}<too deep>");
        }
        let shown: Vec<String> = generics
            .iter()
            .map(|g| bind[&g.name.node].to_string())
            .collect();
        let key = format!("{name}<{}>", shown.join(", "));
        if !self.fn_sigs.contains_key(&key) {
            self.fn_sigs
                .insert(key.clone(), (params.to_vec(), ret.clone()));
            let mut renamed = decl.clone();
            renamed.name.node = key.clone();
            renamed.generics = Vec::new();
            self.pending.push(Pending {
                f: renamed,
                impl_ty: None,
                subst: bind.clone(),
            });
        }
        key
    }

    /// Picks the instance of a call left open by `generic_call`, now that its literals are
    /// pinned to what the function used them as (or to their defaults).
    fn close_generic_call(&mut self, placeholder: &str) -> String {
        let index: usize = placeholder[OPEN_CALL.len()..].parse().unwrap_or(0);
        let call = self.open_calls[index].clone();
        let bind: HashMap<String, Ty> = call
            .bind
            .iter()
            .map(|(k, v)| (k.clone(), self.finalize_ty(v)))
            .collect();
        let mut bounds_ok = true;
        for g in &call.generics {
            let ty = bind[&g.name.node].clone();
            bounds_ok &= self.check_bounds(g, &ty, &call.span);
        }
        if !bounds_ok {
            return format!("{}<invalid>", call.name);
        }
        let params: Vec<Ty> = call
            .params
            .iter()
            .map(|p| self.subst_params(p, &bind))
            .collect();
        let ret = self.subst_params(&call.ret, &bind);
        self.fn_instance(&call.name, &call.decl, &bind, &params, &ret, &call.span)
    }

    /// The full name (`Point::norm`) of the method `name` of a struct or enum type, if there is one.
    fn method_of(&self, ty: &Ty, name: &str) -> Option<String> {
        let type_name = match self.resolve(ty) {
            Ty::Struct(n) | Ty::Enum(n) => n,
            _ => return None,
        };
        let full = format!("{type_name}::{name}");
        if self.methods.contains_key(&full) {
            return Some(full);
        }
        // A default method of a trait the type implements.
        let mut candidates: Vec<&String> = self
            .impls
            .iter()
            .filter(|(_, t)| *t == type_name)
            .map(|(tr, _)| tr)
            .collect();
        candidates.sort();
        candidates.into_iter().find_map(|tr| {
            let declares = self.traits[tr]
                .iter()
                .any(|(m, required)| m == name && !required);
            declares.then(|| format!("{tr}::{name}"))
        })
    }

    /// For a value whose type is a trait: the trait, when it declares a method called `name`.
    fn trait_method(&self, ty: &Ty, name: &str) -> Option<String> {
        match self.resolve(ty) {
            Ty::Trait(t)
                if self
                    .traits
                    .get(&t)
                    .is_some_and(|ms| ms.iter().any(|(m, _)| m == name)) =>
            {
                Some(t)
            }
            _ => None,
        }
    }

    /// `receiver.method(args)` where the receiver's type is a trait: chosen when the program runs.
    fn dyn_call(
        &mut self,
        trait_name: &str,
        method: &str,
        receiver: hir::Expr,
        args: &[ast::Expr],
        span: &Span,
    ) -> hir::Expr {
        let full = format!("{trait_name}::{method}");
        let (params, ret) = self.fn_sigs[&full].clone();
        if params.len() != args.len() + 1 {
            self.err(
                span.clone(),
                format!(
                    "expected {} argument(s), found {}",
                    params.len().saturating_sub(1),
                    args.len()
                ),
            );
        }
        let mut checked = vec![receiver];
        for (i, a) in args.iter().enumerate() {
            let want = params.get(i + 1);
            let a = self.expr(a, want);
            checked.push(match want {
                Some(w) => self.coerce(a, w),
                None => a,
            });
        }
        Self::mk(
            hir::ExprKind::DynCall {
                trait_name: trait_name.to_string(),
                method: method.to_string(),
                args: checked,
            },
            ret,
            span,
        )
    }

    /// `ty` with the trait's `Self` replaced by the implementing type.
    fn with_self(ty: &Ty, trait_name: &str, concrete: &Ty) -> Ty {
        let go = |t: &Ty| Self::with_self(t, trait_name, concrete);
        match ty {
            Ty::Trait(n) if n == trait_name => concrete.clone(),
            Ty::Vec(t) => Ty::Vec(Box::new(go(t))),
            Ty::Option(t) => Ty::Option(Box::new(go(t))),
            Ty::Task(t) => Ty::Task(Box::new(go(t))),
            Ty::Result(a, b) => Ty::Result(Box::new(go(a)), Box::new(go(b))),
            Ty::Map(k, v) => Ty::Map(Box::new(go(k)), Box::new(go(v))),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(go).collect()),
            Ty::Fn(p, r) => Ty::Fn(p.iter().map(go).collect(), Box::new(go(r))),
            other => other.clone(),
        }
    }

    /// Every `impl Trait for Type` must define the required methods with the trait's signatures
    /// and may only add methods that the trait declares.
    fn check_trait_impls(&mut self, fns: &[(ast::Fn, Option<String>)]) {
        let impls = self.impl_spans.clone();
        for (trait_name, ty, span) in impls {
            let declared = self.traits[&trait_name].clone();
            let concrete = if self.enums.contains_key(&ty) {
                Ty::Enum(ty.clone())
            } else {
                Ty::Struct(ty.clone())
            };
            for (method, required) in &declared {
                let (trait_key, impl_key) =
                    (format!("{trait_name}::{method}"), format!("{ty}::{method}"));
                match (
                    self.fn_sigs.get(&impl_key).cloned(),
                    self.fn_sigs.get(&trait_key).cloned(),
                ) {
                    (None, _) if *required => self.err(
                        span.clone(),
                        format!("`type {ty}({trait_name})` is missing the method `{method}`"),
                    ),
                    (Some((got_params, got_ret)), Some((want_params, want_ret))) => {
                        let want_params: Vec<Ty> = want_params
                            .iter()
                            .map(|p| Self::with_self(p, &trait_name, &concrete))
                            .collect();
                        let want_ret = Self::with_self(&want_ret, &trait_name, &concrete);
                        let same = got_params.len() == want_params.len()
                            && got_params
                                .iter()
                                .zip(&want_params)
                                .all(|(g, w)| g.accepts(w) && w.accepts(g))
                            && got_ret.accepts(&want_ret)
                            && want_ret.accepts(&got_ret);
                        if !same {
                            let shown = |ps: &[Ty], r: &Ty| {
                                let ps: Vec<String> = ps.iter().map(Ty::to_string).collect();
                                format!("({}) -> {r}", ps.join(", "))
                            };
                            let at = fns
                                .iter()
                                .find(|(f, _)| f.name.node == impl_key)
                                .map_or(span.clone(), |(f, _)| f.name.span.clone());
                            self.err(
                                at,
                                format!(
                                    "`{ty}.{method}` has the signature {} but `{trait_name}` declares {}",
                                    shown(&got_params, &got_ret),
                                    shown(&want_params, &want_ret)
                                ),
                            );
                        }
                    }
                    _ => {}
                }
            }
            for (f, impl_ty) in fns {
                if impl_ty.as_deref() == Some(ty.as_str())
                    && let Some(m) = f.name.node.strip_prefix(&format!("{ty}::"))
                    && !declared.iter().any(|(d, _)| d == m)
                    && self.impls.contains(&(trait_name.clone(), ty.clone()))
                    && self
                        .impl_of_fn
                        .get(&f.name.node)
                        .is_some_and(|t| *t == trait_name)
                {
                    self.err(
                        f.name.span.clone(),
                        format!("`{m}` is not a method of the trait `{trait_name}`"),
                    );
                }
            }
        }
    }

    /// Checks `receiver.method(args)` against the signature of `full` (receiver first).
    fn method_call(
        &mut self,
        full: &str,
        receiver: hir::Expr,
        args: &[ast::Expr],
        span: &Span,
    ) -> hir::Expr {
        let (params, ret) = self.fn_sigs[full].clone();
        let takes_self = params.first().is_some_and(|p| {
            matches!(
                (p, self.methods.get(full)),
                (Ty::Struct(n) | Ty::Enum(n) | Ty::Trait(n), Some(t)) if n == t
            )
        });
        let name = full.rsplit("::").next().unwrap_or(full).to_string();
        if !takes_self {
            self.err(
                span.clone(),
                format!("`{name}` has no `self` parameter, call it as `{full}(..)`"),
            );
            return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
        }
        if params.len() != args.len() + 1 {
            self.err(
                span.clone(),
                format!(
                    "expected {} argument(s), found {}",
                    params.len() - 1,
                    args.len()
                ),
            );
        }
        let mut checked = vec![receiver];
        for (i, a) in args.iter().enumerate() {
            let want = params.get(i + 1);
            let a = self.expr(a, want);
            checked.push(match want {
                Some(w) => self.coerce(a, w),
                None => a,
            });
        }
        let callee = Self::mk(
            hir::ExprKind::Fn(full.to_string()),
            Ty::Fn(params, Box::new(ret.clone())),
            span,
        );
        Self::mk(
            hir::ExprKind::Call {
                callee: Box::new(callee),
                args: checked,
            },
            ret,
            span,
        )
    }

    /// An `async fn` with type parameters, or in a type that has them, is not supported yet.
    fn reject_generic_async(&mut self, program: &ast::Program) {
        let mut bad: Vec<Span> = Vec::new();
        for item in &program.items {
            match item {
                ast::Item::Fn(f) if f.is_async && !f.generics.is_empty() => {
                    bad.push(f.name.span.clone());
                }
                ast::Item::Impl(imp) => {
                    for f in imp.fns.iter().filter(|f| f.is_async) {
                        if !imp.generics.is_empty() || !f.generics.is_empty() {
                            bad.push(f.name.span.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        for span in bad {
            self.err(
                span,
                "an `async fn` cannot have type parameters yet, nor belong to a generic type",
            );
        }
    }

    fn record_labels(&mut self, en: &ast::Enum) {
        for v in &en.variants {
            if v.from {
                if !en.generics.is_empty() {
                    self.err(
                        v.name.span.clone(),
                        "a generic enum cannot mark a variant `from`",
                    );
                } else if v.fields.len() != 1 {
                    self.err(
                        v.name.span.clone(),
                        format!(
                            "a variant marked `from` holds one value: `from {}(Type)`",
                            v.name.node
                        ),
                    );
                } else {
                    self.enum_from
                        .insert((en.name.node.clone(), v.name.node.clone()));
                }
            }
            if !v.names.is_empty() {
                let names = v.names.iter().map(|n| n.node.clone()).collect();
                self.enum_labels
                    .insert((en.name.node.clone(), v.name.node.clone()), names);
            }
        }
    }

    /// The names of the values of `Enum.Variant`, for the enum as declared or one of its
    /// instances (`Maybe<i64>`); empty when the values have no names.
    fn labels_of(&self, enum_name: &str, variant: &str) -> Vec<String> {
        let base = self
            .instance_args
            .get(enum_name)
            .map_or(enum_name, |(b, _)| b.as_str());
        self.enum_labels
            .get(&(base.to_string(), variant.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    /// `Shape.Rect(w: 1.0, h: 2.0)`: the values in the order the variant declares them.
    fn order_variant_args(
        &mut self,
        enum_name: &str,
        variant: &ast::Ident,
        given: &[(ast::Ident, ast::Expr)],
        span: &Span,
    ) -> Option<Vec<ast::Expr>> {
        let labels = self.labels_of(enum_name, &variant.node);
        if labels.is_empty() {
            self.err(
                span.clone(),
                format!(
                    "the values of `{enum_name}.{}` have no names; write them in order, `{enum_name}.{}(a, b)`",
                    variant.node, variant.node
                ),
            );
            return None;
        }
        let mut slots: Vec<Option<ast::Expr>> = vec![None; labels.len()];
        let mut ok = true;
        for (name, value) in given {
            match labels.iter().position(|l| *l == name.node) {
                Some(i) if slots[i].is_none() => slots[i] = Some(value.clone()),
                Some(_) => {
                    self.err(name.span.clone(), format!("`{}` is given twice", name.node));
                    ok = false;
                }
                None => {
                    let hint =
                        closest(&name.node, labels.iter().map(String::as_str)).map(str::to_string);
                    self.err_hint(
                        name.span.clone(),
                        format!(
                            "`{enum_name}.{}` has no value named `{}`",
                            variant.node, name.node
                        ),
                        hint.as_deref(),
                    );
                    ok = false;
                }
            }
        }
        if let Some(i) = slots.iter().position(Option::is_none)
            && ok
        {
            self.err(
                span.clone(),
                format!("`{enum_name}.{}` is missing `{}`", variant.node, labels[i]),
            );
            ok = false;
        }
        ok.then(|| slots.into_iter().flatten().collect())
    }

    /// `Enum::Variant` or `Enum::Variant(args..)`.
    fn enum_ctor(
        &mut self,
        enum_name: &str,
        variant: &ast::Ident,
        args: &[ast::Expr],
        span: &Span,
    ) -> hir::Expr {
        let ty = Ty::Enum(enum_name.to_string());
        if !self.enums.contains_key(enum_name) {
            // The type arguments could not be inferred (already reported).
            for a in args {
                let _ = self.expr(a, None);
            }
            return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
        }
        let Some(fields) = self.enums[enum_name]
            .iter()
            .find(|(n, _)| *n == variant.node)
            .map(|(_, f)| f.clone())
        else {
            self.err(
                variant.span.clone(),
                format!("enum `{enum_name}` has no variant `{}`", variant.node),
            );
            return Self::mk(hir::ExprKind::Error, ty, span);
        };
        if fields.len() != args.len() {
            self.err(
                span.clone(),
                format!(
                    "`{enum_name}.{}` takes {} value(s), found {}",
                    variant.node,
                    fields.len(),
                    args.len()
                ),
            );
        }
        let args = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let want = fields.get(i);
                let a = self.expr(a, want);
                match want {
                    Some(w) => self.coerce(a, w),
                    None => a,
                }
            })
            .collect();
        Self::mk(
            hir::ExprKind::EnumCtor {
                enum_name: enum_name.to_string(),
                variant: variant.node.clone(),
                args,
            },
            ty,
            span,
        )
    }

    fn match_expr(
        &mut self,
        scrutinee: &ast::Expr,
        arms: &[ast::Arm],
        expected: Option<&Ty>,
        span: &Span,
    ) -> hir::Expr {
        let diags_before = self.diags.len();
        let scrut = self.expr(scrutinee, None);
        let sty = self.resolve(&scrut.ty);
        if arms.is_empty() {
            self.err(span.clone(), "a `match` needs at least one arm");
        }
        let expected = expected.cloned();
        let mut ty = Ty::Never;
        let mut out = Vec::new();
        for arm in arms {
            self.scopes.push(HashMap::new());
            let pat = self.pattern(&arm.pat, &sty);
            let guard = arm.guard.as_ref().map(|g| {
                let g = self.expr(g, Some(&Ty::bool()));
                self.coerce(g, &Ty::bool())
            });
            let want = expected
                .clone()
                .or_else(|| (ty != Ty::Never).then(|| ty.clone()));
            let body = self.expr(&arm.body, want.as_ref());
            self.scopes.pop();
            if self.unify(&ty, &body.ty) {
                if ty == Ty::Never {
                    ty = body.ty.clone();
                }
            } else {
                self.err(
                    body.span.clone(),
                    format!(
                        "`match` arms have incompatible types: `{}` and `{}`",
                        self.resolve(&ty),
                        self.resolve(&body.ty)
                    ),
                );
                ty = Ty::Unknown;
            }
            out.push(hir::Arm {
                pat,
                guard,
                body,
                unreachable: false,
            });
        }
        self.check_exhaustive(&sty, &out, span);
        if self.diags.len() == diags_before {
            // The arm the parser adds for `if x is Pattern:` has no pattern of its own.
            let written: Vec<bool> = arms.iter().map(|a| a.pat.span != *span).collect();
            for (i, dead) in self.unreachable_arms(&sty, &out).into_iter().enumerate() {
                out[i].unreachable = dead && written[i];
            }
        }
        Self::mk(
            hir::ExprKind::Match {
                scrutinee: Box::new(scrut),
                arms: out,
            },
            ty,
            span,
        )
    }

    fn pattern(&mut self, p: &ast::Pattern, ty: &Ty) -> hir::Pat {
        use ast::PatKind as P;
        let ty = self.resolve(ty);
        match &p.node {
            P::Wild => hir::Pat::Wild,
            P::Tuple(items) => {
                let parts: Vec<Ty> = match &ty {
                    Ty::Tuple(ts) if ts.len() == items.len() => ts.clone(),
                    Ty::Unknown => vec![Ty::Unknown; items.len()],
                    other => {
                        self.err(
                            p.span.clone(),
                            format!(
                                "a pattern with {} parts cannot match `{other}`",
                                items.len()
                            ),
                        );
                        vec![Ty::Unknown; items.len()]
                    }
                };
                hir::Pat::Tuple(
                    items
                        .iter()
                        .zip(&parts)
                        .map(|(item, t)| self.pattern(item, t))
                        .collect(),
                )
            }
            P::Bind(name) if name == "None" => {
                if !matches!(ty, Ty::Option(_) | Ty::Unknown) {
                    self.err(
                        p.span.clone(),
                        format!("`None` patterns need an `Option`, found `{ty}`"),
                    );
                }
                hir::Pat::Variant {
                    enum_name: Some("Option".into()),
                    variant: "None".into(),
                    args: vec![],
                }
            }
            P::Bind(name) => {
                let id = ast::Ident {
                    node: name.clone(),
                    span: p.span.clone(),
                };
                hir::Pat::Bind(self.declare(&id, ty))
            }
            P::Lit(l) => {
                let lit_ty = match l {
                    ast::Lit::Int(_) => self.fresh(false),
                    ast::Lit::Float(_) => self.fresh(true),
                    ast::Lit::Str(_) => Ty::str(),
                    ast::Lit::Bool(_) => Ty::bool(),
                };
                if !self.unify(&ty, &lit_ty) {
                    self.err(
                        p.span.clone(),
                        format!("this pattern does not match a value of type `{ty}`"),
                    );
                }
                hir::Pat::Lit(l.clone())
            }
            P::Variant { path, args } => self.variant_pattern(p, path, args, &ty),
            P::NegLit(l) => {
                let lit_ty = match l {
                    ast::Lit::Float(_) => self.fresh(true),
                    _ => self.fresh(false),
                };
                if !self.unify(&ty, &lit_ty) {
                    self.err(
                        p.span.clone(),
                        format!("this pattern does not match a value of type `{ty}`"),
                    );
                }
                match l {
                    ast::Lit::Int(n) => hir::Pat::NegInt(*n),
                    ast::Lit::Float(x) => hir::Pat::Lit(ast::Lit::Float(-x)),
                    other => hir::Pat::Lit(other.clone()),
                }
            }
            P::Or(alts) => {
                let alts: Vec<hir::Pat> = alts.iter().map(|a| self.pattern(a, &ty)).collect();
                if alts.iter().any(Self::binds) {
                    self.err(
                        p.span.clone(),
                        "the alternatives of `|` cannot bind names; write a `case` for each one, or test the value with `if`",
                    );
                }
                hir::Pat::Or(alts)
            }
            P::List {
                before,
                rest,
                after,
            } => {
                let elem = match &ty {
                    Ty::Vec(t) => (**t).clone(),
                    Ty::Unknown => Ty::Unknown,
                    other => {
                        self.err(
                            p.span.clone(),
                            format!("a list pattern cannot match `{other}`"),
                        );
                        Ty::Unknown
                    }
                };
                let before = before.iter().map(|a| self.pattern(a, &elem)).collect();
                let rest_ty = Ty::Vec(Box::new(elem.clone()));
                let rest = rest.as_ref().map(|r| Box::new(self.pattern(r, &rest_ty)));
                let after = after.iter().map(|a| self.pattern(a, &elem)).collect();
                hir::Pat::List {
                    before,
                    rest,
                    after,
                }
            }
            P::Struct { path, fields } => self.struct_pattern(p, path, fields, &ty),
        }
    }

    /// Whether a pattern binds a name.
    fn binds(p: &hir::Pat) -> bool {
        match p {
            hir::Pat::Bind(_) => true,
            hir::Pat::Wild | hir::Pat::Lit(_) | hir::Pat::NegInt(_) => false,
            hir::Pat::Tuple(items) | hir::Pat::Or(items) => items.iter().any(Self::binds),
            hir::Pat::Variant { args, .. } => args.iter().any(Self::binds),
            hir::Pat::List {
                before,
                rest,
                after,
            } => {
                before.iter().any(Self::binds)
                    || rest.as_deref().is_some_and(Self::binds)
                    || after.iter().any(Self::binds)
            }
            hir::Pat::Struct { fields, .. } => fields.iter().any(|(_, f)| Self::binds(f)),
        }
    }

    /// `Point(x: a, y: 0)`: some of the fields of a `type`, each with its own pattern.
    fn struct_pattern(
        &mut self,
        p: &ast::Pattern,
        path: &[ast::Ident],
        fields: &[(ast::Ident, ast::Pattern)],
        ty: &Ty,
    ) -> hir::Pat {
        // `Shape.Rect(w: a)`: a variant, by the names of its values.
        if path.len() == 2 {
            let enum_name = match ty {
                Ty::Enum(k) => Some(k.clone()),
                _ => None,
            };
            let labels = enum_name
                .as_deref()
                .map(|k| self.labels_of(k, &path[1].node))
                .unwrap_or_default();
            if !labels.is_empty() {
                let mut args: Vec<ast::Pattern> = labels
                    .iter()
                    .map(|_| ast::Pattern {
                        node: ast::PatKind::Wild,
                        span: p.span.clone(),
                    })
                    .collect();
                for (field, sub) in fields {
                    match labels.iter().position(|l| *l == field.node) {
                        Some(i) => args[i] = sub.clone(),
                        None => {
                            let hint = closest(&field.node, labels.iter().map(String::as_str))
                                .map(str::to_string);
                            self.err_hint(
                                field.span.clone(),
                                format!(
                                    "`{}.{}` has no value named `{}`",
                                    path[0].node, path[1].node, field.node
                                ),
                                hint.as_deref(),
                            );
                        }
                    }
                }
                return self.variant_pattern(p, path, &args, ty);
            }
        }
        let name = path
            .iter()
            .map(|i| i.node.as_str())
            .collect::<Vec<_>>()
            .join(".");
        // A value of a generic `type` is an instance (`Box<Int>`); the pattern names the base.
        let key = match ty {
            Ty::Struct(k)
                if *k == name || self.instance_args.get(k).is_some_and(|(b, _)| *b == name) =>
            {
                Some(k.clone())
            }
            Ty::Unknown if self.struct_fields.contains_key(&name) => Some(name.clone()),
            _ => None,
        };
        let Some(key) = key else {
            let message = if self.struct_fields.contains_key(&name)
                || self.generic_structs.contains_key(&name)
            {
                format!("this pattern is a `{name}` but the value is `{ty}`")
            } else {
                format!("`{name}` is not a type; a pattern with `field: pattern` names a `type`")
            };
            self.err(p.span.clone(), message);
            return hir::Pat::Wild;
        };
        let declared = self.struct_fields.get(&key).cloned().unwrap_or_default();
        let mut out: Vec<(String, hir::Pat)> = Vec::new();
        for (field, sub) in fields {
            let Some((_, field_ty)) = declared.iter().find(|(n, _)| *n == field.node) else {
                let hint = closest(&field.node, declared.iter().map(|(n, _)| n.as_str()))
                    .map(str::to_string);
                self.err_hint(
                    field.span.clone(),
                    format!("no field `{}` on type `{name}`", field.node),
                    hint.as_deref(),
                );
                continue;
            };
            if out.iter().any(|(n, _)| *n == field.node) {
                self.err(
                    field.span.clone(),
                    format!("the field `{}` is in this pattern twice", field.node),
                );
                continue;
            }
            let field_ty = field_ty.clone();
            out.push((field.node.clone(), self.pattern(sub, &field_ty)));
        }
        hir::Pat::Struct {
            name: key,
            fields: out,
        }
    }

    fn variant_pattern(
        &mut self,
        p: &ast::Pattern,
        path: &[ast::Ident],
        args: &[ast::Pattern],
        ty: &Ty,
    ) -> hir::Pat {
        let bad = |this: &mut Self, msg: String| {
            this.err(p.span.clone(), msg);
            hir::Pat::Wild
        };
        let (enum_name, variant, field_tys) = match path {
            [one] if one.node == "Some" => {
                let payload = match ty {
                    Ty::Option(t) => Some((**t).clone()),
                    Ty::Unknown => Some(Ty::Unknown),
                    _ => None,
                };
                let Some(payload) = payload else {
                    return bad(
                        self,
                        format!("`Some` patterns need an `Option`, found `{ty}`"),
                    );
                };
                (
                    Some("Option".to_string()),
                    "Some".to_string(),
                    vec![payload],
                )
            }
            [one] if matches!(one.node.as_str(), "Ok" | "Err") => {
                let payload = match ty {
                    Ty::Result(o, e) => Some(if one.node == "Ok" {
                        (**o).clone()
                    } else {
                        (**e).clone()
                    }),
                    Ty::Unknown => Some(Ty::Unknown),
                    _ => None,
                };
                let Some(payload) = payload else {
                    return bad(
                        self,
                        format!("`{}` patterns need a `Result`, found `{ty}`", one.node),
                    );
                };
                (None, one.node.clone(), vec![payload])
            }
            [en, v] if self.generic_enums.contains_key(&en.node) => {
                let key = match &ty {
                    Ty::Enum(k)
                        if self
                            .instance_args
                            .get(k)
                            .is_some_and(|(b, _)| *b == en.node) =>
                    {
                        k.clone()
                    }
                    _ => {
                        return bad(
                            self,
                            format!(
                                "pattern of type `{}` cannot match a value of type `{ty}`",
                                en.node
                            ),
                        );
                    }
                };
                let Some(fields) = self.enums[&key]
                    .iter()
                    .find(|(n, _)| *n == v.node)
                    .map(|(_, f)| f.clone())
                else {
                    return bad(
                        self,
                        format!("enum `{}` has no variant `{}`", en.node, v.node),
                    );
                };
                (Some(key), v.node.clone(), fields)
            }
            [en, v] if self.enums.contains_key(&en.node) => {
                let enum_ty = Ty::Enum(en.node.clone());
                if !ty.accepts(&enum_ty) {
                    return bad(
                        self,
                        format!(
                            "pattern of type `{}` cannot match a value of type `{ty}`",
                            en.node
                        ),
                    );
                }
                let Some(fields) = self.enums[&en.node]
                    .iter()
                    .find(|(n, _)| *n == v.node)
                    .map(|(_, f)| f.clone())
                else {
                    return bad(
                        self,
                        format!("enum `{}` has no variant `{}`", en.node, v.node),
                    );
                };
                (Some(en.node.clone()), v.node.clone(), fields)
            }
            _ => {
                let name = path
                    .iter()
                    .map(|i| i.node.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                return bad(self, format!("`{name}` is not a variant of a known enum"));
            }
        };
        if field_tys.len() != args.len() {
            self.err(
                p.span.clone(),
                format!(
                    "`{variant}` has {} value(s), the pattern has {}",
                    field_tys.len(),
                    args.len()
                ),
            );
        }
        let args = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.pattern(a, field_tys.get(i).unwrap_or(&Ty::Unknown)))
            .collect();
        hir::Pat::Variant {
            enum_name,
            variant,
            args,
        }
    }

    /// Reports a `match` that can fall through. An arm with a guard may not be taken, so it does
    /// not count.
    fn check_exhaustive(&mut self, sty: &Ty, arms: &[hir::Arm], span: &Span) {
        let rows: Vec<Row> = arms
            .iter()
            .filter(|a| a.guard.is_none())
            .map(|a| vec![Some(&a.pat)])
            .collect();
        if self.exhaustive(&rows, std::slice::from_ref(sty)) {
            return;
        }
        let missing = match self.constructors_of(sty) {
            Some(ctors)
                if ctors
                    .iter()
                    .all(|(en, _, _)| !matches!(en.as_deref(), Some("(tuple)" | "(struct)"))) =>
            {
                let names: Vec<String> = ctors
                    .iter()
                    .filter(|(en, v, fields)| {
                        let sub = self.specialize(&rows, en.as_deref(), v, fields.len());
                        !self.exhaustive(&sub, fields)
                    })
                    .map(|(en, v, _)| match en.as_deref() {
                        Some("Option") | None => format!("`{v}`"),
                        Some(name) => format!("`{name}.{v}`"),
                    })
                    .collect();
                format!(": missing {}", names.join(", "))
            }
            _ if arms.iter().any(|a| a.guard.is_some()) => {
                ": an arm with `if` may not be taken, so add a `_` arm".to_string()
            }
            _ => ": add a `_` arm".to_string(),
        };
        self.err(span.clone(), format!("non-exhaustive `match`{missing}"));
    }

    /// For each arm, whether the arms before it (leaving out the ones with a guard) already take
    /// every value it could take.
    fn unreachable_arms(&self, sty: &Ty, arms: &[hir::Arm]) -> Vec<bool> {
        let mut rows: Vec<Row> = Vec::new();
        let mut out = Vec::new();
        for arm in arms {
            let row = [Some(&arm.pat)];
            out.push(!rows.is_empty() && !self.useful(&rows, &row, std::slice::from_ref(sty)));
            if arm.guard.is_none() {
                rows.push(vec![Some(&arm.pat)]);
            }
        }
        out
    }

    /// The constructors a value of `ty` can have, when `match` has to cover each of them.
    fn constructors_of(&self, ty: &Ty) -> Option<Vec<Constructor>> {
        match self.resolve(ty) {
            Ty::Enum(name) => self.enums.get(&name).map(|vs| {
                vs.iter()
                    .map(|(v, f)| (Some(name.clone()), v.clone(), f.clone()))
                    .collect()
            }),
            Ty::Result(o, e) => Some(vec![
                (None, "Ok".to_string(), vec![*o]),
                (None, "Err".to_string(), vec![*e]),
            ]),
            Ty::Tuple(parts) => Some(vec![(
                Some("(tuple)".to_string()),
                "(tuple)".to_string(),
                parts,
            )]),
            Ty::Struct(name) => self.struct_fields.get(&name).map(|fields| {
                vec![(
                    Some("(struct)".to_string()),
                    name.clone(),
                    fields.iter().map(|(_, t)| t.clone()).collect(),
                )]
            }),
            Ty::Option(t) => Some(vec![
                (Some("Option".to_string()), "Some".to_string(), vec![*t]),
                (Some("Option".to_string()), "None".to_string(), vec![]),
            ]),
            _ => None,
        }
    }

    /// The constructor a pattern stands for: its enum (or the marker of a tuple or a `type`) and
    /// its variant.
    fn ctor_of_pat(p: &hir::Pat) -> Option<(Option<String>, String)> {
        match p {
            hir::Pat::Variant {
                enum_name, variant, ..
            } => Some((enum_name.clone(), variant.clone())),
            hir::Pat::Tuple(_) => Some((Some("(tuple)".into()), "(tuple)".into())),
            hir::Pat::Struct { name, .. } => Some((Some("(struct)".into()), name.clone())),
            _ => None,
        }
    }

    /// The patterns of the parts of a constructor pattern, in order (a `type`'s fields in the
    /// order they were declared; the ones the pattern leaves out match anything).
    fn parts_of_pat<'p>(&self, p: &'p hir::Pat) -> Vec<Option<&'p hir::Pat>> {
        match p {
            hir::Pat::Variant { args, .. } | hir::Pat::Tuple(args) => {
                args.iter().map(Some).collect()
            }
            hir::Pat::Struct { name, fields } => self
                .struct_fields
                .get(name)
                .map(|declared| {
                    declared
                        .iter()
                        .map(|(n, _)| fields.iter().find(|(f, _)| f == n).map(|(_, p)| p))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Rows whose first column can match `variant`, with that column replaced by its parts.
    fn specialize<'p>(
        &self,
        rows: &[Row<'p>],
        enum_name: Option<&str>,
        variant: &str,
        arity: usize,
    ) -> Vec<Row<'p>> {
        let mut out = Vec::new();
        for row in expand_or(rows) {
            let rest = &row[1..];
            match row[0] {
                None | Some(hir::Pat::Wild | hir::Pat::Bind(_)) => {
                    let mut r = vec![None; arity];
                    r.extend_from_slice(rest);
                    out.push(r);
                }
                Some(p) => {
                    let Some((en, v)) = Self::ctor_of_pat(p) else {
                        continue;
                    };
                    if v == variant && en.as_deref() == enum_name {
                        let mut r = self.parts_of_pat(p);
                        r.resize(arity, None);
                        r.extend_from_slice(rest);
                        out.push(r);
                    }
                }
            }
        }
        out
    }

    /// Rows whose first column can match a list of exactly `n` items, with that column replaced by
    /// the `n` items.
    fn list_rows<'p>(rows: &[Row<'p>], n: usize) -> Vec<Row<'p>> {
        let mut out = Vec::new();
        for row in expand_or(rows) {
            let rest = &row[1..];
            match row[0] {
                None | Some(hir::Pat::Wild | hir::Pat::Bind(_)) => {
                    let mut r = vec![None; n];
                    r.extend_from_slice(rest);
                    out.push(r);
                }
                Some(hir::Pat::List {
                    before,
                    rest: gap,
                    after,
                }) => {
                    if let Some(mut r) = list_items(before, gap.is_some(), after, n) {
                        r.extend_from_slice(rest);
                        out.push(r);
                    }
                }
                Some(_) => {}
            }
        }
        out
    }

    /// Whether every value of `tys` matches some row.
    fn exhaustive(&self, rows: &[Row], tys: &[Ty]) -> bool {
        !self.useful(rows, &vec![None; tys.len()], tys)
    }

    /// Whether some value matches `q` and none of `rows`. With `q` made of wildcards, that is the
    /// question "is something left over?"; with the pattern of an arm, "can this arm ever be taken?".
    fn useful<'p>(&self, rows: &[Row<'p>], q: &[Option<&'p hir::Pat>], tys: &[Ty]) -> bool {
        let Some((first_ty, rest_tys)) = tys.split_first() else {
            return rows.is_empty();
        };
        // Nothing left to rule a value out. Without this, the constructors of a type that holds
        // itself (`Add(Expr, Expr)`) would be opened one level after another, without end.
        if rows.is_empty() {
            return true;
        }
        let rows = expand_or(rows);
        let (q0, q_rest) = (q[0], &q[1..]);
        if let Some(hir::Pat::Or(alts)) = q0 {
            return alts.iter().any(|alt| {
                let mut row = vec![Some(alt)];
                row.extend_from_slice(q_rest);
                self.useful(&rows, &row, tys)
            });
        }
        let first_ty = self.resolve(first_ty);
        let with_rest = |mut parts: Vec<Option<&'p hir::Pat>>| {
            parts.extend_from_slice(q_rest);
            parts
        };
        let types_then_rest = |mut parts: Vec<Ty>| {
            parts.extend_from_slice(rest_tys);
            parts
        };
        match q0 {
            None | Some(hir::Pat::Wild | hir::Pat::Bind(_)) => {
                // No row looks at the first value (all of them take anything): the rest decides.
                if rows
                    .iter()
                    .all(|r| matches!(r[0], None | Some(hir::Pat::Wild | hir::Pat::Bind(_))))
                {
                    let sub: Vec<Row> = rows.iter().map(|r| r[1..].to_vec()).collect();
                    return self.useful(&sub, q_rest, rest_tys);
                }
                if let Some(ctors) = self.constructors_of(&first_ty) {
                    return ctors.iter().any(|(en, v, fields)| {
                        let sub = self.specialize(&rows, en.as_deref(), v, fields.len());
                        self.useful(
                            &sub,
                            &with_rest(vec![None; fields.len()]),
                            &types_then_rest(fields.clone()),
                        )
                    });
                }
                if matches!(first_ty, Ty::Prim(Prim::Bool)) {
                    return [true, false].iter().any(|b| {
                        let sub = Self::literal_rows(&rows, &hir::Pat::Lit(ast::Lit::Bool(*b)));
                        self.useful(&sub, q_rest, rest_tys)
                    });
                }
                if let Ty::Vec(item) = &first_ty {
                    let longest = rows.iter().map(|r| list_min_len(r[0])).max().unwrap_or(0);
                    return (0..=longest + 1).any(|n| {
                        let sub = Self::list_rows(&rows, n);
                        self.useful(
                            &sub,
                            &with_rest(vec![None; n]),
                            &types_then_rest(vec![(**item).clone(); n]),
                        )
                    });
                }
                // Numbers, texts and unknown types: only a catch-all covers every value.
                let sub: Vec<Row> = rows
                    .iter()
                    .filter(|r| matches!(r[0], None | Some(hir::Pat::Wild | hir::Pat::Bind(_))))
                    .map(|r| r[1..].to_vec())
                    .collect();
                self.useful(&sub, q_rest, rest_tys)
            }
            Some(lit @ (hir::Pat::Lit(_) | hir::Pat::NegInt(_))) => {
                let sub = Self::literal_rows(&rows, lit);
                self.useful(&sub, q_rest, rest_tys)
            }
            Some(hir::Pat::List {
                before,
                rest,
                after,
            }) => {
                let min = before.len() + after.len();
                let longest = rows
                    .iter()
                    .map(|r| list_min_len(r[0]))
                    .max()
                    .unwrap_or(0)
                    .max(min);
                let lengths: Vec<usize> = if rest.is_some() {
                    (min..=longest + 1).collect()
                } else {
                    vec![min]
                };
                let item = match &first_ty {
                    Ty::Vec(t) => (**t).clone(),
                    _ => Ty::Unknown,
                };
                lengths.into_iter().any(|n| {
                    let sub = Self::list_rows(&rows, n);
                    let items = list_items(before, rest.is_some(), after, n).unwrap_or_default();
                    self.useful(
                        &sub,
                        &with_rest(items),
                        &types_then_rest(vec![item.clone(); n]),
                    )
                })
            }
            Some(p) => {
                let Some((en, v)) = Self::ctor_of_pat(p) else {
                    return true;
                };
                let Some((_, _, fields)) = self
                    .constructors_of(&first_ty)
                    .and_then(|cs| cs.into_iter().find(|(e, n, _)| *e == en && *n == v))
                else {
                    // A pattern of another type: that is reported where it is checked.
                    return true;
                };
                let sub = self.specialize(&rows, en.as_deref(), &v, fields.len());
                let mut parts = self.parts_of_pat(p);
                parts.resize(fields.len(), None);
                self.useful(&sub, &with_rest(parts), &types_then_rest(fields))
            }
        }
    }

    /// Rows whose first column matches the literal `lit` (or anything), without that column.
    fn literal_rows<'p>(rows: &[Row<'p>], lit: &hir::Pat) -> Vec<Row<'p>> {
        rows.iter()
            .filter(|r| match r[0] {
                None | Some(hir::Pat::Wild | hir::Pat::Bind(_)) => true,
                Some(p) => p == lit,
            })
            .map(|r| r[1..].to_vec())
            .collect()
    }

    fn path(&mut self, p: &[ast::Ident], expected: Option<&Ty>, span: &Span) -> hir::Expr {
        let mut names: Vec<String> = p.iter().map(|i| i.node.clone()).collect();
        // `Self::new` inside an `impl` is `Type::new`.
        if names.len() == 2
            && names[0] == "Self"
            && let Some(t) = &self.impl_ty
        {
            names[0] = t.clone();
        }
        let first = names[0].as_str();
        if names.len() == 1 {
            if let Some((id, ty)) = self.lookup_local(first) {
                return Self::mk(hir::ExprKind::Local(id), ty, span);
            }
            if let Some(ty) = self.globals.get(first) {
                return Self::mk(hir::ExprKind::Global(first.to_string()), ty.clone(), span);
            }
            if let Some((params, ret)) = self.fn_sigs.get(first) {
                let ty = Ty::Fn(params.clone(), Box::new(ret.clone()));
                return Self::mk(hir::ExprKind::Fn(first.to_string()), ty, span);
            }
            if self.generic_fns.contains_key(first) {
                self.err(
                    span.clone(),
                    format!("the generic function `{first}` has to be called, it cannot be used as a value"),
                );
                return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
            }
            if self.struct_fields.contains_key(first) {
                self.err(
                    span.clone(),
                    format!("`{first}` is a type, not a value; construct it with `{first}(field: value)`"),
                );
                return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
            }
        }
        if matches!(first, "Ok" | "Err") && names.len() == 1 {
            return Self::mk(hir::ExprKind::Extern(names), Ty::Unknown, span);
        }
        if names.len() == 2 {
            let is_variant = self
                .enums
                .get(first)
                .is_some_and(|vs| vs.iter().any(|(v, _)| *v == names[1]));
            let full = format!("{first}::{}", names[1]);
            if !is_variant && let Some((params, ret)) = self.fn_sigs.get(&full) {
                let ty = Ty::Fn(params.clone(), Box::new(ret.clone()));
                return Self::mk(hir::ExprKind::Fn(full), ty, span);
            }
            if self.enums.contains_key(first) {
                return self.enum_ctor(first, &p[1], &[], span);
            }
            if self.generic_enums.contains_key(first) {
                let key = self.generic_enum_instance(first, &p[1], &[], expected, span);
                return self.enum_ctor(&key, &p[1], &[], span);
            }
        }
        let resolved = if first == "std" {
            Some(names.clone())
        } else if names.len() == 1
            && let Some(full) = prelude(first)
        {
            Some(full)
        } else {
            self.imports.get(first).map(|base| {
                let mut full = base.clone();
                full.extend(names[1..].iter().cloned());
                full
            })
        };
        if let Some(full) = resolved {
            if full[0] == "std" {
                let joined = full.join("::");
                if stdlib::function(&joined).is_none() {
                    let what = if stdlib::is_type(&joined) || stdlib::is_module(&joined) {
                        format!("`{}` is not a value", names.join("."))
                    } else {
                        format!(
                            "cannot find `{}` in the standard library",
                            joined.replace("::", ".")
                        )
                    };
                    self.err(span.clone(), what);
                    return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
                }
            }
            return Self::mk(hir::ExprKind::Extern(full), Ty::Unknown, span);
        }
        let hint = if names.len() == 1 {
            let mut known: Vec<&str> = self
                .scopes
                .iter()
                .flat_map(|s| s.keys().map(String::as_str))
                .collect();
            known.extend(self.globals.keys().map(String::as_str));
            known.extend(self.fn_sigs.keys().map(String::as_str));
            // Spellings from other languages for the constants Telvax writes with a capital.
            let constant = match first {
                "true" => Some("True"),
                "false" => Some("False"),
                "none" | "null" | "nil" | "nothing" => Some("None"),
                _ => None,
            };
            constant
                .map(str::to_string)
                .or_else(|| closest(first, known).map(str::to_string))
        } else {
            None
        };
        self.err_hint(
            span.clone(),
            format!("cannot find `{}` in this scope", names.join(".")),
            hint.as_deref(),
        );
        Self::mk(hir::ExprKind::Error, Ty::Unknown, span)
    }

    /// Reports a method that the runtime does not have on a built-in type.
    fn no_method(&mut self, name: &ast::Ident, ty: &str, methods: &[&str]) -> Ty {
        let hint = closest(&name.node, methods.iter().copied()).map(str::to_string);
        self.err_hint(
            name.span.clone(),
            format!("no method `{}` on type `{ty}`", name.node),
            hint.as_deref(),
        );
        Ty::Unknown
    }

    fn field_ty(&mut self, base: &hir::Expr, name: &ast::Ident) -> Ty {
        let n = name.node.as_str();
        // Methods declared by a trait are called, not read.
        if self.trait_method(&base.ty, n).is_some() {
            return Ty::Unknown;
        }
        if let Ty::Trait(t) = self.resolve(&base.ty) {
            self.err(
                name.span.clone(),
                format!("the trait `{t}` has no method `{n}`"),
            );
            return Ty::Unknown;
        }
        // Methods of the user's own types are called, not read.
        if self.method_of(&base.ty, n).is_some() {
            return Ty::Unknown;
        }
        if self.generic_method_owner(&base.ty, n).is_some() {
            return Ty::Unknown;
        }
        // Methods that `call` turns into std functions are not looked up here.
        if method_module(&self.resolve(&base.ty), n).is_some() {
            return Ty::Unknown;
        }
        // `pair.0`, `pair.1`: the parts of a tuple.
        if let Ty::Tuple(parts) = self.resolve(&base.ty) {
            return match n.parse::<usize>().ok().and_then(|i| parts.get(i)) {
                Some(t) => t.clone(),
                None => {
                    let ty = base.ty.clone();
                    self.err(
                        name.span.clone(),
                        format!("`{ty}` has no part `{n}` (parts are numbered from 0)"),
                    );
                    Ty::Unknown
                }
            };
        }
        match &base.ty {
            Ty::Struct(s) => {
                let found = self
                    .struct_fields
                    .get(s)
                    .and_then(|f| f.iter().find(|(fname, _)| fname == n));
                match found {
                    Some((_, ty)) => ty.clone(),
                    None => {
                        let ty = base.ty.clone();
                        let hint = self.struct_fields.get(s).and_then(|f| {
                            closest(n, f.iter().map(|(name, _)| name.as_str())).map(str::to_string)
                        });
                        self.err_hint(
                            name.span.clone(),
                            format!("no field `{n}` on type `{ty}`"),
                            hint.as_deref(),
                        );
                        Ty::Unknown
                    }
                }
            }
            Ty::Prim(p) if p.is_text() => match n {
                "is_empty" => Ty::Fn(vec![], Box::new(Ty::bool())),
                "len" => Ty::Fn(vec![], Box::new(Ty::Prim(Prim::U64))),
                "trim" | "to_lowercase" | "to_uppercase" => Ty::Fn(vec![], Box::new(Ty::str())),
                "contains" | "starts_with" | "ends_with" => {
                    Ty::Fn(vec![Ty::str()], Box::new(Ty::bool()))
                }
                "replace" => Ty::Fn(vec![Ty::str(), Ty::str()], Box::new(Ty::str())),
                "split" => Ty::Fn(vec![Ty::str()], Box::new(Ty::Vec(Box::new(Ty::str())))),
                "find" => Ty::Fn(
                    vec![Ty::str()],
                    Box::new(Ty::Option(Box::new(Ty::Prim(Prim::U64)))),
                ),
                "to_int" => Ty::Fn(vec![], Box::new(Ty::Option(Box::new(Ty::Prim(Prim::I64))))),
                "to_float" => Ty::Fn(vec![], Box::new(Ty::Option(Box::new(Ty::Prim(Prim::F64))))),
                "chars" | "lines" => Ty::Fn(vec![], Box::new(Ty::Vec(Box::new(Ty::str())))),
                _ => {
                    let all = stdlib::builtin_method_names(&Ty::str());
                    self.no_method(name, "Text", &all)
                }
            },
            Ty::Vec(elem) => match n {
                "is_empty" => Ty::Fn(vec![], Box::new(Ty::bool())),
                "len" => Ty::Fn(vec![], Box::new(Ty::Prim(Prim::U64))),
                "push" => Ty::Fn(vec![(**elem).clone()], Box::new(Ty::Unit)),
                "contains" => Ty::Fn(vec![(**elem).clone()], Box::new(Ty::bool())),
                "first" | "last" | "pop" => Ty::Fn(vec![], Box::new(Ty::Option(elem.clone()))),
                "get" => {
                    let index = self.fresh(false);
                    Ty::Fn(vec![index], Box::new(Ty::Option(elem.clone())))
                }
                "sort" | "reverse" => Ty::Fn(vec![], Box::new(Ty::Unit)),
                _ => {
                    let shown = base.ty.to_string();
                    let all = stdlib::builtin_method_names(&base.ty);
                    self.no_method(name, &shown, &all)
                }
            },
            Ty::Option(inner) => match n {
                "is_some" | "is_none" => Ty::Fn(vec![], Box::new(Ty::bool())),
                "unwrap" => Ty::Fn(vec![], inner.clone()),
                "unwrap_or" => Ty::Fn(vec![(**inner).clone()], inner.clone()),
                _ => {
                    let shown = base.ty.to_string();
                    let all = stdlib::builtin_method_names(&base.ty);
                    self.no_method(name, &shown, &all)
                }
            },
            Ty::Result(..) => {
                let shown = base.ty.to_string();
                let all = stdlib::builtin_method_names(&base.ty);
                self.no_method(name, &shown, &all)
            }
            Ty::Map(k, v) => match n {
                "insert" => Ty::Fn(vec![(**k).clone(), (**v).clone()], Box::new(Ty::Unit)),
                "get" | "remove" => Ty::Fn(vec![(**k).clone()], Box::new(Ty::Option(v.clone()))),
                "contains_key" => Ty::Fn(vec![(**k).clone()], Box::new(Ty::bool())),
                "len" => Ty::Fn(vec![], Box::new(Ty::Prim(Prim::U64))),
                "is_empty" => Ty::Fn(vec![], Box::new(Ty::bool())),
                "keys" => Ty::Fn(vec![], Box::new(Ty::Vec(k.clone()))),
                "entries" => Ty::Fn(
                    vec![],
                    Box::new(Ty::Vec(Box::new(Ty::Tuple(vec![
                        (**k).clone(),
                        (**v).clone(),
                    ])))),
                ),
                "values" => Ty::Fn(vec![], Box::new(Ty::Vec(v.clone()))),
                _ => {
                    let shown = base.ty.to_string();
                    let all = stdlib::builtin_method_names(&base.ty);
                    self.no_method(name, &shown, &all)
                }
            },
            Ty::Table(s) => match n {
                // A method call: `call` turns it into `std::table::sql(table, ..)`.
                "sql" | "filter" | "head" | "tail" | "sort_by" | "sort_by_desc" | "concat"
                | "split" | "select" | "group_by" => Ty::Unknown,
                "is_empty" => Ty::Fn(vec![], Box::new(Ty::bool())),
                "len" => Ty::Fn(vec![], Box::new(Ty::Prim(Prim::U64))),
                _ => {
                    let column = self
                        .struct_fields
                        .get(s)
                        .and_then(|f| f.iter().find(|(fname, _)| fname == n))
                        .map(|(_, t)| t.clone());
                    match column {
                        // Numeric columns are tensors that share the table's Arrow buffer.
                        Some(Ty::Prim(p)) if p.is_numeric() => Ty::Tensor {
                            elem: Box::new(Ty::Prim(p)),
                            shape: vec![Dim::Named("N".into())],
                        },
                        Some(Ty::Prim(p)) => Ty::Vec(Box::new(Ty::Prim(p))),
                        // A column with empty values is a list of `T?`.
                        Some(opt @ Ty::Option(_)) => Ty::Vec(Box::new(opt)),
                        Some(other) => other,
                        None => {
                            let ty = base.ty.clone();
                            self.err(name.span.clone(), format!("no column `{n}` on type `{ty}`"));
                            Ty::Unknown
                        }
                    }
                }
            },
            // An enum has the methods it declares (found above), and no others.
            Ty::Enum(owner_name) => {
                let own: Vec<String> = self
                    .methods
                    .iter()
                    .filter(|(_, owner)| *owner == owner_name)
                    .filter_map(|(full, _)| full.rsplit("::").next().map(str::to_string))
                    .collect();
                let own: Vec<&str> = own.iter().map(String::as_str).collect();
                let shown = owner_name.clone();
                self.no_method(name, &shown, &own)
            }
            // Numbers and booleans have no methods of their own.
            Ty::Prim(_) | Ty::IntVar(_) | Ty::FloatVar(_) => {
                let shown = self.resolve(&base.ty).to_string();
                self.no_method(name, &shown, &[])
            }
            _ => Ty::Unknown,
        }
    }

    /// The variant that `?` wraps an error of type `from` in, so a function that returns an
    /// error enum can use `?` on the errors of what it calls: the variant of `to` marked `from`
    /// that holds a value of that type.
    fn error_conversion(&self, to: &Ty, from: &Ty) -> Option<String> {
        let Ty::Enum(name) = self.resolve(to) else {
            return None;
        };
        if self.instance_args.contains_key(&name) {
            return None;
        }
        let from = self.resolve(from);
        let (variant, _) = self.enums.get(&name)?.iter().find(|(variant, fields)| {
            self.enum_from.contains(&(name.clone(), variant.clone()))
                && fields.len() == 1
                && fields[0].accepts(&from)
                && from.accepts(&fields[0])
        })?;
        Some(variant.clone())
    }

    /// `x?` where the error is wrapped: `match x: case Ok(v): v; case Err(e): return Err(Enum.Variant(e))`.
    fn convert_error(inner: &ast::Expr, to: &Ty, variant: &str, span: &Span) -> ast::Expr {
        let Ty::Enum(enum_name) = to else {
            unreachable!("a conversion target is an enum")
        };
        let ident = |name: &str| ast::Ident {
            node: name.to_string(),
            span: span.clone(),
        };
        let expr = |node: ast::ExprKind| ast::Expr {
            node,
            span: span.clone(),
        };
        let path = |names: &[&str]| {
            expr(ast::ExprKind::Path(
                names.iter().map(|n| ident(n)).collect(),
            ))
        };
        let (value, error) = (
            format!("__try_value{}", span.start),
            format!("__try_error{}", span.start),
        );
        let wrapped = expr(ast::ExprKind::Call {
            callee: Box::new(path(&["Err"])),
            args: vec![expr(ast::ExprKind::Call {
                callee: Box::new(path(&[enum_name.as_str(), variant])),
                args: vec![path(&[error.as_str()])],
            })],
        });
        let arm = |name: &str, bound: &str, body: ast::Expr| ast::Arm {
            pat: ast::Pattern {
                node: ast::PatKind::Variant {
                    path: vec![ident(name)],
                    args: vec![ast::Pattern {
                        node: ast::PatKind::Bind(bound.to_string()),
                        span: span.clone(),
                    }],
                },
                span: span.clone(),
            },
            guard: None,
            body,
        };
        expr(ast::ExprKind::Match {
            scrutinee: Box::new(inner.clone()),
            arms: vec![
                arm("Ok", &value, path(&[value.as_str()])),
                arm(
                    "Err",
                    &error,
                    expr(ast::ExprKind::Return(Some(Box::new(wrapped)))),
                ),
            ],
        })
    }

    /// A path such as `Shape.Empty.area` or `Shape.Empty.size`: the variant `Shape.Empty`, then
    /// field reads and method calls on it. The parser cannot tell, because it reads any path that
    /// starts with a capital as one name.
    fn variant_then_fields(&self, p: &[ast::Ident], span: &Span) -> Option<ast::Expr> {
        if p.len() < 3 || self.lookup_local(&p[0].node).is_some() {
            return None;
        }
        let first = p[0].node.as_str();
        let has_variant = self
            .enums
            .get(first)
            .is_some_and(|vs| vs.iter().any(|(v, _)| *v == p[1].node))
            || self
                .generic_enums
                .get(first)
                .is_some_and(|en| en.variants.iter().any(|v| v.name.node == p[1].node));
        if !has_variant {
            return None;
        }
        let mut chain = ast::Expr {
            node: ast::ExprKind::Path(p[..2].to_vec()),
            span: span.clone(),
        };
        for name in &p[2..] {
            chain = ast::Expr {
                node: ast::ExprKind::Field {
                    base: Box::new(chain),
                    name: name.clone(),
                },
                span: span.clone(),
            };
        }
        Some(chain)
    }

    fn call(
        &mut self,
        callee: &ast::Expr,
        args: &[ast::Expr],
        expected: Option<&Ty>,
        span: &Span,
    ) -> hir::Expr {
        // `Shape.Empty.area()`: a method of a variant that has no values.
        if let ast::ExprKind::Path(p) = &callee.node
            && let Some(chain) = self.variant_then_fields(p, &callee.span)
        {
            return self.call(&chain, args, expected, span);
        }
        // `first[Int](xs)`: the types a generic function is called with, written down.
        if let ast::ExprKind::Index { base, index } = &callee.node
            && let ast::ExprKind::Path(p) = &base.node
            && p.len() == 1
            && self.lookup_local(&p[0].node).is_none()
            && !self.fn_sigs.contains_key(&p[0].node)
            && self.generic_fns.contains_key(&p[0].node)
        {
            let parts = match &index.node {
                ast::ExprKind::Tuple(items) => items.clone(),
                _ => vec![(**index).clone()],
            };
            let mut types = Vec::new();
            for part in &parts {
                match expr_as_type(part) {
                    Some(t) => types.push(t),
                    None => {
                        self.err(
                            part.span.clone(),
                            "a type argument is a type name, for example `first[Int](xs)`",
                        );
                        return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
                    }
                }
            }
            return self.generic_call(&p[0].node.clone(), None, &types, args, expected, span);
        }
        if let ast::ExprKind::Path(p) = &callee.node {
            if p.len() == 1
                && self.lookup_local(&p[0].node).is_none()
                && !self.fn_sigs.contains_key(&p[0].node)
                && self.generic_fns.contains_key(&p[0].node)
            {
                return self.generic_call(&p[0].node.clone(), None, &[], args, expected, span);
            }
            if p.len() == 2
                && (self.generic_structs.contains_key(&p[0].node)
                    || self.generic_enums.contains_key(&p[0].node))
                && self.generic_impls.get(&p[0].node).is_some_and(|imps| {
                    imps.iter()
                        .any(|i| i.fns.iter().any(|f| f.name.node == p[1].node))
                })
            {
                return self.generic_call(
                    &p[1].node.clone(),
                    Some(&p[0].node.clone()),
                    &[],
                    args,
                    expected,
                    span,
                );
            }
        }
        if let ast::ExprKind::Path(p) = &callee.node
            && p.len() == 2
        {
            let first = if p[0].node == "Self" {
                self.impl_ty.clone().unwrap_or_default()
            } else {
                p[0].node.clone()
            };
            let is_variant = self
                .enums
                .get(&first)
                .is_some_and(|vs| vs.iter().any(|(v, _)| *v == p[1].node));
            if is_variant {
                return self.enum_ctor(&first, &p[1], args, span);
            }
            if self.generic_enums.contains_key(&first)
                && self.generic_enums[&first]
                    .variants
                    .iter()
                    .any(|v| v.name.node == p[1].node)
            {
                let key = self.generic_enum_instance(&first, &p[1], args, expected, span);
                return self.enum_ctor(&key, &p[1], args, span);
            }
        }
        if let ast::ExprKind::Path(p) = &callee.node
            && self.is_option_ctor(p, "Some")
        {
            if args.len() != 1 {
                self.err(span.clone(), "`Some` takes exactly one argument");
            }
            if let Some(arg) = args.first() {
                return self.some_ctor(arg, expected, span);
            }
        }
        // `Ok(x)` / `Err(x)` build a `Result`, typed from context when available.
        if let ast::ExprKind::Path(p) = &callee.node
            && p.len() == 1
            && self.lookup_local(&p[0].node).is_none()
            && !self.fn_sigs.contains_key(&p[0].node)
        {
            let ctor = p[0].node.as_str();
            if (ctor == "Ok" || ctor == "Err") && args.len() == 1 {
                return self.result_ctor(ctor == "Ok", callee, &args[0], expected, span);
            }
            if ctor == "Ok" || ctor == "Err" {
                self.err(span.clone(), format!("`{ctor}` takes exactly one argument"));
            }
        }

        let callee = self.expr(callee, None);
        // A method called on a trait-typed value is dispatched on the actual type at run time.
        if let hir::ExprKind::Field { base, name } = &callee.kind
            && let Some(trait_name) = self.trait_method(&base.ty, name)
        {
            let receiver = (**base).clone();
            return self.dyn_call(&trait_name, name, receiver, args, span);
        }
        // `b.map(f)` where `map` has type parameters of its own.
        if let hir::ExprKind::Field { base, name } = &callee.kind
            && let Some(owner) = self.generic_method_owner(&base.ty, name)
        {
            let receiver = (**base).clone();
            return self.generic_method_call(receiver, &owner, name, args, expected, span);
        }
        // `p.norm()` where `norm` is a method of `p`'s type: `Point::norm(p)`.
        if let hir::ExprKind::Field { base, name } = &callee.kind
            && let Some(full) = self.method_of(&base.ty, name)
        {
            let receiver = (**base).clone();
            return self.method_call(&full, receiver, args, span);
        }
        // `t.relu()`, `x.matmul(w)`, `sales.sql("..")`: a method on a tensor or table is the std
        // function of the same name with the receiver as its first argument.
        if let hir::ExprKind::Field { base, name } = &callee.kind
            && let Some(module) = method_module(&self.resolve(&base.ty), name)
        {
            let path = ["std", module, name.as_str()].map(String::from).to_vec();
            if let Some(sig) = stdlib::function(&path.join("::")) {
                let receiver = (**base).clone();
                let func = Self::mk(hir::ExprKind::Extern(path), Ty::Unknown, &callee.span);
                return self.std_call_with(func, sig, Some(receiver), args, expected, span);
            }
        }
        if let Some(sig) = self.std_sig(&callee) {
            return self.std_call(callee, sig, args, expected, span);
        }
        match callee.ty.clone() {
            Ty::Fn(params, ret) => {
                if params.len() != args.len() {
                    self.err(
                        span.clone(),
                        format!(
                            "expected {} argument(s), found {}",
                            params.len(),
                            args.len()
                        ),
                    );
                }
                let args = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let want = params.get(i);
                        let a = self.expr(a, want);
                        match want {
                            Some(w) => self.coerce(a, w),
                            None => a,
                        }
                    })
                    .collect();
                Self::mk(
                    hir::ExprKind::Call {
                        callee: Box::new(callee),
                        args,
                    },
                    *ret,
                    span,
                )
            }
            other => {
                if other != Ty::Unknown {
                    self.err(
                        callee.span.clone(),
                        format!("expected a function, found `{other}`"),
                    );
                }
                let args = args.iter().map(|a| self.expr(a, None)).collect();
                Self::mk(
                    hir::ExprKind::Call {
                        callee: Box::new(callee),
                        args,
                    },
                    Ty::Unknown,
                    span,
                )
            }
        }
    }

    /// Typing rule for `callee` when it names a std function or a method of a std type.
    fn std_sig(&mut self, callee: &hir::Expr) -> Option<Sig> {
        match &callee.kind {
            hir::ExprKind::Extern(path) if path.first().is_some_and(|p| p == "std") => {
                stdlib::function(&path.join("::"))
            }
            hir::ExprKind::Field { base, name } => {
                let Ty::Named(ty) = &base.ty else { return None };
                match stdlib::method(ty, name) {
                    Some(sig) => Some(sig),
                    None => {
                        if stdlib::is_std_type(ty) {
                            self.err(
                                callee.span.clone(),
                                format!("no method `{name}` on type `{ty}`"),
                            );
                        }
                        None
                    }
                }
            }
            _ => None,
        }
    }

    fn std_call(
        &mut self,
        callee: hir::Expr,
        sig: Sig,
        args: &[ast::Expr],
        expected: Option<&Ty>,
        span: &Span,
    ) -> hir::Expr {
        self.std_call_with(callee, sig, None, args, expected, span)
    }

    /// Like `std_call`; `receiver` is an already checked first argument (`x.f(a)` is `f(x, a)`).
    fn std_call_with(
        &mut self,
        callee: hir::Expr,
        sig: Sig,
        receiver: Option<hir::Expr>,
        args: &[ast::Expr],
        expected: Option<&Ty>,
        span: &Span,
    ) -> hir::Expr {
        let mut checked: Vec<hir::Expr> = Vec::with_capacity(args.len() + 1);
        let mut tys: Vec<Ty> = Vec::with_capacity(args.len() + 1);
        if let Some(r) = receiver {
            tys.push(r.ty.clone());
            checked.push(r);
        }
        for a in args {
            let hint = (sig.hint)(checked.len(), &tys);
            let a = self.expr(a, hint.as_ref());
            tys.push(a.ty.clone());
            checked.push(a);
        }
        // Distributed calls ship the function by name, so only top-level functions qualify.
        if let hir::ExprKind::Extern(path) = &callee.kind
            && path.join("::") == "std::dist::map"
            && let Some(f) = checked.get(1)
            && !matches!(f.kind, hir::ExprKind::Fn(_))
        {
            self.diags.push(
                Diagnostic::error(
                    f.span.clone(),
                    "`std.dist.map` needs the name of a top-level function",
                )
                .with_note("closures capture local variables and cannot be sent to other machines"),
            );
        }
        let tys: Vec<Ty> = tys.iter().map(|t| self.resolve(t)).collect();
        let mut ty = match (sig.check)(&tys) {
            Ok(t) => t,
            Err(msg) => {
                self.err(span.clone(), msg);
                Ty::Unknown
            }
        };
        if let Some(from_expected) = sig.from_expected {
            let want = expected.map(|e| self.resolve(e));
            ty = match want {
                // The context knows nothing about the type; the runtime checks the real shape.
                Some(Ty::Unknown | Ty::Never) => Ty::Unknown,
                other => match from_expected(&tys, &other.unwrap_or(Ty::Unknown)) {
                    Ok(t) => t,
                    Err(msg) => {
                        self.err(span.clone(), msg);
                        Ty::Unknown
                    }
                },
            };
        }
        if let hir::ExprKind::Extern(path) = &callee.kind
            && path.join("::") == "std::table::select"
            && matches!(ty, Ty::Table(_))
        {
            ty = self.select_type(&checked, span);
        }
        if let hir::ExprKind::Extern(path) = &callee.kind
            && path.join("::") == "std::table::group_by"
            && matches!(ty, Ty::Table(_))
        {
            ty = self.group_by_type(&checked, span);
        }
        // `str(x)` is `f"{x}"`, so a value is written the same way in both.
        if let hir::ExprKind::Extern(path) = &callee.kind
            && path.join("::") == "std::text::to_text"
            && checked.len() == 1
        {
            let value = checked.remove(0);
            return Self::mk(
                hir::ExprKind::FString(vec![hir::FPart::Expr(value)]),
                Ty::str(),
                span,
            );
        }
        Self::mk(
            hir::ExprKind::Call {
                callee: Box::new(callee),
                args: checked,
            },
            ty,
            span,
        )
    }

    /// Whether `p` is the bare name `ctor` (`Some` / `None`) and nothing in scope shadows it.
    fn is_option_ctor(&self, p: &[ast::Ident], ctor: &str) -> bool {
        p.len() == 1
            && p[0].node == ctor
            && self.lookup_local(ctor).is_none()
            && !self.fn_sigs.contains_key(ctor)
            && !self.globals.contains_key(ctor)
    }

    /// `Some(value)`, typed from the expected `Option<T>` when there is one.
    fn some_ctor(&mut self, arg: &ast::Expr, expected: Option<&Ty>, span: &Span) -> hir::Expr {
        let want = match expected {
            Some(Ty::Option(t)) => (**t).clone(),
            _ => Ty::Unknown,
        };
        let arg = self.expr(arg, Some(&want));
        let arg = self.coerce(arg, &want);
        let ty = match expected {
            Some(e @ Ty::Option(_)) if e.accepts(&Ty::Option(Box::new(arg.ty.clone()))) => {
                e.clone()
            }
            _ => Ty::Option(Box::new(arg.ty.clone())),
        };
        Self::mk(
            hir::ExprKind::EnumCtor {
                enum_name: "Option".into(),
                variant: "Some".into(),
                args: vec![arg],
            },
            ty,
            span,
        )
    }

    fn result_ctor(
        &mut self,
        is_ok: bool,
        callee: &ast::Expr,
        arg: &ast::Expr,
        expected: Option<&Ty>,
        span: &Span,
    ) -> hir::Expr {
        let (want_ok, want_err) = match expected {
            Some(Ty::Result(o, e)) => ((**o).clone(), (**e).clone()),
            _ => (Ty::Unknown, Ty::Unknown),
        };
        let want = if is_ok { &want_ok } else { &want_err };
        let arg = self.expr(arg, Some(want));
        let errors = self.diags.len();
        let arg = self.coerce(arg, want);
        let ty = if is_ok {
            Ty::Result(Box::new(arg.ty.clone()), Box::new(want_err))
        } else {
            Ty::Result(Box::new(want_ok), Box::new(arg.ty.clone()))
        };
        // Keep the declared type when the argument is less specific.
        let ty = match expected {
            Some(e @ Ty::Result(..)) if e.accepts(&ty) || self.diags.len() > errors => e.clone(),
            _ => ty,
        };
        let callee_name = match &callee.node {
            ast::ExprKind::Path(p) => p.iter().map(|i| i.node.clone()).collect(),
            _ => vec![],
        };
        let callee = Self::mk(
            hir::ExprKind::Extern(callee_name),
            Ty::Unknown,
            &callee.span,
        );
        Self::mk(
            hir::ExprKind::Call {
                callee: Box::new(callee),
                args: vec![arg],
            },
            ty,
            span,
        )
    }

    /// `lhs |> f(a, b)` is checked as `f(lhs, a, b)`; `lhs |> f` as `f(lhs)`.
    fn pipe(&mut self, lhs: &ast::Expr, rhs: &ast::Expr, span: &Span) -> hir::Expr {
        // `named` turns every pipe with a function or a call on the right into a call, so what
        // arrives here is a right side that is neither.
        let mut d = Diagnostic::error(
            rhs.span.clone(),
            "the right side of `|>` must be a function or a call",
        )
        .with_note("write `x |> f()` or `x |> f`");
        if matches!(rhs.node, ast::ExprKind::Binary { .. }) {
            d = d.with_note(
                "`|>` binds looser than every operator, so `x |> f == 1` pipes into `f == 1`: write `(x |> f()) == 1`",
            );
        }
        self.diags.push(d);
        let _ = self.expr(lhs, None);
        Self::mk(hir::ExprKind::Error, Ty::Unknown, span)
    }

    /// `a + b` and friends when at least one side is a tensor.
    /// A call of the std function `std.<module>.<name>` with arguments that are already checked.
    fn std_call_expr(
        &mut self,
        module: &str,
        name: &str,
        args: Vec<hir::Expr>,
        span: &Span,
    ) -> hir::Expr {
        let path: Vec<String> = ["std", module, name].map(String::from).to_vec();
        let sig = stdlib::function(&path.join("::")).expect("std function exists");
        let tys: Vec<Ty> = args.iter().map(|a| self.resolve(&a.ty)).collect();
        let ty = match (sig.check)(&tys) {
            Ok(t) => t,
            Err(msg) => {
                self.err(span.clone(), msg);
                Ty::Unknown
            }
        };
        let callee = Self::mk(hir::ExprKind::Extern(path), Ty::Unknown, span);
        Self::mk(
            hir::ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            ty,
            span,
        )
    }

    fn tensor_operator(
        &mut self,
        op: ast::BinOp,
        l: hir::Expr,
        r: hir::Expr,
        span: &Span,
    ) -> hir::Expr {
        use ast::BinOp::{Add, Div, Mul, Sub};
        let is_tensor = |t: &Ty| matches!(t, Ty::Tensor { .. });
        let (lt, rt) = (self.resolve(&l.ty), self.resolve(&r.ty));
        let is_number = |t: &Ty| {
            matches!(t, Ty::Prim(p) if p.is_numeric())
                || matches!(t, Ty::IntVar(_) | Ty::FloatVar(_))
        };
        let (name, args) = match (op, is_tensor(&lt), is_tensor(&rt)) {
            (Add, true, true) => ("add", vec![l, r]),
            (Sub, true, true) => ("sub", vec![l, r]),
            (Mul, true, true) => ("mul", vec![l, r]),
            (Div, true, true) => ("div", vec![l, r]),
            (Mul, true, false) if is_number(&rt) => ("scale", vec![l, r]),
            (Mul, false, true) if is_number(&lt) => ("scale", vec![r, l]),
            _ => {
                let hint = match op {
                    Div => {
                        "to divide a tensor by a number, multiply by its reciprocal: `t * (1.0 / k)`"
                    }
                    _ => {
                        "operators on tensors need two tensors of the same shape, or `*` with a number"
                    }
                };
                self.diags.push(
                    Diagnostic::error(
                        span.clone(),
                        format!("cannot apply this operator to `{lt}` and `{rt}`"),
                    )
                    .with_note(hint),
                );
                return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
            }
        };
        let path: Vec<String> = ["std", "tensor", name].map(String::from).to_vec();
        let sig = stdlib::function(&path.join("::")).expect("tensor function exists");
        let tys: Vec<Ty> = args.iter().map(|a| self.resolve(&a.ty)).collect();
        let ty = match (sig.check)(&tys) {
            Ok(t) => t,
            Err(msg) => {
                self.err(span.clone(), msg);
                Ty::Unknown
            }
        };
        let callee = Self::mk(hir::ExprKind::Extern(path), Ty::Unknown, span);
        Self::mk(
            hir::ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            ty,
            span,
        )
    }

    fn binary(
        &mut self,
        op: ast::BinOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: &Span,
    ) -> hir::Expr {
        use ast::BinOp::*;
        let l = self.expr(lhs, None);
        let r = self.expr(rhs, Some(&l.ty));
        // Literals adopt the other side's numeric type: `x + 1` with `x: f32`.
        let l = if matches!(lhs.node, ast::ExprKind::Lit(_)) && l.ty != r.ty && r.ty != Ty::Unknown
        {
            self.expr(lhs, Some(&r.ty))
        } else {
            l
        };
        // Operators on tensors are the elementwise std functions: `a + b`, `a - b`, `a * b`,
        // `a / b` for tensors of the same shape, and `a * k` / `k * a` to scale by a number.
        if matches!(op, Add | Sub | Mul | Div) {
            let (lt, rt) = (self.resolve(&l.ty), self.resolve(&r.ty));
            let is_tensor = |t: &Ty| matches!(t, Ty::Tensor { .. });
            if is_tensor(&lt) || is_tensor(&rt) {
                return self.tensor_operator(op, l, r, span);
            }
        }
        // `"ab" * 3` repeats a text and `xs + ys` joins two lists.
        {
            let (lt, rt) = (self.resolve(&l.ty), self.resolve(&r.ty));
            let is_int =
                |t: &Ty| matches!(t, Ty::Prim(p) if p.is_int()) || matches!(t, Ty::IntVar(_));
            let call = match op {
                Mul if lt == Ty::str() && is_int(&rt) => Some(("string", "repeat")),
                Add if matches!((&lt, &rt), (Ty::Vec(_), Ty::Vec(_))) => Some(("iter", "concat")),
                _ => None,
            };
            if let Some((module, name)) = call {
                return self.std_call_expr(module, name, vec![l, r], span);
            }
        }
        let compatible = self.unify(&l.ty, &r.ty);
        let mut reported = !compatible;
        if !compatible {
            let (lt, rt) = (self.resolve(&l.ty), self.resolve(&r.ty));
            let mut diag = Diagnostic::error(
                span.clone(),
                format!("mismatched operand types: `{lt}` and `{rt}`"),
            )
            .with_label(l.span.clone(), format!("this is `{lt}`"))
            .with_label(r.span.clone(), format!("this is `{rt}`"));
            // A text and something else: what was meant is most often a value in a text.
            if (lt == Ty::str()) != (rt == Ty::str()) {
                diag = diag.with_note(if op == Rem && lt == Ty::str() {
                    "`%` does not format a text; write an f-string: `f\"{n} items\"`"
                } else {
                    "to put a value in a text, write an f-string, `f\"n={x}\"`, or turn it into text with `str(x)`"
                });
            }
            self.diags.push(diag);
        }
        let is_num = |t: &Ty| {
            matches!(t, Ty::Prim(p) if p.is_numeric())
                || matches!(t, Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Unknown)
        };
        let ty = match op {
            Add | Sub | Mul | Div | Rem => {
                let string_concat = op == Add && l.ty == Ty::str();
                if compatible && !is_num(&l.ty) && !string_concat {
                    self.err(
                        span.clone(),
                        format!("cannot apply arithmetic to `{}`", l.ty),
                    );
                    reported = true;
                }
                // After an error the result is unknown, so it is not reported a second time.
                if reported {
                    Ty::Unknown
                } else if l.ty == Ty::Unknown {
                    r.ty.clone()
                } else {
                    l.ty.clone()
                }
            }
            Lt | LtEq | Gt | GtEq => {
                let unit_enum = matches!(&l.ty, Ty::Enum(name)
                    if self.enums.get(name).is_some_and(|vs| vs.iter().all(|(_, f)| f.is_empty())));
                if compatible && !is_num(&l.ty) && l.ty != Ty::str() && !unit_enum {
                    self.err(
                        span.clone(),
                        format!("cannot order values of type `{}`", l.ty),
                    );
                }
                Ty::bool()
            }
            Eq | NotEq => Ty::bool(),
            And | Or => {
                for side in [&l, &r] {
                    if !Ty::bool().accepts(&side.ty) {
                        self.err(
                            side.span.clone(),
                            format!("expected `Bool`, found `{}`", side.ty),
                        );
                    }
                }
                Ty::bool()
            }
        };
        Self::mk(
            hir::ExprKind::Binary {
                op,
                lhs: Box::new(l),
                rhs: Box::new(r),
            },
            ty,
            span,
        )
    }

    /// Which instance of the generic struct `base` a literal builds: the expected type, the
    /// enclosing `impl`, or what its field values say.
    fn generic_struct_instance(
        &mut self,
        base: &str,
        fields: &[(ast::Ident, ast::Expr)],
        expected: Option<&Ty>,
        span: &Span,
    ) -> String {
        let of_base =
            |this: &Self, k: &str| this.instance_args.get(k).is_some_and(|(b, _)| b == base);
        if let Some(Ty::Struct(k)) = expected.map(|t| self.resolve(t))
            && of_base(self, &k)
        {
            return k;
        }
        if let Some(k) = self.impl_ty.clone()
            && of_base(self, &k)
        {
            return k;
        }
        let decl = self.generic_structs[base].clone();
        let param_types: HashMap<String, Ty> = decl
            .generics
            .iter()
            .map(|g| (g.name.node.clone(), Ty::Param(g.name.node.clone())))
            .collect();
        let saved = std::mem::replace(&mut self.type_params, param_types);
        let declared: Vec<(String, Ty)> = decl
            .fields
            .iter()
            .map(|f| (f.name.node.clone(), self.resolve_type(&f.ty)))
            .collect();
        self.type_params = saved;
        let mut bind = HashMap::new();
        let keep = self.diags.len();
        for (fname, value) in fields {
            if let Some((_, d)) = declared.iter().find(|(n, _)| *n == fname.node) {
                let e = self.expr(value, None);
                self.infer_params(d, &e.ty, &mut bind);
            }
        }
        self.diags.truncate(keep);
        let mut args = Vec::new();
        for g in &decl.generics {
            match bind.get(&g.name.node) {
                Some(t) => args.push(t.clone()),
                None => {
                    self.err(
                        span.clone(),
                        format!(
                            "cannot infer the type parameter `{}` of `{base}`: add a type annotation",
                            g.name.node
                        ),
                    );
                    return base.to_string();
                }
            }
        }
        match self.instantiate_adt(base, args, span) {
            Ty::Struct(k) => k,
            _ => base.to_string(),
        }
    }

    /// Which instance of the generic enum `base` a variant builds: the expected type, the
    /// enclosing `impl`, or what the variant's values say.
    fn generic_enum_instance(
        &mut self,
        base: &str,
        variant: &ast::Ident,
        args: &[ast::Expr],
        expected: Option<&Ty>,
        span: &Span,
    ) -> String {
        let of_base =
            |this: &Self, k: &str| this.instance_args.get(k).is_some_and(|(b, _)| b == base);
        if let Some(Ty::Enum(k)) = expected.map(|t| self.resolve(t))
            && of_base(self, &k)
        {
            return k;
        }
        if let Some(k) = self.impl_ty.clone()
            && of_base(self, &k)
        {
            return k;
        }
        let decl = self.generic_enums[base].clone();
        let param_types: HashMap<String, Ty> = decl
            .generics
            .iter()
            .map(|g| (g.name.node.clone(), Ty::Param(g.name.node.clone())))
            .collect();
        let saved = std::mem::replace(&mut self.type_params, param_types);
        let declared: Vec<Ty> = decl
            .variants
            .iter()
            .find(|v| v.name.node == variant.node)
            .map(|v| v.fields.iter().map(|t| self.resolve_type(t)).collect())
            .unwrap_or_default();
        self.type_params = saved;
        let mut bind = HashMap::new();
        let keep = self.diags.len();
        for (a, d) in args.iter().zip(&declared) {
            let e = self.expr(a, None);
            self.infer_params(d, &e.ty, &mut bind);
        }
        self.diags.truncate(keep);
        let mut targs = Vec::new();
        for g in &decl.generics {
            match bind.get(&g.name.node) {
                Some(t) => targs.push(t.clone()),
                None => {
                    self.err(
                        span.clone(),
                        format!(
                            "cannot infer the type parameter `{}` of `{base}`: add a type annotation",
                            g.name.node
                        ),
                    );
                    return base.to_string();
                }
            }
        }
        match self.instantiate_adt(base, targs, span) {
            Ty::Enum(k) => k,
            _ => base.to_string(),
        }
    }

    fn struct_lit(
        &mut self,
        path: &[ast::Ident],
        fields: &[(ast::Ident, ast::Expr)],
        expected: Option<&Ty>,
        span: &Span,
    ) -> hir::Expr {
        // `Shape.Rect(w: 1.0, h: 2.0)`: a variant with named values.
        if path.len() == 2 {
            let first = if path[0].node == "Self" {
                self.impl_ty.clone().unwrap_or_default()
            } else {
                path[0].node.clone()
            };
            let is_variant = self
                .enums
                .get(&first)
                .is_some_and(|vs| vs.iter().any(|(v, _)| *v == path[1].node))
                || self
                    .generic_enums
                    .get(&first)
                    .is_some_and(|en| en.variants.iter().any(|v| v.name.node == path[1].node));
            if is_variant {
                let Some(args) = self.order_variant_args(&first, &path[1], fields, span) else {
                    for (_, e) in fields {
                        let _ = self.expr(e, None);
                    }
                    return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
                };
                let callee = ast::Expr {
                    node: ast::ExprKind::Path(path.to_vec()),
                    span: span.clone(),
                };
                return self.call(&callee, &args, expected, span);
            }
        }
        let mut name = path
            .iter()
            .map(|i| i.node.as_str())
            .collect::<Vec<_>>()
            .join("::");
        if name == "Self"
            && let Some(t) = &self.impl_ty
        {
            name = t.clone();
        }
        if self.generic_structs.contains_key(&name) {
            name = self.generic_struct_instance(&name, fields, expected, span);
        }
        let Some(decl) = self.struct_fields.get(&name).cloned() else {
            if !self.generic_structs.contains_key(&name) {
                self.err(span.clone(), format!("cannot find type `{name}`"));
            }
            for (_, e) in fields {
                let _ = self.expr(e, None);
            }
            return Self::mk(hir::ExprKind::Error, Ty::Unknown, span);
        };
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (fname, value) in fields {
            let want = decl
                .iter()
                .find(|(n, _)| *n == fname.node)
                .map(|(_, t)| t.clone());
            if want.is_none() {
                self.err(
                    fname.span.clone(),
                    format!("type `{name}` has no field `{}`", fname.node),
                );
            }
            if !seen.insert(fname.node.clone()) {
                self.err(
                    fname.span.clone(),
                    format!("field `{}` given twice", fname.node),
                );
            }
            let v = self.expr(value, want.as_ref());
            let v = match &want {
                Some(w) => self.coerce(v, w),
                None => v,
            };
            out.push((fname.node.clone(), v));
        }
        let missing: Vec<_> = decl
            .iter()
            .filter(|(n, _)| !seen.contains(n))
            .map(|(n, _)| format!("`{n}`"))
            .collect();
        if !missing.is_empty() {
            self.err(
                span.clone(),
                format!("missing field(s) in `{name}`: {}", missing.join(", ")),
            );
        }
        Self::mk(
            hir::ExprKind::StructLit {
                name: name.clone(),
                fields: out,
            },
            Ty::Struct(name),
            span,
        )
    }
}
