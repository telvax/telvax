//! Prints a tree parsed from the old (Rust-style) syntax as Python-style source.
//!
//! The old parser throws comments away, so the printer takes the original source too and puts each
//! `//` comment back, as a `#` comment, before the first statement that follows it (or at the end
//! of the line it was on).

use std::collections::HashSet;

use telvax_ast::*;

pub struct Printer<'a> {
    src: &'a str,
    comments: Vec<Comment>,
    next_comment: usize,
    /// The source line of the last line written, so a blank line is copied only once.
    last_line: usize,
    out: String,
    /// Things the printer could not express in the new syntax.
    pub warnings: Vec<String>,
    /// Imported names, which start a dotted path in the new syntax.
    heads: HashSet<String>,
    from_py: bool,
    /// Names assigned in the function being printed: they are `var`, the others `let`.
    assigned: HashSet<String>,
}

struct Comment {
    start: usize,
    text: String,
    /// Code comes before it on the same line.
    trailing: bool,
    used: bool,
}

fn collect_comments(src: &str, marker: &str) -> Vec<Comment> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || (c == b'\'' && marker == "#") {
            quote = Some(c);
            i += 1;
            continue;
        }
        if src[i..].starts_with(marker) {
            let end = src[i..].find('\n').map_or(src.len(), |n| i + n);
            let line_start = src[..i].rfind('\n').map_or(0, |n| n + 1);
            let trailing = !src[line_start..i].trim().is_empty();
            out.push(Comment {
                start: i,
                text: src[i + marker.len()..end].trim_end().to_string(),
                trailing,
                used: false,
            });
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

const INDENT: &str = "    ";

impl<'a> Printer<'a> {
    pub fn new(src: &'a str, marker: &str) -> Self {
        Printer {
            src,
            comments: collect_comments(src, marker),
            next_comment: 0,
            last_line: usize::MAX,
            out: String::new(),
            warnings: Vec::new(),
            heads: HashSet::new(),
            from_py: marker == "#",
            assigned: HashSet::new(),
        }
    }

    pub fn program(mut self, p: &Program) -> (String, Vec<String>) {
        for item in &p.items {
            if let Item::Import(i) = item {
                for s in i.path.iter().chain(&i.group) {
                    self.heads.insert(s.node.clone());
                }
            }
        }
        self.items(&p.items);
        self.flush_all();
        let mut out = self.out;
        while out.ends_with("\n\n") {
            out.pop();
        }
        (out, self.warnings)
    }

    // ---------- output helpers ----------

    fn flush_before(&mut self, indent: usize, at: usize) {
        while self.next_comment < self.comments.len() && self.comments[self.next_comment].start < at
        {
            let c = &mut self.comments[self.next_comment];
            self.next_comment += 1;
            if c.used {
                continue;
            }
            c.used = true;
            let text = c.text.trim_start().to_string();
            let line = if text.is_empty() {
                "#".to_string()
            } else {
                format!("# {text}")
            };
            self.out.push_str(&INDENT.repeat(indent));
            self.out.push_str(&line);
            self.out.push('\n');
        }
    }

    fn flush_all(&mut self) {
        self.flush_before(0, usize::MAX);
    }

    /// Writes one line. Standalone comments before `at` come first; a comment at the end of the
    /// source line that holds `at` is appended to this line.
    fn line(&mut self, indent: usize, text: &str, at: usize) {
        self.flush_standalone_before(indent, at);
        let src_line = self.src[..at.min(self.src.len())].matches('\n').count();
        let new_src_line = src_line != self.last_line;
        self.last_line = src_line;
        if new_src_line
            && self.blank_line_before(at)
            && !self.out.is_empty()
            && !self.out.ends_with(":\n")
            && !self.out.ends_with("\n\n")
        {
            self.out.push('\n');
        }
        self.out.push_str(&INDENT.repeat(indent));
        self.out.push_str(text);
        if let Some(c) = self.trailing_on_line_of(at) {
            self.out.push_str("  # ");
            self.out.push_str(c.trim_start());
        }
        self.out.push('\n');
    }

    /// Whether the source has a blank line right before the line that holds `at`.
    fn blank_line_before(&self, at: usize) -> bool {
        let at = at.min(self.src.len());
        let line_start = self.src[..at].rfind('\n').map_or(0, |n| n + 1);
        if line_start == 0 {
            return false;
        }
        // `before` ends with the newline that closes the previous line.
        let before = &self.src[..line_start - 1];
        let prev_start = before.rfind('\n').map_or(0, |n| n + 1);
        before[prev_start..].trim().is_empty()
    }

    fn flush_standalone_before(&mut self, indent: usize, at: usize) {
        let mut i = self.next_comment;
        while i < self.comments.len() && self.comments[i].start < at {
            if !self.comments[i].trailing && !self.comments[i].used {
                self.comments[i].used = true;
                let text = self.comments[i].text.trim_start().to_string();
                let line = if text.is_empty() {
                    "#".to_string()
                } else {
                    format!("# {text}")
                };
                self.out.push_str(&INDENT.repeat(indent));
                self.out.push_str(&line);
                self.out.push('\n');
            }
            i += 1;
        }
    }

    fn trailing_on_line_of(&mut self, at: usize) -> Option<String> {
        let line_start = self.src[..at.min(self.src.len())]
            .rfind('\n')
            .map_or(0, |n| n + 1);
        let line_end = self.src[at.min(self.src.len())..]
            .find('\n')
            .map_or(self.src.len(), |n| at + n);
        for c in &mut self.comments {
            if c.trailing && !c.used && c.start >= line_start && c.start < line_end {
                c.used = true;
                return Some(c.text.clone());
            }
        }
        None
    }

    fn blank(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    // ---------- items ----------

    fn items(&mut self, items: &[Item]) {
        let mut i = 0;
        let mut prev_kind = "";
        while i < items.len() {
            let kind = match &items[i] {
                Item::Import(_) => "import",
                Item::Let(_) => "let",
                _ => "block",
            };
            if !prev_kind.is_empty() && (kind != prev_kind || kind == "block") {
                self.blank();
            }
            prev_kind = kind;
            match &items[i] {
                Item::Import(im) => self.import(im),
                Item::Let(l) => {
                    let text = self.let_text(l);
                    self.line(0, &text, l.span.start);
                }
                Item::Struct(s) => {
                    // The `impl` blocks right after a struct become part of the class.
                    let mut impls: Vec<&Impl> = Vec::new();
                    while let Some(Item::Impl(im)) = items.get(i + 1) {
                        if im.ty.node == s.name.node {
                            impls.push(im);
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    self.class(s, &impls);
                }
                Item::Enum(e) => self.enum_(e),
                Item::Alias(a) => {
                    let generics = self.generics_text(&a.generics);
                    let ty = self.ty(&a.ty);
                    self.line(
                        0,
                        &format!("type {}{generics} = {ty}", a.name.node),
                        a.span.start,
                    );
                }
                Item::Impl(im) => self.extend(im),
                Item::Trait(t) => self.trait_(t),
                Item::Serve(_) => {}
                Item::Fn(f) => self.func(f, 0, true),
            }
            i += 1;
        }
    }

    fn import(&mut self, im: &Import) {
        let path: Vec<_> = im.path.iter().map(|p| p.node.as_str()).collect();
        let text = if im.group.is_empty() {
            format!("import {}", path.join("."))
        } else {
            let names: Vec<_> = im.group.iter().map(|p| p.node.as_str()).collect();
            format!("from {} import {}", path.join("."), names.join(", "))
        };
        self.line(0, &text, im.span.start);
    }

    fn generics_text(&self, gs: &[GenericParam]) -> String {
        if gs.is_empty() {
            return String::new();
        }
        let parts: Vec<_> = gs
            .iter()
            .map(|g| match g.bounds.len() {
                0 => g.name.node.clone(),
                1 => format!("{}: {}", g.name.node, g.bounds[0].node),
                _ => format!(
                    "{}: ({})",
                    g.name.node,
                    g.bounds
                        .iter()
                        .map(|b| b.node.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
            .collect();
        format!("[{}]", parts.join(", "))
    }

    fn class(&mut self, s: &Struct, impls: &[&Impl]) {
        let generics = self.generics_text(&s.generics);
        let first = impls.first().copied();
        let trait_part = first
            .and_then(|im| im.trait_name.as_ref())
            .map(|t| format!("({})", t.node))
            .unwrap_or_default();
        self.line(
            0,
            &format!("type {}{generics}{trait_part}:", s.name.node),
            s.span.start,
        );
        let mut empty = true;
        for f in &s.fields {
            let ty = self.ty(&f.ty);
            let default = f
                .default
                .as_ref()
                .map(|d| format!(" = {}", self.expr(d, 0)))
                .unwrap_or_default();
            self.line(
                1,
                &format!("{}: {ty}{default}", f.name.node),
                f.name.span.start,
            );
            empty = false;
        }
        if let Some(im) = first {
            for (n, f) in im.fns.iter().enumerate() {
                if n > 0 || !empty {
                    self.out.push('\n');
                }
                self.func(f, 1, false);
                empty = false;
            }
        }
        if empty {
            self.line(1, "pass", s.span.end);
        }
        for im in impls.iter().skip(1) {
            self.blank();
            self.extend(im);
        }
    }

    fn extend(&mut self, im: &Impl) {
        let generics = self.generics_text(&im.generics);
        let trait_part = im
            .trait_name
            .as_ref()
            .map(|t| format!("({})", t.node))
            .unwrap_or_default();
        self.line(
            0,
            &format!("extend {}{generics}{trait_part}:", im.ty.node),
            im.span.start,
        );
        for (n, f) in im.fns.iter().enumerate() {
            if n > 0 {
                self.out.push('\n');
            }
            self.func(f, 1, false);
        }
    }

    fn trait_(&mut self, t: &Trait) {
        self.line(0, &format!("trait {}:", t.name.node), t.span.start);
        for (n, f) in t.fns.iter().enumerate() {
            if n > 0 {
                self.out.push('\n');
            }
            self.func(f, 1, false);
        }
    }

    fn enum_(&mut self, e: &Enum) {
        let generics = self.generics_text(&e.generics);
        self.line(0, &format!("enum {}{generics}:", e.name.node), e.span.start);
        for v in &e.variants {
            let text = if v.fields.is_empty() {
                v.name.node.clone()
            } else {
                let tys: Vec<_> = v
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, t)| match v.names.get(i) {
                        Some(n) => format!("{}: {}", n.node, self.ty(t)),
                        None => self.ty(t),
                    })
                    .collect();
                format!("{}({})", v.name.node, tys.join(", "))
            };
            let text = if v.from { format!("from {text}") } else { text };
            self.line(1, &text, v.name.span.start);
        }
    }

    fn func(&mut self, f: &Fn, indent: usize, top: bool) {
        let _ = top;
        for a in &f.attrs {
            let text = self.attr_text(a);
            self.line(indent, &text, a.span.start);
        }
        let generics = self.generics_text(&f.generics);
        let params: Vec<_> = f
            .params
            .iter()
            .map(|p| {
                if p.name.node == "self" {
                    "self".to_string()
                } else {
                    let default = p
                        .default
                        .as_ref()
                        .map(|d| format!(" = {}", self.expr(d, 0)))
                        .unwrap_or_default();
                    format!("{}: {}{default}", p.name.node, self.ty(&p.ty))
                }
            })
            .collect();
        let ret = f
            .ret
            .as_ref()
            .map(|t| format!(" -> {}", self.ty(t)))
            .unwrap_or_default();
        self.assigned = assigned_names(&f.body);
        let head = format!("fn {}{generics}({}){ret}", f.name.node, params.join(", "));
        if f.required {
            self.line(indent, &format!("{head}: ..."), f.span.start);
        } else {
            self.line(indent, &format!("{head}:"), f.span.start);
            self.body(&f.body, indent + 1, f.ret.is_some());
        }
    }

    fn attr_text(&mut self, a: &Attribute) -> String {
        if a.args.is_empty() {
            return format!("@{}", a.name.node);
        }
        let find = |k: &str| a.args.iter().find(|(n, _)| n.node == k).map(|(_, v)| v);
        // `@endpoint(POST, "/x")` when the method and the path are plain strings.
        if a.name.node == "endpoint"
            && a.args.len() == 2
            && let (
                Some(Spanned {
                    node: ExprKind::Lit(Lit::Str(m)),
                    ..
                }),
                Some(path),
            ) = (find("method"), find("path"))
            && m.chars().all(|c| c.is_ascii_uppercase())
        {
            return format!("@endpoint({m}, {})", self.expr(path, 0));
        }
        let args: Vec<_> = a
            .args
            .iter()
            .map(|(k, v)| format!("{}: {}", k.node, self.expr(v, 0)))
            .collect();
        format!("@{}({})", a.name.node, args.join(", "))
    }

    // ---------- types ----------

    fn ty(&mut self, t: &Type) -> String {
        if t.path.len() == 1 && t.path[0].node == "(tuple)" {
            let items: Vec<_> = t
                .args
                .iter()
                .map(|a| match a {
                    TypeArg::Type(t) => self.ty(t),
                    _ => String::new(),
                })
                .collect();
            return format!("({})", items.join(", "));
        }
        let name = t
            .path
            .iter()
            .map(|p| p.node.as_str())
            .collect::<Vec<_>>()
            .join(".");
        if name == "Option"
            && t.args.len() == 1
            && let TypeArg::Type(inner) = &t.args[0]
        {
            return format!("{}?", self.ty(inner));
        }
        let name = match name.as_str() {
            "i64" => "Int",
            "f64" => "Float",
            "String" => "Text",
            "bool" => "Bool",
            "Vec" => "List",
            other => other,
        }
        .to_string();
        if t.args.is_empty() {
            return name;
        }
        let args: Vec<_> = t
            .args
            .iter()
            .map(|a| match a {
                TypeArg::Type(t) => self.ty(t),
                TypeArg::Shape(dims) => {
                    let d: Vec<_> = dims
                        .iter()
                        .map(|d| match d {
                            Dim::Lit(n) => n.to_string(),
                            Dim::Named(n) => n.node.clone(),
                        })
                        .collect();
                    d.join(", ")
                }
                TypeArg::Row(fields) => {
                    let f: Vec<_> = fields
                        .iter()
                        .map(|f| format!("{}: {}", f.name.node, self.ty(&f.ty)))
                        .collect();
                    format!("{{{}}}", f.join(", "))
                }
            })
            .collect();
        format!("{name}[{}]", args.join(", "))
    }

    // ---------- statements ----------

    /// The statements of a block. When `returns`, a final value is written as `return value`.
    fn body(&mut self, b: &Block, indent: usize, returns: bool) {
        let start = self.out.len();
        self.stmts(b, indent, returns);
        if self.out.len() == start {
            self.line(indent, "pass", b.span.end);
        }
    }

    fn keyword(&self, l: &Let) -> &'static str {
        if l.mutable && self.assigned.contains(&l.name.node) {
            "var"
        } else {
            "let"
        }
    }

    fn let_text(&mut self, l: &Let) -> String {
        let init = self.expr(&l.init, 0);
        let kw = self.keyword(l);
        match &l.ty {
            Some(t) => format!("{kw} {}: {} = {init}", l.name.node, self.ty(t)),
            None => format!("{kw} {} = {init}", l.name.node),
        }
    }

    fn stmts(&mut self, b: &Block, indent: usize, returns: bool) {
        let mut i = 0;
        while i < b.stmts.len() {
            match &b.stmts[i] {
                Stmt::Let(l) if l.name.node.starts_with("__tuple") => {
                    // `a, b = value`: the temporary, then one `let` per name.
                    let temp = l.name.node.clone();
                    let mut names: Vec<String> = Vec::new();
                    let mut j = i + 1;
                    while let Some(Stmt::Let(part)) = b.stmts.get(j) {
                        let ExprKind::Field { base, name } = &part.init.node else {
                            break;
                        };
                        let ExprKind::Path(p) = &base.node else { break };
                        if p.len() != 1 || p[0].node != temp {
                            break;
                        }
                        let Ok(idx) = name.node.parse::<usize>() else {
                            break;
                        };
                        while names.len() < idx {
                            names.push("_".into());
                        }
                        names.push(part.name.node.clone());
                        j += 1;
                    }
                    let init = self.expr(&l.init, 0);
                    let kw = if names.iter().any(|n| self.assigned.contains(n)) {
                        "var"
                    } else {
                        "let"
                    };
                    self.line(
                        indent,
                        &format!("{kw} {} = {init}", names.join(", ")),
                        l.span.start,
                    );
                    i = j;
                    continue;
                }
                Stmt::Let(l) if is_statement_closure(&l.init) => {
                    // A closure with statements in its body is a local `def`.
                    let ExprKind::Closure {
                        params,
                        types,
                        body,
                    } = &l.init.node
                    else {
                        unreachable!()
                    };
                    let ExprKind::Block(b) = &body.node else {
                        unreachable!()
                    };
                    let ps: Vec<_> = params
                        .iter()
                        .zip(types)
                        .map(|(p, t)| match t {
                            Some(t) => format!("{}: {}", p.node, self.ty(t)),
                            None => p.node.clone(),
                        })
                        .collect();
                    self.line(
                        indent,
                        &format!("fn {}({}):", l.name.node, ps.join(", ")),
                        l.span.start,
                    );
                    self.body(b, indent + 1, true);
                }
                Stmt::Let(l) => {
                    let text = self.let_text(l);
                    self.line(indent, &text, l.span.start);
                }
                Stmt::Expr(e) => self.stmt_expr(e, indent, false),
            }
            i += 1;
        }
        if let Some(t) = &b.tail {
            self.stmt_expr(t, indent, returns);
        }
    }

    /// An expression used as a statement. `value` means its result is the block's result.
    fn stmt_expr(&mut self, e: &Expr, indent: usize, value: bool) {
        match &e.node {
            ExprKind::Return(v) => {
                let text = match v {
                    Some(v) => format!("return {}", self.expr(v, 0)),
                    None => "return".to_string(),
                };
                self.line(indent, &text, e.span.start);
            }
            ExprKind::Break => self.line(indent, "break", e.span.start),
            ExprKind::Continue => self.line(indent, "continue", e.span.start),
            ExprKind::Assign { target, value: v } => {
                let t = self.expr(target, 0);
                let compound = match &v.node {
                    ExprKind::Binary { op, lhs, rhs } if **lhs == **target => {
                        let sym = match op {
                            BinOp::Add => Some("+="),
                            BinOp::Sub => Some("-="),
                            BinOp::Mul => Some("*="),
                            BinOp::Div => Some("/="),
                            _ => None,
                        };
                        sym.map(|s| (s, rhs))
                    }
                    _ => None,
                };
                let text = match compound {
                    Some((sym, rhs)) => format!("{t} {sym} {}", self.expr(rhs, 0)),
                    None => format!("{t} = {}", self.expr(v, 0)),
                };
                self.line(indent, &text, e.span.start);
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.expr(cond, 0);
                self.line(indent, &format!("if {c}:"), e.span.start);
                self.body(then, indent + 1, value);
                self.else_chain(otherwise.as_deref(), indent, value);
            }
            ExprKind::Match { scrutinee, arms } if is_if_let(e, arms) => {
                let s = self.expr(scrutinee, 0);
                let p = self.pat(&arms[0].pat);
                self.line(indent, &format!("if {s} is {p}:"), e.span.start);
                self.arm_body(&arms[0].body, indent + 1, value);
                if !is_empty_block(&arms[1].body) {
                    self.line(indent, "else:", e.span.start);
                    self.arm_body(&arms[1].body, indent + 1, value);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let s = self.expr(scrutinee, 0);
                self.line(indent, &format!("match {s}:"), e.span.start);
                for arm in arms {
                    let p = self.pat(&arm.pat);
                    let guard = match &arm.guard {
                        Some(g) => format!(" if {}", self.expr(g, 0)),
                        None => String::new(),
                    };
                    self.line(indent + 1, &format!("case {p}{guard}:"), arm.pat.span.start);
                    self.arm_body(&arm.body, indent + 2, value);
                }
            }
            ExprKind::While { cond, body } => {
                if let Some((scrutinee, pat, inner)) = while_let_parts(e, cond, body) {
                    let s = self.expr(scrutinee, 0);
                    let p = self.pat(pat);
                    self.line(indent, &format!("while {s} is {p}:"), e.span.start);
                    self.arm_body(inner, indent + 1, false);
                } else {
                    let c = self.expr(cond, 0);
                    self.line(indent, &format!("while {c}:"), e.span.start);
                    self.body(body, indent + 1, false);
                }
            }
            ExprKind::For { var, iter, body } => self.for_loop(e, var, iter, body, indent),
            ExprKind::Block(b) => {
                // A bare block is only a scope.
                self.line(indent, "if True:", e.span.start);
                self.body(b, indent + 1, value);
            }
            _ => {
                let text = self.expr(e, 0);
                self.line(indent, &text, e.span.start);
            }
        }
    }

    fn else_chain(&mut self, otherwise: Option<&Expr>, indent: usize, value: bool) {
        let Some(o) = otherwise else { return };
        match &o.node {
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.expr(cond, 0);
                self.line(indent, &format!("elif {c}:"), o.span.start);
                self.body(then, indent + 1, value);
                self.else_chain(otherwise.as_deref(), indent, value);
            }
            ExprKind::Block(b) => {
                self.line(indent, "else:", o.span.start);
                self.body(b, indent + 1, value);
            }
            _ => {
                self.line(indent, "else:", o.span.start);
                self.stmt_expr(o, indent + 1, value);
            }
        }
    }

    /// The body of a `case`, an `if x is ...` or a `while x is ...`: a block, or one expression.
    fn arm_body(&mut self, e: &Expr, indent: usize, value: bool) {
        match &e.node {
            ExprKind::Block(b) => self.body(b, indent, value),
            _ => self.stmt_expr(e, indent, value),
        }
    }

    fn for_loop(&mut self, e: &Expr, var: &Ident, iter: &ForIter, body: &Block, indent: usize) {
        let it = match iter {
            ForIter::Range {
                start,
                end,
                inclusive,
            } => {
                let end_text = if *inclusive {
                    match &end.node {
                        ExprKind::Lit(Lit::Int(n)) => (n + 1).to_string(),
                        _ => format!("{} + 1", self.expr(end, 8)),
                    }
                } else {
                    self.expr(end, 0)
                };
                if matches!(start.node, ExprKind::Lit(Lit::Int(0))) && !*inclusive {
                    format!("range({end_text})")
                } else {
                    format!("range({}, {end_text})", self.expr(start, 0))
                }
            }
            ForIter::Expr(x) => self.expr(x, 0),
        };
        // `for (a, b) in items`: the parts are bound by `let`s at the top of the body.
        if var.node.starts_with("__item") {
            let mut names: Vec<String> = Vec::new();
            let mut skip = 0;
            for s in &body.stmts {
                let Stmt::Let(part) = s else { break };
                let ExprKind::Field { base, name } = &part.init.node else {
                    break;
                };
                let ExprKind::Path(p) = &base.node else { break };
                if p.len() != 1 || p[0].node != var.node {
                    break;
                }
                let Ok(idx) = name.node.parse::<usize>() else {
                    break;
                };
                while names.len() < idx {
                    names.push("_".into());
                }
                names.push(part.name.node.clone());
                skip += 1;
            }
            self.line(
                indent,
                &format!("for {} in {it}:", names.join(", ")),
                e.span.start,
            );
            let rest = Block {
                stmts: body.stmts[skip..].to_vec(),
                tail: body.tail.clone(),
                span: body.span.clone(),
            };
            self.body(&rest, indent + 1, false);
        } else {
            self.line(indent, &format!("for {} in {it}:", var.node), e.span.start);
            self.body(body, indent + 1, false);
        }
    }

    // ---------- patterns ----------

    fn pat(&mut self, p: &Pattern) -> String {
        match &p.node {
            PatKind::Wild => "_".into(),
            PatKind::Bind(n) => n.clone(),
            PatKind::Lit(l) => lit(l),
            PatKind::Tuple(items) => {
                let parts: Vec<_> = items.iter().map(|i| self.pat(i)).collect();
                format!("({})", parts.join(", "))
            }
            PatKind::Variant { path, args } => {
                let name = path
                    .iter()
                    .map(|p| p.node.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                if args.is_empty() {
                    name
                } else {
                    let parts: Vec<_> = args.iter().map(|i| self.pat(i)).collect();
                    format!("{name}({})", parts.join(", "))
                }
            }
            PatKind::NegLit(l) => format!("-{}", lit(l)),
            PatKind::Or(alts) => {
                let parts: Vec<_> = alts.iter().map(|i| self.pat(i)).collect();
                parts.join(" | ")
            }
            PatKind::List {
                before,
                rest,
                after,
            } => {
                let mut parts: Vec<String> = before.iter().map(|i| self.pat(i)).collect();
                if let Some(r) = rest {
                    parts.push(format!("*{}", self.pat(r)));
                }
                parts.extend(after.iter().map(|i| self.pat(i)));
                format!("[{}]", parts.join(", "))
            }
            PatKind::Struct { path, fields } => {
                let name = path
                    .iter()
                    .map(|p| p.node.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                let parts: Vec<_> = fields
                    .iter()
                    .map(|(n, i)| format!("{}: {}", n.node, self.pat(i)))
                    .collect();
                format!("{name}({})", parts.join(", "))
            }
        }
    }

    // ---------- expressions ----------

    fn expr(&mut self, e: &Expr, min: u8) -> String {
        let (text, prec) = self.expr_prec(e);
        if prec < min {
            format!("({text})")
        } else {
            text
        }
    }

    /// Precedence levels: 0 lambda, 1 conditional, 2 pipe, 3 or, 4 and, 5 not, 6 equality,
    /// 7 comparison, 8 additive, 9 multiplicative, 10 unary, 11 postfix, 12 atom.
    fn expr_prec(&mut self, e: &Expr) -> (String, u8) {
        match &e.node {
            ExprKind::Lit(l) => (lit(l), 12),
            ExprKind::Path(p) => {
                let names: Vec<&str> = p.iter().map(|i| i.node.as_str()).collect();
                let text = match names.as_slice() {
                    // Only the Rust-style syntax had `println`; `print` there did not end the line.
                    ["println"] | ["std", "io", "println"] if !self.from_py => "print".to_string(),
                    ["print"] if !self.from_py => "std.io.print".to_string(),
                    _ => names.join("."),
                };
                (text, 12)
            }
            ExprKind::Call { callee, args } => {
                let c = self.expr(callee, 11);
                let a: Vec<_> = args.iter().map(|x| self.expr(x, 0)).collect();
                (format!("{c}({})", a.join(", ")), 11)
            }
            ExprKind::Named { name, value } => {
                (format!("{}: {}", name.node, self.expr(value, 0)), 0)
            }
            ExprKind::Field { base, name } => {
                let b = self.expr(base, 11);
                (format!("{b}.{}", name.node), 11)
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base, 11);
                let i = self.expr(index, 0);
                (format!("{b}[{i}]"), 11)
            }
            ExprKind::Try(x) => (format!("{}?", self.expr(x, 11)), 11),
            ExprKind::Await(x) => (format!("await {}", self.expr(x, 11)), 10),
            ExprKind::Unary { op, expr } => match op {
                UnOp::Neg => (format!("-{}", self.expr(expr, 10)), 10),
                UnOp::Not => (format!("not {}", self.expr(expr, 5)), 5),
            },
            ExprKind::Binary { op, lhs, rhs } => {
                let (sym, prec) = match op {
                    BinOp::Or => ("or", 3),
                    BinOp::And => ("and", 4),
                    BinOp::Eq => ("==", 6),
                    BinOp::NotEq => ("!=", 6),
                    BinOp::Lt => ("<", 7),
                    BinOp::LtEq => ("<=", 7),
                    BinOp::Gt => (">", 7),
                    BinOp::GtEq => (">=", 7),
                    BinOp::Add => ("+", 8),
                    BinOp::Sub => ("-", 8),
                    BinOp::Mul => ("*", 9),
                    BinOp::Div => ("/", 9),
                    BinOp::Rem => ("%", 9),
                };
                // Left-associative: the right operand needs parentheses at the same level.
                let l = self.expr(lhs, prec);
                let r = self.expr(rhs, prec + 1);
                (format!("{l} {sym} {r}"), prec)
            }
            ExprKind::Pipe { lhs, rhs } => {
                let l = self.expr(lhs, 2);
                let r = self.expr(rhs, 3);
                (format!("{l} |> {r}"), 2)
            }
            ExprKind::Closure { params, body, .. } => {
                let (names, body) = self.closure_parts(params, body);
                let b = self.expr(&body, 0);
                let params = if names.len() == 1 && !names[0].starts_with('(') {
                    names[0].clone()
                } else {
                    format!("({})", names.join(", "))
                };
                (format!("{params} => {b}"), 0)
            }
            ExprKind::StructLit { path, fields } => {
                let name = path
                    .iter()
                    .map(|p| p.node.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                let f: Vec<_> = fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.node, self.expr(v, 0)))
                    .collect();
                (format!("{name}({})", f.join(", ")), 11)
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let (Some(a), Some(o)) = (then.tail.as_deref(), otherwise.as_deref()) else {
                    self.warn("an `if` used as a value without both branches");
                    return ("None".into(), 12);
                };
                if !then.stmts.is_empty() {
                    self.warn("an `if` used as a value with statements in a branch");
                }
                let c = self.expr(cond, 2);
                let a = self.expr(a, 2);
                let b = match &o.node {
                    ExprKind::Block(b) if b.stmts.is_empty() && b.tail.is_some() => {
                        self.expr(b.tail.as_deref().expect("checked"), 0)
                    }
                    _ => self.expr(o, 0),
                };
                (format!("{a} if {c} else {b}"), 1)
            }
            ExprKind::Block(b) if b.stmts.is_empty() && b.tail.is_some() => {
                self.expr_prec(b.tail.as_deref().expect("checked"))
            }
            ExprKind::Block(_) | ExprKind::Match { .. } => {
                self.warn("a `match` or block used as a value");
                ("None".into(), 12)
            }
            ExprKind::Return(_)
            | ExprKind::Break
            | ExprKind::Continue
            | ExprKind::While { .. }
            | ExprKind::For { .. }
            | ExprKind::Assign { .. } => {
                self.warn("a statement used as a value");
                ("None".into(), 12)
            }
            ExprKind::Tuple(items) => {
                let parts: Vec<_> = items.iter().map(|x| self.expr(x, 0)).collect();
                (format!("({})", parts.join(", ")), 12)
            }
            ExprKind::Array(items) => {
                let parts: Vec<_> = items.iter().map(|x| self.expr(x, 0)).collect();
                (format!("[{}]", parts.join(", ")), 12)
            }
            ExprKind::FString(parts) => {
                let mut s = String::from("f\"");
                for p in parts {
                    match p {
                        FPart::Text(t) => s.push_str(&escape(t)),
                        FPart::Expr(x) => {
                            s.push('{');
                            let inner = self.expr(x, 0);
                            s.push_str(&inner);
                            s.push('}');
                        }
                        FPart::Fmt { expr, repr, spec } => {
                            s.push('{');
                            let inner = self.expr(expr, 0);
                            s.push_str(&inner);
                            if *repr {
                                s.push_str("!r");
                            }
                            if let Some(ExprKind::Lit(Lit::Str(spec))) =
                                spec.as_ref().map(|e| &e.node)
                            {
                                s.push(':');
                                s.push_str(spec);
                            }
                            s.push('}');
                        }
                    }
                }
                s.push('"');
                (s, 12)
            }
            ExprKind::Error => ("None".into(), 12),
        }
    }

    /// Parameter names of a closure, and the body without the `let`s that unpack tuple parameters.
    fn closure_parts(&mut self, params: &[Ident], body: &Expr) -> (Vec<String>, Expr) {
        let mut names = Vec::new();
        let mut body = body.clone();
        for p in params {
            if let Some(stripped) = p.node.strip_prefix("__arg") {
                let _ = stripped;
                // The unpacking `let`s are at the top of the body.
                let mut parts: Vec<String> = Vec::new();
                if let ExprKind::Block(b) = &body.node {
                    let mut skip = 0;
                    for s in &b.stmts {
                        let Stmt::Let(part) = s else { break };
                        let ExprKind::Field { base, name } = &part.init.node else {
                            break;
                        };
                        let ExprKind::Path(path) = &base.node else {
                            break;
                        };
                        if path.len() != 1 || path[0].node != p.node {
                            break;
                        }
                        let Ok(idx) = name.node.parse::<usize>() else {
                            break;
                        };
                        while parts.len() < idx {
                            parts.push("_".into());
                        }
                        parts.push(part.name.node.clone());
                        skip += 1;
                    }
                    let rest = Block {
                        stmts: b.stmts[skip..].to_vec(),
                        tail: b.tail.clone(),
                        span: b.span.clone(),
                    };
                    body = match rest.tail {
                        Some(tail) if rest.stmts.is_empty() => *tail,
                        tail => Spanned {
                            node: ExprKind::Block(Block { tail, ..rest }),
                            span: body.span.clone(),
                        },
                    };
                }
                names.push(format!("({})", parts.join(", ")));
            } else {
                names.push(p.node.clone());
            }
        }
        (names, body)
    }
}

/// A closure whose body has statements; it cannot be a `lambda`.
fn is_statement_closure(e: &Expr) -> bool {
    matches!(&e.node, ExprKind::Closure { params, body, .. }
        if !params.iter().any(|p| p.node.starts_with("__arg"))
            && matches!(&body.node, ExprKind::Block(b) if !b.stmts.is_empty()))
}

fn is_empty_block(e: &Expr) -> bool {
    matches!(&e.node, ExprKind::Block(b) if b.stmts.is_empty() && b.tail.is_none())
}

/// An `if let` the old parser built: two arms, the second a catch-all spanning the whole `match`.
fn is_if_let(e: &Expr, arms: &[Arm]) -> bool {
    arms.len() == 2 && matches!(arms[1].pat.node, PatKind::Wild) && arms[1].pat.span == e.span
}

/// A `while let` the old parser built: `while true { match value { pat => body, _ => break } }`.
fn while_let_parts<'e>(
    e: &Expr,
    cond: &Expr,
    body: &'e Block,
) -> Option<(&'e Expr, &'e Pattern, &'e Expr)> {
    if !matches!(cond.node, ExprKind::Lit(Lit::Bool(true))) || cond.span != e.span {
        return None;
    }
    let tail = body.tail.as_deref()?;
    let ExprKind::Match { scrutinee, arms } = &tail.node else {
        return None;
    };
    if arms.len() != 2 || !matches!(arms[1].pat.node, PatKind::Wild) || arms[1].pat.span != e.span {
        return None;
    }
    Some((scrutinee, &arms[0].pat, &arms[0].body))
}

fn lit(l: &Lit) -> String {
    match l {
        Lit::Int(n) => n.to_string(),
        Lit::Float(f) => {
            let s = format!("{f:?}");
            if s.contains('.') && !s.contains('e') {
                s
            } else if let Some((mant, exp)) = s.split_once('e') {
                let mant = if mant.contains('.') {
                    mant.to_string()
                } else {
                    format!("{mant}.0")
                };
                format!("{mant}e{exp}")
            } else {
                format!("{s}.0")
            }
        }
        Lit::Str(s) => format!("\"{}\"", escape(s)),
        Lit::Bool(true) => "True".into(),
        Lit::Bool(false) => "False".into(),
    }
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out
}

/// The variables a function assigns to (the root of `v[i] = x` and `p.a.b = x` counts).
fn assigned_names(b: &Block) -> HashSet<String> {
    let mut out = HashSet::new();
    block_assigns(b, &mut out);
    out
}

fn block_assigns(b: &Block, out: &mut HashSet<String>) {
    for s in &b.stmts {
        match s {
            Stmt::Let(l) => expr_assigns(&l.init, out),
            Stmt::Expr(e) => expr_assigns(e, out),
        }
    }
    if let Some(t) = &b.tail {
        expr_assigns(t, out);
    }
}

fn root_name(e: &Expr) -> Option<&str> {
    match &e.node {
        ExprKind::Path(p) if p.len() == 1 => Some(&p[0].node),
        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => root_name(base),
        _ => None,
    }
}

fn expr_assigns(e: &Expr, out: &mut HashSet<String>) {
    match &e.node {
        ExprKind::Assign { target, value } => {
            if let Some(n) = root_name(target) {
                out.insert(n.to_string());
            }
            expr_assigns(target, out);
            expr_assigns(value, out);
        }
        ExprKind::Lit(_)
        | ExprKind::Path(_)
        | ExprKind::Break
        | ExprKind::Continue
        | ExprKind::Error => {}
        ExprKind::Call { callee, args } => {
            expr_assigns(callee, out);
            args.iter().for_each(|a| expr_assigns(a, out));
        }
        ExprKind::Named { value, .. } => expr_assigns(value, out),
        ExprKind::Field { base, .. } => expr_assigns(base, out),
        ExprKind::Unary { expr, .. } | ExprKind::Try(expr) | ExprKind::Await(expr) => {
            expr_assigns(expr, out)
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Pipe { lhs, rhs } => {
            expr_assigns(lhs, out);
            expr_assigns(rhs, out);
        }
        ExprKind::Closure { body, .. } => expr_assigns(body, out),
        ExprKind::StructLit { fields, .. } => fields.iter().for_each(|(_, v)| expr_assigns(v, out)),
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            expr_assigns(cond, out);
            block_assigns(then, out);
            if let Some(o) = otherwise {
                expr_assigns(o, out);
            }
        }
        ExprKind::Block(b) => block_assigns(b, out),
        ExprKind::Return(v) => {
            if let Some(v) = v {
                expr_assigns(v, out);
            }
        }
        ExprKind::Tuple(items) | ExprKind::Array(items) => {
            items.iter().for_each(|i| expr_assigns(i, out));
        }
        ExprKind::Match { scrutinee, arms } => {
            expr_assigns(scrutinee, out);
            arms.iter().for_each(|a| expr_assigns(&a.body, out));
        }
        ExprKind::FString(parts) => {
            for p in parts {
                for x in p.exprs() {
                    expr_assigns(x, out);
                }
            }
        }
        ExprKind::While { cond, body } => {
            expr_assigns(cond, out);
            block_assigns(body, out);
        }
        ExprKind::For { iter, body, .. } => {
            match iter {
                ForIter::Range { start, end, .. } => {
                    expr_assigns(start, out);
                    expr_assigns(end, out);
                }
                ForIter::Expr(x) => expr_assigns(x, out),
            }
            block_assigns(body, out);
        }
        ExprKind::Index { base, index } => {
            expr_assigns(base, out);
            expr_assigns(index, out);
        }
    }
}
