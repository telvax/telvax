//! `docs/reference/cli.md` shows the output of `telvax --help` and describes every command.

use std::path::PathBuf;
use std::process::Command;

fn page() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/reference/cli.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The first ```text block after `## Synopsis`.
fn synopsis(page: &str) -> String {
    let after = page
        .split("## Synopsis")
        .nth(1)
        .expect("a Synopsis section");
    let block = after.split("```text\n").nth(1).expect("a text block");
    block
        .split("\n```")
        .next()
        .expect("the end of the block")
        .to_string()
}

fn help() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_telvax"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

#[test]
fn the_synopsis_is_the_help_text() {
    assert_eq!(
        synopsis(&page()),
        help(),
        "docs/reference/cli.md is out of date: paste the output of `telvax --help` into its Synopsis"
    );
}

#[test]
fn every_command_has_a_section() {
    let page = page();
    let help = help();
    let commands: std::collections::BTreeSet<&str> = help
        .lines()
        .filter_map(|l| l.strip_prefix("  telvax "))
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert!(commands.len() >= 14, "{commands:?}");
    for command in commands {
        assert!(
            page.contains(&format!("### `telvax {command}`")),
            "docs/reference/cli.md has no section for `telvax {command}`"
        );
    }
}
