//! `telvax check [--json] [file...]`: parse and type-check files, or the package in the current
//! directory when none is named.

use std::io::IsTerminal;
use std::path::Path;

use serde_json::{Value, json};
use telvax_diagnostics::{Diagnostic, Severity};

use crate::{CliError, failed};

/// 1-based line and column (in characters) of `offset` in `text`.
fn position(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rfind('\n')
        .map_or(before, |n| &before[n + 1..])
        .chars()
        .count()
        + 1;
    (line, column)
}

/// A diagnostic as a JSON object with the file, the span as line and column, the severity, the
/// message and the notes.
fn to_json(loaded: &telvax_modules::Loaded, d: &Diagnostic) -> Value {
    let place = |offset: usize| {
        loaded.map.locate(offset).map(|(file, at)| {
            let (line, column) = position(&file.text, at);
            (file.name.clone(), line, column)
        })
    };
    let start = place(d.span.start);
    // The end is exclusive, so it is read from the last byte of the span.
    let end = place(d.span.end.saturating_sub(1).max(d.span.start));
    json!({
        "file": start.as_ref().map(|s| s.0.clone()),
        "line": start.as_ref().map(|s| s.1),
        "column": start.as_ref().map(|s| s.2),
        "end_line": end.as_ref().map(|e| e.1),
        "end_column": end.as_ref().map(|e| e.2 + 1),
        "severity": match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        },
        "message": loaded.map.demangle(&d.message),
        "notes": d.notes.iter().map(|n| loaded.map.demangle(n)).collect::<Vec<_>>(),
    })
}

/// The files to check: the ones named, or the entry of the package around the current directory.
fn targets(files: &[&str]) -> Result<Vec<String>, CliError> {
    if !files.is_empty() {
        return Ok(files.iter().map(|f| (*f).to_string()).collect());
    }
    match telvax_pkg::Project::discover(Path::new(".")) {
        Ok(Some(project)) => {
            let entry = project.entry().or_else(failed)?;
            let shown = entry
                .strip_prefix(std::env::current_dir().unwrap_or_default())
                .unwrap_or(&entry);
            Ok(vec![shown.display().to_string()])
        }
        Ok(None) => failed(
            "name a file to check, or run `telvax check` inside a package (a directory with a telvax.toml)",
        ),
        Err(e) => failed(e),
    }
}

pub fn run(args: &[&str]) -> Result<(), CliError> {
    let json_output = args.contains(&"--json");
    // A warning (a deprecated form) counts as an error, for a CI that wants none.
    let deny_warnings = args.contains(&"--deny-warnings");
    let files: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| *a != "--json" && *a != "--deny-warnings")
        .collect();
    if let Some(flag) = files.iter().find(|f| f.starts_with('-')) {
        return failed(format!(
            "`check` has no option `{flag}`; it takes `--json` and `--deny-warnings`"
        ));
    }
    let targets = targets(&files)?;
    let color = std::io::stderr().is_terminal();
    let mut records: Vec<Value> = Vec::new();
    let mut failing = 0;
    for file in &targets {
        let path = Path::new(file);
        let project = telvax_pkg::Project::discover(path);
        let edition = crate::edition_of(project.as_ref().ok().and_then(Option::as_ref));
        let deps = match project {
            Ok(Some(project)) => project.dependency_packages(),
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(e),
        };
        let loaded = deps.and_then(|deps| telvax_modules::load(path, &deps));
        let loaded = match loaded {
            Ok(loaded) => loaded,
            Err(message) => {
                failing += 1;
                if json_output {
                    records.push(json!({
                        "file": file, "line": null, "column": null, "end_line": null,
                        "end_column": null, "severity": "error", "message": message, "notes": [],
                    }));
                } else {
                    eprintln!("error: {message}");
                }
                continue;
            }
        };
        let (_, lowered) = telvax_hir::lower_in(&loaded.program, edition);
        let all: Vec<Diagnostic> = loaded.diagnostics.iter().cloned().chain(lowered).collect();
        let errors = all.iter().filter(|d| d.is_error() || deny_warnings).count();
        if json_output {
            records.extend(all.iter().map(|d| to_json(&loaded, d)));
        } else {
            eprint!("{}", loaded.map.render(&all, color));
            if errors == 0 {
                eprintln!("`{file}` ok");
            } else {
                eprintln!("error: {errors} error(s) in `{file}`");
            }
        }
        failing += usize::from(errors > 0);
    }
    if json_output {
        println!("{}", Value::Array(records));
    } else if targets.len() > 1 {
        eprintln!("{} file(s) checked, {} with errors", targets.len(), failing);
    }
    if failing > 0 { failed("") } else { Ok(()) }
}
