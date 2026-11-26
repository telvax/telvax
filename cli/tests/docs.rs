#![cfg(feature = "tables")] // the guides read CSV files, which the lite build cannot

//! The code in the guides and under `examples/` must keep working: every ```` ```telvax ```` block
//! in the documentation is checked and formatted, and a block with `fn main` is also run.
//! A block that is only a piece of a program is marked ```` ```telvax ignore ````.

use std::path::{Path, PathBuf};
use std::process::Command;

const TELVAX: &str = env!("CARGO_BIN_EXE_telvax");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn telvax(args: &[&str]) -> (bool, String) {
    let out = Command::new(TELVAX)
        .args(args)
        .current_dir(root())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

struct Block {
    file: String,
    line: usize,
    code: String,
}

fn blocks(path: &Path) -> Vec<Block> {
    let text = std::fs::read_to_string(path).unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let mut found = Vec::new();
    let mut open: Option<(usize, bool, String)> = None;
    for (n, line) in text.lines().enumerate() {
        match &mut open {
            None => {
                if let Some(info) = line.strip_prefix("```") {
                    let mut words = info.split_whitespace();
                    let keep = words.next() == Some("telvax") && words.next() != Some("ignore");
                    open = Some((n + 2, keep, String::new()));
                }
            }
            Some((start, keep, code)) => {
                if line.starts_with("```") {
                    if *keep {
                        found.push(Block {
                            file: name.clone(),
                            line: *start,
                            code: std::mem::take(code),
                        });
                    }
                    open = None;
                } else {
                    code.push_str(line);
                    code.push('\n');
                }
            }
        }
    }
    found
}

fn tvx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            tvx_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "tvx") {
            out.push(path);
        }
    }
}

#[test]
fn code_blocks_in_the_guides_work() {
    let docs = [
        root().join("README.md"),
        root().join("docs/guide/tour.md"),
        root().join("docs/guide/use-cases.md"),
        root().join("docs/guide/deploy.md"),
    ];
    let dir = std::env::temp_dir().join(format!("telvax-docs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut failures = Vec::new();
    let mut count = 0;
    for doc in &docs {
        for block in blocks(doc) {
            count += 1;
            let file = dir.join(format!("block{count}.tvx"));
            std::fs::write(&file, &block.code).unwrap();
            let path = file.to_str().unwrap();
            let mut steps = vec![vec!["check", path], vec!["fmt", "--check", path]];
            if block.code.contains("fn main") {
                steps.push(vec!["run", path]);
            }
            for step in steps {
                let (ok, text) = telvax(&step);
                if !ok {
                    failures.push(format!(
                        "{}:{} `telvax {}` failed:\n{}",
                        block.file,
                        block.line,
                        step[0],
                        text.lines().take(4).collect::<Vec<_>>().join("\n")
                    ));
                    break;
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(count > 0, "no ```telvax blocks found");
    assert!(
        failures.is_empty(),
        "write the missing pieces, or mark the block ```telvax ignore:\n\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn examples_are_checked_and_formatted() {
    let mut files = Vec::new();
    tvx_files(&root().join("examples"), &mut files);
    assert!(!files.is_empty());
    let mut failures = Vec::new();
    for file in files {
        let path = file.to_str().unwrap();
        for step in [vec!["check", path], vec!["fmt", "--check", path]] {
            let (ok, text) = telvax(&step);
            if !ok {
                failures.push(format!(
                    "`telvax {}` failed:\n{}",
                    step.join(" "),
                    text.trim()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
