//! The standard library reference (`docs/reference/std.md`) covers every function and method, and is the
//! text that `telvax_hir::stdlib_docs::reference_markdown` writes.
//!
//! After adding or changing an item of `stdlib.rs`, describe it in `stdlib_docs.rs` and run
//! `UPDATE_REFERENCE=1 cargo test -p telvax_hir --test reference` to write the page again.

use std::collections::BTreeSet;
use std::path::PathBuf;

use telvax_hir::stdlib;
use telvax_hir::stdlib_docs::{
    FUNCTIONS, METHODS, STD_TYPE_METHODS, TYPES, method_tables, reference_markdown,
};

fn page() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/std.md")
}

#[test]
fn every_function_is_described_once() {
    let documented: Vec<&str> = FUNCTIONS.iter().map(|d| d.path).collect();
    let unique: BTreeSet<&str> = documented.iter().copied().collect();
    assert_eq!(
        unique.len(),
        documented.len(),
        "a function is described twice"
    );

    let real: BTreeSet<&str> = stdlib::function_paths().collect();
    let missing: Vec<_> = real.difference(&unique).collect();
    let unknown: Vec<_> = unique.difference(&real).collect();
    assert!(
        missing.is_empty(),
        "describe these in compiler/semantic/src/stdlib_docs.rs: {missing:?}"
    );
    assert!(
        unknown.is_empty(),
        "stdlib_docs.rs describes functions that do not exist: {unknown:?}"
    );
    for d in FUNCTIONS {
        assert!(
            !d.summary.is_empty() && !d.signature.is_empty(),
            "{}",
            d.path
        );
        let name = d.path.replace("::", ".");
        assert!(
            d.signature.starts_with(&name),
            "{}: signature starts `{}`",
            d.path,
            d.signature
        );
    }
}

/// The number of parameters of each form of a signature: `std.x(a, b: (T) -> U) -> V` has two.
fn arities(signature: &str) -> Vec<usize> {
    signature
        .lines()
        .map(|line| {
            let open = line.find('(').expect("a signature has parentheses");
            let (mut depth, mut count, mut any) = (0, 0, false);
            for c in line[open..].chars() {
                match c {
                    '(' | '[' => depth += 1,
                    ')' | ']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    ',' if depth == 1 => count += 1,
                    c if depth >= 1 && !c.is_whitespace() => any = true,
                    _ => {}
                }
            }
            if any { count + 1 } else { 0 }
        })
        .collect()
}

/// The parameters written in the reference are the ones the type checker asks for.
#[test]
fn signatures_have_the_arity_the_checker_wants() {
    use telvax_hir::Ty;
    let is_arity_error = |r: Result<Ty, String>| r.err().is_some_and(|e| e.starts_with("expected"));
    for d in FUNCTIONS {
        let sig =
            stdlib::function(d.path).unwrap_or_else(|| panic!("{} is not a function", d.path));
        let forms = arities(d.signature);
        let most = *forms.iter().max().unwrap();
        for n in &forms {
            let args = vec![Ty::Unknown; *n];
            assert!(
                !is_arity_error((sig.check)(&args)),
                "{}: the checker does not take {n} argument(s), as `{}` says",
                d.path,
                d.signature
            );
        }
        let too_many = vec![Ty::Unknown; most + 1];
        assert!(
            is_arity_error((sig.check)(&too_many)),
            "{}: the checker takes more than the {most} argument(s) of `{}`",
            d.path,
            d.signature
        );
    }
}

#[test]
fn every_type_and_method_is_described() {
    let real: BTreeSet<&str> = stdlib::type_paths().collect();
    let documented: BTreeSet<&str> = TYPES.iter().map(|(p, _)| *p).collect();
    assert_eq!(real, documented, "types of std and stdlib_docs.rs differ");

    for (ty, table) in method_tables() {
        for (name, _) in table {
            assert!(
                METHODS
                    .iter()
                    .any(|(t, n, s)| *t == ty && n == name && !s.is_empty()),
                "describe the method {ty}.{name} in stdlib_docs.rs"
            );
        }
        for (t, n, _) in METHODS.iter().filter(|(t, _, _)| *t == ty) {
            assert!(
                table.iter().any(|(name, _)| name == n),
                "stdlib_docs.rs describes {t}.{n}, which does not exist"
            );
        }
    }
    let known: BTreeSet<&str> = method_tables().iter().map(|(t, _)| *t).collect();
    for (t, n, _) in METHODS {
        assert!(known.contains(t), "{t}.{n}: unknown type");
    }

    let real: BTreeSet<(&str, &str)> = stdlib::method_names().collect();
    let documented: BTreeSet<(&str, &str)> = STD_TYPE_METHODS
        .iter()
        .map(|(t, n, _, _)| (*t, *n))
        .collect();
    assert_eq!(
        real, documented,
        "methods of std types and stdlib_docs.rs differ"
    );
}

#[test]
fn the_page_is_up_to_date() {
    let want = reference_markdown();
    let path = page();
    if std::env::var_os("UPDATE_REFERENCE").is_some() {
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "docs/reference/std.md is out of date. Run: UPDATE_REFERENCE=1 cargo test -p telvax_hir --test reference"
    );
}
