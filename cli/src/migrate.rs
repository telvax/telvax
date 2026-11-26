//! `telvax migrate`: rewrites a program or a package from edition 2026 to the next one (`2027-preview`) where that
//! can be done mechanically, and says what it changed. See `docs/guide/stability.md`.

use std::path::{Path, PathBuf};

use telvax_lexer::{Token, tokenize_flat};

use crate::{CliError, failed};

/// The edition `migrate` writes.
pub const TO: &str = "2027-preview";

/// `std.string` became `std.text`: the `string` of every `std . string` in the code, and in the expressions of the
/// f-strings, is rewritten; text and comments are left alone. The rewritten source and how many places changed.
pub fn rewrite(source: &str) -> (String, usize) {
    let (tokens, _) = tokenize_flat(source);
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut count = 0;
    for i in 0..tokens.len() {
        match &tokens[i].0 {
            Token::Ident("std")
                if matches!(tokens.get(i + 1), Some((Token::Dot, _)))
                    && matches!(tokens.get(i + 2), Some((Token::Ident("string"), _))) =>
            {
                edits.push((tokens[i + 2].1.clone(), "text".into()));
                count += 1;
            }
            Token::FStr(_) => {
                let span = tokens[i].1.clone();
                let (text, n) = rewrite_fstring(&source[span.clone()]);
                if n > 0 {
                    edits.push((span, text));
                    count += n;
                }
            }
            _ => {}
        }
    }
    let mut out = source.to_string();
    for (range, text) in edits.into_iter().rev() {
        out.replace_range(range, &text);
    }
    (out, count)
}

/// The expressions between `{` and `}` of an f-string (not `{{`), each rewritten as code.
fn rewrite_fstring(literal: &str) -> (String, usize) {
    let bytes = literal.as_bytes();
    let (mut out, mut count, mut i) = (String::new(), 0, 0);
    while i < bytes.len() {
        if bytes[i] == b'{' && bytes.get(i + 1) == Some(&b'{') {
            out.push_str("{{");
            i += 2;
            continue;
        }
        if bytes[i] == b'{' {
            // The expression runs to its matching `}`.
            let mut depth = 0;
            let mut j = i;
            while j < bytes.len() {
                match bytes[j] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let end = j.min(bytes.len().saturating_sub(1));
            let inner = &literal[i + 1..end.max(i + 1)];
            let (code, n) = rewrite(inner);
            count += n;
            out.push('{');
            out.push_str(&code);
            if j < bytes.len() {
                out.push('}');
            }
            i = j + 1;
            continue;
        }
        let ch = literal[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, count)
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for e in entries {
            let name = e.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') && name != "target" {
                collect(&e, out);
            }
        }
    } else if path.extension().is_some_and(|x| x == "tvx") {
        out.push(path.to_path_buf());
    }
}

/// `telvax migrate [--check] [path...]`: rewrites the `.tvx` files (a directory: every one under it; no path: this
/// directory) and sets `edition = "2027-preview"` in the `telvax.toml` of the package. `--check` only says what
/// would change, and fails when something would.
pub fn run(args: &[&str]) -> Result<(), CliError> {
    let check = args.contains(&"--check");
    let mut paths: Vec<&str> = args.iter().copied().filter(|a| *a != "--check").collect();
    if let Some(flag) = paths.iter().find(|a| a.starts_with('-')) {
        return failed(format!(
            "`migrate` has no option `{flag}`; it takes `--check`"
        ));
    }
    if paths.is_empty() {
        paths.push(".");
    }
    let mut files = Vec::new();
    for p in &paths {
        let path = Path::new(p);
        if !path.exists() {
            return failed(format!("`{p}` does not exist"));
        }
        collect(path, &mut files);
    }
    let mut changed = 0;
    for file in &files {
        let source = std::fs::read_to_string(file)
            .or_else(|e| failed(format!("{}: {e}", file.display())))?;
        let (out, n) = rewrite(&source);
        if n > 0 {
            changed += 1;
            println!(
                "{} {}: {n} place(s) of `std.string` to `std.text`",
                if check { "would rewrite" } else { "rewrote" },
                file.display()
            );
            if !check {
                std::fs::write(file, out)
                    .or_else(|e| failed(format!("{}: {e}", file.display())))?;
            }
        }
    }
    // The packages of the paths move to the new edition.
    let mut manifests: Vec<PathBuf> = Vec::new();
    for p in &paths {
        if let Ok(Some(project)) = telvax_pkg::Project::discover(Path::new(p)) {
            let manifest = project.dir.join("telvax.toml");
            if !manifests.contains(&manifest) {
                manifests.push(manifest);
            }
        }
    }
    for manifest in manifests {
        let text = std::fs::read_to_string(&manifest)
            .or_else(|e| failed(format!("{}: {e}", manifest.display())))?;
        let new = set_edition(&text, TO);
        if new != text {
            changed += 1;
            println!(
                "{} {}: edition = \"{TO}\"",
                if check { "would set" } else { "set" },
                manifest.display()
            );
            if !check {
                std::fs::write(&manifest, new)
                    .or_else(|e| failed(format!("{}: {e}", manifest.display())))?;
            }
        }
    }
    if changed == 0 {
        println!("nothing to migrate: the files are already in edition {TO}");
    } else if check {
        return failed(format!(
            "{changed} file(s) would change; run `telvax migrate` to write them"
        ));
    }
    Ok(())
}

/// The manifest with its `edition` line set to `edition` in `[package]` (added when there is none).
fn set_edition(manifest: &str, edition: &str) -> String {
    let line = format!("edition = \"{edition}\"");
    let mut out = Vec::new();
    let mut in_package = false;
    let mut done = false;
    for l in manifest.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            if in_package && !done {
                out.push(line.clone());
                done = true;
            }
            in_package = t == "[package]";
        }
        if in_package && t.starts_with("edition") && t[7..].trim_start().starts_with('=') {
            out.push(line.clone());
            done = true;
            continue;
        }
        out.push(l.to_string());
    }
    if in_package && !done {
        out.push(line);
    }
    let mut text = out.join("\n");
    if manifest.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn std_string_becomes_std_text_in_code_and_nowhere_else() {
        let src = "import std.string\nfrom std.string import join\n\n# std.string.join is old\nfn f(xs: List[Int]) -> Text:\n    let s = \"std.string.join\"\n    std.string.join(xs, \",\") + std.string.repeat(s, 2) + f\"{std.string.slice(s, 0, 3)} std.string {{x}}\"\n";
        let (out, n) = rewrite(src);
        assert_eq!(n, 5, "{out}");
        assert_eq!(
            out,
            "import std.text\nfrom std.text import join\n\n# std.string.join is old\nfn f(xs: List[Int]) -> Text:\n    let s = \"std.string.join\"\n    std.text.join(xs, \",\") + std.text.repeat(s, 2) + f\"{std.text.slice(s, 0, 3)} std.string {{x}}\"\n"
        );
        // Already migrated: nothing to do.
        assert_eq!(rewrite(&out).1, 0);
        // A name that only looks like it is not touched.
        assert_eq!(rewrite("my.string.join\nstring.join\n").1, 0);
    }

    #[test]
    fn the_edition_line_is_set_or_added() {
        assert_eq!(
            set_edition(
                "[package]\nname = \"a\"\nedition = \"2026\"\n\n[dependencies]\nx = \"1\"\n",
                TO
            ),
            "[package]\nname = \"a\"\nedition = \"2027-preview\"\n\n[dependencies]\nx = \"1\"\n"
        );
        assert_eq!(
            set_edition("[package]\nname = \"a\"\n\n[dependencies]\n", TO),
            "[package]\nname = \"a\"\n\nedition = \"2027-preview\"\n[dependencies]\n"
        );
        assert_eq!(
            set_edition("[package]\nname = \"a\"\n", TO),
            "[package]\nname = \"a\"\nedition = \"2027-preview\"\n"
        );
    }
}
