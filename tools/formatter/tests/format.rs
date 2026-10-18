//! The formatter keeps the meaning of a program and its line breaks, and normalises the rest.

use std::path::{Path, PathBuf};

use telvax_fmt::{format, is_formatted};

fn examples() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "tvx") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples"),
        &mut out,
    );
    out.sort();
    assert!(!out.is_empty(), "no examples found");
    out
}

fn fmt(src: &str) -> String {
    format(src).unwrap_or_else(|e| panic!("cannot format {src:?}: {e}"))
}

#[test]
fn spacing_inside_a_line_is_normalised() {
    assert_eq!(
        fmt("fn f(a:Int,b:Int)->Int:\n  a+b\n"),
        "fn f(a: Int, b: Int) -> Int:\n    a + b\n"
    );
    assert_eq!(
        fmt("let y   =  x [0]\nlet o: Int?= None\n"),
        "let y = x[0]\nlet o: Int? = None\n"
    );
    assert_eq!(
        fmt("let m = f(a:1,b:2)\nlet t = (1,2)\n"),
        "let m = f(a: 1, b: 2)\nlet t = (1, 2)\n"
    );
    assert_eq!(
        fmt("fn f(x:Int)->Int:\n    if x>1 and not x<5:\n        x|>g(1)\n    else:\n        -x\n"),
        "fn f(x: Int) -> Int:\n    if x > 1 and not x < 5:\n        x |> g(1)\n    else:\n        -x\n"
    );
}

#[test]
fn indentation_is_four_spaces_per_level() {
    let out = fmt("fn f():\n  if True:\n     pass\n");
    assert_eq!(out, "fn f():\n    if True:\n        pass\n");
}

#[test]
fn runs_of_blank_lines_shrink_to_one() {
    assert_eq!(
        fmt("let x = 1\n\n\n\nlet y = 2\n"),
        "let x = 1\n\nlet y = 2\n"
    );
}

#[test]
fn comments_survive() {
    let src = "# header\nfn f():\n    #  inside\n    pass  # trailing\n";
    let out = fmt(src);
    assert!(out.contains("# header"), "{out}");
    assert!(out.contains("#  inside"), "{out}");
    assert!(out.contains("pass  # trailing"), "{out}");
}

#[test]
fn the_file_ends_with_exactly_one_newline() {
    assert_eq!(fmt("let x = 1"), "let x = 1\n");
    assert_eq!(fmt("let x = 1\n\n\n"), "let x = 1\n");
}

#[test]
fn line_breaks_are_kept_where_the_author_put_them() {
    let src = "let xs = [\n    1,\n    2,\n]\n";
    let out = fmt(src);
    assert_eq!(out.lines().count(), 4, "{out}");
}

#[test]
fn strings_are_not_touched() {
    let src = "let s = \"a  ,  b:c\"\nlet t = f\"{x}  {y}\"\nlet u = 'it  is'\n";
    let out = fmt(src);
    assert!(out.contains("\"a  ,  b:c\""), "{out}");
    assert!(out.contains("f\"{x}  {y}\""), "{out}");
    assert!(out.contains("'it  is'"), "{out}");
}

#[test]
fn formatting_twice_changes_nothing() {
    for src in [
        "fn f(a:Int,b:Int)->Int:\n  a+b\n",
        "let xs=[1,2,3]\nlet y   =  xs [0]\n\n\n\nlet z=1",
        "type P:\n  x:Int\n  y : Float = 1.0\n",
        "fn f(x:Int)->Int:\n    match x:\n        case 1:\n            2\n        case _:\n            3\n",
        "import a.b.C\nfrom std.text import trim  as  strip\n",
    ] {
        let once = fmt(src);
        let twice = fmt(&once);
        assert_eq!(once, twice, "not idempotent for {src:?}");
        assert!(is_formatted(&once).unwrap());
    }
}

#[test]
fn every_example_is_formatted_and_stays_so() {
    for path in examples() {
        let src = std::fs::read_to_string(&path).unwrap();
        let out = fmt(&src);
        assert_eq!(out, src, "{} is not formatted", path.display());
        assert_eq!(fmt(&out), out);
    }
}

#[test]
fn input_that_cannot_be_read_is_an_error_not_a_panic() {
    for src in [
        "let s = \"never closed\n",
        "let x = 1 ; let y = 2\n",
        "let x = $\n",
    ] {
        assert!(format(src).is_err(), "{src:?} should not format");
    }
}

#[test]
fn no_prefix_of_an_example_makes_the_formatter_panic() {
    for path in examples() {
        let src = std::fs::read_to_string(&path).unwrap();
        for end in (0..=src.len()).filter(|&i| src.is_char_boundary(i)) {
            let prefix = &src[..end];
            // Either it refuses, or what it returns is stable.
            if let Ok(once) = format(prefix) {
                assert_eq!(
                    format(&once).as_deref(),
                    Ok(once.as_str()),
                    "{} cut at {end}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn diff_shows_only_the_lines_that_change() {
    let old = "fn f(a:Int):\n  a\n\nfn g():\n  1\n";
    let new = fmt(old);
    assert_eq!(
        telvax_fmt::diff(old, &new),
        "@@ line 1 @@\n-fn f(a:Int):\n-  a\n+fn f(a: Int):\n+    a\n@@ line 5 @@\n-  1\n+    1\n"
    );
    assert_eq!(telvax_fmt::diff(&new, &new), "");
    assert_eq!(
        telvax_fmt::diff("a\nb\n", "a\nx\nb\n"),
        "@@ line 2 @@\n+x\n"
    );
    assert_eq!(
        telvax_fmt::diff("a\nx\nb\n", "a\nb\n"),
        "@@ line 2 @@\n-x\n"
    );
}

#[test]
fn a_minus_that_starts_a_line_is_a_prefix() {
    let src = "fn f(xs: List[Int]) -> Int:\n    for k in range(3):\n        if xs[k] == 1:\n            return k\n    -1\n\nfn g() -> Int:\n    let a = 1\n    -a\n\nfn h(a: Int, b: Int) -> Int:\n    a\n    - b\n";
    let out = fmt(src);
    assert!(out.contains("\n    -1\n"), "{out}");
    assert!(out.contains("\n    -a\n"), "{out}");
    // A binary minus keeps its spaces.
    assert!(out.contains("a - b") || out.contains("\n    -b\n"), "{out}");
    assert_eq!(fmt(&out), out);
}
