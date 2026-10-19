//! Completion: what to offer at the cursor, from the last typed program and the text before it.
//!
//! The text being typed rarely parses, so the types come from the last analysis that worked and
//! the position is read from the text itself: the word being typed, the dot before it, the
//! brackets still open and the string or comment it may be inside.

use telvax_ast as ast;
use telvax_hir::stdlib::{self, ItemKind};
use telvax_hir::{Prim, Ty};

use super::{Completion, CompletionKind, KEYWORDS, PRIMS, Snapshot, Span, signature};

/// Things the typed program no longer shows, read from the syntax tree.
#[derive(Debug, Default, Clone)]
pub struct Decls {
    traits: Vec<TraitDecl>,
    aliases: Vec<String>,
    /// `(type, trait)`: the type implements the trait.
    impls: Vec<(String, String)>,
    /// The type parameters in scope over a span (`T` in `fn first[T](..)`).
    generics: Vec<(Span, Vec<String>)>,
}

#[derive(Debug, Clone)]
struct TraitDecl {
    name: String,
    /// `(name, how it reads, required)`
    methods: Vec<(String, String, bool)>,
}

impl Decls {
    pub fn of(program: &ast::Program) -> Decls {
        let mut d = Decls::default();
        let mut params = |span: &Span, generics: &[ast::GenericParam]| {
            if !generics.is_empty() {
                let names = generics.iter().map(|g| g.name.node.clone()).collect();
                d.generics.push((span.clone(), names));
            }
        };
        let mut found = Vec::new();
        for item in &program.items {
            match item {
                ast::Item::Struct(s) => params(&s.span, &s.generics),
                ast::Item::Enum(e) => params(&e.span, &e.generics),
                ast::Item::Alias(a) => params(&a.name.span, &a.generics),
                ast::Item::Fn(f) => params(&f.span, &f.generics),
                ast::Item::Impl(i) => {
                    params(&i.span, &i.generics);
                    for f in &i.fns {
                        params(&f.span, &f.generics);
                    }
                    if let Some(t) = &i.trait_name {
                        found.push((i.ty.node.clone(), t.node.clone()));
                    }
                }
                _ => {}
            }
        }
        d.impls = found;
        for item in &program.items {
            match item {
                ast::Item::Trait(t) => d.traits.push(TraitDecl {
                    name: t.name.node.clone(),
                    methods: t
                        .fns
                        .iter()
                        .map(|f| {
                            let args: Vec<&str> =
                                f.params.iter().map(|p| p.name.node.as_str()).collect();
                            (
                                f.name.node.clone(),
                                format!("fn {}({})", f.name.node, args.join(", ")),
                                f.required,
                            )
                        })
                        .collect(),
                }),
                ast::Item::Alias(a) => d.aliases.push(a.name.node.clone()),
                _ => {}
            }
        }
        d
    }

    fn trait_named(&self, name: &str) -> Option<&TraitDecl> {
        self.traits.iter().find(|t| t.name == name)
    }
}

const GENERIC_TYPES: &[(&str, &str)] = &[
    ("List", "List[T]: a growable list"),
    ("Map", "Map[K, V]: keys to values"),
    ("Tensor", "Tensor[f32, 2, 3]: numbers with a fixed shape"),
    ("Table", "Table[Row]: columns, one per field of Row"),
    ("Result", "Result[T, E]: a value or an error"),
    ("Task", "Task[T]: what an `async fn` returns"),
];

fn keyword_doc(k: &str) -> &'static str {
    match k {
        "fn" => "declares a function",
        "let" => "binds a name that does not change",
        "var" => "binds a name that can change",
        "type" => "declares a type with fields and methods, or another name for a type",
        "enum" => "declares a type with a fixed set of variants",
        "trait" => "declares methods that types can promise to have",
        "extend" => "adds methods to a type declared elsewhere",
        "import" => "brings in a module or a name from one",
        "from" => "imports names from a module; marks an enum variant that `?` converts into",
        "return" => "leaves the function with a value",
        "if" => "runs a block when the condition is true",
        "elif" => "another condition after an `if`",
        "else" => "runs when no condition above was true",
        "for" => "repeats a block for each item",
        "in" => "the items a `for` goes through; membership test",
        "is" => "compares with a value or pattern",
        "while" => "repeats a block while the condition is true",
        "match" => "chooses a block by the shape of a value",
        "case" => "one arm of a `match`",
        "break" => "leaves the loop",
        "continue" => "goes to the next turn of the loop",
        "pass" => "does nothing; fills an empty block",
        "and" => "true when both sides are true",
        "or" => "true when either side is true",
        "not" => "true when the value is false",
        "True" | "False" => "a Bool value",
        "async" => "marks a function that returns a `Task`",
        "await" => "runs a `Task` and gives its value",
        _ => "",
    }
}

fn prim_doc(p: &str) -> &'static str {
    match p {
        "Int" => "whole number (i32 unless the use says otherwise)",
        "Float" => "decimal number (f64 unless the use says otherwise)",
        "Text" => "a string",
        "Bool" => "True or False",
        _ => "number with a fixed size",
    }
}

fn item(label: &str, kind: CompletionKind, detail: Option<String>) -> Completion {
    use CompletionKind as K;
    let rank = match kind {
        K::Variable | K::Field | K::Method => 1,
        K::Keyword => 3,
        _ => 2,
    };
    Completion {
        label: label.to_string(),
        kind,
        detail,
        doc: None,
        rank,
        ty: None,
    }
}

impl Completion {
    fn doc(mut self, text: &str) -> Completion {
        if !text.is_empty() {
            self.doc = Some(text.to_string());
        }
        self
    }

    fn of_type(mut self, ty: impl ToString) -> Completion {
        self.ty = Some(ty.to_string());
        self
    }
}

/// The brackets still open in `text`, outermost first, as `(bracket, byte offset)`. Strings and
/// comments are skipped.
fn open_brackets(text: &str) -> Vec<(char, usize)> {
    let mut stack = Vec::new();
    let mut in_str = false;
    let mut in_comment = false;
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        if in_comment {
            in_comment = c != '\n';
        } else if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' | '\n' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                '#' => in_comment = true,
                '"' => in_str = true,
                '(' | '[' | '{' => stack.push((c, i)),
                ')' | ']' | '}' => {
                    stack.pop();
                }
                _ => {}
            }
        }
    }
    stack
}

/// Where the cursor is, within its line.
enum LineState {
    Code,
    Comment,
    /// Inside a string; `start` is the byte offset of its opening quote in the line.
    Str {
        start: usize,
    },
}

fn line_state(line: &str) -> LineState {
    let mut in_str: Option<(usize, bool, u32)> = None; // (start, is f-string, open `{`)
    let mut prev = ' ';
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match &mut in_str {
            None => match c {
                '#' => return LineState::Comment,
                '"' => in_str = Some((i, prev == 'f' || prev == 'F', 0)),
                _ => {}
            },
            Some((_, is_f, braces)) => match c {
                '\\' => {
                    chars.next();
                }
                '"' if *braces == 0 => in_str = None,
                '{' if *is_f => {
                    if chars.peek().is_some_and(|&(_, n)| n == '{') && *braces == 0 {
                        chars.next();
                    } else {
                        *braces += 1;
                    }
                }
                '}' if *is_f && *braces > 0 => *braces -= 1,
                _ => {}
            },
        }
        prev = c;
    }
    match in_str {
        Some((_, true, braces)) if braces > 0 => LineState::Code,
        Some((start, ..)) => LineState::Str { start },
        None => LineState::Code,
    }
}

/// The text of the line that opened the block `line_start` is in (the nearest earlier line
/// that is indented less).
fn block_header(text: &str, line_start: usize) -> Option<&str> {
    let indent = |l: &str| l.len() - l.trim_start().len();
    let own = text[line_start..].lines().next().unwrap_or("");
    let own_indent = indent(own);
    text[..line_start]
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .find(|l| indent(l) < own_indent)
        .map(str::trim)
}

fn line_start_of(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// The identifier that ends `text`.
fn ident_tail(text: &str) -> &str {
    let start = text
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |i| {
            i + text[i..].chars().next().map_or(1, char::len_utf8)
        });
    &text[start..]
}

/// The offset of the `(` or `[` that closes at `close` (the byte of `)` or `]`).
fn matching_open(text: &str, close: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in text[..=close].char_indices().rev() {
        match c {
            ')' | ']' | '}' => depth += 1,
            '(' | '[' | '{' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The commas at the top level of `args` (what follows an opening parenthesis).
fn top_level_commas(args: &str) -> Vec<usize> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    let mut in_str = false;
    let mut chars = args.char_indices();
    while let Some((i, c)) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => out.push(i),
            _ => {}
        }
    }
    out
}

fn element_of(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Vec(t) => Some((**t).clone()),
        Ty::Table(s) => Some(Ty::Struct(s.clone())),
        Ty::Tensor { elem, shape } if shape.len() == 1 => Some((**elem).clone()),
        Ty::Tensor { elem, shape } => Some(Ty::Tensor {
            elem: elem.clone(),
            shape: shape[1..].to_vec(),
        }),
        _ => None,
    }
}

/// What a method of a built-in type gives, for the ones a chain is likely to continue from.
fn builtin_result(ty: &Ty, name: &str) -> Option<Ty> {
    let text = || Ty::Prim(Prim::Str);
    let option = |t: Ty| Ty::Option(Box::new(t));
    Some(match (ty, name) {
        (Ty::Prim(p), "trim" | "to_lowercase" | "to_uppercase" | "replace" | "slice")
            if p.is_text() =>
        {
            text()
        }
        (Ty::Prim(p), "split" | "chars" | "lines") if p.is_text() => Ty::Vec(Box::new(text())),
        (Ty::Prim(p), "find") if p.is_text() => option(Ty::Prim(Prim::U64)),
        (Ty::Prim(p), "to_int") if p.is_text() => option(Ty::Prim(Prim::I64)),
        (Ty::Prim(p), "to_float") if p.is_text() => option(Ty::Prim(Prim::F64)),
        (Ty::Vec(e), "first" | "last" | "pop" | "get") => option((**e).clone()),
        (Ty::Vec(_), "filter" | "take" | "skip" | "slice") => ty.clone(),
        (Ty::Option(i), "unwrap" | "expect" | "unwrap_or" | "unwrap_or_else") => (**i).clone(),
        (Ty::Option(_), "filter") => ty.clone(),
        (Ty::Result(ok, _), "unwrap" | "expect" | "unwrap_or" | "unwrap_or_else") => (**ok).clone(),
        (Ty::Result(ok, _), "ok") => option((**ok).clone()),
        (Ty::Result(_, err), "err") => option((**err).clone()),
        (Ty::Map(_, v), "get" | "remove") => option((**v).clone()),
        (Ty::Map(k, _), "keys") => Ty::Vec(k.clone()),
        (Ty::Map(_, v), "values") => Ty::Vec(v.clone()),
        (Ty::Map(k, v), "entries") => {
            Ty::Vec(Box::new(Ty::Tuple(vec![(**k).clone(), (**v).clone()])))
        }
        _ => return None,
    })
}

/// The type of parameter `param` of the closure passed as argument `arg` to `method`, called on
/// a value of type `recv`.
fn closure_param(recv: &Ty, method: &str, arg: usize, param: usize) -> Option<Ty> {
    const EACH: &[&str] = &[
        "map",
        "filter",
        "any",
        "all",
        "find",
        "for_each",
        "count",
        "flat_map",
        "sort_by",
        "min_by",
        "max_by",
        "group_by",
        "partition",
        "take_while",
        "skip_while",
    ];
    match recv {
        Ty::Vec(_) | Ty::Table(_) | Ty::Tensor { .. } => {
            let elem = element_of(recv)?;
            if method == "fold" {
                return (arg == 1 && param == 1).then_some(elem);
            }
            (EACH.contains(&method) && param == 0).then_some(elem)
        }
        Ty::Option(inner) => (matches!(method, "map" | "and_then" | "filter") && param == 0)
            .then(|| (**inner).clone()),
        Ty::Result(ok, err) => match method {
            "map" | "and_then" if param == 0 => Some((**ok).clone()),
            "map_err" | "unwrap_or_else" if param == 0 => Some((**err).clone()),
            _ => None,
        },
        _ => None,
    }
}

impl Snapshot {
    pub fn completions(&self, offset: usize) -> Vec<Completion> {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let before = &self.text[..offset];
        let line_start = line_start_of(before, offset);
        let state = line_state(&before[line_start..]);
        let word_start = before
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(0, |i| {
                i + before[i..].chars().next().map_or(1, char::len_utf8)
            });
        let prefix = &before[word_start..];
        let head = &before[..word_start];

        let mut items = match state {
            LineState::Comment => return Vec::new(),
            LineState::Str { start } => self.string_completions(line_start + start, offset),
            LineState::Code => {
                if let Some(receiver) = head.strip_suffix('.') {
                    self.dot_completions(receiver, offset)
                } else if head.ends_with('@') {
                    vec![
                        item(
                            "endpoint",
                            CompletionKind::Function,
                            Some("HTTP endpoint attribute".into()),
                        )
                        .doc("serves the function over HTTP"),
                        item(
                            "middleware",
                            CompletionKind::Function,
                            Some("middleware attribute".into()),
                        )
                        .doc("wraps every request; list it in `middleware:` of `serve:`"),
                    ]
                } else if self.in_type_position(head) {
                    self.type_completions(offset)
                } else {
                    let mut items = self.general_completions(offset);
                    if head.trim_end_matches([' ', '\t']).ends_with("case") {
                        // The first word of an arm: a variant, or an enum to take one from.
                        for (name, doc) in [
                            ("Some", "a value that is there"),
                            ("None", "no value"),
                            ("Ok", "a success"),
                            ("Err", "an error"),
                        ] {
                            let mut c = item(name, CompletionKind::EnumMember, None).doc(doc);
                            c.rank = 0;
                            items.push(c);
                        }
                    }
                    items
                }
            }
        };
        if let Some(expected) = self.expected_type(head, offset) {
            for c in &mut items {
                if c.ty.as_deref() == Some(expected.as_str()) {
                    c.rank = 0;
                }
            }
        }
        items.retain(|c| c.label.starts_with(prefix));
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label && a.kind == b.kind);
        items
    }

    fn dot_completions(&self, receiver: &str, offset: usize) -> Vec<Completion> {
        if self.starts_a_path(receiver) {
            let mut out = self.path_completions(receiver);
            out.extend(self.type_path_completions(receiver));
            if !out.is_empty() {
                return out;
            }
        }
        self.member_completions(receiver, offset)
    }

    // ---- names -----------------------------------------------------------------------------

    /// A name as the user wrote it, without the module prefix the compiler adds.
    fn shown(&self, name: &str) -> String {
        match self.map.display_names.get(name) {
            Some(shown) => shown.rsplit('.').next().unwrap_or(shown).to_string(),
            None => name.to_string(),
        }
    }

    fn is_own(&self, span: &Span) -> bool {
        span.end <= self.text.len()
    }

    /// The compiler's name for the struct or enum the user calls `shown`.
    fn resolve_type_name(&self, shown: &str) -> Option<String> {
        let prog = self.program()?;
        let declared = |n: &str| {
            prog.structs.iter().any(|s| s.name == n) || prog.enums.iter().any(|e| e.name == n)
        };
        if declared(shown) {
            return Some(shown.to_string());
        }
        self.map
            .display_names
            .iter()
            .find(|(mangled, _)| declared(mangled) && self.shown(mangled) == shown)
            .map(|(mangled, _)| mangled.clone())
    }

    fn is_type_name(&self, name: &str) -> bool {
        GENERIC_TYPES.iter().any(|(t, _)| *t == name)
            || PRIMS.contains(&name)
            || self.resolve_type_name(name).is_some()
            || self.decls.aliases.iter().any(|a| a == name)
    }

    fn declared_types(&self, offset: usize) -> Vec<Completion> {
        let mut out = Vec::new();
        let Some(prog) = self.program() else {
            return out;
        };
        let struct_item =
            |name: &str, detail: String| item(name, CompletionKind::Struct, Some(detail));
        for s in &prog.structs {
            if self.is_own(&s.name_span) {
                let fields: Vec<String> =
                    s.fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
                out.push(struct_item(
                    &s.name,
                    format!("type, fields {}", fields.join(", ")),
                ));
            } else if let Some(shown) = self.map.display_names.get(&s.name) {
                out.push(struct_item(&self.shown(&s.name), shown.clone()));
            }
        }
        for e in &prog.enums {
            let variants: Vec<&str> = e.variants.iter().map(|(v, _)| v.as_str()).collect();
            let detail = format!("enum {}", variants.join(" | "));
            if self.is_own(&e.name_span) {
                out.push(item(&e.name, CompletionKind::Enum, Some(detail)));
            } else if self.map.display_names.contains_key(&e.name) {
                out.push(item(
                    &self.shown(&e.name),
                    CompletionKind::Enum,
                    Some(detail),
                ));
            }
        }
        for t in &self.decls.traits {
            out.push(item(
                &self.shown(&t.name),
                CompletionKind::Trait,
                Some("trait".into()),
            ));
        }
        for a in &self.decls.aliases {
            out.push(item(
                &self.shown(a),
                CompletionKind::Type,
                Some("type alias".into()),
            ));
        }
        for imp in &prog.imports {
            if let Some(last) = imp
                .last()
                .filter(|l| l.starts_with(|c: char| c.is_uppercase()))
            {
                out.push(item(last, CompletionKind::Type, Some(imp.join("."))));
            }
        }
        for (_, names) in self
            .decls
            .generics
            .iter()
            .filter(|(s, _)| s.start <= offset && offset <= s.end)
        {
            for n in names {
                out.push(item(n, CompletionKind::Type, Some("type parameter".into())));
            }
        }
        out.retain(|c| !c.label.contains("__"));
        out
    }

    fn general_completions(&self, offset: usize) -> Vec<Completion> {
        let mut out: Vec<Completion> = KEYWORDS
            .iter()
            .map(|k| item(k, CompletionKind::Keyword, Some("keyword".into())).doc(keyword_doc(k)))
            .collect();
        out.extend(
            PRIMS.iter().map(|p| {
                item(p, CompletionKind::Type, Some("built-in type".into())).doc(prim_doc(p))
            }),
        );
        out.extend(GENERIC_TYPES.iter().map(|(t, doc)| {
            let (head, rest) = doc.split_once(": ").unwrap_or((doc, ""));
            item(t, CompletionKind::Type, Some(head.to_string())).doc(rest)
        }));
        out.push(
            item(
                "std",
                CompletionKind::Module,
                Some("standard library".into()),
            )
            .doc("modules and functions that come with the language"),
        );
        let Some(prog) = self.program() else {
            return out;
        };
        out.extend(
            self.declared_types(offset)
                .into_iter()
                .filter(|c| matches!(c.kind, CompletionKind::Struct | CompletionKind::Enum | CompletionKind::Type if c.detail.as_deref() != Some("type parameter") && c.detail.as_deref() != Some("type alias"))),
        );
        for f in prog
            .fns
            .iter()
            .filter(|f| self.is_own(&f.name_span) && !f.name.contains("::"))
        {
            out.push(item(&f.name, CompletionKind::Function, Some(signature(f))).of_type(&f.ret));
        }
        for g in prog.globals.iter().filter(|g| self.is_own(&g.span)) {
            out.push(
                item(&g.name, CompletionKind::Variable, Some(g.ty.to_string())).of_type(&g.ty),
            );
        }
        for imp in &prog.imports {
            if let Some(last) = imp.last() {
                out.push(item(last, CompletionKind::Module, Some(imp.join("."))));
            }
        }
        // Items imported from other modules, under the names the user wrote.
        for (mangled, shown) in &self.map.display_names {
            let Some(name) = shown.rsplit('.').next() else {
                continue;
            };
            if let Some(f) = prog.fns.iter().find(|f| &f.name == mangled) {
                out.push(
                    item(
                        name,
                        CompletionKind::Function,
                        Some(self.map.demangle(&signature(f))),
                    )
                    .of_type(&f.ret),
                );
            }
        }
        out.extend(self.visible_locals(offset).into_iter().map(|l| {
            item(&l.name, CompletionKind::Variable, Some(l.ty.to_string())).of_type(&l.ty)
        }));
        out
    }

    /// Types, traits and type parameters: what can follow `x:` or `->`.
    fn type_completions(&self, offset: usize) -> Vec<Completion> {
        let mut out: Vec<Completion> = PRIMS
            .iter()
            .map(|p| item(p, CompletionKind::Type, Some("built-in type".into())).doc(prim_doc(p)))
            .collect();
        out.extend(GENERIC_TYPES.iter().map(|(t, doc)| {
            let (head, rest) = doc.split_once(": ").unwrap_or((doc, ""));
            item(t, CompletionKind::Type, Some(head.to_string())).doc(rest)
        }));
        out.push(item(
            "std",
            CompletionKind::Module,
            Some("standard library".into()),
        ));
        out.extend(self.declared_types(offset));
        for c in &mut out {
            c.rank = 0;
        }
        out
    }

    /// Whether the cursor is where a type is written: after `->`, after the `:` of a parameter,
    /// field or `let`, in the brackets of `List[..]`, after `type Id =` or `extend`.
    fn in_type_position(&self, head: &str) -> bool {
        let trimmed = head.trim_end_matches([' ', '\t']);
        if trimmed.ends_with("->") {
            return true;
        }
        let line_start = line_start_of(trimmed, trimmed.len());
        let line = trimmed[line_start..].trim_start();
        let after_colon = trimmed.ends_with(':');
        match open_brackets(head).last() {
            Some(&('(', pos)) => {
                let open_line = line_start_of(head, pos);
                let opener = head[open_line..pos].trim_start();
                return after_colon
                    || opener.starts_with("type ")
                    || opener.starts_with("extend ")
                    || block_header(head, open_line).is_some_and(|h| h.starts_with("enum "));
            }
            Some(&('[', pos)) => {
                return after_colon || self.is_type_name(ident_tail(&head[..pos]));
            }
            Some(_) => return false,
            None => {}
        }
        if (line.starts_with("let ") || line.starts_with("var "))
            && after_colon
            && !line.contains('=')
        {
            return true;
        }
        if line.starts_with("type ") && trimmed.ends_with('=') {
            return true;
        }
        if line.starts_with("extend ") {
            return !line.contains(':');
        }
        // A field: `name:` inside a `type` block.
        let before_colon = line.trim_end_matches(':');
        after_colon
            && !before_colon.is_empty()
            && before_colon
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_')
            && block_header(head, line_start).is_some_and(|h| h.starts_with("type "))
    }

    // ---- after a dot -----------------------------------------------------------------------

    /// The run of dotted names that ends `text`: `std.text` in `x = std.text`.
    fn dotted_tail<'t>(&self, text: &'t str) -> &'t str {
        let start = text
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
            .map_or(0, |i| {
                i + text[i..].chars().next().map_or(1, char::len_utf8)
            });
        &text[start..]
    }

    /// Whether `receiver` (the text before a `.`) names a module or type rather than a value:
    /// its first name is `std`, an imported name or capitalized.
    fn starts_a_path(&self, receiver: &str) -> bool {
        let run = self.dotted_tail(receiver);
        let Some(first) = run.split('.').next().filter(|s| !s.is_empty()) else {
            return false;
        };
        first == "std"
            || first.starts_with(|c: char| c.is_uppercase())
            || self.program().is_some_and(|p| {
                p.imports
                    .iter()
                    .any(|i| i.last().is_some_and(|l| l == first))
            })
    }

    fn path_completions(&self, path_text: &str) -> Vec<Completion> {
        let mut segments: Vec<String> = self
            .dotted_tail(path_text)
            .split('.')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if segments.is_empty() {
            return Vec::new();
        }
        // An imported name stands for its full path.
        if let Some(prog) = self.program()
            && let Some(full) = prog.imports.iter().find(|p| p.last() == segments.first())
        {
            let mut resolved = full.clone();
            resolved.extend(segments.drain(1..));
            segments = resolved;
        }
        if segments[0] != "std" {
            return Vec::new();
        }
        let parent = segments.join("::");
        stdlib::children(&parent)
            .into_iter()
            .map(|(name, kind)| {
                let (kind, what) = match kind {
                    ItemKind::Module => (CompletionKind::Module, "module"),
                    ItemKind::Function => (CompletionKind::Function, "function"),
                    ItemKind::Type => (CompletionKind::Type, "type"),
                };
                let detail = format!("std {what} {}.{name}", segments.join("."));
                item(&name, kind, Some(detail))
            })
            .collect()
    }

    /// `Shape.`: the variants of an enum and the functions a type declares without `self`.
    fn type_path_completions(&self, receiver: &str) -> Vec<Completion> {
        let run = self.dotted_tail(receiver);
        if run.contains('.') {
            return Vec::new();
        }
        let (Some(prog), Some(name)) = (self.program(), self.resolve_type_name(run)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(e) = prog.enums.iter().find(|e| e.name == name) {
            for (variant, payload) in &e.variants {
                let detail = if payload.is_empty() {
                    format!("{}.{variant}", run)
                } else {
                    let parts: Vec<String> = payload.iter().map(ToString::to_string).collect();
                    format!("{}.{variant}({})", run, parts.join(", "))
                };
                let mut c = item(variant, CompletionKind::EnumMember, Some(detail));
                c.rank = 0;
                out.push(c);
            }
        }
        let prefix = format!("{name}::");
        for f in &prog.fns {
            let Some(rest) = f.name.strip_prefix(&prefix) else {
                continue;
            };
            if !rest.contains("::") && f.params.first().is_none_or(|p| p.name != "self") {
                out.push(
                    item(
                        rest,
                        CompletionKind::Function,
                        Some(self.map.demangle(&signature(f))),
                    )
                    .of_type(&f.ret),
                );
            }
        }
        out
    }

    fn member_completions(&self, receiver: &str, offset: usize) -> Vec<Completion> {
        match self.type_of_chain(receiver, offset) {
            Some(ty) => self.members_of(&ty),
            None => Vec::new(),
        }
    }

    fn members_of(&self, ty: &Ty) -> Vec<Completion> {
        let mut out: Vec<Completion> = stdlib::builtin_methods(ty)
            .iter()
            .map(|(n, sig)| item(n, CompletionKind::Method, Some(format!("{n}{sig}"))))
            .collect();
        let prog = self.program();
        let columns = |name: &str, what: &str| -> Vec<Completion> {
            prog.and_then(|p| p.structs.iter().find(|s| s.name == name))
                .map(|s| {
                    s.fields
                        .iter()
                        .map(|(n, t)| {
                            item(n, CompletionKind::Field, Some(format!("{what}{t}"))).of_type(t)
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        match ty {
            Ty::Struct(name) => {
                out.extend(columns(name, ""));
                out.extend(self.user_methods(name));
            }
            Ty::Enum(name) => out.extend(self.user_methods(name)),
            Ty::Table(name) => out.extend(columns(name, "column: ")),
            Ty::Trait(name) => {
                if let Some(t) = self.decls.trait_named(name) {
                    out.extend(
                        t.methods
                            .iter()
                            .filter(|(n, ..)| n != "new")
                            .map(|(n, how, _)| item(n, CompletionKind::Method, Some(how.clone()))),
                    );
                }
            }
            Ty::Named(t) => out.extend(
                stdlib::methods_of(t)
                    .into_iter()
                    .map(|n| item(n, CompletionKind::Method, Some(format!("method of {t}")))),
            ),
            _ => {}
        }
        out
    }

    /// The methods a user's type declares, and the defaults of the traits it implements.
    fn user_methods(&self, type_name: &str) -> Vec<Completion> {
        let Some(prog) = self.program() else {
            return Vec::new();
        };
        let prefix = format!("{type_name}::");
        let mut out: Vec<Completion> =
            prog.fns
                .iter()
                .filter_map(|f| {
                    let rest = f.name.strip_prefix(&prefix)?;
                    (!rest.contains("::") && f.params.first().is_some_and(|p| p.name == "self"))
                        .then(|| {
                            item(
                                rest,
                                CompletionKind::Method,
                                Some(self.map.demangle(&signature(f))),
                            )
                            .of_type(&f.ret)
                        })
                })
                .collect();
        for (_, tr) in self.decls.impls.iter().filter(|(t, _)| t == type_name) {
            if let Some(t) = self.decls.trait_named(tr) {
                for (name, how, _) in &t.methods {
                    out.push(
                        item(name, CompletionKind::Method, Some(how.clone()))
                            .doc(&format!("from trait {}", self.shown(tr))),
                    );
                }
            }
        }
        out
    }

    // ---- inside a string -------------------------------------------------------------------

    /// The columns of a table, when the string at `quote` is a name or query meant for one:
    /// `t["..`, `t.sql("..`, `std.table.filter(t, "..`.
    fn string_completions(&self, quote: usize, _offset: usize) -> Vec<Completion> {
        let before = self.text[..quote].trim_end();
        let (receiver, at) = if let Some(r) = before.strip_suffix('[') {
            (r.to_string(), r.len())
        } else if before.ends_with('(') || before.ends_with(',') {
            let Some(open) = open_brackets(before)
                .last()
                .filter(|(c, _)| *c == '(')
                .map(|&(_, p)| p)
            else {
                return Vec::new();
            };
            let callee = self.dotted_tail(&before[..open]);
            if callee.starts_with("std.") {
                let args = &before[open + 1..];
                let end = top_level_commas(args)
                    .first()
                    .copied()
                    .unwrap_or(args.len());
                (args[..end].trim().to_string(), open)
            } else if let Some((r, _)) = callee.rsplit_once('.') {
                (r.to_string(), open - (callee.len() - r.len()))
            } else {
                return Vec::new();
            }
        } else {
            return Vec::new();
        };
        match self.type_of_chain(&receiver, at.min(self.text.len())) {
            Some(Ty::Table(name)) => self
                .program()
                .and_then(|p| p.structs.iter().find(|s| s.name == name))
                .map(|s| {
                    s.fields
                        .iter()
                        .map(|(n, t)| {
                            let mut c =
                                item(n, CompletionKind::Field, Some(format!("column: {t}")));
                            c.rank = 0;
                            c
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    // ---- ranking ---------------------------------------------------------------------------

    /// The type the code at the cursor wants, as the checker prints it: the parameter of the
    /// call being written, the value of `return`, the variable being assigned.
    fn expected_type(&self, head: &str, offset: usize) -> Option<String> {
        let prog = self.program()?;
        if let Some(&('(', open)) = open_brackets(head).last() {
            let callee = self.dotted_tail(&head[..open]);
            let name = callee.rsplit('.').next().filter(|n| !n.is_empty())?;
            let f = prog.fns.iter().find(|f| f.name == name).or_else(|| {
                prog.fns
                    .iter()
                    .find(|f| f.name.rsplit("::").next() == Some(name) && callee.contains('.'))
            })?;
            let arg = top_level_commas(&head[open + 1..]).len();
            let skip = usize::from(f.params.first().is_some_and(|p| p.name == "self"));
            return f.params.get(arg + skip).map(|p| p.ty.to_string());
        }
        let line = head[line_start_of(head, head.len())..].trim();
        if line.starts_with("return") {
            let f = prog
                .fns
                .iter()
                .find(|f| f.span.start <= offset && offset <= f.span.end)?;
            return Some(f.ret.to_string());
        }
        let (target, _) = line.split_once('=')?;
        let name = target.trim_end_matches(['+', '-', '*', '/', ' ']);
        self.visible_locals(offset)
            .into_iter()
            .filter(|l| l.name == name)
            .max_by_key(|l| l.span.start)
            .map(|l| l.ty.to_string())
    }

    // ---- types of what comes before a dot --------------------------------------------------

    /// Locals declared in the function around `offset`, before it.
    pub(super) fn visible_locals(&self, offset: usize) -> Vec<&super::LocalInfo> {
        let Some(prog) = self.program() else {
            return Vec::new();
        };
        let Some(f) = prog
            .fns
            .iter()
            .find(|f| f.span.start <= offset && offset <= f.span.end)
        else {
            return Vec::new();
        };
        self.locals
            .values()
            .filter(|l| {
                l.span.start >= f.span.start && l.span.start < offset && l.span.end <= f.span.end
            })
            .collect()
    }

    /// The pieces of `a.b(c).d[0]`, cut at the dots outside brackets.
    fn chain_segments(text: &str) -> Vec<String> {
        let mut segments: Vec<String> = Vec::new();
        let mut depth = 0i32;
        let mut current = String::new();
        for c in text.trim_end().chars().rev() {
            match c {
                ')' | ']' => {
                    depth += 1;
                    current.insert(0, c);
                }
                '(' | '[' => {
                    depth -= 1;
                    current.insert(0, c);
                }
                '.' if depth == 0 => {
                    segments.insert(0, std::mem::take(&mut current));
                }
                c if depth > 0 || c.is_alphanumeric() || c == '_' => current.insert(0, c),
                _ => break,
            }
        }
        segments.insert(0, current);
        segments.retain(|s| !s.is_empty());
        segments
    }

    /// Resolves `a.b.c` / `f(x).field` / `xs.first().unwrap().name` from the text before a dot,
    /// using the last analysis.
    pub(super) fn type_of_chain(&self, text: &str, offset: usize) -> Option<Ty> {
        let prog = self.program()?;
        let segments = Self::chain_segments(text);
        let (first, mut rest) = segments.split_first()?;
        let name = first.split(['(', '[']).next()?;
        let is_call = first.contains('(');
        let mut ty = if let Some(t) = self.value_named(name, offset) {
            t
        } else if let Some(f) = prog.fns.iter().find(|f| f.name == name) {
            is_call.then(|| f.ret.clone())?
        } else {
            // `Circle.new(1.0).`: an associated function of a type.
            let type_name = self.resolve_type_name(name)?;
            let (seg, tail) = rest.split_first()?;
            rest = tail;
            let function = seg.split(['(', '[']).next()?;
            let f = prog
                .fns
                .iter()
                .find(|f| f.name == format!("{type_name}::{function}"))?;
            seg.contains('(').then(|| f.ret.clone())?
        };
        if first.ends_with(']') {
            ty = element_of(&ty)?;
        }
        for seg in rest {
            let member = seg.split(['(', '[']).next()?;
            ty = if seg.contains('(') {
                self.method_result(&ty, member)?
            } else {
                self.field_of(&ty, member)?
            };
            if seg.ends_with(']') {
                ty = element_of(&ty)?;
            }
        }
        Some(ty)
    }

    fn field_of(&self, ty: &Ty, field: &str) -> Option<Ty> {
        let prog = self.program()?;
        let field_of_struct = |name: &String| {
            prog.structs
                .iter()
                .find(|s| &s.name == name)?
                .fields
                .iter()
                .find(|(n, _)| n == field)
                .map(|(_, t)| t.clone())
        };
        match ty {
            Ty::Struct(name) => field_of_struct(name),
            Ty::Table(name) => Some(match field_of_struct(name)? {
                Ty::Prim(p) if p.is_numeric() => Ty::Tensor {
                    elem: Box::new(Ty::Prim(p)),
                    shape: vec![telvax_hir::Dim::Named("N".into())],
                },
                Ty::Prim(p) => Ty::Vec(Box::new(Ty::Prim(p))),
                other => other,
            }),
            Ty::Tuple(parts) => parts.get(field.parse::<usize>().ok()?).cloned(),
            _ => None,
        }
    }

    /// What calling the method `name` on a value of type `ty` gives.
    fn method_result(&self, ty: &Ty, name: &str) -> Option<Ty> {
        let prog = self.program()?;
        if let Ty::Struct(t) | Ty::Enum(t) | Ty::Trait(t) = ty
            && let Some(f) = prog.fns.iter().find(|f| f.name == format!("{t}::{name}"))
        {
            return Some(f.ret.clone());
        }
        builtin_result(ty, name)
    }

    /// The type of a name that is a value: a local, a closure parameter or a global.
    fn value_named(&self, name: &str, offset: usize) -> Option<Ty> {
        let local = self
            .visible_locals(offset)
            .into_iter()
            .filter(|l| l.name == name)
            .max_by_key(|l| l.span.start);
        match (local, self.closure_param_named(name, offset)) {
            (Some(l), Some((start, ty))) => Some(if start > l.span.start {
                ty
            } else {
                l.ty.clone()
            }),
            (Some(l), None) => Some(l.ty.clone()),
            (None, Some((_, ty))) => Some(ty),
            (None, None) => self
                .program()?
                .globals
                .iter()
                .find(|g| g.name == name)
                .map(|g| g.ty.clone()),
        }
    }

    /// `name` as a parameter of a closure that is still open at `offset`, like `r` in
    /// `t.filter(r => r.`: where the closure starts and the type its call gives the parameter.
    /// Read from the text, so it works while the closure does not parse yet.
    fn closure_param_named(&self, name: &str, offset: usize) -> Option<(usize, Ty)> {
        let before = &self.text[..offset];
        for (arrow, _) in before.rmatch_indices("=>") {
            let ptext = before[..arrow].trim_end();
            let (start, names): (usize, Vec<&str>) = if ptext.ends_with(')') {
                let Some(open) = matching_open(ptext, ptext.len() - 1) else {
                    continue;
                };
                let inner = &ptext[open + 1..ptext.len() - 1];
                (
                    open,
                    inner
                        .split(',')
                        .map(|p| p.split(':').next().unwrap_or("").trim())
                        .collect(),
                )
            } else {
                let word = ident_tail(ptext);
                (ptext.len() - word.len(), vec![word])
            };
            let Some(param) = names.iter().position(|n| *n == name) else {
                continue;
            };
            // Still open: no bracket in the body closes something opened before it.
            let mut depth = 0i32;
            for c in before[arrow + 2..].chars() {
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth -= 1,
                    _ => {}
                }
                if depth < 0 {
                    break;
                }
            }
            if depth < 0 {
                continue;
            }
            let lead = before[..start].trim_end();
            let (open, arg) = if lead.ends_with('(') {
                (lead.len() - 1, 0)
            } else if lead.ends_with(',') {
                let Some(&(_, open)) = open_brackets(lead).last().filter(|(c, _)| *c == '(') else {
                    continue;
                };
                (open, top_level_commas(&lead[open + 1..]).len())
            } else {
                continue;
            };
            let callee = self.dotted_tail(&before[..open]);
            let (receiver, method) = if callee.starts_with("std.") {
                // `std.table.filter(t, r => ..)`: the value is the first argument.
                let args = &lead[open + 1..];
                let end = top_level_commas(args)
                    .first()
                    .copied()
                    .unwrap_or(args.len());
                (args[..end].trim(), callee.rsplit('.').next()?)
            } else if let Some((r, m)) = callee.rsplit_once('.') {
                (r, m)
            } else {
                continue;
            };
            // Only what comes before the closure can give its parameter a type.
            let Some(recv_ty) = self.type_of_chain(receiver, start) else {
                continue;
            };
            if let Some(ty) = closure_param(&recv_ty, method, arg, param) {
                return Some((start, ty));
            }
        }
        None
    }
}
