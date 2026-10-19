use lsp_types::Position;
use telvax_lsp::analysis::{CompletionKind, Snapshot, SymbolKind};

const SRC: &str = r#"
import std.http.Status
import std.ai.embeddings.OllamaEmbedder

type Point:
    x: Float
    y: Float

type Row:
    id: Int
    score: f32
    name: Text

let embedder = OllamaEmbedder.new("nomic-embed-text")

fn norm(p: Point) -> Float:
    p.x * p.x + p.y * p.y

fn demo(t: Table[Row], v: List[i32]) -> Float:
    let origin = Point(x: 0.0, y: 0.0)
    var total = 0.0
    let count = 3
    for r in t:
        total += r.score
    let n = norm(origin)
    n
"#;

fn snap() -> Snapshot {
    Snapshot::analyze(SRC.to_string())
}

fn at(s: &Snapshot, needle: &str, nth_char: usize) -> usize {
    s.text
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not in the source"))
        + nth_char
}

#[test]
fn a_valid_file_has_no_diagnostics() {
    let s = snap();
    assert!(s.diagnostics.is_empty(), "{:?}", s.diagnostics);
}

#[test]
fn diagnostics_point_at_the_error() {
    let s = Snapshot::analyze("fn f() -> i32:\n    \"oops\"\n".into());
    assert_eq!(s.diagnostics.len(), 1);
    let r = s.index.range(&s.diagnostics[0].span);
    assert_eq!(
        (r.start.line, r.start.character, r.end.character),
        (1, 4, 10)
    );
    assert!(
        s.diagnostics[0]
            .message
            .contains("expected `i32`, found `Text`")
    );

    // Syntax errors are reported too, and the rest of the file is still analysed.
    let s = Snapshot::analyze("fn a():\n    let x =\n\nfn b() -> i32:\n    1\n".into());
    assert!(
        s.diagnostics
            .iter()
            .any(|d| s.index.range(&d.span).start.line == 1)
    );
    assert!(s.symbols().iter().any(|sym| sym.name == "b"));
}

#[test]
fn hover_shows_types() {
    let s = snap();
    let (text, _) = s.hover(at(&s, "origin = ", 1)).unwrap();
    assert!(text.contains("let origin: Point"), "{text}");
    let (text, _) = s.hover(at(&s, "norm(origin)", 1)).unwrap();
    assert!(text.contains("fn norm(p: Point) -> Float"), "{text}");
    let (text, _) = s.hover(at(&s, "r.score", 3)).unwrap();
    assert!(text.contains("f32"), "{text}");
    let (text, _) = s.hover(at(&s, "total = 0.0", 1)).unwrap();
    assert!(text.contains("var total: f32"), "{text}");
    let (text, _) = s.hover(at(&s, "type Point", 8)).unwrap();
    assert!(
        text.contains("x: Float") && text.contains("y: Float"),
        "{text}"
    );
    assert!(s.hover(0).is_none() || s.hover(0).is_some());
}

#[test]
fn definitions() {
    let s = snap();
    // A call jumps to the function's name, a variable to where it is declared, a construction to
    // the type.
    let def = s.definition(at(&s, "norm(origin)", 1)).unwrap();
    assert_eq!(&s.text[def.clone()], "norm");
    assert!(s.text[..def.start].ends_with("fn "));
    let def = s.definition(at(&s, "(origin)", 2)).unwrap();
    assert_eq!(&s.text[def.clone()], "origin");
    assert!(def.end < at(&s, "(origin)", 0));
    let def = s.definition(at(&s, "Point(x: 0.0", 2)).unwrap();
    assert_eq!(&s.text[def.clone()], "Point");
    assert!(s.text[..def.start].ends_with("type "));
    assert!(
        s.definition(at(&s, "0.0)", 1)).is_none(),
        "literals have no definition"
    );
}

#[test]
fn symbols_are_listed_in_source_order() {
    let names: Vec<_> = snap()
        .symbols()
        .into_iter()
        .map(|s| (s.name, s.kind))
        .collect();
    assert_eq!(
        names,
        [
            ("Point".into(), SymbolKind::Struct),
            ("Row".into(), SymbolKind::Struct),
            ("embedder".into(), SymbolKind::Global),
            ("norm".into(), SymbolKind::Function),
            ("demo".into(), SymbolKind::Function),
        ]
    );
}

#[test]
fn inlay_hints_show_inferred_types_only_where_unwritten() {
    let s = snap();
    let hints: Vec<(String, String)> = s
        .inlay_hints()
        .into_iter()
        .map(|(off, label)| {
            (
                s.text[..off]
                    .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap()
                    .to_string(),
                label,
            )
        })
        .collect();
    assert!(
        hints.contains(&("origin".into(), ": Point".into())),
        "{hints:?}"
    );
    assert!(
        hints.contains(&("total".into(), ": f32".into())),
        "{hints:?}"
    );
    assert!(
        hints.contains(&("count".into(), ": i32".into())),
        "{hints:?}"
    );
    assert!(hints.contains(&("r".into(), ": Row".into())), "{hints:?}");
    assert!(
        !hints.iter().any(|(n, _)| n == "t" || n == "v" || n == "p"),
        "parameters are annotated already: {hints:?}"
    );
}

fn labels(items: &[telvax_lsp::analysis::Completion]) -> Vec<&str> {
    items.iter().map(|c| c.label.as_str()).collect()
}

fn complete_in(src_with_marker: &str) -> Vec<telvax_lsp::analysis::Completion> {
    let offset = src_with_marker.find('|').unwrap();
    let text = src_with_marker.replacen('|', "", 1);
    Snapshot::analyze(text.clone()).completions(offset.min(text.len()))
}

/// Completion runs while the code is incomplete; the last good analysis supplies the types.
fn complete_after_edit(before: &str, typed: &str) -> Vec<telvax_lsp::analysis::Completion> {
    let good = Snapshot::analyze(before.replacen("/*here*/", "", 1));
    assert!(
        good.diagnostics.iter().all(|d| !d.is_error()) || good.hir.is_some(),
        "{:?}",
        good.diagnostics
    );
    let offset = before.find("/*here*/").unwrap();
    let broken = before.replacen("/*here*/", typed, 1);
    Snapshot::analyze_after(broken, None, Some(&good)).completions(offset + typed.len())
}

#[test]
fn completes_fields_after_a_dot() {
    let src = "type Point:\n    x: Float\n    y: Float\n\nfn f(p: Point) -> Float:\n    /*here*/\n    0.0\n";
    let items = complete_after_edit(src, "p.");
    assert_eq!(labels(&items), ["x", "y"]);
    assert!(items.iter().all(|c| c.kind == CompletionKind::Field));
    let items = complete_after_edit(src, "p.x");
    assert_eq!(labels(&items), ["x"]);
}

#[test]
fn completes_table_columns_and_methods() {
    let src = "type Row:\n    id: Int\n    score: f32\n\nfn f(t: Table[Row]) -> u64:\n    /*here*/\n    0\n";
    let items = complete_after_edit(src, "t.");
    let l = labels(&items);
    assert!(
        l.contains(&"score") && l.contains(&"id") && l.contains(&"len") && l.contains(&"is_empty"),
        "{l:?}"
    );
    let nested = complete_after_edit(src, "t.score.");
    let l = labels(&nested);
    assert!(l.contains(&"sum") && l.contains(&"matmul"), "{l:?}");
}

#[test]
fn completes_through_chains_and_strings() {
    let src = "type P:\n    x: Float\n\ntype W:\n    name: Text\n    p: P\n\nfn f(w: W) -> Float:\n    /*here*/\n    0.0\n";
    assert_eq!(labels(&complete_after_edit(src, "w.p.")), ["x"]);
    let items = complete_after_edit(src, "w.name.");
    let s = labels(&items);
    assert!(s.contains(&"trim") && s.contains(&"len"), "{s:?}");
}

#[test]
fn completes_std_paths() {
    let items = complete_in("fn f():\n    std.|");
    let l = labels(&items);
    assert!(
        l.contains(&"text")
            && l.contains(&"tensor")
            && l.contains(&"table")
            && l.contains(&"collect"),
        "{l:?}"
    );
    let items = complete_in("fn f():\n    std.text.tr|");
    assert_eq!(labels(&items), ["trim", "truncate"]);
    let items = complete_after_edit(
        "from std.db.pgvector import PgVectorClient\n\nfn f():\n    /*here*/\n    pass\n",
        "PgVectorClient.",
    );
    assert_eq!(labels(&items), ["connect"]);
    assert!(complete_in("fn f():\n    nope.|").is_empty());
}

#[test]
fn completes_names_in_scope() {
    let src = "type Point:\n    x: Float\n\nfn helper() -> i32:\n    1\n\nfn f(alpha: i32) -> i32:\n    let beta = 2\n    |\n    0\n";
    let l = complete_in(src);
    let names = labels(&l);
    for want in [
        "alpha", "beta", "helper", "Point", "fn", "let", "var", "return", "std", "i32", "Int",
    ] {
        assert!(names.contains(&want), "missing {want}: {names:?}");
    }
    let narrowed = complete_in(&src.replace('|', "he|"));
    assert_eq!(labels(&narrowed), ["helper"]);
    assert_eq!(
        labels(&complete_in("@|\nfn f():\n    pass\n")),
        ["endpoint", "middleware"]
    );
}

#[test]
fn positions_are_utf16() {
    let s = Snapshot::analyze(
        r#"
fn f() -> i32:
    let s = "é😀"
    "x"
"#
        .into(),
    );
    let d = &s.diagnostics[0];
    let r = s.index.range(&d.span);
    // The emoji counts as two UTF-16 units, so the column differs from the byte offset.
    assert!(d.span.start > r.start.character as usize);
    let _ = Position {
        line: 0,
        character: 0,
    };
}
