//! A conservative formatter for Telvax source.
//!
//! It works on the token stream, so it never changes what a program means. Indentation is part of
//! the syntax, so the formatter keeps every line break and every block exactly where you put it
//! and normalises the rest: four spaces per level, spacing inside a line, continuation lines,
//! comments, and runs of blank lines. After formatting, the result is tokenised again and compared
//! with the input; if the tokens differ (layout tokens included), formatting fails instead of
//! returning something wrong.

use telvax_lexer::{Token, tokenize};

const INDENT: &str = "    ";

/// Formats `src`. Fails on lexical errors or if the result would not tokenise to the same tokens.
pub fn format(src: &str) -> Result<String, String> {
    let (tokens, errors) = tokenize(src);
    if let Some(span) = errors.first() {
        return Err(format!(
            "cannot format: unexpected input at byte {}",
            span.start
        ));
    }
    let out = layout(src, &tokens);
    let (again, errs) = tokenize(&out);
    let same = errs.is_empty()
        && again.len() == tokens.len()
        && again.iter().zip(&tokens).all(|((a, sa), (b, sb))| {
            a == b && (is_layout(a) || out[sa.clone()] == src[sb.clone()])
        });
    if !same {
        return Err("internal error: formatting would change the program".into());
    }
    Ok(out)
}

/// Whether `src` is already formatted.
pub fn is_formatted(src: &str) -> Result<bool, String> {
    format(src).map(|f| f == src)
}

/// The lines that formatting changes: a `@@ line N @@` header for each change (N counts lines of
/// `old`), then the lines taken out with `-` and the lines put in with `+`.
pub fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a_mid, b_mid) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    // `keep[i][j]`: lines of the common subsequence of `a_mid[i..]` and `b_mid[j..]`. A change too
    // large for the table is shown as one block.
    let mut out = String::new();
    let mut hunk = |line: usize, removed: &[&str], added: &[&str]| {
        if removed.is_empty() && added.is_empty() {
            return;
        }
        out.push_str(&format!("@@ line {line} @@\n"));
        removed
            .iter()
            .for_each(|l| out.push_str(&format!("-{l}\n")));
        added.iter().for_each(|l| out.push_str(&format!("+{l}\n")));
    };
    if a_mid.len() * b_mid.len() > 4_000_000 {
        hunk(prefix + 1, a_mid, b_mid);
        return out;
    }
    let (n, m) = (a_mid.len(), b_mid.len());
    let mut keep = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            keep[i][j] = if a_mid[i] == b_mid[j] {
                keep[i + 1][j + 1] + 1
            } else {
                keep[i + 1][j].max(keep[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let (mut start, mut removed, mut added) = (0, Vec::new(), Vec::new());
    while i < n || j < m {
        if i < n && j < m && a_mid[i] == b_mid[j] {
            hunk(start, &removed, &added);
            removed.clear();
            added.clear();
            i += 1;
            j += 1;
            continue;
        }
        if removed.is_empty() && added.is_empty() {
            start = prefix + i + 1;
        }
        if j == m || (i < n && keep[i + 1][j] >= keep[i][j + 1]) {
            removed.push(a_mid[i]);
            i += 1;
        } else {
            added.push(b_mid[j]);
            j += 1;
        }
    }
    hunk(start, &removed, &added);
    out
}

fn is_layout(t: &Token) -> bool {
    matches!(t, Token::Newline | Token::Indent | Token::Dedent)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    Keyword,
    Literal,
    Open(char),
    Close(char),
    Comma,
    Colon,
    Dot,
    At,
    Question,
    Pipe,
    Assign,
    Op,
}

fn kind(t: &Token) -> Kind {
    use Token::*;
    match t {
        Import | From | Fn | Async | Await | Let | Var | Enum | Trait | Extend | Match | Case
        | Return | Break | Continue | Pass | If | Elif | Else | For | In | Is | While | And
        | Or | Not => Kind::Keyword,
        True | False | Int(_) | Float(_) | Str(_) | TStr(_) | RStr(_) | FStr(_) | FTStr(_)
        | Ellipsis => Kind::Literal,
        Ident(_) => Kind::Word,
        LParen | MacroOpen(_) => Kind::Open('('),
        LBracket => Kind::Open('['),
        LBrace => Kind::Open('{'),
        RParen => Kind::Close('('),
        RBracket => Kind::Close('['),
        RBrace => Kind::Close('{'),
        Comma => Kind::Comma,
        Colon => Kind::Colon,
        Dot => Kind::Dot,
        At => Kind::At,
        Question => Kind::Question,
        Pipe => Kind::Pipe,
        Eq => Kind::Assign,
        _ => Kind::Op,
    }
}

struct Comment<'a> {
    text: &'a str,
    /// On the same line as the token before it.
    trailing: bool,
    /// An empty line comes before it.
    blank_before: bool,
    /// Column where it started in the source.
    col: usize,
}

struct Piece<'a> {
    text: &'a str,
    kind: Kind,
    token: Token<'a>,
    comments: Vec<Comment<'a>>,
    /// Line breaks between the last comment (or the previous token) and this token.
    newlines_before: usize,
    /// Layout tokens that come right before this token.
    layout: Vec<Token<'a>>,
    /// Column of this token in the source.
    col: usize,
    /// No space or line break between this token and the one before it in the source.
    tight: bool,
}

fn column(src: &str, at: usize) -> usize {
    let line_start = src[..at].rfind('\n').map_or(0, |n| n + 1);
    src[line_start..at].chars().count()
}

#[allow(unused_assignments)]
fn layout(src: &str, tokens: &[(Token<'_>, std::ops::Range<usize>)]) -> String {
    // Split every gap between tokens into comments and line breaks.
    let mut pieces: Vec<Piece> = Vec::new();
    let mut pending_layout: Vec<Token> = Vec::new();
    let mut prev_end = 0;
    for (tok, span) in tokens {
        if is_layout(tok) {
            pending_layout.push(*tok);
            continue;
        }
        let gap = &src[prev_end..span.start];
        let (comments, newlines_before) = scan_gap(gap, src, prev_end, prev_end == 0);
        pieces.push(Piece {
            text: &src[span.clone()],
            kind: kind(tok),
            token: *tok,
            comments,
            newlines_before,
            layout: std::mem::take(&mut pending_layout),
            col: column(src, span.start),
            tight: gap.is_empty(),
        });
        prev_end = span.end;
    }
    let (tail_comments, _) = scan_gap(&src[prev_end..], src, prev_end, false);

    let mut out = String::new();
    // Original columns of the open blocks; index = level.
    let mut cols: Vec<usize> = vec![0];
    // For every open bracket, the output line it was opened on. Brackets opened on the same line
    // add only one level of indentation.
    let mut open_lines: Vec<usize> = Vec::new();
    let mut open_kinds: Vec<char> = Vec::new();
    let mut line_no = 0usize;
    let mut at_line_start = true;
    let mut prev: Option<&Piece> = None;
    let mut prev_prev_kind: Option<Kind> = None;

    macro_rules! newline {
        () => {{
            out.push('\n');
            line_no += 1;
            at_line_start = true;
        }};
    }

    for (i, p) in pieces.iter().enumerate() {
        let level = cols.len() - 1;
        let indents = p.layout.iter().filter(|t| **t == Token::Indent).count();
        let logical_start = i == 0 || p.layout.contains(&Token::Newline);
        // Columns the comments before this token can belong to: an indent opens a level that is
        // not on the stack yet.
        let mut eff_cols = cols.clone();
        if indents > 0 {
            eff_cols.push(p.col);
        }
        for c in &p.comments {
            if c.trailing && !at_line_start {
                out.push_str("  ");
                out.push_str(c.text);
                newline!();
                continue;
            }
            if !at_line_start {
                newline!();
            }
            if c.blank_before && !out.is_empty() && !out.ends_with("\n\n") && !out.ends_with(":\n")
            {
                out.push('\n');
                line_no += 1;
            }
            let lvl = if open_lines.is_empty() {
                eff_cols.iter().rposition(|x| *x <= c.col).unwrap_or(0)
            } else {
                level + levels(&open_lines)
            };
            out.push_str(&INDENT.repeat(lvl));
            out.push_str(c.text);
            newline!();
        }
        for t in &p.layout {
            match t {
                Token::Indent => cols.push(p.col),
                Token::Dedent => {
                    cols.pop();
                }
                _ => {}
            }
        }
        let level = cols.len() - 1;

        let after_comment = !p.comments.is_empty() && at_line_start;
        let breaks = if after_comment {
            // After a comment line the token starts a new line; keep a blank line if one was there.
            usize::from(p.newlines_before > 1) * 2
        } else if logical_start {
            p.newlines_before.max(1)
        } else {
            p.newlines_before
        };
        if i > 0 && breaks > 0 && !at_line_start {
            newline!();
        }
        if breaks > 1
            && !out.is_empty()
            && !out.ends_with("\n\n")
            && !out.ends_with(":\n")
            && at_line_start
            && (logical_start || after_comment)
        {
            out.push('\n');
            line_no += 1;
        }

        // Closers at the start of a line are accounted for before indenting that line.
        let closers_here = if at_line_start {
            pieces[i..]
                .iter()
                .enumerate()
                .take_while(|(n, q)| {
                    matches!(q.kind, Kind::Close(_))
                        && (*n == 0 || (q.newlines_before == 0 && q.comments.is_empty()))
                })
                .count()
        } else {
            0
        };
        let indent_now = level
            + if logical_start {
                0
            } else {
                levels(&open_lines[..open_lines.len().saturating_sub(closers_here)])
            };
        if let Kind::Close(_) = p.kind {
            open_lines.pop();
            open_kinds.pop();
        }

        if at_line_start {
            // A line that starts with `|>` continues the one before it, one level deeper.
            let extra = usize::from(p.kind == Kind::Pipe);
            out.push_str(&INDENT.repeat(indent_now + extra));
            at_line_start = false;
        } else if let Some(prev) = prev
            && space_between(prev, p, prev_prev_kind, open_kinds.last().copied())
        {
            out.push(' ');
        }
        out.push_str(p.text);

        if let Kind::Open(c) = p.kind {
            open_lines.push(line_no);
            open_kinds.push(c);
        }
        prev_prev_kind = prev.map(|x| x.kind);
        prev = Some(p);
        let _ = p.token;
    }
    for c in &tail_comments {
        if c.trailing && !at_line_start {
            out.push_str("  ");
            out.push_str(c.text);
            newline!();
            continue;
        }
        if !at_line_start {
            newline!();
        }
        if c.blank_before && !out.ends_with("\n\n") && !out.is_empty() {
            out.push('\n');
        }
        let lvl = cols.iter().rposition(|x| *x <= c.col).unwrap_or(0);
        out.push_str(&INDENT.repeat(lvl));
        out.push_str(c.text);
        newline!();
    }
    if !at_line_start {
        out.push('\n');
    }
    out
}

/// Comments in a gap, and the number of line breaks that separate the previous token (or the last
/// comment) from the next token. `base` is where the gap starts in `src`.
fn scan_gap<'a>(gap: &'a str, src: &str, base: usize, first: bool) -> (Vec<Comment<'a>>, usize) {
    let mut comments = Vec::new();
    let mut newlines = 0usize;
    let mut total = 0usize;
    let mut rest = gap;
    let mut offset = 0usize;
    while !rest.is_empty() {
        if let Some(pos) = rest.find('#') {
            let (before, after) = rest.split_at(pos);
            let nl = before.matches('\n').count();
            total += nl;
            newlines += nl;
            let end = after.find('\n').unwrap_or(after.len());
            let text = after[..end].trim_end();
            let trailing = newlines == 0 && !first;
            comments.push(Comment {
                text,
                trailing,
                blank_before: newlines > 1,
                col: column(src, base + offset + pos),
            });
            offset += pos + end;
            rest = &after[end..];
            newlines = 0;
        } else {
            let nl = rest.matches('\n').count();
            total += nl;
            newlines += nl;
            rest = "";
        }
    }
    // Trailing newlines after the last comment decide the spacing before the token.
    (comments, if total == 0 { 0 } else { newlines.max(1) })
}

/// Indentation levels for brackets opened on the given lines (one per distinct line).
fn levels(open_lines: &[usize]) -> usize {
    let mut n = 0;
    let mut last = None;
    for l in open_lines {
        if last != Some(*l) {
            n += 1;
            last = Some(*l);
        }
    }
    n
}

fn is_value_end(k: Kind) -> bool {
    matches!(
        k,
        Kind::Word | Kind::Literal | Kind::Close(_) | Kind::Question
    )
}

fn space_between(
    prev: &Piece,
    next: &Piece,
    before_prev: Option<Kind>,
    innermost: Option<char>,
) -> bool {
    use Kind::*;
    match (prev.kind, next.kind) {
        // Nothing before these.
        (_, Comma | Dot | Question | Colon) => false,
        (_, Close(_)) => false,
        (Open(_), _) => false,
        (Dot | At, _) => false,
        // A slice written without spaces (`xs[1:3]`) stays that way.
        (Colon, _) if innermost == Some('[') && next.tight => false,
        (Colon | Comma, _) => true,
        // Calls and indexing.
        (Word | Literal | Close(_) | Question, Open('(' | '[')) => false,
        (Keyword, Open(_)) => true,
        // Prefix `-` sticks to its operand, and so does the `*` of `[first, *rest]`.
        // A `-` that starts a line is a prefix too, whatever the line above ended with.
        (Op, _)
            if matches!(prev.text, "-" | "*")
                && (!prev.layout.is_empty() || !before_prev.is_some_and(is_value_end)) =>
        {
            false
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        format(src).unwrap()
    }

    #[test]
    fn normalises_spacing_and_indentation() {
        let out =
            fmt("fn add(a:Int,b:Int)->Int:\n  let x=a+b*2\n  if x>3:\n      return x\n  x - 1\n");
        assert_eq!(
            out,
            "fn add(a: Int, b: Int) -> Int:\n    let x = a + b * 2\n    if x > 3:\n        return x\n    x - 1\n"
        );
    }

    #[test]
    fn macro_calls_keep_their_name_and_parenthesis_together() {
        let out = fmt("fn f():\n  let x=dbg!( g(1))\n  assert!(x>1,\"big\")\n");
        assert_eq!(
            out,
            "fn f():\n    let x = dbg!(g(1))\n    assert!(x > 1, \"big\")\n"
        );
    }

    #[test]
    fn keeps_comments_and_limits_blank_lines() {
        let src = "# header\n\n\n\nfn f() -> Int:   # trailing\n  # inside\n  1 # one\n";
        assert_eq!(
            fmt(src),
            "# header\n\nfn f() -> Int:  # trailing\n    # inside\n    1  # one\n"
        );
    }

    #[test]
    fn a_comment_before_a_dedent_stays_with_its_block() {
        let src = "fn f():\n    if a:\n        b()\n        # last of the branch\n    # back in f\n    c()\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn handles_types_unary_calls_and_pipes() {
        let src = "fn f(t:Tensor[f32,2,3],v:List[List[Int]],o:Int?)->Result[Int,Text]:\n    let a= -1\n    let b=not True\n    for i in range(n):\n        g(i,-a)\n    let c=x\n    |>f()\n    |>g()\n    let d=(x,y)=>x+y\n    Ok(t[0]  [1])\n";
        let out = fmt(src);
        assert_eq!(
            out,
            "fn f(t: Tensor[f32, 2, 3], v: List[List[Int]], o: Int?) -> Result[Int, Text]:\n    let a = -1\n    let b = not True\n    for i in range(n):\n        g(i, -a)\n    let c = x\n        |> f()\n        |> g()\n    let d = (x, y) => x + y\n    Ok(t[0][1])\n"
        );
    }

    #[test]
    fn named_arguments_and_defaults() {
        let out = fmt(
            "@endpoint( POST ,\"/x\")\nfn f(limit:Int=5):\n    let p=Point(x:1,y:2)\n    search(v,limit:3)\n",
        );
        assert_eq!(
            out,
            "@endpoint(POST, \"/x\")\nfn f(limit: Int = 5):\n    let p = Point(x: 1, y: 2)\n    search(v, limit: 3)\n"
        );
    }

    #[test]
    fn is_idempotent_on_the_bundled_examples() {
        for src in [
            include_str!("../../../examples/rag-service.tvx"),
            include_str!("../../../examples/basics/primes.tvx"),
            include_str!("../../../examples/basics/sieve.tvx"),
            include_str!("../../../examples/hello-world/hello.tvx"),
            include_str!("../../../examples/errors/parse-config.tvx"),
            include_str!("../../../examples/service/sentiment.tvx"),
        ] {
            let once = fmt(src);
            assert_eq!(fmt(&once), once, "not idempotent");
        }
    }

    #[test]
    fn the_bundled_examples_are_already_formatted() {
        for src in [
            include_str!("../../../examples/basics/primes.tvx"),
            include_str!("../../../examples/errors/parse-config.tvx"),
            include_str!("../../../examples/hello-world/hello.tvx"),
        ] {
            assert_eq!(fmt(src), src);
        }
    }

    #[test]
    fn enums_match_and_break() {
        let src = "enum S:\n    A(Int)\n    B\n\nfn f(s:S)->Int:\n    match s:\n      case S.A(n):\n        n\n      case S.B:\n        0\n\nfn g():\n    while True:\n        break\n";
        let out = fmt(src);
        assert_eq!(fmt(&out), out);
        assert!(out.contains("    A(Int)\n"), "{out}");
        assert!(
            out.contains("        case S.A(n):\n            n\n"),
            "{out}"
        );
    }

    #[test]
    fn rejects_input_it_cannot_lex() {
        assert!(format("fn f():\n    let s = \"unterminated\n").is_err());
        assert!(format("fn f():\n\t1\n").is_err());
    }

    #[test]
    fn brackets_continue_lines() {
        let src = "fn f() -> R:\n    Ok(Response(\n    x: 1,\n    y: 2,\n    ))\n";
        let out = fmt(src);
        assert_eq!(
            out,
            "fn f() -> R:\n    Ok(Response(\n        x: 1,\n        y: 2,\n    ))\n"
        );
        assert_eq!(fmt(&out), out);
    }

    #[test]
    fn imports_and_row_types() {
        let out = fmt(
            "from std.http import Request,Status\nlet t:Table[{region:Text,revenue:Float}]=x\n",
        );
        assert_eq!(
            out,
            "from std.http import Request, Status\nlet t: Table[{region: Text, revenue: Float}] = x\n"
        );
    }

    #[test]
    fn slices_stay_tight_and_short_operators_get_spaces() {
        let out = fmt(
            "let a=xs[1:3]\nlet b=xs[:n-1]\nlet c=xs[-2:]\nlet d=2**3//2\nlet e=m??0\nlet f=2** -1\n",
        );
        assert_eq!(
            out,
            "let a = xs[1:3]\nlet b = xs[:n - 1]\nlet c = xs[-2:]\nlet d = 2 ** 3 // 2\nlet e = m ?? 0\nlet f = 2 ** -1\n"
        );
        // Spaces the author wrote around a slice colon are kept, and bounds in a type are not slices.
        assert_eq!(fmt("let a = xs[1: 3]\n"), "let a = xs[1: 3]\n");
        assert_eq!(
            fmt("fn f[T: Num](x:T)->T:\n    x\n"),
            "fn f[T: Num](x: T) -> T:\n    x\n"
        );
    }

    #[test]
    fn comprehensions_maps_and_triple_quoted_text() {
        let src = "let a = [x*2 for x in xs if x>1]\nlet m = {\"a\":1,\"b\":2}\nlet t = \"\"\"one\n  two\"\"\"\nlet s = f\"{m[\"a\"]:>8}\"\n";
        assert_eq!(
            fmt(src),
            "let a = [x * 2 for x in xs if x > 1]\nlet m = {\"a\": 1, \"b\": 2}\nlet t = \"\"\"one\n  two\"\"\"\nlet s = f\"{m[\"a\"]:>8}\"\n"
        );
        // Formatting twice changes nothing.
        let once = fmt(src);
        assert_eq!(fmt(&once), once);
    }

    #[test]
    fn patterns_keep_their_shape() {
        let src = "fn f(x:Int)->Int:\n    match x:\n        case 1|2:\n            0\n        case -1:\n            1\n        case [a,*rest]:\n            2\n        case [*_,z]:\n            3\n        case P(x:0,y:b)if b>1:\n            4\n        case _:\n            5\n    a,b=b,a\n    let (p,q),r=t\n";
        let out = fmt(src);
        assert_eq!(
            out,
            "fn f(x: Int) -> Int:\n    match x:\n        case 1 | 2:\n            0\n        case -1:\n            1\n        case [a, *rest]:\n            2\n        case [*_, z]:\n            3\n        case P(x: 0, y: b) if b > 1:\n            4\n        case _:\n            5\n    a, b = b, a\n    let (p, q), r = t\n"
        );
        assert_eq!(fmt(&out), out);
        // The star of a product keeps its spaces.
        assert_eq!(
            fmt("let a=b*c\nlet d=[x*2 for x in xs]\n"),
            "let a = b * c\nlet d = [x * 2 for x in xs]\n"
        );
    }

    #[test]
    fn enums_with_methods_and_aliases() {
        let src = "type Id=Int\ntype Pair[T]=(T,T)\nenum S(Named):\n    Rect(w:Float,h:Float)\n    Empty\n\n    fn name(self)->Text:\n        \"s\"\n";
        assert_eq!(
            fmt(src),
            "type Id = Int\ntype Pair[T] = (T, T)\nenum S(Named):\n    Rect(w: Float, h: Float)\n    Empty\n\n    fn name(self) -> Text:\n        \"s\"\n"
        );
    }

    #[test]
    fn from_variants() {
        assert_eq!(
            fmt("enum E:\n    from  Parse(Text)\n    Other\n"),
            "enum E:\n    from Parse(Text)\n    Other\n"
        );
    }

    #[test]
    fn strings_that_span_lines_or_keep_backslashes() {
        let src = "let a=f\"\"\"x {n:>{w}}\n  {{y}}\"\"\"\nlet b=r\"a\\nb\"\nlet c=f\"{x!r:>8} {y = }\"\n";
        let out = fmt(src);
        assert_eq!(
            out,
            "let a = f\"\"\"x {n:>{w}}\n  {{y}}\"\"\"\nlet b = r\"a\\nb\"\nlet c = f\"{x!r:>8} {y = }\"\n"
        );
        assert_eq!(fmt(&out), out);
    }

    #[test]
    fn async_and_await() {
        let src = "async  fn load(id:Int)->Result[Text,Text]:\n    let a=await  fetch(id)?\n    let b=-await g()\n    Ok(a)\n";
        let out = fmt(src);
        assert_eq!(
            out,
            "async fn load(id: Int) -> Result[Text, Text]:\n    let a = await fetch(id)?\n    let b = -await g()\n    Ok(a)\n"
        );
        assert_eq!(fmt(&out), out);
    }
}
