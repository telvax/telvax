//! An interactive session: definitions and variables that last from one input to the next.
//!
//! The session keeps two things. The *definitions* are the source of every function, type, enum,
//! trait, `extend` block and import entered so far. The *variables* are the names bound by `let`
//! and `var`, each with its type and its current value.
//!
//! An input that is a definition is added to the definitions, after the whole set has been checked
//! again; one that is not (an expression, a `let`, an assignment, a loop) is wrapped in a function
//! that takes the variables as parameters and gives back the value of its last expression and the
//! variables as they are afterwards. The function is checked with the definitions, run, and its
//! results become the new values. Nothing is evaluated twice, so printing, files and the like
//! happen once.

use std::io::Write;
use std::sync::{Arc, Mutex};

use telvax_ast as ast;
use telvax_diagnostics::Diagnostic;
use telvax_hir::Ty;
use telvax_hir::hir;
use telvax_lsp::analysis::Snapshot;
use telvax_runtime::{Caps, Interp, Value};

/// The name of the function an input is wrapped in.
const INPUT: &str = "__input";
/// The name of the value of the last expression inside it.
const RESULT: &str = "__r";
const INDENT: &str = "    ";
/// What a variable is called as a parameter, before the body binds it with `let` or `var`.
const PARAM: &str = "__p_";

/// Where the output of the program goes.
#[derive(Debug, Clone)]
enum Sink {
    /// Kept, and given back in [`Outcome::output`].
    Capture(Arc<Mutex<Vec<u8>>>),
    /// Written to standard output as it happens.
    Stdout,
}

struct SinkWriter(Sink);

impl Write for SinkWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &self.0 {
            Sink::Capture(bytes) => bytes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(buf),
            Sink::Stdout => std::io::stdout().write_all(buf)?,
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if matches!(self.0, Sink::Stdout) {
            std::io::stdout().flush()?;
        }
        Ok(())
    }
}

/// A function, type, enum, trait, `extend` block or import.
#[derive(Debug, Clone)]
struct Def {
    /// What it replaces when it is entered again: `fn area`, `type Point`.
    key: String,
    text: String,
}

/// A name bound by `let` or `var`.
struct Binding {
    name: String,
    ty: Ty,
    mutable: bool,
    value: Value,
}

/// The value of an expression, as the session shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    pub ty: String,
    /// A form that shows more than the text, for a table or a tensor.
    pub html: Option<String>,
}

/// What an input did.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// What the program printed (empty when the session writes to standard output).
    pub output: String,
    /// The value of the last expression, when it has one.
    pub value: Option<Shown>,
    /// The names this input defined or changed the definition of.
    pub defined: Vec<String>,
    /// Things worth saying that are not errors.
    pub notes: Vec<String>,
    /// Why the input was not accepted. Nothing it did is kept, except for what ran before the
    /// error.
    pub error: Option<String>,
}

/// One thing the Tab key can complete to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub detail: Option<String>,
}

/// An interactive session.
pub struct Session {
    defs: Vec<Def>,
    bindings: Vec<Binding>,
    previous: Option<Interp>,
    sink: Sink,
    caps: Caps,
    step_limit: Option<u64>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("definitions", &self.defs.len())
            .field("variables", &self.bindings.len())
            .finish_non_exhaustive()
    }
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

/// An input, compiled but not run.
struct Compiled {
    hir: hir::Program,
    /// The names the function returns after the value of the last expression.
    names: Vec<String>,
    /// Their types, and the type of the last expression first.
    types: Vec<Ty>,
    /// Which of the names are new `let`s, with whether they are `var` and whether they were
    /// written as plain numbers (see [`widen`]).
    lets: Vec<(String, bool, bool)>,
    /// The input ended with an expression.
    has_value: bool,
}

impl Session {
    /// A session that keeps what the program prints and gives it back with each [`Outcome`].
    pub fn new() -> Session {
        Session {
            defs: Vec::new(),
            bindings: Vec::new(),
            previous: None,
            sink: Sink::Capture(Arc::new(Mutex::new(Vec::new()))),
            caps: Caps::ALL,
            step_limit: None,
        }
    }

    /// A session whose program prints to standard output as it runs.
    pub fn printing() -> Session {
        Session {
            sink: Sink::Stdout,
            ..Session::new()
        }
    }

    /// What the program may do outside itself (files, environment, network).
    pub fn set_caps(&mut self, caps: Caps) {
        self.caps = caps;
    }

    /// Stops an input that runs for more than this many evaluation steps.
    pub fn set_step_limit(&mut self, steps: Option<u64>) {
        self.step_limit = steps;
    }

    /// Forgets every definition and variable.
    pub fn reset(&mut self) {
        self.defs.clear();
        self.bindings.clear();
        self.previous = None;
    }

    /// The variables, with their types and values.
    pub fn variables(&self) -> Vec<(String, String, String)> {
        let interp = self.previous.as_ref();
        self.bindings
            .iter()
            .map(|b| {
                let shown = match interp {
                    Some(i) => i.render(&b.value, Some(&b.ty)),
                    None => b.value.to_string(),
                };
                (b.name.clone(), b.ty.to_string(), shown)
            })
            .collect()
    }

    /// The names of the functions, types, enums and traits entered so far.
    pub fn definitions(&self) -> Vec<String> {
        self.defs.iter().map(|d| d.key.clone()).collect()
    }

    /// Runs one input: a definition, a variable, an expression, a statement.
    pub fn eval(&mut self, input: &str) -> Outcome {
        let mut out = Outcome::default();
        if input.trim().is_empty() {
            return out;
        }
        if let Err(e) = self.run(input, &mut out) {
            out.error = Some(e);
        }
        if let Sink::Capture(bytes) = &self.sink {
            let taken = std::mem::take(&mut *bytes.lock().unwrap_or_else(|e| e.into_inner()));
            out.output = String::from_utf8_lossy(&taken).into_owned();
        }
        out
    }

    /// Enters a whole file: its definitions, then its variables in order.
    pub fn load(&mut self, source: &str) -> Outcome {
        self.eval(source)
    }

    /// The type of an expression, without running it.
    pub fn type_of(&mut self, expression: &str) -> Result<String, String> {
        let compiled = self.compile(expression)?;
        if !compiled.has_value {
            return Ok(Ty::Unit.to_string());
        }
        Ok(compiled.types[0].to_string())
    }

    fn run(&mut self, input: &str, out: &mut Outcome) -> Result<(), String> {
        let (program, errors) = telvax_parser::parse(input);
        match program {
            Some(p) if errors.is_empty() && !p.items.is_empty() => self.define(input, &p, out),
            _ if starts_a_definition(input) => {
                // Say what is wrong with the definition, not with a statement.
                let diags: Vec<Diagnostic> = errors;
                Err(render_all(&diags, input, 0, input.len()))
            }
            _ => self.statements(input, out),
        }
    }

    // ---- definitions ----------------------------------------------------------------------

    fn define(
        &mut self,
        input: &str,
        program: &ast::Program,
        out: &mut Outcome,
    ) -> Result<(), String> {
        // Each item runs from where it starts to where the next one does.
        let mut starts: Vec<usize> = program.items.iter().map(item_start).collect();
        starts.push(input.len());
        let mut new_defs: Vec<Def> = Vec::new();
        let mut lets: Vec<String> = Vec::new();
        for (i, item) in program.items.iter().enumerate() {
            let text = input[starts[i]..starts[i + 1].max(starts[i])]
                .trim_end()
                .to_string();
            match item {
                ast::Item::Let(_) => lets.push(text),
                other => new_defs.push(Def {
                    key: key_of(other, &text),
                    text,
                }),
            }
        }
        if !new_defs.is_empty() {
            self.add_definitions(new_defs, out)?;
        }
        for text in lets {
            self.statements(&text, out)?;
        }
        Ok(())
    }

    fn add_definitions(&mut self, new_defs: Vec<Def>, out: &mut Outcome) -> Result<(), String> {
        let mut defs: Vec<Def> = self
            .defs
            .iter()
            .filter(|d| !new_defs.iter().any(|n| n.key == d.key))
            .cloned()
            .collect();
        let new_text = new_defs
            .iter()
            .map(|d| d.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        defs.extend(new_defs.iter().cloned());
        let source = defs
            .iter()
            .map(|d| d.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let base = source.len() - new_text.len();
        let (program, errors) = telvax_parser::parse(&source);
        let mut diags = errors;
        if let Some(p) = &program {
            diags.extend(telvax_hir::lower(p).1);
        }
        if diags.iter().any(Diagnostic::is_error) {
            return Err(render_errors(&diags, &new_text, base));
        }
        // A type that is defined again may no longer fit the variables that hold one.
        for d in &new_defs {
            if let Some(name) = d.key.strip_prefix("type ") {
                let stale: Vec<String> = self
                    .bindings
                    .iter()
                    .filter(|b| mentions(&b.ty.to_string(), name))
                    .map(|b| b.name.clone())
                    .collect();
                if !stale.is_empty() && self.defs.iter().any(|old| old.key == d.key) {
                    self.bindings.retain(|b| !stale.contains(&b.name));
                    out.notes.push(format!(
                        "`{}` held a value of the old `{name}` and was forgotten",
                        stale.join("`, `")
                    ));
                }
            }
            out.defined.push(
                d.key
                    .split_once(' ')
                    .map_or(d.key.clone(), |(_, n)| n.to_string()),
            );
        }
        self.defs = defs;
        Ok(())
    }

    fn definitions_text(&self) -> String {
        self.defs
            .iter()
            .map(|d| d.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    // ---- statements and expressions ----------------------------------------------------------

    /// The start of the function an input is wrapped in: the variables as parameters, then bound
    /// again with `let` or `var` so that the checker keeps a `let` from being assigned to.
    fn header(&self) -> String {
        let params = self
            .bindings
            .iter()
            .map(|b| format!("{PARAM}{}: {}", b.name, b.ty))
            .collect::<Vec<_>>()
            .join(", ");
        let mut header = format!("fn {INPUT}({params}):\n");
        for b in &self.bindings {
            let keyword = if b.mutable { "var" } else { "let" };
            header.push_str(&format!(
                "{INDENT}{keyword} {} = {PARAM}{}\n",
                b.name, b.name
            ));
        }
        header
    }

    /// Wraps the input in a function over the variables and checks it with the definitions.
    fn compile(&self, input: &str) -> Result<Compiled, String> {
        let probe = format!("fn {INPUT}():\n{}", indent(input));
        let header_len = probe.find('\n').map_or(0, |i| i + 1);
        let (program, errors) = telvax_parser::parse(&probe);
        if !errors.is_empty() || program.is_none() {
            let map = |p: usize| probe_to_input(&probe, header_len, input, p);
            return Err(render_mapped(&errors, input, &map));
        }
        let Some(ast::Item::Fn(function)) = program.as_ref().and_then(|p| p.items.first()) else {
            return Err("that is not something to run".into());
        };
        // The new `let`s, the last of each name.
        let mut lets: Vec<(String, bool, bool)> = Vec::new();
        for s in &function.body.stmts {
            if let ast::Stmt::Let(l) = s
                && !l.name.node.starts_with("__")
            {
                lets.retain(|(n, _, _)| *n != l.name.node);
                let plain = l.ty.is_none() && is_plain_number(&l.init);
                lets.push((l.name.node.clone(), l.mutable, plain));
            }
        }
        // The last expression is the value of the input, unless it is a statement.
        let tail = function
            .body
            .tail
            .as_deref()
            .or(match function.body.stmts.last() {
                Some(ast::Stmt::Expr(e)) if gives_value(e) => Some(e),
                _ => None,
            });
        let body = &probe[header_len..];
        let (text, tail_at) = match tail {
            Some(t) => {
                let before = &probe[header_len..t.span.start];
                let slice = &probe[t.span.clone()];
                (
                    format!("{before}let {RESULT} = {slice}\n"),
                    Some(before.len()),
                )
            }
            None => (format!("{}\n", body.trim_end()), None),
        };
        let mut names: Vec<String> = lets.iter().map(|(n, _, _)| n.clone()).collect();
        for b in &self.bindings {
            if !names.contains(&b.name) {
                names.push(b.name.clone());
            }
        }
        let first = if tail.is_some() { RESULT } else { "0" };
        let mut elements = vec![first.to_string()];
        elements.extend(names.iter().cloned());
        if names.is_empty() {
            elements.push("0".into());
        }
        let header = self.header();
        let definitions = self.definitions_text();
        let separator = if definitions.is_empty() { "" } else { "\n\n" };
        let source = format!(
            "{definitions}{separator}{header}{text}{INDENT}({})\n",
            elements.join(", ")
        );
        let body_at = definitions.len() + separator.len() + header.len();
        let (checked, errors) = telvax_parser::parse(&source);
        let mut diags = errors;
        let mut hir = None;
        if let Some(p) = &checked {
            let (h, d) = telvax_hir::lower(p);
            diags.extend(d);
            hir = Some(h);
        }
        if diags.iter().any(Diagnostic::is_error) {
            let map = |o: usize| {
                let rel = o.checked_sub(body_at)?;
                // Past the start of the last expression the text moved by `let __r = `.
                let probe_at = match tail_at {
                    Some(t) if rel >= t + format!("let {RESULT} = ").len() => {
                        rel - format!("let {RESULT} = ").len() + header_len
                    }
                    _ => rel + header_len,
                };
                probe_to_input(&probe, header_len, input, probe_at)
            };
            return Err(render_mapped(&diags, input, &map));
        }
        let hir = hir.ok_or("that is not something to run")?;
        let function = hir
            .fns
            .iter()
            .find(|f| f.name == INPUT)
            .ok_or("that is not something to run")?;
        let Ty::Tuple(types) = &function.ret else {
            return Err("that is not something to run".into());
        };
        let types = types.clone();
        for (i, name) in names.iter().enumerate() {
            let is_new = lets.iter().any(|(n, _, _)| n == name);
            if is_new && types.get(i + 1).is_some_and(has_unknown) {
                return Err(format!(
                    "the type of `{name}` is not known yet: say the type of the parameters, as in `let inc = (n: Int) => n + 1`"
                ));
            }
        }
        Ok(Compiled {
            hir,
            names,
            types,
            lets,
            has_value: tail.is_some(),
        })
    }

    fn statements(&mut self, input: &str, out: &mut Outcome) -> Result<(), String> {
        let compiled = self.compile(input)?;
        let interp =
            Interp::with_caps(compiled.hir.clone(), false, self.caps).map_err(|e| e.to_string())?;
        if let Some(old) = &self.previous {
            interp.inherit_state(old);
        }
        if let Some(steps) = self.step_limit {
            interp.set_step_limit(steps);
        }
        interp.set_output(SinkWriter(self.sink.clone()));
        let args: Vec<Value> = self.bindings.iter().map(|b| b.value.clone()).collect();
        let result = interp.call(INPUT, args);
        let result = match result {
            Ok(Value::Tuple(values)) => values,
            Ok(_) => return Err("that is not something to run".into()),
            Err(e) => {
                self.previous = Some(interp);
                return Err(format!("error: {e}"));
            }
        };
        if compiled.has_value
            && !matches!(compiled.types[0], Ty::Unit | Ty::Never)
            && let Some(v) = result.first()
        {
            out.value = Some(Shown {
                text: interp.render(v, Some(&compiled.types[0])),
                ty: compiled.types[0].to_string(),
                html: telvax_runtime::rich::html(v),
            });
        }
        for (i, name) in compiled.names.iter().enumerate() {
            let (Some(value), Some(ty)) = (result.get(i + 1), compiled.types.get(i + 1)) else {
                continue;
            };
            if matches!(ty, Ty::Unit | Ty::Never) {
                self.bindings.retain(|b| b.name != *name);
                continue;
            }
            let new = compiled.lets.iter().find(|(n, _, _)| n == name);
            let ty = &match new {
                Some((_, _, true)) => widen(ty),
                _ => ty.clone(),
            };
            match self.bindings.iter_mut().find(|b| b.name == *name) {
                Some(b) => {
                    b.value = value.clone();
                    b.ty = ty.clone();
                    if let Some((_, mutable, _)) = new {
                        b.mutable = *mutable;
                    }
                }
                None => self.bindings.push(Binding {
                    name: name.clone(),
                    ty: ty.clone(),
                    mutable: new.is_some_and(|(_, m, _)| *m),
                    value: value.clone(),
                }),
            }
            if new.is_some() {
                out.defined.push(name.clone());
            }
        }
        self.previous = Some(interp);
        Ok(())
    }

    // ---- editing help -----------------------------------------------------------------------

    /// Whether `input` can be run as it is, or the person is in the middle of something: an
    /// open bracket or string, or a block that has not been ended with an empty line.
    pub fn is_complete(input: &str) -> bool {
        let mut depth = 0i32;
        let mut quote: Option<char> = None;
        let mut chars = input.chars().peekable();
        while let Some(c) = chars.next() {
            match quote {
                Some(q) => {
                    if c == '\\' {
                        chars.next();
                    } else if c == q {
                        quote = None;
                    }
                }
                None => match c {
                    '"' | '\'' => quote = Some(c),
                    '#' => {
                        for n in chars.by_ref() {
                            if n == '\n' {
                                break;
                            }
                        }
                    }
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth -= 1,
                    _ => {}
                },
            }
        }
        if depth > 0 || quote.is_some() {
            return false;
        }
        let lines: Vec<&str> = input.lines().collect();
        let last = lines.iter().rev().find(|l| !l.trim().is_empty());
        let Some(last) = last else { return true };
        let ended = input.ends_with("\n\n") || lines.last().is_some_and(|l| l.trim().is_empty());
        let opens_block = lines.iter().any(|l| {
            let code = l.split('#').next().unwrap_or("").trim_end();
            code.ends_with(':')
        });
        let attribute = last.trim_start().starts_with('@');
        if attribute {
            return false;
        }
        !opens_block || ended
    }

    /// What is known about the name at `cursor`, as Markdown: its type or its signature.
    pub fn hover(&self, input: &str, cursor: usize) -> Option<String> {
        let cursor = cursor.min(input.len());
        let definitions = self.definitions_text();
        let separator = if definitions.is_empty() { "" } else { "\n\n" };
        let header = self.header();
        let body = indent(input);
        let text = format!("{definitions}{separator}{header}{body}\n");
        let offset =
            definitions.len() + separator.len() + header.len() + body_offset(input, cursor);
        let snap = Snapshot::analyze(text);
        // A name that is not known has the type `_`, which says nothing.
        snap.hover(offset)
            .map(|(markdown, _)| markdown)
            .filter(|markdown| !markdown.contains("\n_\n"))
    }

    /// What the Tab key can complete the word before `cursor` to.
    pub fn complete(&self, input: &str, cursor: usize) -> (usize, Vec<Suggestion>) {
        let cursor = cursor.min(input.len());
        let start = input[..cursor]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or(cursor, |(i, _)| i);
        let prefix = &input[start..cursor];
        let definitions = self.definitions_text();
        let separator = if definitions.is_empty() { "" } else { "\n\n" };
        let header = self.header();
        let base = format!("{definitions}{separator}{header}{INDENT}pass\n");
        let first = Snapshot::analyze(base);
        let text = format!("{definitions}{separator}{header}{}\n", indent(input));
        let offset =
            definitions.len() + separator.len() + header.len() + body_offset(input, cursor);
        let snap = Snapshot::analyze_after(text, None, Some(&first));
        let mut seen = std::collections::HashSet::new();
        let list = snap
            .completions(offset)
            .into_iter()
            .filter(|c| c.label.starts_with(prefix) && !c.label.starts_with("__"))
            .filter(|c| seen.insert(c.label.clone()))
            .map(|c| Suggestion {
                label: c.label,
                detail: c.detail,
            })
            .collect();
        (start, list)
    }
}

// ---- helpers --------------------------------------------------------------------------------

/// Whether the expression, as the last of an input, is a value to show rather than a statement.
fn gives_value(e: &ast::Expr) -> bool {
    use ast::ExprKind as K;
    !matches!(
        e.node,
        K::Assign { .. }
            | K::While { .. }
            | K::For { .. }
            | K::Break
            | K::Continue
            | K::Return(_)
            | K::Block(_)
            | K::Error
            | K::If {
                otherwise: None,
                ..
            }
    )
}

/// Whether the expression is written with numbers only: `5`, `-2`, `[1, 2]`, `(1, 2.5)`, `1 + 2`.
fn is_plain_number(e: &ast::Expr) -> bool {
    use ast::ExprKind as K;
    match &e.node {
        K::Lit(ast::Lit::Int(_) | ast::Lit::Float(_)) => true,
        K::Unary { expr, .. } => is_plain_number(expr),
        K::Binary { lhs, rhs, .. } => is_plain_number(lhs) && is_plain_number(rhs),
        K::Array(items) | K::Tuple(items) => items.iter().all(is_plain_number),
        _ => false,
    }
}

/// The type with `i32` (what a number written alone is) as `Int`, which is what the functions
/// of a program ask for. Within one program the use of the number settles this; here each input
/// is checked alone, so a variable that holds a plain number is an `Int`.
fn widen(ty: &Ty) -> Ty {
    use telvax_hir::Prim;
    match ty {
        Ty::Prim(Prim::I32) => Ty::Prim(Prim::I64),
        Ty::Vec(t) => Ty::Vec(Box::new(widen(t))),
        Ty::Option(t) => Ty::Option(Box::new(widen(t))),
        Ty::Tuple(items) => Ty::Tuple(items.iter().map(widen).collect()),
        Ty::Map(a, b) => Ty::Map(Box::new(widen(a)), Box::new(widen(b))),
        other => other.clone(),
    }
}

/// Whether part of the type is not known (a closure parameter that nothing has fixed).
fn has_unknown(ty: &Ty) -> bool {
    match ty {
        Ty::Unknown | Ty::IntVar(_) | Ty::FloatVar(_) => matches!(ty, Ty::Unknown),
        Ty::Vec(t) | Ty::Option(t) | Ty::Task(t) => has_unknown(t),
        Ty::Map(a, b) | Ty::Result(a, b) => has_unknown(a) || has_unknown(b),
        Ty::Tuple(items) => items.iter().any(has_unknown),
        Ty::Fn(params, ret) => params.iter().any(has_unknown) || has_unknown(ret),
        Ty::Tensor { elem, .. } => has_unknown(elem),
        _ => false,
    }
}

/// The text with every line indented one level, empty lines left empty.
fn indent(text: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !line.trim().is_empty() {
            out.push_str(INDENT);
            out.push_str(line);
        }
    }
    out
}

/// Where byte `cursor` of the input is in the input indented by [`indent`].
fn body_offset(input: &str, cursor: usize) -> usize {
    let line_start = input[..cursor].rfind('\n').map_or(0, |i| i + 1);
    indent(&input[..line_start]).len() + INDENT.len() + (cursor - line_start)
}

/// Where in the input the byte `at` of the indented probe is.
fn probe_to_input(probe: &str, header_len: usize, input: &str, at: usize) -> Option<usize> {
    let at = at.checked_sub(header_len)?;
    let body = &probe[header_len..];
    let at = at.min(body.len());
    let line = body[..at].matches('\n').count();
    let column = at - body[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_start: usize = input
        .split('\n')
        .take(line)
        .map(|l| l.len() + 1)
        .sum::<usize>();
    Some((line_start + column.saturating_sub(INDENT.len())).min(input.len()))
}

fn starts_a_definition(input: &str) -> bool {
    let word = input
        .trim_start()
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '@')
        .find(|w| !w.is_empty())
        .unwrap_or("");
    matches!(
        word,
        "fn" | "async" | "type" | "enum" | "trait" | "extend" | "import" | "from" | "@"
    ) || input.trim_start().starts_with('@')
}

/// Where an item starts, counting its attributes.
fn item_start(item: &ast::Item) -> usize {
    match item {
        ast::Item::Fn(f) => f
            .attrs
            .first()
            .map_or(f.span.start, |a| a.span.start.min(f.span.start)),
        ast::Item::Struct(s) => s.span.start,
        ast::Item::Enum(e) => e.span.start,
        ast::Item::Alias(a) => a.span.start,
        ast::Item::Impl(i) => i.span.start,
        ast::Item::Trait(t) => t.span.start,
        ast::Item::Import(i) => i.span.start,
        ast::Item::Let(l) => l.span.start,
        ast::Item::Serve(s) => s.span.start,
    }
}

/// What a definition replaces when it is entered again.
fn key_of(item: &ast::Item, text: &str) -> String {
    match item {
        ast::Item::Fn(f) => format!("fn {}", f.name.node),
        ast::Item::Struct(s) => format!("type {}", s.name.node),
        ast::Item::Enum(e) => format!("type {}", e.name.node),
        ast::Item::Alias(a) => format!("type {}", a.name.node),
        ast::Item::Trait(t) => format!("type {}", t.name.node),
        ast::Item::Impl(i) => format!("extend {} {}", i.ty.node, text),
        ast::Item::Import(_) => format!("import {text}"),
        ast::Item::Let(l) => format!("let {}", l.name.node),
        ast::Item::Serve(_) => "serve".to_string(),
    }
}

/// Whether `name` is a word of `text`.
fn mentions(text: &str, name: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| w == name)
}

/// The errors among `diags`, each with the line it points at. `base` is where `source` starts in
/// the text that was checked; an error before it is in an earlier definition.
fn render_errors(diags: &[Diagnostic], source: &str, base: usize) -> String {
    render_all(diags, source, base, source.len())
}

fn render_all(diags: &[Diagnostic], source: &str, base: usize, _len: usize) -> String {
    let map = |o: usize| o.checked_sub(base).filter(|r| *r <= source.len());
    render_mapped(diags, source, &map)
}

/// The errors among `diags`; `map` turns a position in the checked text into one in `source`.
fn render_mapped(
    diags: &[Diagnostic],
    source: &str,
    map: &dyn Fn(usize) -> Option<usize>,
) -> String {
    let mut out = Vec::new();
    for d in diags.iter().filter(|d| d.is_error()) {
        let mut text = format!("error: {}", d.message);
        match (map(d.span.start), map(d.span.end)) {
            (Some(start), end) => {
                let end = end.unwrap_or(start).max(start);
                let line_start = source[..start.min(source.len())]
                    .rfind('\n')
                    .map_or(0, |i| i + 1);
                let line_end = source[line_start..]
                    .find('\n')
                    .map_or(source.len(), |i| line_start + i);
                let line = &source[line_start..line_end];
                let column = source[line_start..start.min(source.len())].chars().count();
                let width = source[start.min(line_end)..end.min(line_end)]
                    .chars()
                    .count()
                    .max(1);
                let number = source[..line_start].matches('\n').count() + 1;
                text.push_str(&format!(
                    "\n  {number} | {line}\n    {} | {}{}",
                    " ".repeat(number.to_string().len()),
                    " ".repeat(column),
                    "^".repeat(width)
                ));
            }
            _ => text = format!("error: in an earlier definition: {}", d.message),
        }
        for (_, label) in &d.labels {
            text.push_str(&format!("\n  note: {label}"));
        }
        for note in &d.notes {
            text.push_str(&format!("\n  note: {note}"));
        }
        out.push(text);
    }
    if out.is_empty() {
        out.push("error: that is not valid".into());
    }
    out.join("\n")
}
