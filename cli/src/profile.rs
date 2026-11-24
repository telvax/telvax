//! `telvax profile`: runs a function and reports where the time and the memory went.

use telvax_runtime::Interp;
use telvax_runtime::Value;
use telvax_runtime::profile::Profiler;

use super::{CliError, Invocation, call_arguments, check_project, failed, take_allow};

/// What the report is written to, besides the terminal.
#[derive(Default)]
struct Outputs {
    top: Option<usize>,
    json: Option<String>,
    collapsed: Option<String>,
    speedscope: Option<String>,
}

/// Splits the options of `profile` out of `args`; the rest are the file, the function and its
/// arguments.
fn options<'a>(args: &[&'a str]) -> Result<(Outputs, Vec<&'a str>), CliError> {
    let mut out = Outputs::default();
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| match it.next() {
            Some(v) => Ok((*v).to_string()),
            None => failed(format!("{name} takes a value")),
        };
        match *a {
            "--top" => {
                out.top = Some(value("--top")?.parse().or_else(|_| {
                    failed("--top takes a number of statements, such as `--top 20`")
                })?)
            }
            "--json" => out.json = Some(value("--json")?),
            "--collapsed" => out.collapsed = Some(value("--collapsed")?),
            "--speedscope" => out.speedscope = Some(value("--speedscope")?),
            other => rest.push(other),
        }
    }
    Ok((out, rest))
}

/// `telvax profile <file.tvx> [function] [args...] [--allow list] [--top N] [--json file]
/// [--collapsed file] [--speedscope file]`
pub fn run(args: &[&str]) -> Result<(), CliError> {
    let (caps, args) = take_allow(args)?;
    let (outputs, args) = options(&args)?;
    let [file, rest @ ..] = args.as_slice() else {
        return Err(CliError::Usage);
    };
    let (func, words) = match rest {
        [func, words @ ..] if !func.starts_with('-') => (*func, words),
        _ => ("main", rest),
    };
    let checked = check_project(file)?;
    let map = checked.map.clone();
    let interp = Interp::with_caps(checked.hir, false, caps).or_else(|e| failed(e.to_string()))?;
    if interp.signature(func).is_none() {
        let mut names = interp.function_names();
        names.sort();
        return failed(format!(
            "`{file}` has no function `{func}`; it has: {}",
            names.join(", ")
        ));
    }
    let values = match call_arguments(&interp, file, func, words)? {
        Invocation::Help(text) => {
            print!("{text}");
            return Ok(());
        }
        Invocation::Call(values) => values,
    };

    let profiler = Profiler::new();
    interp.set_profiler(Some(profiler.clone()));
    let result = interp.call(func, values);
    interp.set_profiler(None);
    let report = profiler.report();

    let locate = |offset: usize| -> String {
        let Some((source, local)) = map.locate(offset) else {
            return "?".to_string();
        };
        let before = &source.text[..local.min(source.text.len())];
        let line = before.matches('\n').count() + 1;
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        let text = source.text[start..].lines().next().unwrap_or("").trim();
        format!("{}:{line}  {text}", source.name)
    };

    if let Ok(value) = &result
        && !matches!(value, Value::Unit)
    {
        println!("{value}");
    }
    println!(
        "\nprofile of {func}\n{}",
        report.render(&locate, outputs.top.unwrap_or(15))
    );
    let write = |path: &Option<String>, text: String| -> Result<(), CliError> {
        match path {
            Some(p) => {
                std::fs::write(p, text).or_else(|e| failed(format!("cannot write {p}: {e}")))
            }
            None => Ok(()),
        }
    };
    write(
        &outputs.json,
        serde_json::to_string_pretty(&report.to_json(&locate)).unwrap_or_default(),
    )?;
    write(&outputs.collapsed, report.collapsed())?;
    write(
        &outputs.speedscope,
        serde_json::to_string(&report.speedscope(func)).unwrap_or_default(),
    )?;
    result.map(|_| ()).or_else(|e| failed(e.to_string()))
}
