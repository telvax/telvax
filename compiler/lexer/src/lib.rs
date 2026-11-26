//! Zero-copy tokenizer for Telvax source code.
//!
//! Blocks are made by indentation, as in Python. [`tokenize`] turns the indentation into
//! [`Token::Indent`], [`Token::Dedent`] and the ends of lines into [`Token::Newline`], so the parser
//! never looks at whitespace. Line breaks inside `()`, `[]` and `{}` are ignored, and a line that
//! starts with `|>` continues the line before it.

use std::ops::Range;

use logos::Logos;

pub type Span = Range<usize>;

/// Raised for input that matches no token (stray character, unterminated string, ...).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LexError;

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
#[logos(error = LexError)]
#[logos(skip r"[ \t\r\n]+")]
#[logos(skip(r"#[^\n]*", allow_greedy = true))]
pub enum Token<'src> {
    // Keywords
    #[token("import")]
    Import,
    #[token("from")]
    From,
    #[token("fn")]
    Fn,
    #[token("async")]
    Async,
    #[token("await")]
    Await,
    #[token("let")]
    Let,
    #[token("var")]
    Var,
    #[token("enum")]
    Enum,
    #[token("trait")]
    Trait,
    #[token("extend")]
    Extend,
    #[token("match")]
    Match,
    #[token("case")]
    Case,
    #[token("return")]
    Return,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("pass")]
    Pass,
    #[token("if")]
    If,
    #[token("elif")]
    Elif,
    #[token("else")]
    Else,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("is")]
    Is,
    #[token("while")]
    While,
    #[token("and")]
    And,
    #[token("or")]
    Or,
    #[token("not")]
    Not,
    #[token("True")]
    True,
    #[token("False")]
    False,

    // Literals and identifiers
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Ident(&'src str),
    /// `name!(`: the start of a macro call. The parenthesis is part of the token.
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*!\(")]
    MacroOpen(&'src str),
    #[regex(r"[0-9][0-9_]*")]
    Int(&'src str),
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9]+)?")]
    Float(&'src str),
    #[regex(r#""([^"\\]|\\.)*"|'([^'\\]|\\.)*'"#)]
    Str(&'src str),
    /// `"""..."""`: a string that may span lines.
    #[regex(r#""""([^"\\]|\\.|"[^"]|""[^"])*""""#)]
    TStr(&'src str),
    /// `r"..."`: a text whose backslashes are kept as written.
    #[regex(r#"r"([^"\\]|\\.)*"|r'([^'\\]|\\.)*'"#)]
    RStr(&'src str),
    /// `f"""..."""`: an f-string that may span lines.
    #[regex(r#"f"""([^"\\{]|\\.|"[^"]|""[^"]|\{([^{}]|\{[^{}]*\})*\}|\{)*""""#)]
    FTStr(&'src str),
    /// `f"..."`; interpolation is split by the parser. A `{...}` part may hold quotes of its own
    /// (`f"{m["a"]}"`), and a lone `{` is kept so the parser can say it is not closed.
    #[regex(
        r#"f"([^"\\{]|\\.|\{([^{}]|\{[^{}]*\})*\}|\{)*"|f'([^'\\{]|\\.|\{([^{}]|\{[^{}]*\})*\}|\{)*'"#
    )]
    FStr(&'src str),

    // Operators
    #[token("|>")]
    Pipe,
    #[token("...")]
    Ellipsis,
    #[token("**")]
    StarStar,
    #[token("//")]
    SlashSlash,
    #[token("??")]
    QuestionQuestion,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("|")]
    Bar,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token(".")]
    Dot,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("@")]
    At,
    #[token("?")]
    Question,

    // Delimiters
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,

    // Layout, produced by `tokenize` and never by the lexer itself.
    /// End of a logical line.
    Newline,
    /// The next line is indented one more level.
    Indent,
    /// The block ends.
    Dedent,
}

impl std::fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use Token::*;
        let s = match self {
            Import => "import",
            From => "from",
            Fn => "fn",
            Async => "async",
            Await => "await",
            Let => "let",
            Var => "var",
            Enum => "enum",
            Trait => "trait",
            Extend => "extend",
            Match => "match",
            Case => "case",
            Return => "return",
            Break => "break",
            Continue => "continue",
            Pass => "pass",
            If => "if",
            Elif => "elif",
            Else => "else",
            For => "for",
            In => "in",
            Is => "is",
            While => "while",
            And => "and",
            Or => "or",
            Not => "not",
            True => "True",
            False => "False",
            Ident(s) | MacroOpen(s) | Int(s) | Float(s) | Str(s) | TStr(s) | RStr(s) | FStr(s)
            | FTStr(s) => s,
            Pipe => "|>",
            Ellipsis => "...",
            StarStar => "**",
            SlashSlash => "//",
            QuestionQuestion => "??",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            SlashEq => "/=",
            Arrow => "->",
            FatArrow => "=>",
            EqEq => "==",
            NotEq => "!=",
            LtEq => "<=",
            GtEq => ">=",
            Eq => "=",
            Lt => "<",
            Gt => ">",
            Bar => "|",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            Dot => ".",
            Comma => ",",
            Colon => ":",
            At => "@",
            Question => "?",
            LParen => "(",
            RParen => ")",
            LBrace => "{",
            RBrace => "}",
            LBracket => "[",
            RBracket => "]",
            Newline => "end of line",
            Indent => "indent",
            Dedent => "dedent",
        };
        f.write_str(s)
    }
}

/// Tokenizes `src` without any layout tokens, collecting lexical errors instead of stopping at the
/// first one. Used for pieces that never span lines, such as the expressions inside an f-string.
pub fn tokenize_flat(src: &str) -> (Vec<(Token<'_>, Span)>, Vec<Span>) {
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    for (res, span) in Token::lexer(src).spanned() {
        match res {
            Ok(tok) => tokens.push((tok, span)),
            Err(LexError) => errors.push(span),
        }
    }
    (tokens, errors)
}

/// Tokenizes a whole file: [`tokenize_flat`] plus `Newline`, `Indent` and `Dedent`.
///
/// A `Newline` ends every logical line; `Indent` and `Dedent` open and close blocks. Tabs in the
/// indentation and a dedent to a level that was never opened are lexical errors.
pub fn tokenize(src: &str) -> (Vec<(Token<'_>, Span)>, Vec<Span>) {
    let (raw, mut errors) = tokenize_flat(src);
    let mut out: Vec<(Token<'_>, Span)> = Vec::with_capacity(raw.len() + raw.len() / 4);
    let mut levels = vec![0usize];
    let mut depth = 0usize;
    let mut prev_end = 0usize;
    for (tok, span) in raw {
        let first = out.is_empty();
        let new_line = first || src[prev_end..span.start].contains('\n');
        if new_line && depth == 0 && (first || tok != Token::Pipe) {
            if !first {
                out.push((Token::Newline, prev_end..prev_end));
            }
            let line_start = src[..span.start].rfind('\n').map_or(0, |i| i + 1);
            let indent = &src[line_start..span.start];
            if indent.contains('\t') {
                errors.push(line_start..span.start);
            }
            let col = indent.chars().count();
            let top = *levels.last().unwrap_or(&0);
            if col > top {
                levels.push(col);
                out.push((Token::Indent, span.start..span.start));
            } else {
                while col < *levels.last().unwrap_or(&0) {
                    levels.pop();
                    out.push((Token::Dedent, span.start..span.start));
                }
                if col != *levels.last().unwrap_or(&0) {
                    errors.push(line_start..span.start);
                }
            }
        }
        match tok {
            Token::LParen | Token::MacroOpen(_) | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => depth = depth.saturating_sub(1),
            _ => {}
        }
        prev_end = span.end;
        out.push((tok, span));
    }
    if !out.is_empty() {
        out.push((Token::Newline, prev_end..prev_end));
        while levels.len() > 1 {
            levels.pop();
            out.push((Token::Dedent, src.len()..src.len()));
        }
    }
    (out, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Token::*;

    fn flat(src: &str) -> Vec<Token<'_>> {
        let (toks, errs) = tokenize_flat(src);
        assert!(errs.is_empty(), "unexpected lex errors: {errs:?}");
        toks.into_iter().map(|(t, _)| t).collect()
    }

    /// Texts that end in an escaped backslash, several on a line and nested in lists: the row of emoticons of the
    /// tweet port, which a report said did not lex. Each literal is one `Str` token.
    #[test]
    fn texts_that_end_in_an_escaped_backslash_are_one_token_each() {
        let line =
            r#"[[">:/", ":/", ":\\", ">:\\", ":-."], [":'(", ":'''(", ";'("], ['a\\', 'b\\']]"#;
        let strs: Vec<&str> = flat(line)
            .into_iter()
            .filter_map(|t| match t {
                Str(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(
            strs,
            [
                r#"">:/""#,
                r#"":/""#,
                r#"":\\""#,
                r#"">:\\""#,
                r#"":-.""#,
                r#"":'(""#,
                r#"":'''(""#,
                r#"";'(""#,
                r"'a\\'",
                r"'b\\'"
            ]
        );
    }

    fn kinds(src: &str) -> Vec<Token<'_>> {
        let (toks, errs) = tokenize(src);
        assert!(errs.is_empty(), "unexpected lex errors: {errs:?}");
        toks.into_iter().map(|(t, _)| t).collect()
    }

    #[test]
    fn pipeline() {
        assert_eq!(
            flat("text |> std.text.trim()"),
            [
                Ident("text"),
                Pipe,
                Ident("std"),
                Dot,
                Ident("text"),
                Dot,
                Ident("trim"),
                LParen,
                RParen
            ]
        );
    }

    #[test]
    fn arrow_function_does_not_collide_with_pipe() {
        assert_eq!(
            flat("d => d.text |> f()"),
            [
                Ident("d"),
                FatArrow,
                Ident("d"),
                Dot,
                Ident("text"),
                Pipe,
                Ident("f"),
                LParen,
                RParen
            ]
        );
    }

    #[test]
    fn tensor_type() {
        assert_eq!(
            flat("-> Tensor[float32, [768]]"),
            [
                Arrow,
                Ident("Tensor"),
                LBracket,
                Ident("float32"),
                Comma,
                LBracket,
                Int("768"),
                RBracket,
                RBracket
            ]
        );
    }

    #[test]
    fn endpoint_decorator() {
        assert_eq!(
            flat(r#"@endpoint(path="/v1/rag/query", method="POST")"#),
            [
                At,
                Ident("endpoint"),
                LParen,
                Ident("path"),
                Eq,
                Str(r#""/v1/rag/query""#),
                Comma,
                Ident("method"),
                Eq,
                Str(r#""POST""#),
                RParen
            ]
        );
    }

    #[test]
    fn strings_use_either_quote() {
        assert_eq!(flat(r#"f"a {b} \" c""#), [FStr(r#"f"a {b} \" c""#)]);
        assert_eq!(flat("f'{d.get(\"k\")}'"), [FStr("f'{d.get(\"k\")}'")]);
        assert_eq!(flat("'it\\'s'"), [Str("'it\\'s'")]);
        assert_eq!(flat(r#"f "x""#), [Ident("f"), Str(r#""x""#)]);
    }

    #[test]
    fn numbers() {
        assert_eq!(
            flat("1_000 3.14 2.5e-3"),
            [Int("1_000"), Float("3.14"), Float("2.5e-3")]
        );
    }

    #[test]
    fn operators_and_words() {
        assert_eq!(
            flat("a += 1\nb -= 2"),
            [Ident("a"), PlusEq, Int("1"), Ident("b"), MinusEq, Int("2")]
        );
        assert_eq!(flat("x.y"), [Ident("x"), Dot, Ident("y")]);
        assert_eq!(
            flat("a -> b - c"),
            [Ident("a"), Arrow, Ident("b"), Minus, Ident("c")]
        );
        assert_eq!(
            flat("not a and b or c"),
            [Not, Ident("a"), And, Ident("b"), Or, Ident("c")]
        );
        assert_eq!(flat("True False None"), [True, False, Ident("None")]);
        assert_eq!(
            flat("fn f(): ..."),
            [Fn, Ident("f"), LParen, RParen, Colon, Ellipsis]
        );
    }

    #[test]
    fn keywords_vs_identifiers() {
        assert_eq!(flat("fn define"), [Fn, Ident("define")]);
        assert_eq!(
            flat("def class type lambda mut"),
            [
                Ident("def"),
                Ident("class"),
                Ident("type"),
                Ident("lambda"),
                Ident("mut")
            ]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(flat("a # hi\nb"), [Ident("a"), Ident("b")]);
    }

    #[test]
    fn spans_are_byte_offsets() {
        let (toks, _) = tokenize_flat("fn x");
        assert_eq!(toks[1].1, 3..4);
    }

    #[test]
    fn errors_are_collected_and_lexing_continues() {
        let (toks, errs) = tokenize_flat("a $ b \"open");
        assert_eq!(toks.len(), 2);
        assert_eq!(errs, [2..3, 6..11]);
    }

    #[test]
    fn blocks_become_indent_and_dedent() {
        assert_eq!(
            kinds("if x:\n    a\n    b\nc\n"),
            [
                If,
                Ident("x"),
                Colon,
                Newline,
                Indent,
                Ident("a"),
                Newline,
                Ident("b"),
                Newline,
                Dedent,
                Ident("c"),
                Newline
            ]
        );
    }

    #[test]
    fn nested_blocks_close_together() {
        assert_eq!(
            kinds("if a:\n    if b:\n        c\nd"),
            [
                If,
                Ident("a"),
                Colon,
                Newline,
                Indent,
                If,
                Ident("b"),
                Colon,
                Newline,
                Indent,
                Ident("c"),
                Newline,
                Dedent,
                Dedent,
                Ident("d"),
                Newline
            ]
        );
        // The blocks still open at the end of the file are closed.
        let toks = kinds("if a:\n    b");
        assert_eq!(&toks[toks.len() - 2..], [Newline, Dedent]);
    }

    #[test]
    fn blank_and_comment_lines_do_not_count() {
        assert_eq!(
            kinds("if x:\n\n    # note\n    a\n\n# end\nb"),
            kinds("if x:\n    a\nb")
        );
    }

    #[test]
    fn brackets_join_lines() {
        assert_eq!(
            kinds("f(1,\n  2)\ng"),
            [
                Ident("f"),
                LParen,
                Int("1"),
                Comma,
                Int("2"),
                RParen,
                Newline,
                Ident("g"),
                Newline
            ]
        );
    }

    #[test]
    fn a_line_starting_with_a_pipe_continues_the_previous_one() {
        assert_eq!(
            kinds("x = a\n    |> f()\n    |> g()\ny"),
            [
                Ident("x"),
                Eq,
                Ident("a"),
                Pipe,
                Ident("f"),
                LParen,
                RParen,
                Pipe,
                Ident("g"),
                LParen,
                RParen,
                Newline,
                Ident("y"),
                Newline
            ]
        );
    }

    #[test]
    fn tabs_and_bad_dedents_are_errors() {
        let (_, errs) = tokenize("if x:\n\ta");
        assert_eq!(errs.len(), 1);
        let (_, errs) = tokenize("if x:\n        a\n    b");
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn rag_example_lexes_cleanly() {
        let src = include_str!("../../../examples/rag-service.tvx");
        let (toks, errs) = tokenize(src);
        assert!(errs.is_empty(), "{errs:?}");
        assert!(toks.iter().any(|(t, _)| *t == Pipe));
    }

    #[test]
    fn short_operators() {
        assert_eq!(
            flat("a ** b // c ?? d"),
            [
                Ident("a"),
                StarStar,
                Ident("b"),
                SlashSlash,
                Ident("c"),
                QuestionQuestion,
                Ident("d")
            ]
        );
        assert_eq!(flat("a * b / c ? d"), flat("a * b / c ? d"));
        assert_eq!(flat("x?"), [Ident("x"), Question]);
    }

    #[test]
    fn triple_quoted_strings_span_lines() {
        assert_eq!(flat("\"\"\"a\nb\"\"\""), [TStr("\"\"\"a\nb\"\"\"")]);
        assert_eq!(
            flat("\"\"\"say \"hi\" now\"\"\""),
            [TStr("\"\"\"say \"hi\" now\"\"\"")]
        );
        // The empty string is still a plain string.
        assert_eq!(flat("\"\""), [Str("\"\"")]);
        // A line break inside one does not start a block.
        assert_eq!(
            kinds("let s = \"\"\"a\n  b\"\"\"\n"),
            [Let, Ident("s"), Eq, TStr("\"\"\"a\n  b\"\"\""), Newline]
        );
    }

    #[test]
    fn fstrings_may_hold_quotes_inside_braces() {
        assert_eq!(flat(r#"f"{m["a"]}""#), [FStr(r#"f"{m["a"]}""#)]);
        assert_eq!(flat(r#"f"{x:.2f} and {y}" + z"#).len(), 3);
        // A brace that is never closed is still one token, so the parser can say so.
        assert_eq!(flat(r#"f"{x""#), [FStr(r#"f"{x""#)]);
    }

    #[test]
    fn f_strings_over_lines_and_raw_text() {
        let src = "f\"\"\"a {x}\n b {m[\"k\"]} {{c}}\"\"\"";
        assert_eq!(flat(src), [FTStr(src)]);
        // A plain f-string next to one that spans lines.
        assert_eq!(flat("f\"a\" f\"\"\"b\"\"\"").len(), 2);
        assert_eq!(
            flat(r#"r"a\nb" r'c\d'"#),
            [RStr(r#"r"a\nb""#), RStr(r"r'c\d'")]
        );
        // `r` alone is still a name.
        assert_eq!(flat("r + r2"), [Ident("r"), Plus, Ident("r2")]);
        assert_eq!(flat(r#"r "x""#), [Ident("r"), Str(r#""x""#)]);
        // A line break inside one does not start a block.
        assert_eq!(
            kinds("let s = f\"\"\"a\n  {x}\"\"\"\n"),
            [Let, Ident("s"), Eq, FTStr("f\"\"\"a\n  {x}\"\"\""), Newline]
        );
    }

    #[test]
    fn async_and_await_are_words_of_their_own() {
        assert_eq!(
            flat("async fn f(): await g()"),
            [
                Async,
                Fn,
                Ident("f"),
                LParen,
                RParen,
                Colon,
                Await,
                Ident("g"),
                LParen,
                RParen
            ]
        );
        // They are only words when they stand alone.
        assert_eq!(
            flat("asynchronous awaiting"),
            [Ident("asynchronous"), Ident("awaiting")]
        );
    }

    #[test]
    fn a_macro_call_starts_with_one_token() {
        assert_eq!(
            flat("assert!(a != b)"),
            [MacroOpen("assert!("), Ident("a"), NotEq, Ident("b"), RParen]
        );
        // Without the parenthesis, `!=` is still the comparison.
        assert_eq!(flat("a!=b"), [Ident("a"), NotEq, Ident("b")]);
    }
}
