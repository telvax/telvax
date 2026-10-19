//! Parser for Telvax: token stream -> AST.
//!
//! The surface syntax reads like Python (blocks by indentation, `elif`, `and` / `or` / `not`,
//! `from a.b import C`) with its own words: `fn`, `let` / `var`, `type`, `x => x + 1`, `Int?`. Everything is rewritten into the same tree the compiler
//! always used, so nothing after the parser knows about the surface: see `telvax_ast::tail` for the
//! rewrites that need more than one statement to decide (declarations and `return`).

use std::collections::HashSet;
use std::rc::Rc;

use chumsky::{
    input::{Stream, ValueInput},
    prelude::*,
};
use telvax_ast::*;
use telvax_ast::{Span, Spanned};
use telvax_diagnostics::Diagnostic;
use telvax_lexer::Token;

/// Names that start a dotted path (`util.clamp`) instead of a field access: `std`, the names
/// brought in by `import`, and anything that starts with a capital letter (`Shape.Circle`).
type Heads = Rc<Ctx>;

/// What the expression parser knows besides the tokens: the path heads, and the text the
/// tokens came from (a macro such as `assert!(x > 1)` quotes its argument).
struct Ctx {
    heads: HashSet<String>,
    src: String,
    base: usize,
}

impl std::ops::Deref for Ctx {
    type Target = HashSet<String>;
    fn deref(&self) -> &HashSet<String> {
        &self.heads
    }
}

macro_rules! run {
    ($parser:expr, $tokens:expr, $eoi:expr, $src:expr, $base:expr) => {{
        let iter = $tokens.into_iter().map(|(t, s)| (t, SimpleSpan::from(s)));
        let stream = Stream::from_iter(iter).map(SimpleSpan::from($eoi..$eoi), |(t, s)| (t, s));
        let (out, errs) = $parser.parse(stream).into_output_errors();
        let errs: Vec<Diagnostic> = errs
            .into_iter()
            .map(|e| friendly(&e, $src, $base))
            .collect();
        (out, errs)
    }};
}

/// Turns a parser error into a readable diagnostic: long lists of acceptable tokens collapse to
/// what was found, layout tokens are described in words, and the mistakes people bring from other
/// languages get a hint.
fn friendly(e: &Rich<'_, Token<'_>>, src: &str, base: usize) -> Diagnostic {
    use chumsky::error::RichPattern;
    let span = e.span().into_range();
    let expects = |t: Token<'_>| {
        e.expected()
            .any(|p| matches!(p, RichPattern::Token(x) if **x == t))
    };
    let rest = src.get(span.start.saturating_sub(base)..).unwrap_or("");
    match e.found() {
        Some(Token::Indent) => {
            return Diagnostic::error(span, "unexpected indentation")
                .with_note("the lines of a block start in the same column");
        }
        Some(Token::Dedent) => {
            return Diagnostic::error(span, "the block ends here, but something is missing");
        }
        Some(Token::LBrace) if expects(Token::Colon) => {
            return Diagnostic::error(span, "a block starts with `:` and is indented, not `{ }`")
                .with_note("write `:` at the end of the line and indent the lines below it");
        }
        Some(Token::RParen | Token::Comma) if expects(Token::Colon) => {
            return Diagnostic::error(span, "a parameter or a field needs a type")
                .with_note("write `name: Type`, for example `x: Int`");
        }
        Some(Token::Ident(_))
            if expects(Token::Newline) && after_colon_in_fn(src, span.start, base) =>
        {
            return Diagnostic::error(span, "a return type is written `-> Type`, not `: Type`")
                .with_note("for example `fn area(r: Float) -> Float:`");
        }
        Some(Token::Newline) if expects(Token::Colon) => {
            return Diagnostic::error(span, "expected `:` at the end of this line")
                .with_note("a line that opens a block ends with `:`");
        }
        Some(Token::Newline) if e.expected().count() > 5 => {
            return Diagnostic::error(
                span,
                "the line ends here, but the statement is not finished",
            );
        }
        Some(Token::Dot)
            if rest.starts_with("..") || src[..span.start.saturating_sub(base)].ends_with('.') =>
        {
            return Diagnostic::error(span, "`..` is not used in Telvax")
                .with_note("write a range of numbers as `range(start, end)`");
        }
        Some(Token::LParen) if expects(Token::Eq) => {
            return Diagnostic::error(span, "`let` and `var` take names, not a pattern")
                .with_note(
                    "to take a value apart, write `match value:` with a `case`, or `if value is Pattern:`",
                );
        }
        _ => {}
    }
    if expects(Token::Indent) {
        return Diagnostic::error(span, "expected an indented block here")
            .with_note("the lines after a line that ends with `:` are indented");
    }
    if e.expected().count() > 5
        && let Some(found) = e.found()
    {
        return Diagnostic::error(span, format!("unexpected `{found}`"));
    }
    Diagnostic::error(span, e.to_string())
}

/// Whether the error at `start` follows the `:` of a `fn` header (`fn f(x: Int): Int:`).
fn after_colon_in_fn(src: &str, start: usize, base: usize) -> bool {
    let before = &src[..start.saturating_sub(base).min(src.len())];
    let line = before.rsplit('\n').next().unwrap_or("");
    line.trim_start().starts_with("fn ") && line.trim_end().ends_with(':')
}

/// What a lexical error is about, from the text it covers.
fn lex_message(text: &str) -> Diagnostic {
    let span = 0..0;
    let (message, note) = if text.contains('\t') {
        (
            "tabs are not allowed in indentation",
            Some("indent with spaces (four per level)"),
        )
    } else if text.trim().is_empty() {
        (
            "this line is indented differently from the lines above it",
            Some("every line of a block starts in the same column"),
        )
    } else if text == ";" {
        ("`;` is not used in Telvax", Some("end the line instead"))
    } else if text == "&" {
        ("`&&` is not used in Telvax", Some("write `and`"))
    } else if text == "!" {
        ("`!` is not used in Telvax", Some("write `not`"))
    } else if text.starts_with(['"', '\'']) {
        (
            "this string is never closed",
            Some("add the closing quote on the same line"),
        )
    } else {
        ("unexpected character", None)
    };
    let d = Diagnostic::error(span, message);
    match note {
        Some(n) => d.with_note(n),
        None => d,
    }
}

/// The deepest nesting of blocks and of brackets the parser reads. Each level is a few frames of
/// recursion, so a text nested a million deep would overflow the stack and end the process
/// instead of reporting an error; nothing a person writes comes near these.
const MAX_BLOCK_DEPTH: usize = 64;
const MAX_BRACKET_DEPTH: usize = 200;

/// An error for a text nested more deeply than `MAX_BLOCK_DEPTH` / `MAX_BRACKET_DEPTH`.
fn nesting_error(tokens: &[(Token<'_>, std::ops::Range<usize>)]) -> Option<Diagnostic> {
    let (mut blocks, mut brackets) = (0usize, 0usize);
    for (token, span) in tokens {
        match token {
            Token::Indent => {
                blocks += 1;
                if blocks > MAX_BLOCK_DEPTH {
                    return Some(
                        Diagnostic::error(
                            span.clone(),
                            format!(
                                "blocks are nested too deeply (at most {MAX_BLOCK_DEPTH} levels)"
                            ),
                        )
                        .with_note("move the inner part into a function of its own"),
                    );
                }
            }
            Token::Dedent => blocks = blocks.saturating_sub(1),
            Token::LParen | Token::MacroOpen(_) | Token::LBracket | Token::LBrace => {
                brackets += 1;
                if brackets > MAX_BRACKET_DEPTH {
                    return Some(Diagnostic::error(
                        span.clone(),
                        format!(
                            "brackets are nested too deeply (at most {MAX_BRACKET_DEPTH} levels)"
                        ),
                    ));
                }
            }
            Token::RParen | Token::RBracket | Token::RBrace => {
                brackets = brackets.saturating_sub(1);
            }
            _ => {}
        }
    }
    None
}

/// Words from other languages that start a line, with what to write in Telvax.
fn foreign_word(word: &str) -> Option<&'static str> {
    Some(match word {
        "def" => "`def` is not used in Telvax: write `fn name(...)`",
        "class" => "`class` is not used in Telvax: write `type Name:`",
        "lambda" => "`lambda` is not used in Telvax: write `x => x + 1`",
        "mut" => "`mut` is not used in Telvax: write `var` for a variable that changes",
        "struct" => "`struct` is not used in Telvax: write `type Name:`",
        "impl" => {
            "`impl` is not used in Telvax: put the methods in the `type`, or use `extend Name:`"
        }
        "use" => "`use` is not used in Telvax: write `from a.b import C` or `import a.b.C`",
        "const" => "`const` is not used in Telvax: write `let` for a name that does not change",
        "function" | "func" | "fun" => "`function` is not used in Telvax: write `fn name(...)`",
        "pub" => {
            "`pub` is not used in Telvax: names that start with `_` are private, all others are public"
        }
        _ => return None,
    })
}

/// The names `import` and `from` bring in, which may start a dotted path: the last name of
/// `import a.b.C`, and the names after `import` in `from a.b import C, D`.
fn import_heads(tokens: &[(Token<'_>, std::ops::Range<usize>)]) -> HashSet<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Outside,
        /// `import a.b.C`
        Plain,
        /// `from a.b` before `import`
        FromPath,
        /// `from a.b import C, D`
        FromNames,
    }
    let mut heads = HashSet::from(["std".to_string()]);
    let mut mode = Mode::Outside;
    let mut last: Option<&str> = None;
    for (t, _) in tokens {
        match (t, mode) {
            (Token::Import, Mode::Outside) => mode = Mode::Plain,
            (Token::Import, Mode::FromPath) => mode = Mode::FromNames,
            (Token::From, Mode::Outside) => mode = Mode::FromPath,
            (Token::Ident(name), Mode::Plain) => last = Some(name),
            (Token::Ident(name), Mode::FromNames) => {
                heads.insert((*name).to_string());
            }
            (Token::Newline, _) => {
                if let (Mode::Plain, Some(name)) = (mode, last) {
                    heads.insert(name.to_string());
                }
                mode = Mode::Outside;
                last = None;
            }
            _ => {}
        }
    }
    heads
}

/// Lexes and parses `src`. Returns the program (when recovery allowed one) and all errors.
pub fn parse(src: &str) -> (Option<Program>, Vec<Diagnostic>) {
    parse_at(src, 0)
}

/// Like [`parse`], with every span shifted by `base`. Lets several files share one offset space,
/// so a single span identifies a place in a whole multi-file program.
pub fn parse_at(src: &str, base: usize) -> (Option<Program>, Vec<Diagnostic>) {
    telvax_ast::stack::with_big_stack(|| parse_here(src, base))
}

fn parse_here(src: &str, base: usize) -> (Option<Program>, Vec<Diagnostic>) {
    let (mut tokens, lex_errors) = telvax_lexer::tokenize(src);
    let heads: Heads = Rc::new(Ctx {
        heads: import_heads(&tokens),
        src: src.to_string(),
        base,
    });
    for (_, span) in &mut tokens {
        *span = span.start + base..span.end + base;
    }
    if let Some(error) = nesting_error(&tokens) {
        return (None, vec![error]);
    }
    let first_words: Vec<(String, std::ops::Range<usize>)> = {
        let mut out = Vec::new();
        let mut at_start = true;
        for (t, span) in &tokens {
            match t {
                Token::Newline | Token::Indent | Token::Dedent => at_start = true,
                Token::Ident(w) if at_start => {
                    out.push(((*w).to_string(), span.clone()));
                    at_start = false;
                }
                _ => at_start = false,
            }
        }
        out
    };
    let mut errors: Vec<Diagnostic> = lex_errors
        .into_iter()
        .map(|span| {
            let mut d = lex_message(&src[span.clone()]);
            d.span = span.start + base..span.end + base;
            d
        })
        .collect();
    let (program, parse_errors) = run!(program(heads), tokens, src.len() + base, src, base);
    errors.extend(parse_errors);
    if !errors.is_empty() {
        // Someone who writes `fn` or `let` at the start of a line needs to hear what to write.
        for (word, span) in first_words {
            if let Some(hint) = foreign_word(&word) {
                // The word's own message says more than "unexpected `def`".
                errors.retain(|e| e.span != span);
                errors.push(Diagnostic::error(span, hint));
            }
        }
    }
    errors.sort_by_key(|e| e.span.start);
    // After a mistake, the lines that hang from it are not worth reporting.
    let mut kept: Vec<Diagnostic> = Vec::new();
    for e in errors {
        let after_error = kept.last().is_some_and(|prev| {
            src[prev.span.end.saturating_sub(base)
                ..e.span
                    .start
                    .saturating_sub(base)
                    .max(prev.span.end.saturating_sub(base))]
                .matches('\n')
                .count()
                <= 1
        });
        if e.message == "unexpected indentation" && after_error {
            continue;
        }
        kept.push(e);
    }
    (program, kept)
}

/// The path `std.a.b` as an expression.
fn std_path(tail: &[&str], span: &Span) -> Expr {
    let mut path = vec![Ident {
        node: "std".into(),
        span: span.clone(),
    }];
    path.extend(tail.iter().map(|part| Ident {
        node: (*part).into(),
        span: span.clone(),
    }));
    Spanned {
        node: ExprKind::Path(path),
        span: span.clone(),
    }
}

/// The macros the compiler knows. Each one is rewritten into a call of a std function, so nothing
/// after the parser sees a macro.
fn expand_macro(name: &str, mut args: Vec<Expr>, span: Span, ctx: &Ctx) -> Result<Expr, String> {
    let text = |value: &str| Spanned {
        node: ExprKind::Lit(Lit::Str(value.to_string())),
        span: span.clone(),
    };
    // The source of an argument on one line, for the message that quotes it.
    let quote = |arg: &Expr| {
        let from = arg.span.start.saturating_sub(ctx.base);
        let to = arg.span.end.saturating_sub(ctx.base);
        let raw = ctx.src.get(from..to).unwrap_or("");
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let count = |low: usize, high: usize, usage: &str| {
        if (low..=high).contains(&args.len()) {
            Ok(())
        } else {
            Err(format!("`{name}!` is written `{usage}`"))
        }
    };
    let call = match name {
        "assert" => {
            count(
                1,
                2,
                "assert!(condition)` or `assert!(condition, \"message\")",
            )?;
            let message = if args.len() == 2 {
                args.pop().unwrap()
            } else {
                text(&format!("assertion failed: {}", quote(&args[0])))
            };
            std_call(&["test", "assert_msg"], vec![args.remove(0), message], span)
        }
        "dbg" => {
            count(1, 1, "dbg!(value)")?;
            let label = text(&quote(&args[0]));
            std_call(&["debug", "dbg"], vec![args.remove(0), label], span)
        }
        "todo" | "unreachable" | "panic" => {
            let usage = if name == "panic" {
                "panic!(\"message\")"
            } else {
                &format!("{name}!()")
            };
            let most = usize::from(name == "panic");
            count(most, most, usage)?;
            let message = args.pop().unwrap_or_else(|| {
                text(if name == "todo" {
                    "not yet implemented"
                } else {
                    "unreachable code was reached"
                })
            });
            std_call(&["test", "fail"], vec![message], span)
        }
        _ => {
            return Err(format!(
                "there is no macro `{name}!`; the macros are `assert!`, `dbg!`, `todo!`, `unreachable!` and `panic!`"
            ));
        }
    };
    Ok(call)
}

/// A call of a std function: `std.math.pow(a, b)`.
fn std_call(tail: &[&str], args: Vec<Expr>, span: Span) -> Expr {
    Spanned {
        node: ExprKind::Call {
            callee: Box::new(std_path(tail, &span)),
            args,
        },
        span,
    }
}

/// `await x`, with the `?`s after `x` applied to the value that comes out.
fn await_expr(operand: Expr, span: Span) -> Expr {
    let mut tries = 0;
    let mut core = operand;
    while let ExprKind::Try(inner) = core.node {
        core = *inner;
        tries += 1;
    }
    let mut out = Spanned {
        node: ExprKind::Await(Box::new(core)),
        span: span.clone(),
    };
    for _ in 0..tries {
        out = Spanned {
            node: ExprKind::Try(Box::new(out)),
            span: span.clone(),
        };
    }
    out
}

/// `base.name(args)`
fn method_call(base: Expr, name: &str, args: Vec<Expr>, span: Span) -> Expr {
    let callee = Spanned {
        node: ExprKind::Field {
            base: Box::new(base),
            name: Ident {
                node: name.into(),
                span: span.clone(),
            },
        },
        span: span.clone(),
    };
    Spanned {
        node: ExprKind::Call {
            callee: Box::new(callee),
            args,
        },
        span,
    }
}

/// `xs[a:b]` is `xs.slice(a, b)`; a missing start is 0 and a missing end is the length.
fn slice_call(base: Expr, lo: Option<Expr>, hi: Option<Expr>, span: Span) -> ExprKind {
    let bound = |value: Option<Expr>, default: u64| {
        value.unwrap_or(Spanned {
            node: ExprKind::Lit(Lit::Int(default)),
            span: span.clone(),
        })
    };
    let (lo, hi) = (bound(lo, 0), bound(hi, i64::MAX as u64));
    method_call(base, "slice", vec![lo, hi], span).node
}

/// `{k: v, ...}` is `std.map.from_entries([(k, v), ...])`, and `{}` is `std.map.new()`.
fn map_literal(pairs: Vec<(Expr, Expr)>, span: Span) -> ExprKind {
    if pairs.is_empty() {
        return std_call(&["map", "new"], Vec::new(), span).node;
    }
    let entries = pairs
        .into_iter()
        .map(|(k, v)| Spanned {
            span: k.span.start..v.span.end,
            node: ExprKind::Tuple(vec![k, v]),
        })
        .collect();
    let list = Spanned {
        node: ExprKind::Array(entries),
        span: span.clone(),
    };
    std_call(&["map", "from_entries"], vec![list], span).node
}

/// A closure from parsed parameters: a name, the names a tuple parameter is taken apart into, and
/// an optional type. The parts of a tuple parameter are bound at the top of the body.
fn closure_of(params: Vec<(Ident, Vec<Name>, Option<Type>)>, body: Expr, span: Span) -> Expr {
    let mut names = Vec::new();
    let mut types = Vec::new();
    let mut lets = Vec::new();
    for (name, parts, ty) in params {
        types.push(ty);
        if !parts.is_empty() {
            let at = name.span.clone();
            lets.extend(tuple_lets(parts, name.clone(), None, false, at));
        }
        names.push(name);
    }
    let body = if lets.is_empty() {
        body
    } else {
        Spanned {
            node: ExprKind::Block(Block {
                stmts: lets.into_iter().map(Stmt::Let).collect(),
                tail: Some(Box::new(body)),
                span: span.clone(),
            }),
            span: span.clone(),
        }
    };
    Spanned {
        node: ExprKind::Closure {
            params: names,
            types,
            body: Box::new(body),
        },
        span,
    }
}

/// Whether `name` appears anywhere in `expr`. It looks at the printed tree, so it can say yes for
/// a field or an argument with the same name; that only costs an extra binding.
fn mentions(expr: &Expr, name: &str) -> bool {
    format!("{expr:?}").contains(&format!("node: {name:?}"))
}

/// `[e for x in xs if c]` is `xs.filter(x => c).map(x => e)`.
fn comprehension_call(
    element: Expr,
    var: (Ident, Vec<Name>, Option<Type>),
    iterable: Expr,
    cond: Option<Expr>,
    span: Span,
) -> ExprKind {
    let mut source = iterable;
    if let Some(cond) = cond {
        // The test only binds the names it uses, so the others are not reported as unused.
        let (name, parts, ty) = var.clone();
        let parts = parts
            .into_iter()
            .map(|part| part.only_used_in(&cond))
            .collect::<Vec<_>>();
        let name = if parts.is_empty() && !mentions(&cond, &name.node) {
            Ident {
                node: format!("_{}", name.node),
                span: name.span,
            }
        } else {
            name
        };
        let keep = closure_of(vec![(name, parts, ty)], cond, span.clone());
        source = method_call(source, "filter", vec![keep], span.clone());
    }
    let make = closure_of(vec![var], element, span.clone());
    method_call(source, "map", vec![make], span).node
}

/// One of the operators that can sit in a comparison chain.
#[derive(Clone, Copy)]
enum Cmp {
    Op(BinOp),
    In,
    NotIn,
}

fn compare(op: Cmp, lhs: Expr, rhs: Expr, span: Span) -> Expr {
    let kind = match op {
        Cmp::Op(op) => ExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        Cmp::In | Cmp::NotIn => {
            // `a in b` is `b.contains(a)`.
            let contains = method_call(rhs, "contains", vec![lhs], span.clone());
            if matches!(op, Cmp::NotIn) {
                ExprKind::Unary {
                    op: UnOp::Not,
                    expr: Box::new(contains),
                }
            } else {
                contains.node
            }
        }
    };
    Spanned { node: kind, span }
}

/// `a < b` and chains such as `a < b < c`, which mean `a < b and b < c` with each operand
/// evaluated once: an operand that is not a name or a literal is bound to a temporary first.
fn comparison(first: Expr, rest: Vec<(Cmp, Expr)>, span: Span) -> Expr {
    if rest.len() <= 1 {
        return match rest.into_iter().next() {
            None => first,
            Some((op, rhs)) => compare(op, first, rhs, span),
        };
    }
    let mut ops = Vec::new();
    let mut operands = vec![first];
    for (op, operand) in rest {
        ops.push(op);
        operands.push(operand);
    }
    let mut lets = Vec::new();
    for (i, operand) in operands.iter_mut().enumerate() {
        let simple = matches!(&operand.node, ExprKind::Path(p) if p.len() == 1)
            || matches!(operand.node, ExprKind::Lit(_));
        if simple {
            continue;
        }
        let name = Ident {
            node: format!("__cmp{}_{i}", span.start),
            span: operand.span.clone(),
        };
        let at = operand.span.clone();
        let value = std::mem::replace(
            operand,
            Spanned {
                node: ExprKind::Path(vec![name.clone()]),
                span: at.clone(),
            },
        );
        lets.push(Stmt::Let(Let {
            mutable: false,
            name,
            ty: None,
            init: value,
            span: at,
        }));
    }
    let mut tests = ops.into_iter().enumerate().map(|(i, op)| {
        compare(
            op,
            operands[i].clone(),
            operands[i + 1].clone(),
            span.clone(),
        )
    });
    let mut all = tests.next().expect("a chain has two comparisons");
    for test in tests {
        all = Spanned {
            node: ExprKind::Binary {
                op: BinOp::And,
                lhs: Box::new(all),
                rhs: Box::new(test),
            },
            span: span.clone(),
        };
    }
    if lets.is_empty() {
        return all;
    }
    Spanned {
        node: ExprKind::Block(Block {
            stmts: lets,
            tail: Some(Box::new(all)),
            span: span.clone(),
        }),
        span,
    }
}

/// Parses a single expression (used for f-string interpolations and tests).
pub fn parse_expr(src: &str) -> (Option<Expr>, Vec<Diagnostic>) {
    let heads = Rc::new(Ctx {
        heads: HashSet::from(["std".to_string()]),
        src: src.to_string(),
        base: 0,
    });
    parse_expr_at(src, 0, &heads)
}

fn parse_expr_at(src: &str, offset: usize, heads: &Heads) -> (Option<Expr>, Vec<Diagnostic>) {
    let (mut tokens, lex_errors) = telvax_lexer::tokenize_flat(src);
    for (_, span) in &mut tokens {
        *span = span.start + offset..span.end + offset;
    }
    let heads = &Rc::new(Ctx {
        heads: heads.heads.clone(),
        src: src.to_string(),
        base: offset,
    });
    let mut errors: Vec<Diagnostic> = lex_errors
        .into_iter()
        .map(|s| Diagnostic::error(s.start + offset..s.end + offset, "invalid token"))
        .collect();
    let (expr, parse_errors) = run!(
        expr(heads.clone()).then_ignore(end()),
        tokens,
        src.len() + offset,
        src,
        offset
    );
    errors.extend(parse_errors);
    (expr, errors)
}

type Extra<'a, 's> = extra::Err<Rich<'a, Token<'s>, SimpleSpan>>;
fn rng(span: SimpleSpan) -> Span {
    span.into_range()
}

fn spanned<T>(node: T, span: SimpleSpan) -> Spanned<T> {
    Spanned {
        node,
        span: span.into_range(),
    }
}

fn ident<'a, 's: 'a, I>() -> impl Parser<'a, I, Ident, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    select! { Token::Ident(s) => s.to_string() }
        .map_with(|s, e| spanned(s, e.span()))
        .labelled("identifier")
}

/// `a.b.c`: a name with optional dotted parts (types, patterns, imports).
fn path<'a, 's: 'a, I>() -> impl Parser<'a, I, Vec<Ident>, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    ident()
        .separated_by(just(Token::Dot))
        .at_least(1)
        .collect::<Vec<_>>()
}

/// A bare name with this exact spelling (`None`, `self`).
fn word<'a, 's: 'a, I>(w: &'static str) -> impl Parser<'a, I, (), Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    select! { Token::Ident(s) if s == w => () }
}

/// `Tensor[f32, 2, 3]`: after the element type, the arguments are the dimensions.
fn tensor_shape(args: Vec<TypeArg>) -> Vec<TypeArg> {
    let mut out = Vec::new();
    let mut dims = Vec::new();
    for (i, a) in args.into_iter().enumerate() {
        match a {
            TypeArg::Shape(d) if i > 0 => dims.extend(d),
            TypeArg::Type(t) if i > 0 && t.path.len() == 1 && t.args.is_empty() => {
                dims.push(Dim::Named(t.path[0].clone()));
            }
            other => out.push(other),
        }
    }
    if !dims.is_empty() {
        out.push(TypeArg::Shape(dims));
    }
    out
}

fn type_<'a, 's: 'a, I>() -> impl Parser<'a, I, Type, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    recursive(|ty| {
        let dim = select! { Token::Int(n) => n }.try_map(|n, span| {
            n.replace('_', "")
                .parse::<u64>()
                .map(|v| TypeArg::Shape(vec![Dim::Lit(v)]))
                .map_err(|_| Rich::custom(span, "integer literal out of range"))
        });
        let row = ident()
            .then_ignore(just(Token::Colon))
            .then(ty.clone())
            .map(|(name, ty)| Field {
                name,
                ty,
                default: None,
            })
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(TypeArg::Row);
        let arg = ty.clone().map(TypeArg::Type).or(dim).or(row);
        let args = arg
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBracket), just(Token::RBracket));
        // `(Int, Text)` is a tuple, `(Int, Int) -> Int` a function, `()` nothing. The types in
        // parentheses are read once and what follows decides which, so nesting does not multiply
        // the work. A tuple type is spelled with a name no program can write.
        let paren_ty = ty
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
            .try_map(|(mut items, ret), span| {
                let span = rng(span);
                let (name, args) = match ret {
                    Some(ret) => {
                        items.push(ret);
                        ("(fn)", items)
                    }
                    None if items.is_empty() => ("(unit)", items),
                    None if items.len() >= 2 => ("(tuple)", items),
                    None => {
                        return Err(Rich::custom(
                            SimpleSpan::from(span),
                            "a type in parentheses is a function type, `(Int) -> Int`, or a tuple of two or more",
                        ));
                    }
                };
                Ok(Type {
                    path: vec![Ident {
                        node: name.into(),
                        span: span.clone(),
                    }],
                    args: args.into_iter().map(TypeArg::Type).collect(),
                    span,
                })
            });
        let named = path().then(args.or_not()).map_with(|(mut path, args), e| {
            if path.len() == 1 {
                let canon = telvax_ast::canonical_type_name(&path[0].node).to_string();
                path[0].node = canon;
            }
            let mut args = args.unwrap_or_default();
            if path.last().is_some_and(|p| p.node == "Tensor") {
                args = tensor_shape(args);
            }
            Type {
                path,
                args,
                span: rng(e.span()),
            }
        });
        // `T?` is `Option<T>`.
        paren_ty
            .or(named)
            .then(just(Token::Question).or_not())
            .map_with(|(base, optional), e| match optional {
                None => base,
                Some(_) => {
                    let span = rng(e.span());
                    Type {
                        path: vec![Ident {
                            node: "Option".into(),
                            span: span.clone(),
                        }],
                        args: vec![TypeArg::Type(base)],
                        span,
                    }
                }
            })
    })
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Splits the body of an f-string (without `f"` and `"`) into text and `{expr}` parts.
/// `offset` is the byte offset of `body` in the source file.
fn fstring_parts(body: &str, offset: usize, heads: &Heads) -> Result<Vec<FPart>, Diagnostic> {
    let err = |start: usize, end: usize, message: &str| {
        Diagnostic::error(offset + start..offset + end, message)
    };
    let mut parts = Vec::new();
    let mut text = String::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => {
                let next = body[i + 1..].chars().next().unwrap();
                text.push_str(&unescape(&body[i..i + 1 + next.len_utf8()]));
                i += 1 + next.len_utf8();
            }
            // `{{` and `}}` are the braces themselves.
            b'{' if bytes.get(i + 1) == Some(&b'{') => {
                text.push('{');
                i += 2;
            }
            b'}' if bytes.get(i + 1) == Some(&b'}') => {
                text.push('}');
                i += 2;
            }
            b'}' => {
                return Err(err(i, i + 1, "a `}` alone in an f-string").with_note(
                    "write `}}` for a brace in the text, or `{...}` around an expression",
                ));
            }
            b'{' => {
                let start = i + 1;
                let mut depth = 1;
                let mut j = start;
                while j < bytes.len() && depth > 0 {
                    match bytes[j] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                if depth != 0 {
                    return Err(err(i, bytes.len(), "unclosed `{` in f-string")
                        .with_note("write `{{` for a brace in the text"));
                }
                if !text.is_empty() {
                    parts.push(FPart::Text(std::mem::take(&mut text)));
                }
                let inner = &body[start..j - 1];
                if inner.trim().is_empty() {
                    return Err(err(i, j, "an f-string `{}` needs an expression inside")
                        .with_note("write `{{}}` for the two braces themselves"));
                }
                let (code, spec) = split_format_spec(inner);
                // `{x!r}` shows a `Text` in quotes; `{x!s}` is the plain form.
                let trimmed = code.trim_end();
                let (code, conversion) = if let Some(rest) = trimmed.strip_suffix("!r") {
                    (rest, Some(true))
                } else if let Some(rest) = trimmed.strip_suffix("!s") {
                    (rest, Some(false))
                } else {
                    (code, None)
                };
                // `{x=}` shows the source text, then the value (in quotes when it is a `Text`,
                // unless a conversion or a format says otherwise).
                let before_equals = code.trim_end();
                let debug = before_equals.len() > 1
                    && before_equals.ends_with('=')
                    && !before_equals[..before_equals.len() - 1].ends_with(['=', '!', '<', '>']);
                let expr_src = if debug {
                    &before_equals[..before_equals.len() - 1]
                } else {
                    code
                };
                if debug {
                    // The text as written, up to and including the `=` and the spaces after it.
                    parts.push(FPart::Text(inner[..code.len()].to_string()));
                }
                let repr = conversion.unwrap_or(debug && spec.is_none());
                match parse_expr_at(expr_src, offset + start, heads) {
                    (Some(e), errs) if errs.is_empty() => {
                        let spec = match spec {
                            Some(spec) => {
                                let spec_start = start + (inner.len() - spec.len());
                                Some(format_spec_expr(
                                    spec,
                                    offset + spec_start,
                                    e.span.clone(),
                                    heads,
                                )?)
                            }
                            None => None,
                        };
                        parts.push(if repr || spec.is_some() {
                            FPart::Fmt {
                                expr: e,
                                repr,
                                spec,
                            }
                        } else {
                            FPart::Expr(e)
                        });
                    }
                    (_, mut errs) => {
                        return Err(errs.drain(..).next().unwrap_or_else(|| {
                            err(start, j - 1, "invalid expression in f-string")
                        }));
                    }
                }
                i = j;
            }
            _ => {
                let c = body[i..].chars().next().unwrap();
                text.push(c);
                i += c.len_utf8();
            }
        }
    }
    if !text.is_empty() {
        parts.push(FPart::Text(text));
    }
    Ok(parts)
}

/// The format after `:` as a text: plain, or an f-string when it holds `{...}` (`>{width}`).
fn format_spec_expr(
    spec: &str,
    offset: usize,
    at: Span,
    heads: &Heads,
) -> Result<Expr, Diagnostic> {
    if !spec.contains('{') {
        return Ok(Spanned {
            node: ExprKind::Lit(Lit::Str(unescape(spec))),
            span: at,
        });
    }
    let parts = fstring_parts(spec, offset, heads)?;
    Ok(Spanned {
        node: ExprKind::FString(parts),
        span: at,
    })
}

/// Splits the inside of an f-string `{...}` at the first `:` that is not inside brackets or a
/// string: `x:.2f` is the code `x` and the format `.2f`.
fn split_format_spec(inner: &str) -> (&str, Option<&str>) {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (i, c) in inner.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth = depth.saturating_sub(1),
            (None, ':') if depth == 0 => return (&inner[..i], Some(&inner[i + 1..])),
            _ => {}
        }
    }
    (inner, None)
}

/// What one line of a block stands for.
#[derive(Clone)]
enum BlockItem {
    Let(Let),
    /// `a, b = value` becomes a temporary and one `let` per name.
    Lets(Vec<Let>),
    Expr(Expr),
    /// `pass`
    Nothing,
}

fn make_block(items: Vec<BlockItem>, span: SimpleSpan) -> Block {
    let mut stmts = Vec::new();
    for item in items {
        match item {
            BlockItem::Let(l) => stmts.push(Stmt::Let(l)),
            BlockItem::Lets(ls) => stmts.extend(ls.into_iter().map(Stmt::Let)),
            BlockItem::Expr(e) => stmts.push(Stmt::Expr(e)),
            BlockItem::Nothing => {}
        }
    }
    Block {
        stmts,
        tail: None,
        span: span.into_range(),
    }
}

fn unit_block(span: SimpleSpan) -> Block {
    Block {
        stmts: Vec::new(),
        tail: None,
        span: rng(span),
    }
}

fn value_block(value: Expr, span: SimpleSpan) -> Block {
    Block {
        stmts: Vec::new(),
        tail: Some(Box::new(value)),
        span: rng(span),
    }
}

/// `[T, U: Shape, V: (A, B)]`: the type parameters of a function, type or trait.
fn generics<'a, 's: 'a, I>() -> impl Parser<'a, I, Vec<GenericParam>, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    let bounds = ident()
        .separated_by(just(Token::Comma))
        .at_least(1)
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen))
        .or(ident().map(|i| vec![i]));
    ident()
        .then(just(Token::Colon).ignore_then(bounds).or_not())
        .map(|(name, bounds)| GenericParam {
            name,
            bounds: bounds.unwrap_or_default(),
        })
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .at_least(1)
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBracket), just(Token::RBracket))
        .or_not()
        .map(Option::unwrap_or_default)
}

/// A name for a part of a tuple: `a`, or `(b, c)` when the part is a tuple too (`_` skips one).
#[derive(Clone, Debug)]
enum Name {
    One(Ident),
    Nest(Vec<Name>),
}

impl Name {
    /// The same names with the ones `expr` does not mention replaced by `_`.
    fn only_used_in(self, expr: &Expr) -> Name {
        match self {
            Name::One(i) if i.node == "_" || mentions(expr, &i.node) => Name::One(i),
            Name::One(i) => Name::One(Ident {
                node: "_".into(),
                span: i.span,
            }),
            Name::Nest(parts) => {
                Name::Nest(parts.into_iter().map(|p| p.only_used_in(expr)).collect())
            }
        }
    }
}

/// `a` or `(b, c)`, nested as deep as needed.
fn name_tree<'a, 's: 'a, I>() -> impl Parser<'a, I, Name, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    recursive(|name| {
        name.separated_by(just(Token::Comma))
            .at_least(2)
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(Name::Nest)
            .or(ident().map(Name::One))
    })
}

/// `(a, b, _)` or `(a, (b, c))`: names for the parts of a tuple.
fn tuple_names<'a, 's: 'a, I>() -> impl Parser<'a, I, Vec<Name>, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    name_tree()
        .separated_by(just(Token::Comma))
        .at_least(2)
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen))
}

/// `a, b` or `a, (b, c)` without the outer parentheses.
fn bare_names<'a, 's: 'a, I>() -> impl Parser<'a, I, Vec<Name>, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    name_tree()
        .separated_by(just(Token::Comma))
        .at_least(2)
        .allow_trailing()
        .collect::<Vec<_>>()
}

/// The `let`s a tuple pattern stands for: a temporary holding the value, then `name = temp.N`. A
/// part that is a tuple too gets a temporary of its own.
fn tuple_lets(
    names: Vec<Name>,
    temp: Ident,
    init: Option<Expr>,
    mutable: bool,
    span: Span,
) -> Vec<Let> {
    let mut out = Vec::new();
    if let Some(init) = init {
        out.push(Let {
            mutable: false,
            name: temp.clone(),
            ty: None,
            init,
            span: span.clone(),
        });
    }
    for (i, name) in names.into_iter().enumerate() {
        let (target, at) = match &name {
            Name::One(n) if n.node == "_" => continue,
            Name::One(n) => (n.clone(), n.span.clone()),
            Name::Nest(parts) => {
                let first = parts.iter().find_map(|p| match p {
                    Name::One(n) => Some(n.span.clone()),
                    Name::Nest(_) => None,
                });
                let at = first.unwrap_or_else(|| span.clone());
                (
                    Ident {
                        node: format!("{}_{i}", temp.node),
                        span: at.clone(),
                    },
                    at,
                )
            }
        };
        let is_nest = matches!(name, Name::Nest(_));
        out.push(Let {
            mutable: mutable && !is_nest,
            name: target.clone(),
            ty: None,
            init: Spanned {
                node: ExprKind::Field {
                    base: Box::new(Spanned {
                        node: ExprKind::Path(vec![temp.clone()]),
                        span: at.clone(),
                    }),
                    name: Spanned {
                        node: i.to_string(),
                        span: at.clone(),
                    },
                },
                span: at,
            },
            span: span.clone(),
        });
        if let Name::Nest(parts) = name {
            out.extend(tuple_lets(parts, target, None, mutable, span.clone()));
        }
    }
    out
}

fn pattern<'a, 's: 'a, I>() -> impl Parser<'a, I, Pattern, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    /// One piece of a list pattern: a pattern, or `*name` for the items in between.
    enum ListItem {
        One(Pattern),
        Rest(Pattern),
    }
    /// One argument of `Name(...)`: a value, or `field: pattern`.
    enum Arg {
        Value(Pattern),
        Field(Ident, Pattern),
    }

    recursive(|pat| {
        let int = select! { Token::Int(n) => n }.try_map(|n, span| {
            n.replace('_', "")
                .parse::<u64>()
                .map(|v| PatKind::Lit(Lit::Int(v)))
                .map_err(|_| Rich::custom(span, "integer literal out of range"))
        });
        let float = select! { Token::Float(n) => n }.try_map(|n, span| {
            n.replace('_', "")
                .parse::<f64>()
                .map(|v| PatKind::Lit(Lit::Float(v)))
                .map_err(|_| Rich::custom(span, "invalid float literal"))
        });
        let negative = just(Token::Minus)
            .ignore_then(select! { Token::Int(n) => (n, false), Token::Float(n) => (n, true) })
            .try_map(|(n, is_float), span| {
                let n = n.replace('_', "");
                if is_float {
                    n.parse::<f64>()
                        .map(|v| PatKind::NegLit(Lit::Float(v)))
                        .map_err(|_| Rich::custom(span, "invalid float literal"))
                } else {
                    n.parse::<u64>()
                        .map(|v| PatKind::NegLit(Lit::Int(v)))
                        .map_err(|_| Rich::custom(span, "integer literal out of range"))
                }
            });
        let string = select! { Token::Str(s) => s }
            .map(|s| PatKind::Lit(Lit::Str(unescape(&s[1..s.len() - 1]))));
        let boolean = select! { Token::True => true, Token::False => false }
            .map(|b| PatKind::Lit(Lit::Bool(b)));
        let argument = ident()
            .then_ignore(just(Token::Colon))
            .then(pat.clone())
            .map(|(name, p)| Arg::Field(name, p))
            .or(pat.clone().map(Arg::Value));
        let variant = path()
            .then(
                argument
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .or_not(),
            )
            .validate(|(path, args), e, emitter| {
                let Some(args) = args else {
                    if path.len() == 1 {
                        let name = &path[0].node;
                        return if name == "_" {
                            PatKind::Wild
                        } else {
                            PatKind::Bind(name.clone())
                        };
                    }
                    return PatKind::Variant {
                        path,
                        args: Vec::new(),
                    };
                };
                let named = args.iter().filter(|a| matches!(a, Arg::Field(..))).count();
                if named == 0 {
                    let args = args
                        .into_iter()
                        .filter_map(|a| match a {
                            Arg::Value(p) => Some(p),
                            Arg::Field(..) => None,
                        })
                        .collect();
                    PatKind::Variant { path, args }
                } else if named == args.len() {
                    let fields = args
                        .into_iter()
                        .filter_map(|a| match a {
                            Arg::Field(n, p) => Some((n, p)),
                            Arg::Value(_) => None,
                        })
                        .collect();
                    PatKind::Struct { path, fields }
                } else {
                    emitter.emit(Rich::custom(
                        e.span(),
                        "a pattern lists the values or names the fields (`Point(x: a, y: b)`), not both",
                    ));
                    PatKind::Wild
                }
            });
        let tuple = pat
            .clone()
            .separated_by(just(Token::Comma))
            .at_least(2)
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(PatKind::Tuple);
        // `[a, b]`, `[first, *rest]`, `[*_, last]`
        let list_item = just(Token::Star)
            .ignore_then(ident())
            .map_with(|name, e| {
                let kind = if name.node == "_" {
                    PatKind::Wild
                } else {
                    PatKind::Bind(name.node)
                };
                ListItem::Rest(spanned(kind, e.span()))
            })
            .or(pat.clone().map(ListItem::One));
        let list = list_item
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .validate(|items, e, emitter| {
                let (mut before, mut after, mut rest) = (Vec::new(), Vec::new(), None);
                for item in items {
                    match item {
                        ListItem::One(p) if rest.is_none() => before.push(p),
                        ListItem::One(p) => after.push(p),
                        ListItem::Rest(p) if rest.is_none() => rest = Some(Box::new(p)),
                        ListItem::Rest(_) => {
                            emitter.emit(Rich::custom(
                                e.span(),
                                "a list pattern has one `*rest` part",
                            ));
                        }
                    }
                }
                PatKind::List {
                    before,
                    rest,
                    after,
                }
            });
        let atom = choice((negative, int, float, string, boolean, tuple, list, variant))
            .map_with(|k, e| spanned(k, e.span()));
        // `1 | 2 | 3`
        atom.separated_by(just(Token::Bar))
            .at_least(1)
            .collect::<Vec<_>>()
            .map_with(|mut alts, e| {
                if alts.len() == 1 {
                    alts.remove(0)
                } else {
                    spanned(PatKind::Or(alts), e.span())
                }
            })
    })
}

/// What follows `case` when it is not a pattern: the tokens up to the `:` are skipped and the
/// mistake is named, so the error says what to write instead of pointing at the indentation.
fn bad_pattern<'a, 's: 'a, I>() -> impl Parser<'a, I, Pattern, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    any()
        .filter(|t: &Token<'s>| !matches!(t, Token::Newline | Token::Indent | Token::Dedent))
        .and_is(just(Token::Colon).then(just(Token::Newline)).not())
        .repeated()
        .at_least(1)
        .collect::<Vec<_>>()
        .validate(|tokens, e, emitter| {
            let span = e.span();
            let has = |t: Token<'s>| tokens.contains(&t);
            let dots = tokens.iter().filter(|t| **t == Token::Dot).count();
            let message = if dots >= 2 && has(Token::LBracket) {
                "the rest of a list is `*rest`: write `[first, *rest]`"
            } else if dots >= 2 {
                "a range is not a pattern; write `case n if 1 <= n < 3:`"
            } else if has(Token::At) || tokens.iter().any(|t| matches!(t, Token::Ident("as"))) {
                "`name @ pattern` is not a pattern; write `case name if <test on name>:`"
            } else if has(Token::Is) || has(Token::And) || has(Token::Or) || has(Token::Not) {
                "this is not a pattern; a condition goes after `if`: `case n if n > 3:`"
            } else {
                "this is not a pattern; a pattern is a literal, a name, `_`, a tuple `(a, b)`, a list `[a, *rest]`, a variant `Shape.Circle(r)` or a type `Point(x: a)`"
            };
            // Keep the first name, so the body does not report it as missing too.
            let first_name = tokens.iter().find_map(|t| match t {
                Token::Ident(n) if n.starts_with(|c: char| c.is_lowercase()) => Some((*n).to_string()),
                _ => None,
            });
            emitter.emit(Rich::custom(span, message));
            spanned(first_name.map_or(PatKind::Wild, PatKind::Bind), span)
        })
}

fn is_head(name: &str, heads: &HashSet<String>) -> bool {
    heads.contains(name) || name.starts_with(|c: char| c.is_uppercase())
}

fn expr<'a, 's: 'a, I>(heads: Heads) -> impl Parser<'a, I, Expr, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    recursive(move |expr| {
        let int = select! { Token::Int(n) => n }.try_map(|n, span| {
            n.replace('_', "")
                .parse::<u64>()
                .map(|v| ExprKind::Lit(Lit::Int(v)))
                .map_err(|_| Rich::custom(span, "integer literal out of range"))
        });
        let float = select! { Token::Float(n) => n }.try_map(|n, span| {
            n.replace('_', "")
                .parse::<f64>()
                .map(|v| ExprKind::Lit(Lit::Float(v)))
                .map_err(|_| Rich::custom(span, "invalid float literal"))
        });
        let string = select! { Token::Str(s) => s }
            .map(|s| ExprKind::Lit(Lit::Str(unescape(&s[1..s.len() - 1]))));
        let triple = select! { Token::TStr(s) => s }
            .map(|s| ExprKind::Lit(Lit::Str(unescape(&s[3..s.len() - 3]))));
        // `r"a\nb"` keeps its backslashes.
        let raw = select! { Token::RStr(s) => s }
            .map(|s| ExprKind::Lit(Lit::Str(s[2..s.len() - 1].to_string())));
        let boolean = select! { Token::True => true, Token::False => false }
            .map(|b| ExprKind::Lit(Lit::Bool(b)));
        let fstring_heads = heads.clone();
        let fstring = select! { Token::FStr(s) => (s, 2, 1), Token::FTStr(s) => (s, 4, 3) }
            .map_with(|(s, open, close), e| (s, open, close, rng(e.span()).start))
            .try_map(move |(s, open, close, start), _| {
                // Strip `f"` (or `f"""`) and the closing quote(s).
                fstring_parts(&s[open..s.len() - close], start + open, &fstring_heads)
                    .map(ExprKind::FString)
                    .map_err(|e| Rich::custom(SimpleSpan::from(e.span), e.message))
            });

        // `(a, b)` is a tuple; `(a)` is just `a` in parentheses. The first expression is read once
        // and what follows it decides which, so nesting does not multiply the work.
        let paren = expr
            .clone()
            .then(
                just(Token::Comma)
                    .ignore_then(expr.clone())
                    .repeated()
                    .at_least(1)
                    .collect::<Vec<_>>()
                    .then_ignore(just(Token::Comma).or_not())
                    .or_not(),
            )
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|(first, rest): (Expr, Option<Vec<Expr>>)| match rest {
                None => first.node,
                Some(rest) => {
                    let mut items = vec![first];
                    items.extend(rest);
                    ExprKind::Tuple(items)
                }
            })
            .recover_with(via_parser(nested_delimiters(
                Token::LParen,
                Token::RParen,
                [
                    (Token::LBrace, Token::RBrace),
                    (Token::LBracket, Token::RBracket),
                ],
                |_| ExprKind::Error,
            )));

        // `[a, b]` is a list and `[e for x in xs if c]` a comprehension, so what follows the first
        // expression decides which.
        let comp_var = tuple_names()
            .or(bare_names())
            .map_with(|names, e| {
                let at = rng(e.span());
                (
                    Ident {
                        node: format!("__arg{}", at.start),
                        span: at,
                    },
                    names,
                    None,
                )
            })
            .or(ident().map(|v| (v, Vec::new(), None)));
        let comprehension = just(Token::For)
            .ignore_then(comp_var)
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .then(just(Token::If).ignore_then(expr.clone()).or_not());
        let array = expr
            .clone()
            .then(
                comprehension
                    .map(Ok)
                    .or(just(Token::Comma)
                        .ignore_then(expr.clone())
                        .repeated()
                        .collect::<Vec<_>>()
                        .then_ignore(just(Token::Comma).or_not())
                        .map(Err))
                    .or_not(),
            )
            .or_not()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map_with(|items, e| match items {
                None => ExprKind::Array(Vec::new()),
                Some((first, None)) => ExprKind::Array(vec![first]),
                Some((first, Some(Err(rest)))) => {
                    let mut all = vec![first];
                    all.extend(rest);
                    ExprKind::Array(all)
                }
                Some((element, Some(Ok(((var, iterable), cond))))) => {
                    comprehension_call(element, var, iterable, cond, rng(e.span()))
                }
            });

        // `{k: v, ...}` builds a `Map`; `{}` is an empty one.
        let map_lit = expr
            .clone()
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map_with(|pairs, e| map_literal(pairs, rng(e.span())));

        let macro_ctx = heads.clone();
        let macro_call = select! { Token::MacroOpen(n) => n }
            .then(
                expr.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(Token::RParen))
            .try_map_with(move |(open, args), e| {
                let name = &open[..open.len() - 2];
                expand_macro(name, args, rng(e.span()), &macro_ctx)
                    .map(|expr| expr.node)
                    .map_err(|message| Rich::custom(e.span(), message))
            });
        let primary = choice((
            macro_call,
            float,
            int,
            triple,
            raw,
            string,
            fstring,
            boolean,
            ident().map(|i| ExprKind::Path(vec![i])),
            paren,
            array,
            map_lit,
        ))
        .map_with(|k, e| spanned(k, e.span()));

        #[derive(Clone)]
        enum Postfix {
            Call(Vec<Expr>),
            Field(Ident),
            Index(Expr),
            Slice(Option<Expr>, Option<Expr>),
            Try,
            Parts(Vec<Ident>),
        }
        // An argument is `value` or `name: value`.
        let argument = ident()
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .map_with(|(name, value), e| {
                spanned(
                    ExprKind::Named {
                        name,
                        value: Box::new(value),
                    },
                    e.span(),
                )
            })
            .or(expr.clone());
        let call = argument
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(Postfix::Call)
            .recover_with(via_parser(nested_delimiters(
                Token::LParen,
                Token::RParen,
                [
                    (Token::LBrace, Token::RBrace),
                    (Token::LBracket, Token::RBracket),
                ],
                |_| Postfix::Call(Vec::new()),
            )));
        // `pair.1` lexes as `.` and an integer, and `pair.0.1` as `.` and the float `0.1`.
        let numeric_field =
            select! { Token::Int(n) => n.to_string(), Token::Float(n) => n.to_string() }.map_with(
                |text, e| {
                    let at = rng(e.span());
                    Postfix::Parts(
                        text.split('.')
                            .map(|part| Ident {
                                node: part.to_string(),
                                span: at.clone(),
                            })
                            .collect(),
                    )
                },
            );
        let field = just(Token::Dot).ignore_then(ident().map(Postfix::Field).or(numeric_field));
        let try_ = just(Token::Question).to(Postfix::Try);
        // `xs[i]`; with commas, `f[Int, Text]` names the types a generic function is called with
        // and the index is the tuple of them.
        // `xs[a:b]`, `xs[a:]`, `xs[:b]` and `xs[:]` are slices; what follows the first expression
        // (or its absence) decides which.
        let index = expr
            .clone()
            .or_not()
            .then(
                just(Token::Colon)
                    .ignore_then(expr.clone().or_not())
                    .map(Ok)
                    .or(just(Token::Comma)
                        .ignore_then(expr.clone())
                        .repeated()
                        .collect::<Vec<_>>()
                        .map(Err)),
            )
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .try_map(|(first, rest), span| match (first, rest) {
                (lo, Ok(hi)) => Ok(Postfix::Slice(lo, hi)),
                (Some(first), Err(more)) => {
                    let mut items = vec![first];
                    items.extend(more);
                    Ok(Postfix::Index(if items.len() == 1 {
                        items.remove(0)
                    } else {
                        spanned(ExprKind::Tuple(items), span)
                    }))
                }
                (None, Err(_)) => Err(Rich::custom(span, "expected an index or a slice here")),
            });
        let path_heads = heads.clone();
        let postfix = primary.foldl_with(
            call.or(field).or(index).or(try_).repeated(),
            move |base, op, e| {
                let kind = match op {
                    Postfix::Index(index) => ExprKind::Index {
                        base: Box::new(base),
                        index: Box::new(index),
                    },
                    Postfix::Slice(lo, hi) => slice_call(base, lo, hi, rng(e.span())),
                    Postfix::Call(args) => {
                        // `Point(x: 1, y: 2)`: a capitalized name with every argument named builds
                        // a value of that type.
                        let builds = !args.is_empty()
                            && args
                                .iter()
                                .all(|a| matches!(a.node, ExprKind::Named { .. }))
                            && matches!(&base.node, ExprKind::Path(p)
                                if p.last().is_some_and(|i| i.node.starts_with(|c: char| c.is_uppercase())));
                        if builds {
                            let ExprKind::Path(path) = base.node else {
                                unreachable!()
                            };
                            let fields = args
                                .into_iter()
                                .filter_map(|a| match a.node {
                                    ExprKind::Named { name, value } => Some((name, *value)),
                                    _ => None,
                                })
                                .collect();
                            ExprKind::StructLit { path, fields }
                        } else {
                            ExprKind::Call {
                                callee: Box::new(base),
                                args,
                            }
                        }
                    }
                    Postfix::Try => ExprKind::Try(Box::new(base)),
                    Postfix::Parts(names) => {
                        let mut current = base;
                        for name in names {
                            current = spanned(
                                ExprKind::Field {
                                    base: Box::new(current),
                                    name,
                                },
                                e.span(),
                            );
                        }
                        current.node
                    }
                    Postfix::Field(name) => match base.node {
                        // `std.text.trim`, `Shape.Circle`, `util.clamp`: a path, not a field.
                        ExprKind::Path(mut path) if is_head(&path[0].node, &path_heads) => {
                            path.push(name);
                            ExprKind::Path(path)
                        }
                        node => ExprKind::Field {
                            base: Box::new(spanned(node, SimpleSpan::from(base.span))),
                            name,
                        },
                    },
                };
                spanned(kind, e.span())
            },
        );

        // `-x` and `await task`. A `?` after the awaited call is for the value it gives:
        // `await fetch(1)?` is `(await fetch(1))?`.
        let negated = |operand: Boxed<'a, 'a, I, Expr, Extra<'a, 's>>| {
            select! { Token::Minus => None, Token::Await => Some(()) }
                .repeated()
                .foldr_with(operand, |op, rhs, e| match op {
                    None => spanned(
                        ExprKind::Unary {
                            op: UnOp::Neg,
                            expr: Box::new(rhs),
                        },
                        e.span(),
                    ),
                    Some(()) => await_expr(rhs, rng(e.span())),
                })
        };
        // `a ** b` is `std.math.pow(a, b)`. It binds tighter than a minus on its left
        // (`-2 ** 2` is `-4`), allows one on its right (`2 ** -1`) and groups to the right.
        let postfix = postfix.boxed();
        let exponent = negated(postfix.clone());
        let power = postfix
            .then(
                just(Token::StarStar)
                    .ignore_then(exponent)
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map_with(|(first, rest), e| {
                let mut operands = vec![first];
                operands.extend(rest);
                let mut acc = operands.pop().expect("one operand");
                while let Some(base) = operands.pop() {
                    acc = std_call(&["math", "pow"], vec![base, acc], rng(e.span()));
                }
                acc
            })
            .boxed();
        let unary = negated(power);

        let binary = |lower: Boxed<'a, 'a, I, Expr, Extra<'a, 's>>,
                      ops: Boxed<'a, 'a, I, BinOp, Extra<'a, 's>>| {
            lower
                .clone()
                .foldl_with(ops.then(lower).repeated(), |lhs, (op, rhs), e| {
                    spanned(
                        ExprKind::Binary {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        e.span(),
                    )
                })
                .boxed()
        };

        // `a // b` is `std.math.floor_div(a, b)`.
        let mul = unary
            .clone()
            .foldl_with(
                select! {
                    Token::Star => Some(BinOp::Mul), Token::Slash => Some(BinOp::Div),
                    Token::Percent => Some(BinOp::Rem), Token::SlashSlash => None,
                }
                .then(unary)
                .repeated(),
                |lhs, (op, rhs), e| match op {
                    Some(op) => spanned(
                        ExprKind::Binary {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        e.span(),
                    ),
                    None => std_call(&["math", "floor_div"], vec![lhs, rhs], rng(e.span())),
                },
            )
            .boxed();
        let add = binary(
            mul,
            select! { Token::Plus => BinOp::Add, Token::Minus => BinOp::Sub }.boxed(),
        );

        // `<`, `<=`, `>`, `>=`, and the membership tests `in` / `not in`. A chain
        // (`a < b < c`) means `a < b and b < c`, with each operand evaluated once.
        let cmp_op = select! {
            Token::Lt => Cmp::Op(BinOp::Lt), Token::LtEq => Cmp::Op(BinOp::LtEq),
            Token::Gt => Cmp::Op(BinOp::Gt), Token::GtEq => Cmp::Op(BinOp::GtEq),
            Token::In => Cmp::In,
        }
        .or(just(Token::Not).ignore_then(just(Token::In)).to(Cmp::NotIn));
        let cmp = add
            .clone()
            .then(cmp_op.then(add).repeated().collect::<Vec<_>>())
            .map_with(|(first, rest), e| comparison(first, rest, rng(e.span())))
            .boxed();
        let eq = binary(
            cmp,
            select! { Token::EqEq => BinOp::Eq, Token::NotEq => BinOp::NotEq }.boxed(),
        );
        let not = just(Token::Not)
            .repeated()
            .foldr_with(eq, |_, rhs, e| {
                spanned(
                    ExprKind::Unary {
                        op: UnOp::Not,
                        expr: Box::new(rhs),
                    },
                    e.span(),
                )
            })
            .boxed();
        let and = binary(not, select! { Token::And => BinOp::And }.boxed());
        let or = binary(and, select! { Token::Or => BinOp::Or }.boxed());

        // `a ?? b` is `a.unwrap_or_else(() => b)`: `b` is only evaluated when `a` is `None`.
        let coalesce = or
            .clone()
            .foldl_with(
                just(Token::QuestionQuestion).ignore_then(or).repeated(),
                |lhs, rhs, e| {
                    let at = rng(e.span());
                    let fallback = closure_of(Vec::new(), rhs, at.clone());
                    method_call(lhs, "unwrap_or_else", vec![fallback], at)
                },
            )
            .boxed();
        let pipe = coalesce
            .clone()
            .foldl_with(
                just(Token::Pipe).ignore_then(coalesce).repeated(),
                |lhs, rhs, e| {
                    spanned(
                        ExprKind::Pipe {
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        e.span(),
                    )
                },
            )
            .boxed();

        // `a if cond else b`
        let ternary = pipe
            .clone()
            .then(
                just(Token::If)
                    .ignore_then(pipe)
                    .then_ignore(just(Token::Else))
                    .then(expr.clone())
                    .or_not(),
            )
            .map_with(|(then, rest), e| match rest {
                None => then,
                Some((cond, otherwise)) => {
                    let (a, b) = (then.span.clone(), otherwise.span.clone());
                    spanned(
                        ExprKind::If {
                            cond: Box::new(cond),
                            then: value_block(then, SimpleSpan::from(a)),
                            otherwise: Some(Box::new(spanned(
                                ExprKind::Block(value_block(
                                    otherwise,
                                    SimpleSpan::from(b.clone()),
                                )),
                                SimpleSpan::from(b),
                            ))),
                        },
                        e.span(),
                    )
                }
            });

        // `x => x + 1`, `(a, b) => a + b`, `() => 1`; a parameter may also be a tuple of names:
        // `((i, s)) => ..`.
        // A name may carry its type: `(x: Int) => x + 1`.
        let arrow_param = ident()
            .then(just(Token::Colon).ignore_then(type_()).or_not())
            .map(|(name, ty)| (name, Vec::new(), ty))
            .or(tuple_names().map_with(|names, e| {
                let at = rng(e.span());
                (
                    Ident {
                        node: format!("__arg{}", at.start),
                        span: at,
                    },
                    names,
                    None,
                )
            }));
        let arrow_params = arrow_param
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .or(arrow_param.map(|p| vec![p]));
        let lambda = arrow_params
            .then_ignore(just(Token::FatArrow))
            .then(expr.clone())
            .map_with(|(params, body), e| closure_of(params, body, rng(e.span())));

        lambda.or(ternary)
    })
}

/// `if PATTERN` -> a `match` with a catch-all arm, as `if x is Some(v):` needs.
fn if_let(
    pat: Pattern,
    guard: Option<Expr>,
    scrutinee: Expr,
    then: Block,
    otherwise: Option<Expr>,
    span: SimpleSpan,
) -> Expr {
    let then_span = then.span.clone();
    spanned(
        ExprKind::Match {
            scrutinee: Box::new(scrutinee),
            arms: vec![
                Arm {
                    pat,
                    guard,
                    body: Spanned {
                        node: ExprKind::Block(then),
                        span: then_span,
                    },
                },
                Arm {
                    pat: spanned(PatKind::Wild, span),
                    guard: None,
                    body: otherwise
                        .unwrap_or_else(|| spanned(ExprKind::Block(unit_block(span)), span)),
                },
            ],
        },
        span,
    )
}

/// `while x is Some(v):` loops until the pattern stops matching.
fn while_let(
    pat: Pattern,
    guard: Option<Expr>,
    scrutinee: Expr,
    body: Block,
    span: SimpleSpan,
) -> Expr {
    let leave = Block {
        stmts: vec![Stmt::Expr(spanned(ExprKind::Break, span))],
        tail: None,
        span: rng(span),
    };
    let body_span = body.span.clone();
    let matched = spanned(
        ExprKind::Match {
            scrutinee: Box::new(scrutinee),
            arms: vec![
                Arm {
                    pat,
                    guard,
                    body: Spanned {
                        node: ExprKind::Block(body),
                        span: body_span,
                    },
                },
                Arm {
                    pat: spanned(PatKind::Wild, span),
                    guard: None,
                    body: spanned(ExprKind::Block(leave), span),
                },
            ],
        },
        span,
    );
    spanned(
        ExprKind::While {
            cond: Box::new(spanned(ExprKind::Lit(Lit::Bool(true)), span)),
            body: Block {
                stmts: Vec::new(),
                tail: Some(Box::new(matched)),
                span: rng(span),
            },
        },
        span,
    )
}

/// `: NEWLINE INDENT statements DEDENT`
fn block_of<'a, 's: 'a, I>(
    stmt: impl Parser<'a, I, BlockItem, Extra<'a, 's>> + Clone,
) -> impl Parser<'a, I, Block, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan>,
{
    just(Token::Colon)
        .ignore_then(just(Token::Newline))
        .ignore_then(just(Token::Indent))
        .ignore_then(stmt.repeated().at_least(1).collect::<Vec<_>>())
        .then_ignore(just(Token::Dedent))
        .map_with(|items, e| make_block(items, e.span()))
}

/// One statement of a block. Simple statements end with a line break; compound ones end with
/// their own block.
fn statement<'a, 's: 'a, I>(
    expr: impl Parser<'a, I, Expr, Extra<'a, 's>> + Clone + 'a,
) -> impl Parser<'a, I, BlockItem, Extra<'a, 's>> + Clone
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan> + 'a,
{
    recursive(move |stmt| {
        let block = block_of(stmt.clone());
        let pattern = pattern();

        // The test of an `if`, `elif` or `while`: an expression, or `expr is PATTERN`, which may be
        // followed by `and condition` (the condition can use the names the pattern binds).
        let test = expr.clone().then(
            just(Token::Is)
                .ignore_then(
                    pattern
                        .clone()
                        .then(just(Token::And).ignore_then(expr.clone()).or_not()),
                )
                .or_not(),
        );

        let else_block = just(Token::Else).ignore_then(block.clone());
        // `if` / `elif` chains are built from the last branch backwards.
        let branch = |kw: Token<'s>| {
            just(kw)
                .ignore_then(test.clone())
                .then(block.clone())
                .map_with(|((cond, pat), then), e| (cond, pat, then, e.span()))
        };
        let if_ = branch(Token::If)
            .then(branch(Token::Elif).repeated().collect::<Vec<_>>())
            .then(else_block.or_not())
            .map_with(|((first, rest), last), e| {
                let total = e.span();
                let mut branches = vec![first];
                branches.extend(rest);
                let mut tail: Option<Expr> = last.map(|b| {
                    let span = SimpleSpan::from(b.span.clone());
                    spanned(ExprKind::Block(b), span)
                });
                while let Some((cond, pat, then, _)) = branches.pop() {
                    let span = SimpleSpan::from(
                        total.start..tail.as_ref().map_or(then.span.end, |t| t.span.end),
                    );
                    tail = Some(match pat {
                        Some((pat, guard)) => if_let(pat, guard, cond, then, tail.take(), span),
                        None => spanned(
                            ExprKind::If {
                                cond: Box::new(cond),
                                then,
                                otherwise: tail.take().map(Box::new),
                            },
                            span,
                        ),
                    });
                }
                BlockItem::Expr(tail.expect("an `if` has at least one branch"))
            });

        let while_ = just(Token::While)
            .ignore_then(test)
            .then(block.clone())
            .map_with(|((cond, pat), body), e| {
                BlockItem::Expr(match pat {
                    Some((pat, guard)) => while_let(pat, guard, cond, body, e.span()),
                    None => spanned(
                        ExprKind::While {
                            cond: Box::new(cond),
                            body,
                        },
                        e.span(),
                    ),
                })
            });

        // `for a, b in items` loops over a temporary and binds the parts at the top of the body.
        let for_var = tuple_names()
            .or(bare_names())
            .map_with(|names, e| {
                let at = rng(e.span());
                (
                    Ident {
                        node: format!("__item{}", at.start),
                        span: at,
                    },
                    names,
                )
            })
            .or(ident().map(|v| (v, Vec::new())));
        let for_ = just(Token::For)
            .ignore_then(for_var)
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .then(block.clone())
            .map_with(|(((var, names), iter), mut body), e| {
                if !names.is_empty() {
                    let lets = tuple_lets(names, var.clone(), None, false, var.span.clone());
                    let mut stmts: Vec<Stmt> = lets.into_iter().map(Stmt::Let).collect();
                    stmts.append(&mut body.stmts);
                    body.stmts = stmts;
                }
                BlockItem::Expr(spanned(
                    ExprKind::For {
                        var,
                        iter: for_iter(iter),
                        body,
                    },
                    e.span(),
                ))
            });

        let case = just(Token::Case)
            .ignore_then(
                pattern
                    .then_ignore(just(Token::If).or(just(Token::Colon)).rewind())
                    .or(bad_pattern()),
            )
            .then(just(Token::If).ignore_then(expr.clone()).or_not())
            .then(block.clone())
            .map_with(|((pat, guard), body), e| Arm {
                pat,
                guard,
                body: spanned(ExprKind::Block(body), e.span()),
            });
        // `match x:` and its `case` lines. As a statement it is the last thing of its block; the same
        // text is also the value in `let v = match ...`, `v = match ...` and `return match ...`.
        let match_value = just(Token::Match)
            .ignore_then(expr.clone())
            .then_ignore(just(Token::Colon))
            .then_ignore(just(Token::Newline))
            .then_ignore(just(Token::Indent))
            .then(case.repeated().at_least(1).collect::<Vec<_>>())
            .then_ignore(just(Token::Dedent))
            .map_with(|(scrutinee, arms), e| {
                spanned(
                    ExprKind::Match {
                        scrutinee: Box::new(scrutinee),
                        arms,
                    },
                    e.span(),
                )
            });
        let match_ = match_value.clone().map(BlockItem::Expr);
        // The arms of a `match` used as a value give the value of their last line.
        let match_as_value = match_value.clone().map(|mut m| {
            telvax_ast::tail::tailify_value(&mut m);
            m
        });

        // `def add(a, b: int):` inside a function is a closure bound to a name.
        let local_param = ident().then(just(Token::Colon).ignore_then(type_()).or_not());
        let local_def = just(Token::Fn)
            .ignore_then(ident())
            .then(
                local_param
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .then_ignore(just(Token::Arrow).ignore_then(type_()).or_not())
            .then(block.clone())
            .map_with(|((name, params), mut body), e| {
                telvax_ast::tail::tailify_block(&mut body);
                let (names, types): (Vec<_>, Vec<_>) = params.into_iter().unzip();
                let body_span = body.span.clone();
                BlockItem::Let(Let {
                    mutable: false,
                    name,
                    ty: None,
                    init: spanned(
                        ExprKind::Closure {
                            params: names,
                            types,
                            body: Box::new(Spanned {
                                node: ExprKind::Block(body),
                                span: body_span,
                            }),
                        },
                        e.span(),
                    ),
                    span: rng(e.span()),
                })
            });

        let return_ = just(Token::Return)
            .ignore_then(expr.clone().or_not())
            .map_with(|v, e| BlockItem::Expr(spanned(ExprKind::Return(v.map(Box::new)), e.span())));
        let break_ =
            just(Token::Break).map_with(|_, e| BlockItem::Expr(spanned(ExprKind::Break, e.span())));
        let continue_ = just(Token::Continue)
            .map_with(|_, e| BlockItem::Expr(spanned(ExprKind::Continue, e.span())));
        let pass = just(Token::Pass)
            .or(just(Token::Ellipsis))
            .to(BlockItem::Nothing);

        // `let name: Type = value` and `var name = value`
        let binding = choice((just(Token::Let).to(false), just(Token::Var).to(true)));
        let binding_one = binding
            .then(ident())
            .then(just(Token::Colon).ignore_then(type_()).or_not())
            .then_ignore(just(Token::Eq))
            .then(expr.clone())
            .map_with(|(((mutable, name), ty), init), e| {
                BlockItem::Let(Let {
                    mutable,
                    name,
                    ty,
                    init,
                    span: rng(e.span()),
                })
            });
        // `1, 2` on the right of `=` is the tuple `(1, 2)`.
        let values = expr
            .clone()
            .then(
                just(Token::Comma)
                    .ignore_then(expr.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map_with(|(first, rest), e| {
                if rest.is_empty() {
                    first
                } else {
                    let mut items = vec![first];
                    items.extend(rest);
                    spanned(ExprKind::Tuple(items), e.span())
                }
            });
        // `let a, b = value`
        let binding_many = binding
            .then(bare_names().or(tuple_names()))
            .then_ignore(just(Token::Eq))
            .then(values.clone())
            .map_with(|((mutable, names), init), e| {
                let span = rng(e.span());
                let temp = Ident {
                    node: format!("__tuple{}", span.start),
                    span: span.clone(),
                };
                BlockItem::Lets(tuple_lets(names, temp, Some(init), mutable, span))
            });
        // `target = value` and the compound forms, which desugar to `target = target op value`.
        let assign_op = select! {
            Token::Eq => None,
            Token::PlusEq => Some(BinOp::Add),
            Token::MinusEq => Some(BinOp::Sub),
            Token::StarEq => Some(BinOp::Mul),
            Token::SlashEq => Some(BinOp::Div),
        };
        // After the first expression: `= value` (and the compound forms), or `, b = c, d` to
        // assign several targets at once (`a, b = b, a`).
        enum Tail {
            Assign(Option<BinOp>, Expr),
            Many(Vec<Expr>, Expr),
        }
        let many = just(Token::Comma)
            .ignore_then(expr.clone())
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>()
            .then_ignore(just(Token::Eq))
            .then(values.clone())
            .map(|(targets, value)| Tail::Many(targets, value));
        let simple_expr = expr
            .clone()
            .then(
                assign_op
                    .then(expr.clone())
                    .map(|(op, value)| Tail::Assign(op, value))
                    .or(many)
                    .or_not(),
            )
            .map_with(|(target, rest), e| {
                BlockItem::Expr(match rest {
                    None => target,
                    Some(Tail::Many(more, value)) => {
                        // `a, b = x, y` evaluates the whole right side, then assigns each target.
                        let span = rng(e.span());
                        let temp = Ident {
                            node: format!("__assign{}", span.start),
                            span: span.clone(),
                        };
                        let mut targets = vec![target];
                        targets.extend(more);
                        let mut stmts = vec![Stmt::Let(Let {
                            mutable: false,
                            name: temp.clone(),
                            ty: None,
                            init: value,
                            span: span.clone(),
                        })];
                        for (i, target) in targets.into_iter().enumerate() {
                            let at = target.span.clone();
                            let part = Spanned {
                                node: ExprKind::Field {
                                    base: Box::new(Spanned {
                                        node: ExprKind::Path(vec![temp.clone()]),
                                        span: at.clone(),
                                    }),
                                    name: Ident {
                                        node: i.to_string(),
                                        span: at.clone(),
                                    },
                                },
                                span: at.clone(),
                            };
                            stmts.push(Stmt::Expr(Spanned {
                                node: ExprKind::Assign {
                                    target: Box::new(target),
                                    value: Box::new(part),
                                },
                                span: at,
                            }));
                        }
                        spanned(
                            ExprKind::Block(Block {
                                stmts,
                                tail: None,
                                span: span.clone(),
                            }),
                            e.span(),
                        )
                    }
                    Some(Tail::Assign(op, value)) => {
                        let value = match op {
                            None => value,
                            Some(op) => spanned(
                                ExprKind::Binary {
                                    op,
                                    lhs: Box::new(target.clone()),
                                    rhs: Box::new(value.clone()),
                                },
                                SimpleSpan::from(target.span.start..value.span.end),
                            ),
                        };
                        spanned(
                            ExprKind::Assign {
                                target: Box::new(target),
                                value: Box::new(value),
                            },
                            e.span(),
                        )
                    }
                })
            });

        let simple = choice((
            return_,
            break_,
            continue_,
            pass,
            binding_many,
            binding_one,
            simple_expr,
        ))
        .then_ignore(just(Token::Newline));

        // `let v = match ...:`, `v = match ...:` and `return match ...:` end with the block of the
        // `match`, so they have no line break of their own.
        let let_match = binding
            .then(ident())
            .then(just(Token::Colon).ignore_then(type_()).or_not())
            .then_ignore(just(Token::Eq))
            .then(match_as_value.clone())
            .map_with(|(((mutable, name), ty), init), e| {
                BlockItem::Let(Let {
                    mutable,
                    name,
                    ty,
                    init,
                    span: rng(e.span()),
                })
            });
        let assign_match = expr
            .clone()
            .then_ignore(just(Token::Eq))
            .then(match_as_value.clone())
            .map_with(|(target, value), e| {
                BlockItem::Expr(spanned(
                    ExprKind::Assign {
                        target: Box::new(target),
                        value: Box::new(value),
                    },
                    e.span(),
                ))
            });
        let return_match = just(Token::Return)
            .ignore_then(match_as_value)
            .map_with(|value, e| {
                BlockItem::Expr(spanned(ExprKind::Return(Some(Box::new(value))), e.span()))
            });

        // They come after `simple`, so a plain statement is read once.
        choice((
            if_,
            while_,
            for_,
            match_,
            local_def,
            simple,
            let_match,
            return_match,
            assign_match,
        ))
        .recover_with(skip_until(
            any().and_is(just(Token::Dedent).not()).ignored(),
            just(Token::Newline).ignored(),
            || BlockItem::Nothing,
        ))
    })
}

/// `range(n)` and `range(a, b)` are counting loops; anything else is walked as a list.
fn for_iter(iter: Expr) -> ForIter {
    if let ExprKind::Call { callee, args } = &iter.node
        && matches!(&callee.node, ExprKind::Path(p) if p.len() == 1 && p[0].node == "range")
        && (1..=2).contains(&args.len())
    {
        let mut args = args.clone();
        let end = args.pop().expect("one or two arguments");
        let start = args.pop().unwrap_or(Spanned {
            node: ExprKind::Lit(Lit::Int(0)),
            span: end.span.start..end.span.start,
        });
        return ForIter::Range {
            start: Box::new(start),
            end: Box::new(end),
            inclusive: false,
        };
    }
    ForIter::Expr(Box::new(iter))
}

/// What can sit in the body of a `class`.
#[derive(Clone)]
enum Member {
    Field(Field),
    Method(Fn),
    Nothing,
}

/// `public` unless the name starts with an underscore.
fn is_public(name: &str) -> bool {
    !name.starts_with('_')
}

fn program<'a, 's: 'a, I>(heads: Heads) -> impl Parser<'a, I, Program, Extra<'a, 's>>
where
    I: ValueInput<'a, Token = Token<'s>, Span = SimpleSpan> + 'a,
{
    let expr = expr(heads);
    let stmt = statement(expr.clone()).boxed();
    let body = block_of(stmt);

    // `import a.b.C` and `from a.b import C, D`
    // `as` is only a word here, so it stays a plain name everywhere else.
    let alias = select! { Token::Ident("as") => () }
        .ignore_then(ident())
        .or_not();
    let import = just(Token::Import)
        .ignore_then(path())
        .then(alias.clone())
        .then_ignore(just(Token::Newline))
        .map_with(|(path, alias), e| {
            Item::Import(Import {
                path,
                group: Vec::new(),
                aliases: vec![alias],
                span: rng(e.span()),
            })
        });
    let names = ident()
        .then(alias)
        .separated_by(just(Token::Comma))
        .at_least(1)
        .allow_trailing()
        .collect::<Vec<_>>();
    let from = just(Token::From)
        .ignore_then(path())
        .then_ignore(just(Token::Import))
        .then(
            names
                .clone()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or(names),
        )
        .then_ignore(just(Token::Newline))
        .map_with(|(path, names), e| {
            let (group, aliases) = names.into_iter().unzip();
            Item::Import(Import {
                path,
                group,
                aliases,
                span: rng(e.span()),
            })
        });

    // `@endpoint(POST, "/x")`, or with names: `@endpoint(method: POST, path: "/x")`. A bare word
    // is the HTTP method and a string is the path.
    let text_value = |name: &'static str| {
        move |value: Expr| -> Expr {
            match value.node {
                ExprKind::Path(ref p) if name == "method" && p.len() == 1 => Expr {
                    span: value.span.clone(),
                    node: ExprKind::Lit(Lit::Str(p[0].node.clone())),
                },
                _ => value,
            }
        }
    };
    let attr_named = ident()
        .then_ignore(just(Token::Colon))
        .then(expr.clone())
        .map(move |(key, value)| {
            let fix = text_value(if key.node == "method" { "method" } else { "" });
            (key, fix(value))
        });
    let attr_method = ident().map_with(|word, e| {
        let span = rng(e.span());
        (
            Ident {
                node: "method".into(),
                span: span.clone(),
            },
            Expr {
                node: ExprKind::Lit(Lit::Str(word.node)),
                span,
            },
        )
    });
    let attr_path = select! { Token::Str(s) => s }.map_with(|text, e| {
        let span = rng(e.span());
        (
            Ident {
                node: "path".into(),
                span: span.clone(),
            },
            Expr {
                node: ExprKind::Lit(Lit::Str(unescape(&text[1..text.len() - 1]))),
                span,
            },
        )
    });
    let attr = just(Token::At)
        .ignore_then(ident())
        .then(
            choice((attr_named, attr_path, attr_method))
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .then_ignore(just(Token::Newline))
        .map_with(|(name, args), e| Attribute {
            name,
            args: args.unwrap_or_default(),
            span: rng(e.span()),
        });

    // `self` takes the type of the `class` it sits in.
    let self_param = word("self").map_with(|(), e| {
        let span = rng(e.span());
        Param {
            mutable: true,
            name: Ident {
                node: "self".into(),
                span: span.clone(),
            },
            ty: Type {
                path: vec![Ident {
                    node: "Self".into(),
                    span: span.clone(),
                }],
                args: Vec::new(),
                span,
            },
            default: None,
        }
    });
    // `(a, b): (Int, Int)` takes the pair apart at the top of the body.
    let tuple_param = tuple_names()
        .then_ignore(just(Token::Colon))
        .then(type_())
        .map_with(|(names, ty), e| {
            let at = rng(e.span());
            let name = Ident {
                node: format!("__arg{}", at.start),
                span: at.clone(),
            };
            let lets = tuple_lets(names, name.clone(), None, false, at);
            (
                Param {
                    mutable: true,
                    name,
                    ty,
                    default: None,
                },
                lets,
            )
        });
    let param = self_param
        .map(|p| (p, Vec::new()))
        .or(tuple_param)
        .or(ident()
            .then_ignore(just(Token::Colon))
            .then(type_())
            .then(just(Token::Eq).ignore_then(expr.clone()).or_not())
            .map(|((name, ty), default)| {
                (
                    Param {
                        mutable: true,
                        name,
                        ty,
                        default,
                    },
                    Vec::new(),
                )
            }));

    let func = attr
        .repeated()
        .collect::<Vec<_>>()
        .then(just(Token::Async).or_not())
        .then_ignore(just(Token::Fn))
        .then(ident())
        .then(generics())
        .then(
            param
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(just(Token::Arrow).ignore_then(type_()).or_not())
        // A body, or `: ...` for a function a trait requires.
        .then(
            body.map(Some).or(just(Token::Colon)
                .ignore_then(just(Token::Ellipsis))
                .ignore_then(just(Token::Newline))
                .to(None)),
        )
        .map_with(
            |((((((attrs, is_async), name), generics), params), ret), body), e| {
                let span = rng(e.span());
                let (params, lets): (Vec<Param>, Vec<Vec<Let>>) = params.into_iter().unzip();
                let mut body = body;
                if let Some(body) = &mut body {
                    let mut stmts: Vec<Stmt> = lets.into_iter().flatten().map(Stmt::Let).collect();
                    stmts.append(&mut body.stmts);
                    body.stmts = stmts;
                }
                Fn {
                    attrs,
                    is_pub: is_public(&name.node),
                    name,
                    is_async: is_async.is_some(),
                    generics,
                    params,
                    ret,
                    required: body.is_none(),
                    body: body.unwrap_or(Block {
                        stmts: Vec::new(),
                        tail: None,
                        span: span.clone(),
                    }),
                    span,
                }
            },
        );

    // The members of a block that holds only functions (`trait`, `extend`).
    let fns_block = func
        .clone()
        .repeated()
        .at_least(1)
        .collect::<Vec<_>>()
        .delimited_by(
            just(Token::Colon)
                .then(just(Token::Newline))
                .then(just(Token::Indent)),
            just(Token::Dedent),
        );

    // `class Name[T](Trait):` with fields (`x: float`) and methods in the body.
    let field = ident()
        .then_ignore(just(Token::Colon))
        .then(type_())
        .then(just(Token::Eq).ignore_then(expr.clone()).or_not())
        .then_ignore(just(Token::Newline))
        .map(|((name, ty), default)| Member::Field(Field { name, ty, default }));
    let member = choice((
        func.clone().map(Member::Method),
        field,
        just(Token::Pass)
            .then(just(Token::Newline))
            .to(Member::Nothing),
    ));
    // `type Id = Int`
    let alias = word("type")
        .ignore_then(ident())
        .then(generics())
        .then_ignore(just(Token::Eq))
        .then(type_())
        .then_ignore(just(Token::Newline))
        .map_with(|((name, generics), ty), e| {
            vec![Item::Alias(Alias {
                is_pub: is_public(&name.node),
                name,
                generics,
                ty,
                span: rng(e.span()),
            })]
        });
    let class = word("type")
        .ignore_then(ident())
        .then(generics())
        .then(
            ident()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .then(
            member
                .repeated()
                .at_least(1)
                .collect::<Vec<_>>()
                .delimited_by(
                    just(Token::Colon)
                        .then(just(Token::Newline))
                        .then(just(Token::Indent)),
                    just(Token::Dedent),
                ),
        )
        .map_with(|(((name, generics), trait_name), members), e| {
            let span = rng(e.span());
            let mut fields = Vec::new();
            let mut fns = Vec::new();
            for m in members {
                match m {
                    Member::Field(f) => fields.push(f),
                    Member::Method(f) => fns.push(f),
                    Member::Nothing => {}
                }
            }
            let mut items = vec![Item::Struct(Struct {
                is_pub: is_public(&name.node),
                name: name.clone(),
                generics: generics.clone(),
                fields,
                span: span.clone(),
            })];
            if !fns.is_empty() || trait_name.is_some() {
                items.push(Item::Impl(Impl {
                    ty_args: type_params_as_args(&generics),
                    generics,
                    ty: name,
                    trait_name,
                    fns,
                    span,
                }));
            }
            items
        });

    // `extend Name[T](Trait):` adds methods to a type declared elsewhere.
    let extend = just(Token::Extend)
        .ignore_then(ident())
        .then(generics())
        .then(
            ident()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .then(fns_block.clone())
        .map_with(|(((ty, generics), trait_name), fns), e| {
            vec![Item::Impl(Impl {
                ty_args: type_params_as_args(&generics),
                generics,
                ty,
                trait_name,
                fns,
                span: rng(e.span()),
            })]
        });

    let trait_ = just(Token::Trait)
        .ignore_then(ident())
        .then(fns_block)
        .map_with(|(name, fns), e| {
            vec![Item::Trait(Trait {
                is_pub: is_public(&name.node),
                name,
                fns,
                span: rng(e.span()),
            })]
        });

    // `enum Shape:` followed by one variant per line. A value may have a name: `Circle(r: Float)`.
    let variant_field = ident()
        .then_ignore(just(Token::Colon))
        .or_not()
        .then(type_());
    let variant = just(Token::From)
        .or_not()
        .then(ident())
        .then(
            variant_field
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .then_ignore(just(Token::Newline))
        .validate(|((from, name), fields), e, emitter| {
            let fields = fields.unwrap_or_default();
            let named = fields.iter().filter(|(n, _)| n.is_some()).count();
            if named != 0 && named != fields.len() {
                emitter.emit(Rich::custom(
                    e.span(),
                    "name every value of a variant or none: `Rect(w: Float, h: Float)`",
                ));
            }
            let (names, fields): (Vec<_>, Vec<_>) = fields.into_iter().unzip();
            Variant {
                name,
                fields,
                names: names.into_iter().flatten().collect(),
                from: from.is_some(),
            }
        });
    // Variants, then methods (`fn size(self) -> Int:`), in any order.
    let enum_member = func
        .clone()
        .map(|f| (None, Some(f)))
        .or(variant.map(|v| (Some(v), None)));
    let enum_ = just(Token::Enum)
        .ignore_then(ident())
        .then(generics())
        .then(
            ident()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .then(
            enum_member
                .repeated()
                .at_least(1)
                .collect::<Vec<_>>()
                .delimited_by(
                    just(Token::Colon)
                        .then(just(Token::Newline))
                        .then(just(Token::Indent)),
                    just(Token::Dedent),
                ),
        )
        .validate(|(((name, generics), trait_name), members), e, emitter| {
            let span = rng(e.span());
            let mut variants = Vec::new();
            let mut fns = Vec::new();
            for (variant, method) in members {
                variants.extend(variant);
                fns.extend(method);
            }
            if variants.is_empty() {
                emitter.emit(Rich::custom(
                    e.span(),
                    "an `enum` needs at least one variant, one per line: `Name` or `Name(Type)`",
                ));
            }
            let mut items = vec![Item::Enum(Enum {
                is_pub: is_public(&name.node),
                name: name.clone(),
                generics: generics.clone(),
                variants,
                span: span.clone(),
            })];
            if !fns.is_empty() || trait_name.is_some() {
                items.push(Item::Impl(Impl {
                    ty_args: type_params_as_args(&generics),
                    generics,
                    ty: name,
                    trait_name,
                    fns,
                    span,
                }));
            }
            items
        });

    // `serve:` and, indented, one `name: value` per line.
    let serve = word("serve")
        .ignore_then(just(Token::Colon))
        .ignore_then(just(Token::Newline))
        .ignore_then(just(Token::Indent))
        .ignore_then(
            ident()
                .then_ignore(just(Token::Colon))
                .then(expr.clone())
                .then_ignore(just(Token::Newline))
                .repeated()
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .then_ignore(just(Token::Dedent))
        .map_with(|options, e| {
            vec![Item::Serve(Serve {
                options,
                span: rng(e.span()),
            })]
        });

    // A name set at the top of the file: `embedder = OllamaEmbedder.new("x")`.
    let global = just(Token::Let)
        .ignore_then(ident())
        .then(just(Token::Colon).ignore_then(type_()).or_not())
        .then_ignore(just(Token::Eq))
        .then(expr)
        .then_ignore(just(Token::Newline))
        .map_with(|((name, ty), init), e| {
            vec![Item::Let(Let {
                mutable: false,
                name,
                ty,
                init,
                span: rng(e.span()),
            })]
        });

    let item_start = one_of([
        Token::Import,
        Token::From,
        Token::Enum,
        Token::Extend,
        Token::Trait,
        Token::Fn,
        Token::Async,
        Token::Let,
        Token::At,
    ])
    .ignored()
    .or(word("type"))
    .or(word("serve"));

    choice((
        import.map(|i| vec![i]),
        serve,
        from.map(|i| vec![i]),
        alias,
        class,
        enum_,
        extend,
        trait_,
        func.map(|f| vec![Item::Fn(f)]),
        global,
    ))
    .recover_with(skip_then_retry_until(any().ignored(), item_start))
    .repeated()
    .collect::<Vec<_>>()
    .then_ignore(end())
    .map(|groups| {
        let mut items: Vec<Item> = groups.into_iter().flatten().collect();
        for item in &mut items {
            match item {
                Item::Fn(f) => telvax_ast::tail::finish_fn(f),
                Item::Impl(i) => i.fns.iter_mut().for_each(telvax_ast::tail::finish_fn),
                Item::Trait(t) => t.fns.iter_mut().for_each(telvax_ast::tail::finish_fn),
                _ => {}
            }
        }
        Program { items }
    })
}

/// `Stack[T]` in `class Stack[T]`: the type arguments the `impl` names are the parameters.
fn type_params_as_args(generics: &[GenericParam]) -> Vec<Type> {
    generics
        .iter()
        .map(|g| Type {
            path: vec![g.name.clone()],
            args: Vec::new(),
            span: g.name.span.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
