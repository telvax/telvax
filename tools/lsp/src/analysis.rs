//! Whole-file analysis behind every language feature: parse, type check, and index the typed
//! HIR by source position.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use telvax_diagnostics::Diagnostic;
use telvax_hir::Ty;
use telvax_hir::hir::{self, ExprKind, ForIter, LocalId, Proj};

use crate::position::LineIndex;

mod completion;
pub(crate) mod names;

pub use names::Key;
use names::{Context, Edit, Names};
use telvax_modules::Files;

pub type Span = Range<usize>;

/// A function or a type that other files may use, and the file that declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRef {
    pub key: Key,
    pub home: String,
}

#[derive(Debug, Clone)]
enum Target {
    Local(LocalId),
    Fn(String),
    Global(String),
    Struct(String),
    Other,
}

#[derive(Debug, Clone)]
struct Node {
    span: Span,
    ty: Ty,
    target: Target,
}

#[derive(Debug, Clone)]
pub struct LocalInfo {
    pub name: String,
    pub ty: Ty,
    pub mutable: bool,
    pub span: Span,
}

/// Everything known about one version of a document.
#[derive(Debug)]
pub struct Snapshot {
    pub text: String,
    pub index: LineIndex,
    pub diagnostics: Vec<Diagnostic>,
    pub hir: Option<hir::Program>,
    /// All files of the program (this document is the first); resolves names across modules.
    pub map: telvax_modules::SourceMap,
    nodes: Vec<Node>,
    locals: HashMap<LocalId, LocalInfo>,
    /// Traits, aliases and type parameters, which the typed program no longer shows.
    decls: completion::Decls,
    /// Functions, globals, types, variants, fields and methods, read from the syntax tree.
    names: Names,
    /// Where the document lives, for analysing a changed copy of it the same way.
    path: Option<PathBuf>,
    /// The document's name in the source map: its path, or `<buffer>`.
    file: String,
    /// The program came from an earlier version of the text because the current one could not
    /// be analysed (typically mid-edit). Types are still useful for completion, positions are not.
    pub stale: bool,
}

/// A name and every span where it appears in the document (see [`Snapshot::occurrences`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrences {
    pub name: String,
    /// Where the name is declared.
    pub decl: Option<Span>,
    pub spans: Vec<Span>,
    /// Finding every use needs the types (a field or a method): without them some are missed.
    pub needs_types: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Global,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub name_span: Span,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Function,
    Struct,
    Variable,
    Field,
    Method,
    Module,
    Type,
    Enum,
    EnumMember,
    Trait,
}

#[derive(Debug, Clone)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    /// A sentence about the item, shown beside the list.
    pub doc: Option<String>,
    /// 0 is offered first: items that fit the type expected at the cursor, then names in scope,
    /// then declarations, then keywords.
    pub rank: u8,
    /// The type of the value the item gives, when it gives one.
    pub ty: Option<String>,
}

const KEYWORDS: &[&str] = &[
    "fn", "let", "var", "type", "enum", "trait", "extend", "import", "from", "return", "if",
    "elif", "else", "for", "in", "is", "while", "match", "case", "break", "continue", "pass",
    "and", "or", "not", "True", "False", "async", "await",
];
const PRIMS: &[&str] = &[
    "Int", "Float", "Text", "Bool", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32",
    "f64",
];

/// `Err` says why `name` cannot be the name of a variable, function, type or field.
pub fn check_name(name: &str) -> Result<(), String> {
    let valid = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid || KEYWORDS.contains(&name) || PRIMS.contains(&name) {
        return Err(format!("`{name}` is not a valid name"));
    }
    Ok(())
}

pub fn signature(f: &hir::FnDef) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            if p.name == "self" {
                "self".to_string()
            } else {
                format!("{}: {}", p.name, p.ty)
            }
        })
        .collect();
    let ret = if f.ret == Ty::Unit {
        String::new()
    } else {
        format!(" -> {}", f.ret)
    };
    format!(
        "fn {}({}){ret}",
        f.name.replace("::", "."),
        params.join(", ")
    )
}

impl Snapshot {
    /// Like [`Snapshot::analyze`], but when `text` cannot be analysed at all the types of `previous`
    /// are kept for completion.
    pub fn analyze_after(
        text: String,
        path: Option<&Path>,
        previous: Option<&Snapshot>,
    ) -> Snapshot {
        let mut snap = Snapshot::analyze_in(text, path);
        if snap.hir.is_none()
            && let Some(prev) = previous.filter(|p| p.hir.is_some())
        {
            // Positions of the earlier version are moved past the edit, so that definitions and
            // references keep working while the text does not parse.
            let edit = Edit::between(&prev.text, &snap.text);
            snap.hir = prev.hir.clone();
            snap.nodes = prev
                .nodes
                .iter()
                .filter_map(|n| edit.map(&n.span).map(|span| Node { span, ..n.clone() }))
                .collect();
            snap.locals = prev
                .locals
                .iter()
                .filter_map(|(id, l)| {
                    edit.map(&l.span)
                        .map(|span| (*id, LocalInfo { span, ..l.clone() }))
                })
                .collect();
            snap.names = prev.names.shifted(&edit);
            snap.decls = prev.decls.clone();
            snap.map = prev.map.clone();
            snap.stale = true;
        }
        snap
    }

    /// Analyses a standalone buffer (no other modules).
    pub fn analyze(text: String) -> Snapshot {
        Snapshot::analyze_in(text, None)
    }

    /// Analyses the document at `path`, loading the modules it imports and the dependencies of its
    /// project. Only diagnostics located in this document are kept.
    pub fn analyze_in(text: String, path: Option<&Path>) -> Snapshot {
        Snapshot::analyze_with(text, path, &telvax_modules::OsFiles)
    }

    /// Like [`Snapshot::analyze_in`], reading the other modules through `files` (a rename checks
    /// the files as they will be, before they are saved).
    pub fn analyze_with(text: String, path: Option<&Path>, files: &dyn Files) -> Snapshot {
        let index = LineIndex::new(&text);
        #[cfg(feature = "project")]
        let project = path.and_then(|p| telvax_pkg::Project::discover(p).ok().flatten());
        #[cfg(feature = "project")]
        let deps = project
            .as_ref()
            .and_then(|project| project.dependency_packages().ok())
            .unwrap_or_default();
        // The edition of the package, so a deprecated form is a warning or an error as `telvax check` says.
        #[cfg(feature = "project")]
        let edition = project
            .as_ref()
            .and_then(|p| p.manifest.package.edition.as_deref())
            .and_then(telvax_hir::Edition::from_name)
            .unwrap_or_default();
        #[cfg(not(feature = "project"))]
        let deps = Vec::new();
        #[cfg(not(feature = "project"))]
        let edition = telvax_hir::Edition::default();
        let name = path.map_or("<buffer>".to_string(), |p| p.display().to_string());
        let loaded =
            telvax_modules::load_in(files, &name, &text, path.and_then(Path::parent), &deps);
        let mut diagnostics = loaded.diagnostics.clone();
        let mut hir = None;
        if loaded.entry_parsed {
            let (h, d) = telvax_hir::lower_in(&loaded.program, edition);
            diagnostics.extend(d);
            hir = Some(h);
        }
        // Problems inside other modules are reported when those files are opened.
        diagnostics.retain(|d| d.span.start <= text.len());
        diagnostics.sort_by_key(|d| d.span.start);
        let mut snap = Snapshot {
            text,
            index,
            diagnostics,
            hir: None,
            map: loaded.map,
            nodes: Vec::new(),
            locals: HashMap::new(),
            decls: completion::Decls::of(&loaded.program),
            names: Names::default(),
            path: path.map(Path::to_path_buf),
            file: name.clone(),
            stale: false,
        };
        if let Some(h) = &hir {
            let mut c = Collector::default();
            c.program(h);
            snap.nodes = c.nodes;
            snap.locals = c.locals;
        }
        let types: Option<HashMap<(usize, usize), Ty>> = hir.as_ref().map(|_| {
            snap.nodes
                .iter()
                .map(|n| ((n.span.start, n.span.end), n.ty.clone()))
                .collect()
        });
        let (ast, errors) = telvax_parser::parse(&snap.text);
        let foreign = ast
            .as_ref()
            .map(|a| crate::workspace::imports(a, path, &deps))
            .unwrap_or_default();
        let demangle = |n: &str| snap.map.demangle(n);
        snap.names = Names::of(
            ast.as_ref(),
            errors.is_empty(),
            &Context {
                file: &name,
                types: types.as_ref(),
                foreign: &foreign,
                demangle: &demangle,
            },
        );
        snap.hir = hir;
        snap
    }

    /// Where `offset` is defined, possibly in another file: `(file name, byte range in that file)`.
    pub fn definition_location(&self, offset: usize) -> Option<(String, Span)> {
        if let Some((key, _)) = self.names.key_at(offset) {
            let home = self.names.home_of(key);
            if home != self.file {
                return crate::workspace::declaration(&home, key).map(|span| (home, span));
            }
            return self.names.decl(key).map(|span| (home, span.clone()));
        }
        let span = self.definition(offset)?;
        let (file, local) = self.map.locate(span.start)?;
        Some((file.name.clone(), local..local + span.len()))
    }

    /// The name of the document in the source map.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The function or type under `offset` that other files may use, with the span of its name.
    pub fn item_at(&self, offset: usize) -> Option<(ItemRef, Span)> {
        if let Some((key, span)) = self.names.key_at(offset) {
            // Globals cannot be imported: they are searched for in this document only.
            if matches!(key, Key::Global(_)) {
                return None;
            }
            let item = ItemRef {
                key: key.clone(),
                home: self.names.home_of(key),
            };
            return Some((item, span.clone()));
        }
        let node = self.node_at(offset)?;
        match &node.target {
            Target::Fn(m) => self.fn_item(m).map(|i| (i, node.span.clone())),
            _ => None,
        }
    }

    /// The function the program calls `mangled`, as a file and a name.
    fn fn_item(&self, mangled: &str) -> Option<ItemRef> {
        let own = Key::Fn(mangled.to_string());
        if self.names.decl(&own).is_some() {
            return Some(ItemRef {
                key: own,
                home: self.file.clone(),
            });
        }
        let f = self.program()?.fns.iter().find(|f| f.name == mangled)?;
        let (file, local) = self.map.locate(f.name_span.start)?;
        let name = file.text.get(local..local + f.name_span.len())?;
        // A method or a closure lowered to a function has a name that is not the written one.
        (mangled == name || mangled.ends_with(&format!("__{name}"))).then(|| ItemRef {
            key: Key::Fn(name.to_string()),
            home: file.name.clone(),
        })
    }

    /// Where `item` is declared and used in this document (empty when it is not mentioned).
    pub fn item_spans(&self, item: &ItemRef) -> (Option<Span>, Vec<Span>) {
        let (decl, mut spans) = if self.names.home_of(&item.key) == item.home {
            self.names.spans(&item.key)
        } else {
            (None, Vec::new())
        };
        if matches!(item.key, Key::Fn(_)) {
            let mut seen: HashMap<&str, bool> = HashMap::new();
            for n in &self.nodes {
                if let Target::Fn(m) = &n.target {
                    let is_it = *seen
                        .entry(m)
                        .or_insert_with(|| self.fn_item(m).as_ref() == Some(item));
                    if is_it {
                        spans.push(n.span.clone());
                    }
                }
            }
            spans.sort_by_key(|s| s.start);
            spans.dedup();
        }
        (decl, spans)
    }

    /// How many errors the document has.
    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }

    /// The document is syntactically valid and its types are known.
    pub fn is_exact(&self) -> (bool, bool) {
        (
            self.names.syntax_ok && !self.stale,
            self.names.typed && !self.stale,
        )
    }

    /// The smallest indexed expression containing `offset`.
    fn node_at(&self, offset: usize) -> Option<&Node> {
        self.nodes
            .iter()
            .filter(|n| n.span.start <= offset && offset <= n.span.end && n.span.start < n.span.end)
            .min_by_key(|n| n.span.end - n.span.start)
    }

    fn program(&self) -> Option<&hir::Program> {
        self.hir.as_ref()
    }

    pub fn hover(&self, offset: usize) -> Option<(String, Span)> {
        self.hover_raw(offset)
            .map(|(text, span)| (self.map.demangle(&text), span))
    }

    fn hover_raw(&self, offset: usize) -> Option<(String, Span)> {
        if self.stale {
            return None;
        }
        let prog = self.program()?;
        // Definition names first: they are not expressions.
        for f in &prog.fns {
            if f.name_span.contains(&offset) {
                return Some((
                    format!("```telvax\n{}\n```", signature(f)),
                    f.name_span.clone(),
                ));
            }
        }
        for s in &prog.structs {
            if s.name_span.contains(&offset) {
                let fields: Vec<String> = s
                    .fields
                    .iter()
                    .map(|(n, t)| format!("    {n}: {t}"))
                    .collect();
                return Some((
                    format!(
                        "```telvax\ntype {}:\n{}\n```",
                        s.name,
                        if fields.is_empty() {
                            "    pass".to_string()
                        } else {
                            fields.join("\n")
                        }
                    ),
                    s.name_span.clone(),
                ));
            }
        }
        if let Some(l) = self.locals.values().find(|l| l.span.contains(&offset)) {
            return Some((
                format!(
                    "```telvax\n{} {}: {}\n```",
                    if l.mutable { "var" } else { "let" },
                    l.name,
                    l.ty
                ),
                l.span.clone(),
            ));
        }
        let node = self.node_at(offset)?;
        let text = match &node.target {
            Target::Local(id) => {
                let l = self.locals.get(id)?;
                format!(
                    "```telvax\n{} {}: {}\n```",
                    if l.mutable { "var" } else { "let" },
                    l.name,
                    node.ty
                )
            }
            Target::Fn(name) => {
                let f = prog.fns.iter().find(|f| &f.name == name)?;
                format!("```telvax\n{}\n```", signature(f))
            }
            Target::Global(name) => format!("```telvax\nlet {name}: {}\n```", node.ty),
            Target::Struct(name) => format!("```telvax\ntype {name}\n```"),
            Target::Other => format!("```telvax\n{}\n```", node.ty),
        };
        Some((text, node.span.clone()))
    }

    pub fn definition(&self, offset: usize) -> Option<Span> {
        if let Some((key, _)) = self.names.key_at(offset) {
            return self.names.decl(key).cloned();
        }
        let prog = self.program()?;
        let node = self.node_at(offset)?;
        match &node.target {
            Target::Local(id) => self.locals.get(id).map(|l| l.span.clone()),
            Target::Fn(name) => self
                .names
                .decl(&Key::Fn(name.clone()))
                .cloned()
                .or_else(|| {
                    prog.fns
                        .iter()
                        .find(|f| &f.name == name)
                        .map(|f| f.name_span.clone())
                }),
            Target::Global(name) => self
                .names
                .decl(&Key::Global(name.clone()))
                .cloned()
                .or_else(|| {
                    prog.globals
                        .iter()
                        .find(|g| &g.name == name)
                        .map(|g| g.span.clone())
                }),
            Target::Struct(name) => prog
                .structs
                .iter()
                .find(|s| &s.name == name)
                .map(|s| s.name_span.clone()),
            Target::Other => None,
        }
    }

    /// The name under `offset` and every place it appears in this document (its declaration
    /// included). `Err` says why a symbol cannot be handled.
    pub fn occurrences(&self, offset: usize) -> Result<Occurrences, String> {
        let target = match self.names.key_at(offset) {
            Some((Key::Fn(n), _)) => Target::Fn(n.clone()),
            Some((Key::Global(n), _)) => Target::Global(n.clone()),
            Some((key, _)) => {
                let (decl, spans) = self.names.spans(key);
                return Ok(Occurrences {
                    name: key.name().to_string(),
                    decl,
                    spans,
                    needs_types: matches!(key, Key::Field(..) | Key::Method(..)),
                });
            }
            None => self
                .locals
                .iter()
                .find(|(_, l)| l.span.start <= offset && offset <= l.span.end)
                .map(|(id, _)| Target::Local(*id))
                .or_else(|| self.node_at(offset).map(|n| n.target.clone()))
                .ok_or("there is no symbol here")?,
        };
        let (name, decl) = match &target {
            Target::Local(id) => {
                let l = self.locals.get(id).ok_or("unknown variable")?;
                (l.name.clone(), l.span.clone())
            }
            Target::Fn(name) | Target::Global(name) => {
                let key = if matches!(target, Target::Fn(_)) {
                    Key::Fn(name.clone())
                } else {
                    Key::Global(name.clone())
                };
                let decl = self
                    .names
                    .decl(&key)
                    .ok_or_else(|| format!("`{name}` is defined in another file"))?;
                (name.clone(), decl.clone())
            }
            Target::Struct(_) | Target::Other => return Err("there is no symbol here".into()),
        };
        let in_document =
            |s: &Span| s.end <= self.text.len() && self.text.get(s.clone()) == Some(name.as_str());
        if !in_document(&decl) {
            return Err(format!("`{name}` is defined in another file"));
        }
        let same = |t: &Target| match (t, &target) {
            (Target::Local(a), Target::Local(b)) => a == b,
            (Target::Fn(a), Target::Fn(b)) | (Target::Global(a), Target::Global(b)) => a == b,
            _ => false,
        };
        let mut spans: Vec<Span> = std::iter::once(decl.clone())
            .chain(
                self.nodes
                    .iter()
                    .filter(|n| same(&n.target))
                    .map(|n| n.span.clone()),
            )
            .filter(in_document)
            .collect();
        spans.sort_by_key(|s| s.start);
        spans.dedup();
        Ok(Occurrences {
            name,
            decl: Some(decl),
            spans,
            needs_types: false,
        })
    }

    /// The edits that rename the symbol under `offset` to `new_name`. The result is checked: the
    /// renamed program must analyse with no more errors than before, which catches a new name
    /// that collides with another one in scope.
    pub fn rename(&self, offset: usize, new_name: &str) -> Result<Vec<(Span, String)>, String> {
        check_name(new_name)?;
        let found = self.occurrences(offset)?;
        if found.name == "main" {
            return Err("`main` is the entry point and cannot be renamed".into());
        }
        if self.stale || !self.names.syntax_ok {
            return Err("fix the syntax errors in this file before renaming".into());
        }
        if found.needs_types && !self.names.typed {
            return Err(
                "the types of this file are not known; fix its errors before renaming".into(),
            );
        }
        let mut renamed = self.text.clone();
        for span in found.spans.iter().rev() {
            renamed.replace_range(span.clone(), new_name);
        }
        let errors = |s: &Snapshot| s.diagnostics.iter().filter(|d| d.is_error()).count();
        let after = Snapshot::analyze_in(renamed, self.path.as_deref());
        if errors(&after) > errors(self) {
            return Err(format!("`{new_name}` collides with another name in scope"));
        }
        Ok(found
            .spans
            .into_iter()
            .map(|s| (s, new_name.to_string()))
            .collect())
    }

    pub fn symbols(&self) -> Vec<Symbol> {
        let Some(prog) = self.program().filter(|_| !self.stale) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for s in &prog.structs {
            out.push(Symbol {
                name: s.name.clone(),
                kind: SymbolKind::Struct,
                span: s.span.clone(),
                name_span: s.name_span.clone(),
                detail: format!("{} field(s)", s.fields.len()),
            });
        }
        for f in &prog.fns {
            out.push(Symbol {
                name: f.name.clone(),
                kind: SymbolKind::Function,
                span: f.span.clone(),
                name_span: f.name_span.clone(),
                detail: signature(f),
            });
        }
        for g in &prog.globals {
            out.push(Symbol {
                name: g.name.clone(),
                kind: SymbolKind::Global,
                span: g.span.clone(),
                name_span: g.span.clone(),
                detail: g.ty.to_string(),
            });
        }
        out.retain(|s| s.span.end <= self.text.len());
        out.sort_by_key(|s| s.span.start);
        out
    }

    /// Inferred types for `let` bindings and `for` variables written without an annotation.
    pub fn inlay_hints(&self) -> Vec<(usize, String)> {
        let mut hints = Vec::new();
        if self.stale {
            return hints;
        }
        for l in self.locals.values() {
            if l.ty == Ty::Unknown || l.span.is_empty() {
                continue;
            }
            let after = self.text[l.span.end.min(self.text.len())..].trim_start();
            // Annotated bindings and function parameters (`name: T`) already show their type.
            if after.starts_with(':') {
                continue;
            }
            // Only names a statement declares (`let x = ...`, `let a, b = ...`, `for x in ...`).
            let line_start = self.text[..l.span.start].rfind('\n').map_or(0, |i| i + 1);
            let prefix = self.text[line_start..l.span.start].trim();
            let prefix = ["let", "var", "for"]
                .iter()
                .find_map(|kw| prefix.strip_prefix(kw))
                .unwrap_or(prefix);
            let declares = prefix
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | ',' | ' '));
            let binds =
                (after.starts_with('=') && !after.starts_with("==") && !after.starts_with("=>"))
                    || after.starts_with(',')
                    || after.starts_with("in ");
            if declares && binds {
                hints.push((l.span.end, format!(": {}", l.ty)));
            }
        }
        hints.sort();
        hints
    }
}

// ---- HIR walk ------------------------------------------------------------------------------

#[derive(Default)]
struct Collector {
    nodes: Vec<Node>,
    locals: HashMap<LocalId, LocalInfo>,
}

impl Collector {
    fn local(&mut self, l: &hir::Local) {
        self.locals.insert(
            l.id,
            LocalInfo {
                name: l.name.clone(),
                ty: l.ty.clone(),
                mutable: l.mutable,
                span: l.span.clone(),
            },
        );
    }

    fn program(&mut self, p: &hir::Program) {
        for g in &p.globals {
            self.expr(&g.init);
        }
        for f in &p.fns {
            for param in &f.params {
                self.local(param);
            }
            self.block(&f.body);
        }
    }

    fn block(&mut self, b: &hir::Block) {
        for s in &b.stmts {
            match s {
                hir::Stmt::Let { local, init } => {
                    self.local(local);
                    self.expr(init);
                }
                hir::Stmt::Expr(e) => self.expr(e),
            }
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
    }

    fn expr(&mut self, e: &hir::Expr) {
        let target = match &e.kind {
            ExprKind::Local(id) => Target::Local(*id),
            ExprKind::Fn(n) => Target::Fn(n.clone()),
            ExprKind::Global(n) => Target::Global(n.clone()),
            ExprKind::StructLit { name, .. } => Target::Struct(name.clone()),
            _ => Target::Other,
        };
        self.nodes.push(Node {
            span: e.span.clone(),
            ty: e.ty.clone(),
            target,
        });
        match &e.kind {
            ExprKind::Lit(_)
            | ExprKind::Local(_)
            | ExprKind::Global(_)
            | ExprKind::Fn(_)
            | ExprKind::Extern(_)
            | ExprKind::Error => {}
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                args.iter().for_each(|a| self.expr(a));
            }
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Unary { expr, .. } | ExprKind::Try(expr) | ExprKind::Await(expr) => {
                self.expr(expr)
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Closure { params, body } => {
                params.iter().for_each(|p| self.local(p));
                self.expr(body);
            }
            ExprKind::StructLit { fields, .. } => fields.iter().for_each(|(_, v)| self.expr(v)),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond);
                self.block(then);
                if let Some(o) = otherwise {
                    self.expr(o);
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v);
                }
            }
            ExprKind::Break | ExprKind::Continue => {}
            ExprKind::FString(parts) => {
                for p in parts {
                    for x in p.exprs() {
                        self.expr(x);
                    }
                }
            }
            ExprKind::Assign { place, value } => {
                self.nodes.push(Node {
                    span: place.span.clone(),
                    ty: place.ty.clone(),
                    target: Target::Local(place.local),
                });
                for pr in &place.projs {
                    if let Proj::Index(i) = pr {
                        self.expr(i);
                    }
                }
                self.expr(value);
            }
            ExprKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            ExprKind::For { var, iter, body } => {
                self.local(var);
                match iter {
                    ForIter::Range { start, end, .. } => {
                        self.expr(start);
                        self.expr(end);
                    }
                    ForIter::Vec(v) => self.expr(v),
                }
                self.block(body);
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Array(items) => items.iter().for_each(|i| self.expr(i)),
            ExprKind::EnumCtor { args, .. }
            | ExprKind::Tuple(args)
            | ExprKind::DynCall { args, .. } => {
                args.iter().for_each(|a| self.expr(a));
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.pattern(&arm.pat);
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    self.expr(&arm.body);
                }
            }
        }
    }

    fn pattern(&mut self, p: &hir::Pat) {
        match p {
            hir::Pat::Bind(l) => self.local(l),
            hir::Pat::Variant { args, .. } | hir::Pat::Tuple(args) | hir::Pat::Or(args) => {
                args.iter().for_each(|a| self.pattern(a));
            }
            hir::Pat::List {
                before,
                rest,
                after,
            } => {
                before
                    .iter()
                    .chain(rest.as_deref())
                    .chain(after)
                    .for_each(|a| self.pattern(a));
            }
            hir::Pat::Struct { fields, .. } => fields.iter().for_each(|(_, a)| self.pattern(a)),
            hir::Pat::Wild | hir::Pat::Lit(_) | hir::Pat::NegInt(_) => {}
        }
    }
}

#[cfg(test)]
mod tests;
