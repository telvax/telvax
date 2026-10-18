mod bundle;
mod check;
mod doctor;
mod native;
mod profile;
mod repl;
mod serve;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use telvax_diagnostics::Diagnostic;
use telvax_runtime::{Interp, Value};

const USAGE: &str = "\
usage:
  telvax check [--json] [file.tvx...]                       no file: the package in this directory
  telvax lint <file.tvx> [--allow|--warn|--deny rule]... [--deny-warnings] [--list-rules]
                                                            unused variables and functions, unreachable cases
  telvax fmt [--check] [--diff] [file.tvx|dir]...             format in place (`-` reads stdin; no path: this directory)
  telvax schema <file.csv|.parquet|.arrow> [--name Row]      the `type` for the columns of a data file
  telvax doc [--public] [-o out.md] [file.tvx|dir]...      Markdown reference from the types, signatures and `#` comments
  telvax test [file.tvx|dir] [name] [--allow list] [--coverage] [--fail-under N]
                                                            run the `test_*` functions (a directory: `test_*.tvx`, `*_test.tvx`)
  telvax run <file.tvx> [function] [args...] [--allow list] [--quantize int8|int8-channel|int4-channel] [--device gpu]
  telvax profile <file.tvx> [function] [args...] [--allow list] [--top N] [--json f] [--collapsed f] [--speedscope f]
                                                            time per function, line and std operation, and memory
  telvax doctor                                             what this build can do and how it is set up
  telvax repl [file.tvx...] [--allow list]                  type definitions, variables and expressions and see the result
  telvax dap                                                debug adapter over stdio (breakpoints, stepping, variables)
  telvax lsp                                                language server over stdio
  telvax worker [--listen host:port] [--token T]            run calls for a coordinator
  telvax serve <file.tvx> [--port N] [--host H] [--seed docs.jsonl] [--live] [--allow list] [--quantize int8|int8-channel|int4-channel] [--device gpu] [--metrics] [--log-format json|text] [--cors origins] [--rate-limit N/min] [--security-headers] [--process-time-header] [--shutdown-wait N] [--error-format telvax|detail]
  telvax build --serve <file.tvx> [-o out] [--dockerfile]   single-binary server (+ Dockerfile)
  telvax jit <file.tvx> <function> [args...]                 (native, needs --features llvm)
  telvax build <file.tvx> [-o out] [--emit exe|obj|llvm-ir|mlir|ptx|stablehlo] [-O 0-3] [--sm sm_80]
                                                    (all but stablehlo need --features llvm)

--allow takes a list of fs.read, fs.write, fs, env, net, all, none. Without it everything is allowed.";

/// Stack for the thread that does the work. Checking and running recurse once per level of a
/// program's nesting (a sum of a few hundred terms, calls inside calls), and the 8 MB of a main
/// thread is not enough for the deepest programs. Only the pages that are used are ever mapped.
const STACK_BYTES: usize = 512 * 1024 * 1024;

/// Counts allocations while `telvax profile` runs; otherwise it only checks a flag.
#[global_allocator]
static ALLOC: telvax_runtime::alloc_count::CountingAlloc =
    telvax_runtime::alloc_count::CountingAlloc;

fn main() -> ExitCode {
    match std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(real_main)
    {
        Ok(worker) => worker.join().unwrap_or(ExitCode::FAILURE),
        Err(_) => real_main(),
    }
}

fn real_main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match bundle::embedded() {
        Some(bundle) => run_bundled(&bundle, &args),
        None => dispatch(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Unknown(word)) => {
            eprint!("error: unknown command `{word}`");
            if let Some(near) = doctor::closest_command(&word) {
                eprint!("; did you mean `{near}`?");
            }
            eprintln!("\nrun `telvax --help` for the commands");
            ExitCode::from(2)
        }
        Err(CliError::Usage) => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(CliError::Failed(msg)) => {
            if !msg.is_empty() {
                eprintln!("error: {msg}");
            }
            ExitCode::FAILURE
        }
    }
}

fn dispatch(args: &[String]) -> Result<(), CliError> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["fmt", rest @ ..] => fmt(rest),
        ["lint", "--list-rules"] => {
            for rule in telvax_lint::Rule::ALL {
                println!("{:<18}{}", rule.name(), rule.describe());
            }
            Ok(())
        }
        ["schema", rest @ ..] => schema(rest),
        ["doc", rest @ ..] => doc(rest),
        ["lint", file, rest @ ..] => lint(file, rest),
        ["--version" | "-V" | "version"] => {
            println!("telvax {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["--help" | "-h" | "help"] => {
            println!("{USAGE}");
            Ok(())
        }
        ["doctor"] => {
            print!("{}", doctor::report());
            Ok(())
        }
        ["check", rest @ ..] => check::run(rest),
        // Without a function name, `main` runs: `telvax run app.tvx --allow fs`.
        ["run", file, rest @ ..] => match rest {
            [func, args @ ..] if !func.starts_with('-') => run(file, func, args),
            _ => run(file, "main", rest),
        },
        // Without a path, `test` looks in this directory.
        ["test", rest @ ..] => {
            let (path, rest) = match rest {
                [path, rest @ ..] if !path.starts_with('-') => (*path, rest),
                _ => (".", rest),
            };
            let (caps, rest) = take_allow(rest)?;
            let (cov, rest) = take_coverage(&rest)?;
            match rest.as_slice() {
                [] => test(path, None, caps, cov),
                [filter] => test(path, Some(filter), caps, cov),
                _ => Err(CliError::Usage),
            }
        }
        ["serve", file, rest @ ..] => {
            let (workers, rest) = take_workers(rest);
            let opts = serve::Options::parse("127.0.0.1", &rest)?;
            let checked = check_project(file)?;
            serve::run(checked.hir, &opts, workers.map(|w| (w, checked.bundle)))
        }
        ["worker", rest @ ..] => worker(rest),
        ["build", "--serve", file, rest @ ..] => bundle::build_serve(file, rest),
        ["profile", rest @ ..] => profile::run(rest),
        ["repl", rest @ ..] => repl::run(rest),
        ["dap"] => telvax_dap::run_stdio().or_else(|e| failed(e.to_string())),
        ["lsp"] => telvax_lsp::run_stdio().or_else(|e| failed(e.to_string())),
        ["jit", file, func, rest @ ..] => native::jit(file, func, rest),
        ["build", file, rest @ ..] => native::build(file, rest),
        [word, ..] if !doctor::COMMANDS.contains(word) && !word.starts_with('-') => {
            Err(CliError::Unknown((*word).to_string()))
        }
        _ => Err(CliError::Usage),
    }
}

/// A binary produced by `telvax build --serve`: serve the embedded program. Containers set
/// `PORT` / `HOST`; flags win over the environment.
/// `telvax worker [--listen host:port] [--token T]`: runs calls sent by a coordinator.
fn worker(flags: &[&str]) -> Result<(), CliError> {
    let mut listen = "127.0.0.1:7077".to_string();
    let mut token = std::env::var("TELVAX_CLUSTER_TOKEN").ok();
    let mut it = flags.iter();
    while let Some(f) = it.next() {
        match (*f, it.next()) {
            ("--listen", Some(a)) => listen = a.to_string(),
            ("--token", Some(t)) => token = Some(t.to_string()),
            _ => return Err(CliError::Usage),
        }
    }
    telvax_cluster::worker::serve(&listen, token).or_else(failed)
}

/// `--healthcheck [path]` on a bundled server: asks the server of this machine for `path` (`/healthz`
/// if left out) and succeeds on a 2xx answer. A container has no `curl` to do it.
fn healthcheck(flags: &[&str]) -> Result<(), CliError> {
    use std::io::{Read, Write};
    let at = flags
        .iter()
        .position(|f| *f == "--healthcheck")
        .unwrap_or(0);
    let path = flags
        .get(at + 1)
        .filter(|p| p.starts_with('/'))
        .copied()
        .unwrap_or("/healthz");
    let port = match flags.iter().position(|f| *f == "--port") {
        Some(i) => flags.get(i + 1).map(|p| p.to_string()),
        None => std::env::var("PORT").ok(),
    }
    .unwrap_or_else(|| "8080".into());
    let address = format!("127.0.0.1:{port}");
    let timeout = std::time::Duration::from_secs(3);
    let socket = address
        .parse()
        .or_else(|_| failed(format!("invalid port `{port}`")))?;
    let mut stream = std::net::TcpStream::connect_timeout(&socket, timeout)
        .or_else(|e| failed(format!("cannot reach {address}: {e}")))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nhost: {address}\r\nconnection: close\r\n\r\n")
                .as_bytes(),
        )
        .or_else(|e| failed(format!("cannot ask {address}: {e}")))?;
    let mut head = [0u8; 32];
    let n = stream.read(&mut head).unwrap_or(0);
    let line = String::from_utf8_lossy(&head[..n]);
    match line.split(' ').nth(1) {
        Some(code) if code.starts_with('2') => Ok(()),
        _ => failed(format!("{address}{path} answered `{}`", line.trim())),
    }
}

fn run_bundled(bundle: &bundle::Bundle, args: &[String]) -> Result<(), CliError> {
    let flags: Vec<&str> = args.iter().map(String::as_str).collect();
    if flags.contains(&"--healthcheck") {
        return healthcheck(&flags);
    }
    let mut opts = serve::Options::parse(
        &std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into()),
        &flags,
    )?;
    if !flags.contains(&"--port")
        && let Ok(p) = std::env::var("PORT")
    {
        opts.port = p
            .parse()
            .or_else(|_| failed(format!("invalid PORT `{p}`")))?;
    }
    opts.live |= std::env::var("TELVAX_LIVE").is_ok_and(|v| v == "1");
    let loaded = bundle.load();
    telvax_runtime::set_embedded_files(bundle.data.clone());
    serve::run(lower_loaded(&loaded)?.0, &opts, None)
}

/// Splits `--workers host:port,host:port` out of `args`; `TELVAX_WORKERS` is the default.
pub fn take_workers<'a>(args: &[&'a str]) -> (Option<Vec<String>>, Vec<&'a str>) {
    let mut workers = std::env::var("TELVAX_WORKERS")
        .ok()
        .filter(|w| !w.is_empty());
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "--workers" {
            workers = it.next().map(|w| w.to_string());
        } else {
            rest.push(*a);
        }
    }
    (
        workers.map(|w| {
            w.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        }),
        rest,
    )
}

/// Sends `std.dist.map` calls to `workers`.
pub fn attach_cluster(
    interp: &Interp,
    workers: Vec<String>,
    image: &bundle::Bundle,
) -> Result<(), CliError> {
    let token = std::env::var("TELVAX_CLUSTER_TOKEN").ok();
    let exec = telvax_cluster::ClusterExecutor::new(workers, image, token).or_else(failed)?;
    interp.set_executor(std::sync::Arc::new(exec));
    Ok(())
}

/// Splits `--device cpu|gpu` out of `args`.
pub fn take_device<'a>(
    args: &[&'a str],
) -> Result<(telvax_runtime::Device, Vec<&'a str>), CliError> {
    let mut device = telvax_runtime::Device::Cpu;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "--device" {
            device = match it.next().copied() {
                Some("gpu") => telvax_runtime::Device::Gpu,
                Some("cpu") => telvax_runtime::Device::Cpu,
                other => {
                    return failed(format!(
                        "--device takes `cpu` or `gpu`, not {}",
                        other.unwrap_or("nothing")
                    ));
                }
            };
        } else {
            rest.push(*a);
        }
    }
    Ok((device, rest))
}

/// Splits `--allow fs.read,net,...` out of `args`. Without it everything is allowed.
pub fn take_allow<'a>(args: &[&'a str]) -> Result<(telvax_runtime::Caps, Vec<&'a str>), CliError> {
    let mut caps = telvax_runtime::Caps::ALL;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "--allow" {
            let Some(list) = it.next() else {
                return failed("--allow takes a list such as `fs.read,net`, or `none`");
            };
            caps = telvax_runtime::Caps::parse(list).or_else(failed)?;
        } else {
            rest.push(*a);
        }
    }
    Ok((caps, rest))
}

/// Splits `--coverage` and `--fail-under N` out of `args`. Either one turns the report on;
/// `--fail-under` also sets the percentage of lines the tests must run.
fn take_coverage<'a>(args: &[&'a str]) -> Result<(Option<Coverage>, Vec<&'a str>), CliError> {
    let mut cov = None;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--coverage" => {
                cov.get_or_insert_with(Coverage::default);
            }
            "--fail-under" => {
                let Some(n) = it.next().and_then(|n| n.parse::<f64>().ok()) else {
                    return failed("--fail-under takes a percentage such as `90`");
                };
                if !(0.0..=100.0).contains(&n) {
                    return failed("--fail-under takes a percentage from 0 to 100");
                }
                cov.get_or_insert_with(Coverage::default).fail_under = Some(n);
            }
            _ => rest.push(*a),
        }
    }
    Ok((cov, rest))
}

/// Splits `--quantize int8|int8-channel|int4-channel|none` out of `args`.
pub fn take_quantize<'a>(
    args: &[&'a str],
) -> Result<(telvax_hir::Quantization, Vec<&'a str>), CliError> {
    let mut quant = telvax_hir::Quantization::None;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "--quantize" {
            quant = match it.next().copied() {
                Some("int8") => telvax_hir::Quantization::Int8,
                Some("int8-channel") => telvax_hir::Quantization::Int8Channel,
                Some("int4-channel") => telvax_hir::Quantization::Int4Channel,
                Some("none") => telvax_hir::Quantization::None,
                other => {
                    return failed(format!(
                        "--quantize takes `int8`, `int8-channel`, `int4-channel` or `none`, not {}",
                        other.unwrap_or("nothing")
                    ));
                }
            };
        } else {
            rest.push(*a);
        }
    }
    Ok((quant, rest))
}

#[derive(Debug)]
pub enum CliError {
    Usage,
    /// The first word is not a command.
    Unknown(String),
    Failed(String),
}

pub fn failed<T>(msg: impl Into<String>) -> Result<T, CliError> {
    Err(CliError::Failed(msg.into()))
}

#[derive(Debug)]
pub struct Checked {
    pub hir: telvax_hir::hir::Program,
    /// Everything needed to rebuild the program without the source tree.
    pub bundle: bundle::Bundle,
    /// Where every span points, for reports by file and line.
    pub map: telvax_modules::SourceMap,
}

/// Type-checks a loaded program, printing every diagnostic against the file it belongs to.
fn lower_loaded(
    loaded: &telvax_modules::Loaded,
) -> Result<(telvax_hir::hir::Program, usize), CliError> {
    let (hir, diags) = telvax_hir::lower(&loaded.program);
    let all: Vec<Diagnostic> = loaded.diagnostics.iter().cloned().chain(diags).collect();
    eprint!(
        "{}",
        loaded.map.render(&all, std::io::stderr().is_terminal())
    );
    let errors = all.iter().filter(|d| d.is_error()).count();
    if errors > 0 {
        let name = loaded
            .map
            .files
            .first()
            .map_or("program", |f| f.name.as_str());
        return failed(format!("{errors} error(s) in `{name}`"));
    }
    Ok((hir, all.len()))
}

/// Loads `file` with its modules and the dependencies of its project (if it has a `telvax.toml`),
/// then type-checks everything. Errors from any file are printed against that file.
pub fn check_project(file: &str) -> Result<Checked, CliError> {
    let path = Path::new(file);
    let deps = match telvax_pkg::Project::discover(path) {
        Ok(Some(project)) => project.dependency_packages().or_else(failed)?,
        Ok(None) => Vec::new(),
        Err(e) => return failed(e),
    };
    let loaded = telvax_modules::load(path, &deps).or_else(failed)?;
    let (hir, _) = lower_loaded(&loaded)?;
    let mut bundle = bundle::Bundle::new(file, &loaded, &deps);
    bundle.data = embed_files(path, &loaded)?;
    telvax_runtime::set_embedded_files(bundle.data.clone());
    Ok(Checked {
        hir,
        bundle,
        map: loaded.map.clone(),
    })
}

/// Reads the files named by `std.fs.embed`, relative to the directory of the main file.
fn embed_files(
    main: &Path,
    loaded: &telvax_modules::Loaded,
) -> Result<Vec<(String, String)>, CliError> {
    let dir = main.parent().unwrap_or(Path::new(""));
    loaded
        .embedded_paths()
        .or_else(failed)?
        .into_iter()
        .map(|p| match std::fs::read_to_string(dir.join(&p)) {
            Ok(text) => Ok((p, text)),
            Err(e) => failed(format!("cannot embed `{p}`: {e}")),
        })
        .collect()
}

/// Parses and checks `file`, printing diagnostics. Fails if any is an error.
pub fn check(file: &str) -> Result<telvax_hir::hir::Program, CliError> {
    check_project(file).map(|c| c.hir)
}

/// The values for the command-line words after the function name, read as the types the function
/// declares: a `Text` parameter takes the word as it is (so `"123"` stays text), the others take it
/// as JSON (`3`, `2.5`, `true`, `[1, 2]`).
fn call_arguments(interp: &Interp, func: &str, words: &[&str]) -> Result<Vec<Value>, CliError> {
    let Some((params, _)) = interp.signature(func) else {
        return failed(format!("no function `{func}`"));
    };
    if params.len() != words.len() {
        let list: Vec<String> = params.iter().map(|(n, t)| format!("{n}: {t}")).collect();
        return failed(format!(
            "`{func}` takes {} argument(s) ({}), got {}",
            params.len(),
            list.join(", "),
            words.len()
        ));
    }
    params
        .iter()
        .zip(words)
        .map(|((name, ty), word)| {
            let json = if *ty == telvax_hir::Ty::str() {
                serde_json::Value::String((*word).to_string())
            } else {
                serde_json::from_str(word).or_else(|_| {
                    failed(format!(
                        "argument `{name}` of `{func}` is {ty}, and `{word}` is not a value of that type (write numbers, true, false, null, [..] or {{..}} as JSON)"
                    ))
                })?
            };
            interp
                .value_from_json(ty, &json)
                .or_else(|e| failed(format!("argument `{name}` of `{func}` is {ty}: {e}")))
        })
        .collect()
}

/// `telvax test [path] [name]`: runs every function called `test_*` that takes no arguments. A
/// test passes when it returns and fails when it stops with an error (`assert`, `assert_eq`, ...).
/// A directory runs every `test_*.tvx` and `*_test.tvx` file under it.
fn test(
    path: &str,
    filter: Option<&str>,
    caps: telvax_runtime::Caps,
    mut cov: Option<Coverage>,
) -> Result<(), CliError> {
    if let Some(cov) = cov.as_mut() {
        cov.root = coverage_root(Path::new(path));
    }
    let outcome = test_all(path, filter, caps, &mut cov);
    match (outcome, cov) {
        (Err(e), _) => Err(e),
        (Ok(()), None) => Ok(()),
        (Ok(()), Some(cov)) => cov.finish(),
    }
}

fn test_all(
    path: &str,
    filter: Option<&str>,
    caps: telvax_runtime::Caps,
    cov: &mut Option<Coverage>,
) -> Result<(), CliError> {
    if !Path::new(path).is_dir() {
        let (passed, failures) = test_file(path, filter, caps, cov)?;
        println!("{passed} passed, {failures} failed");
        return if failures > 0 { failed("") } else { Ok(()) };
    }
    let mut files = Vec::new();
    collect_test_files(Path::new(path), &mut files);
    files.sort();
    if files.is_empty() {
        return failed(format!(
            "no test files in `{path}` (a test file is named `test_*.tvx` or `*_test.tvx`)"
        ));
    }
    let (mut passed, mut failures, mut ran) = (0, 0, 0);
    for file in &files {
        let file = file.to_string_lossy();
        println!("== {file}");
        match test_file(&file, filter, caps, cov) {
            Ok((p, f)) => {
                passed += p;
                failures += f;
                ran += 1;
            }
            Err(CliError::Failed(msg)) if msg.starts_with("no test functions") => {}
            Err(CliError::Failed(msg)) => {
                failures += 1;
                if !msg.is_empty() {
                    eprintln!("error: {msg}");
                }
            }
            Err(e) => return Err(e),
        }
    }
    if ran == 0 && failures == 0 {
        return failed("no test functions found (a test is a function named `test_...`)");
    }
    println!("{passed} passed, {failures} failed");
    if failures > 0 { failed("") } else { Ok(()) }
}

/// The lines the tests ran, per file, for `--coverage`.
#[derive(Default)]
struct Coverage {
    fail_under: Option<f64>,
    /// Files outside this directory (dependencies) are not counted.
    root: PathBuf,
    inside: std::collections::HashMap<String, bool>,
    /// File -> (lines with code, lines that ran).
    files: std::collections::BTreeMap<
        String,
        (
            std::collections::BTreeSet<usize>,
            std::collections::BTreeSet<usize>,
        ),
    >,
}

/// What the tests are about: the project of `path` if it has a `telvax.toml`, else its directory.
fn coverage_root(path: &Path) -> PathBuf {
    let root = match telvax_pkg::Project::discover(path) {
        Ok(Some(project)) => project.dir,
        _ if path.is_dir() => path.to_path_buf(),
        _ => path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
    };
    std::fs::canonicalize(&root).unwrap_or(root)
}

impl Coverage {
    /// Records, for one loaded program, which of `points` (statement offsets) ran.
    fn add(
        &mut self,
        map: &telvax_modules::SourceMap,
        points: &[usize],
        ran: &std::collections::HashSet<usize>,
    ) {
        for &offset in points {
            let Some((file, local)) = map.locate(offset) else {
                continue;
            };
            let root = &self.root;
            let inside = *self.inside.entry(file.name.clone()).or_insert_with(|| {
                std::fs::canonicalize(&file.name).is_ok_and(|p| p.starts_with(root))
            });
            if !inside {
                continue;
            }
            let line = file.text.as_bytes()[..local.min(file.text.len())]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                + 1;
            let entry = self.files.entry(file.name.clone()).or_default();
            entry.0.insert(line);
            if ran.contains(&offset) {
                entry.1.insert(line);
            }
        }
    }

    /// Prints the table and applies `--fail-under`.
    fn finish(&self) -> Result<(), CliError> {
        let percent = |hit: usize, all: usize| {
            if all == 0 {
                100.0
            } else {
                hit as f64 * 100.0 / all as f64
            }
        };
        println!("\ncoverage (lines that ran / lines with code):");
        let (mut hit_total, mut all_total) = (0, 0);
        let here = std::env::current_dir().ok();
        for (name, (all, hit)) in &self.files {
            let name = here
                .as_ref()
                .and_then(|h| Path::new(name).strip_prefix(h).ok())
                .map_or(name.as_str(), |p| p.to_str().unwrap_or(name));
            hit_total += hit.len();
            all_total += all.len();
            let missing: Vec<usize> = all.difference(hit).copied().collect();
            let missing = if missing.is_empty() {
                String::new()
            } else {
                format!("   missing {}", line_ranges(&missing))
            };
            println!(
                "  {name}  {}/{}  {:.1}%{missing}",
                hit.len(),
                all.len(),
                percent(hit.len(), all.len())
            );
        }
        let total = percent(hit_total, all_total);
        println!("  total  {hit_total}/{all_total}  {total:.1}%");
        match self.fail_under {
            Some(min) if total < min => failed(format!(
                "coverage is {total:.1}%, below the {min}% that --fail-under asks for"
            )),
            _ => Ok(()),
        }
    }
}

/// `[1, 2, 3, 7]` as `1-3, 7`.
fn line_ranges(lines: &[usize]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let mut j = i;
        while j + 1 < lines.len() && lines[j + 1] == lines[j] + 1 {
            j += 1;
        }
        out.push(if i == j {
            lines[i].to_string()
        } else {
            format!("{}-{}", lines[i], lines[j])
        });
        i = j + 1;
    }
    out.join(", ")
}

/// The `test_*.tvx` and `*_test.tvx` files under `dir`.
fn collect_test_files(dir: &Path, out: &mut Vec<PathBuf>) {
    collect_tvx_files(dir, out);
    out.retain(|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("test_") || s.ends_with("_test"))
    });
}

/// Runs the tests of one file and returns how many passed and failed.
fn test_file(
    file: &str,
    filter: Option<&str>,
    caps: telvax_runtime::Caps,
    cov: &mut Option<Coverage>,
) -> Result<(usize, usize), CliError> {
    let checked = check_project(file)?;
    let mut names: Vec<String> = checked
        .hir
        .fns
        .iter()
        .filter(|f| f.name.starts_with("test_"))
        .filter(|f| filter.is_none_or(|n| f.name.contains(n)))
        .map(|f| f.name.clone())
        .collect();
    names.sort();
    if names.is_empty() {
        return failed("no test functions found (a test is a function named `test_...`)");
    }
    // Code that only supports the tests is not what the tests are measured on.
    let points = cov.as_ref().map(|_| {
        telvax_hir::coverage::points(&checked.hir, |name| {
            ["test_", "fixture_", "teardown_", "cases_"]
                .iter()
                .any(|p| name.starts_with(p))
        })
    });
    let interp = Interp::with_caps(checked.hir, false, caps).or_else(|e| failed(e.to_string()))?;
    if points.is_some() {
        interp.start_coverage();
    }
    let mut failures = 0;
    let mut total = 0;
    for name in &names {
        let runs = match test_runs(&interp, name) {
            Ok(runs) => runs,
            Err(e) => {
                total += 1;
                failures += 1;
                println!("FAILED  {name}: {e}");
                continue;
            }
        };
        for (label, given) in runs {
            total += 1;
            let outcome = run_test(&interp, name, given);
            interp.reset_mocks();
            match outcome {
                Ok(()) => println!("ok      {label}"),
                Err(e) => {
                    failures += 1;
                    println!("FAILED  {label}: {e}");
                }
            }
        }
    }
    if let (Some(cov), Some(points)) = (cov.as_mut(), points) {
        cov.add(&checked.map, &points, &interp.covered());
    }
    Ok((total - failures, failures))
}

/// The runs of a test: one, or one per case when there is a `cases_<name>()` function, where the
/// name is the test's without `test_`. A case gives the test's first parameters (a tuple gives
/// several); the others are fixtures.
fn test_runs(interp: &Interp, name: &str) -> Result<Vec<(String, Vec<Value>)>, String> {
    let cases = format!("cases_{}", name.strip_prefix("test_").unwrap_or(name));
    let Some((cases_params, ret)) = interp.signature(&cases) else {
        return Ok(vec![(name.to_string(), Vec::new())]);
    };
    let telvax_hir::Ty::Vec(case_ty) = &ret else {
        return Err(format!(
            "`{cases}` must return a list of cases: write `fn {cases}() -> List[...]`"
        ));
    };
    if !cases_params.is_empty() {
        return Err(format!("`{cases}` takes no arguments"));
    }
    let given = match &**case_ty {
        telvax_hir::Ty::Tuple(parts) => parts.clone(),
        one => vec![one.clone()],
    };
    let params = interp.signature(name).map(|(p, _)| p).unwrap_or_default();
    if given.len() > params.len() {
        return Err(format!(
            "a case of `{cases}` has {} value(s) but `{name}` takes {} parameter(s)",
            given.len(),
            params.len()
        ));
    }
    for ((param, ty), case) in params.iter().zip(&given) {
        if !ty.accepts(case) {
            return Err(format!(
                "`{name}` takes `{param}: {ty}` but the cases of `{cases}` give `{case}`"
            ));
        }
    }
    let Value::Vec(list) = interp
        .call(&cases, Vec::new())
        .map_err(|e| format!("`{cases}` failed: {e}"))?
    else {
        return Err(format!("`{cases}` must return a list"));
    };
    if list.is_empty() {
        return Err(format!("`{cases}` has no cases"));
    }
    Ok(list
        .snapshot()
        .into_iter()
        .map(|case| {
            let values = match (&case, given.len()) {
                (Value::Tuple(items), n) if n > 1 => items.to_vec(),
                _ => vec![case.clone()],
            };
            (format!("{name}[{case}]"), values)
        })
        .collect())
}

/// Runs one test. Each parameter of the test that a case does not give is a fixture: `fn test_x(db: Db)` gets the value of
/// `fixture_db()`, made again for every test. A `teardown_db(db)` function, if there is one, runs
/// after the test whether it passed or not.
fn run_test(interp: &Interp, name: &str, given: Vec<Value>) -> Result<(), String> {
    let params = interp.signature(name).map(|(p, _)| p).unwrap_or_default();
    let mut made: Vec<(String, Value)> = Vec::new();
    let result = params
        .iter()
        .enumerate()
        .map(|(i, (p, ty))| match given.get(i) {
            Some(value) => Ok(value.clone()),
            None => fixture(interp, p, Some(ty), &mut made, &mut Vec::new()),
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|args| {
            interp
                .call(name, args)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    let mut teardown = Ok(());
    for (fixture_name, value) in made.iter().rev() {
        let t = format!("teardown_{fixture_name}");
        let Some((params, _)) = interp.signature(&t) else {
            continue;
        };
        let done = if params.len() == 1 {
            interp
                .call(&t, vec![value.clone()])
                .map(|_| ())
                .map_err(|e| format!("`{t}` failed: {e}"))
        } else {
            Err(format!(
                "`{t}` takes the fixture to clean up: write `fn {t}({fixture_name}: ...)`"
            ))
        };
        if teardown.is_ok() {
            teardown = done;
        }
    }
    result.and(teardown)
}

/// The value of the fixture `name`, made once per test; `made` holds the ones already made, in order.
fn fixture(
    interp: &Interp,
    name: &str,
    wanted: Option<&telvax_hir::Ty>,
    made: &mut Vec<(String, Value)>,
    stack: &mut Vec<String>,
) -> Result<Value, String> {
    if let Some((_, v)) = made.iter().find(|(n, _)| n == name) {
        return Ok(v.clone());
    }
    if stack.iter().any(|n| n == name) {
        return Err(format!(
            "fixture `{name}` needs itself ({} -> {name})",
            stack.join(" -> ")
        ));
    }
    let function = format!("fixture_{name}");
    let Some((params, ret)) = interp.signature(&function) else {
        return Err(format!(
            "the test asks for `{name}` but there is no fixture: write `fn {function}() -> ...`"
        ));
    };
    if let Some(ty) = wanted
        && !ty.accepts(&ret)
    {
        return Err(format!(
            "the test takes `{name}: {ty}` but `{function}` returns `{ret}`"
        ));
    }
    stack.push(name.to_string());
    let args = params
        .iter()
        .map(|(p, ty)| fixture(interp, p, Some(ty), made, stack))
        .collect::<Result<Vec<_>, _>>()?;
    stack.pop();
    let value = interp
        .call(&function, args)
        .map_err(|e| format!("`{function}` failed: {e}"))?;
    made.push((name.to_string(), value.clone()));
    Ok(value)
}

fn run(file: &str, func: &str, args: &[&str]) -> Result<(), CliError> {
    let (caps, args) = take_allow(args)?;
    let (quant, args) = take_quantize(&args)?;
    let (device, args) = take_device(&args)?;
    let (workers, args) = take_workers(&args);
    let checked = check_project(file)?;
    let mut names: Vec<String> = checked.hir.fns.iter().map(|f| f.name.clone()).collect();
    names.sort();
    let interp = Interp::with_caps(checked.hir, false, caps).or_else(|e| failed(e.to_string()))?;
    if interp.signature(func).is_none() {
        return failed(format!(
            "`{file}` has no function `{func}`; it has: {}",
            names.join(", ")
        ));
    }
    interp.set_quantization(quant);
    interp.set_device(device);
    if let Some(workers) = workers {
        attach_cluster(&interp, workers, &checked.bundle)?;
    }
    let values = call_arguments(&interp, func, &args)?;
    let value = interp
        .call(func, values)
        .or_else(|e| failed(e.to_string()))?;
    // A function that returns nothing has nothing to show.
    if !matches!(value, Value::Unit) {
        println!("{value}");
    }
    Ok(())
}

/// `telvax fmt [--check] [--diff] [path...]`: rewrites files in place, or only reports with
/// `--check`, or shows what would change with `--diff`. A directory means every `.tvx` file under it;
/// no path means this directory.
fn fmt(args: &[&str]) -> Result<(), CliError> {
    let check = args.contains(&"--check");
    let diff = args.contains(&"--diff");
    let mut paths: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| *a != "--check" && *a != "--diff")
        .collect();
    if let Some(flag) = paths.iter().find(|a| a.starts_with("--")) {
        return failed(format!("unknown option `{flag}` for `telvax fmt`"));
    }
    if paths.is_empty() {
        paths.push(".");
    }
    let mut files: Vec<String> = Vec::new();
    for path in paths {
        if path != "-" && Path::new(path).is_dir() {
            let mut found = Vec::new();
            collect_tvx_files(Path::new(path), &mut found);
            if found.is_empty() {
                return failed(format!("no `.tvx` files in `{path}`"));
            }
            found.sort();
            files.extend(found.iter().map(|f| f.to_string_lossy().into_owned()));
        } else {
            files.push(path.to_string());
        }
    }
    let mut unformatted = 0;
    for file in files {
        if file == "-" {
            let mut src = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut src)
                .or_else(|e| failed(format!("cannot read stdin: {e}")))?;
            print!("{}", telvax_fmt::format(&src).or_else(failed)?);
            continue;
        }
        let src = std::fs::read_to_string(&file)
            .or_else(|e| failed(format!("cannot read `{file}`: {e}")))?;
        let formatted = telvax_fmt::format(&src).or_else(|e| failed(format!("`{file}`: {e}")))?;
        if formatted == src {
            continue;
        }
        if diff {
            println!("--- {file}\n{}", telvax_fmt::diff(&src, &formatted));
            unformatted += 1;
        } else if check {
            println!("would reformat {file}");
            unformatted += 1;
        } else {
            std::fs::write(&file, &formatted)
                .or_else(|e| failed(format!("cannot write `{file}`: {e}")))?;
            println!("formatted {file}");
        }
    }
    if unformatted > 0 {
        return failed(format!("{unformatted} file(s) need formatting"));
    }
    Ok(())
}

/// `telvax doc [--public] [-o out.md] [path...]`: a Markdown reference of the functions, types, enums
/// and traits in each file, with the `#` comment lines above them. A directory means every `.tvx` file
/// under it; no path means this directory. `--public` leaves out names that start with `_`.
/// `telvax schema data.csv`: prints the `type` for the columns of a data file, to paste above
/// `let t: Table[Name] = read_csv("data.csv")`.
fn schema(args: &[&str]) -> Result<(), CliError> {
    let mut name: Option<&str> = None;
    let mut file: Option<&str> = None;
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "--name" => name = Some(it.next().copied().ok_or(CliError::Usage)?),
            _ if a.starts_with('-') => {
                return failed(format!("unknown option `{a}` for `telvax schema`"));
            }
            _ if file.is_none() => file = Some(a),
            _ => return Err(CliError::Usage),
        }
    }
    let file = file.ok_or(CliError::Usage)?;
    let name = match name {
        Some(n) => n.to_string(),
        None => type_name_of(file),
    };
    let fields = telvax_runtime::infer_schema(file).map_err(|e| CliError::Failed(e.to_string()))?;
    let text = telvax_runtime::schema_source(&name, &fields)
        .map_err(|e| CliError::Failed(e.to_string()))?;
    print!("{text}");
    Ok(())
}

/// `sales_2024.csv` -> `Sales2024`.
fn type_name_of(file: &str) -> String {
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Row");
    let name: String = stem
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut cs = w.chars();
            cs.next()
                .map(|f| f.to_uppercase().chain(cs).collect::<String>())
                .unwrap_or_default()
        })
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        "Row".to_string()
    } else {
        name
    }
}

fn doc(args: &[&str]) -> Result<(), CliError> {
    let mut opts = telvax_doc::Options::default();
    let mut output: Option<&str> = None;
    let mut paths: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "--public" => opts.public_only = true,
            "-o" => output = Some(it.next().copied().ok_or(CliError::Usage)?),
            _ if a.starts_with('-') => {
                return failed(format!("unknown option `{a}` for `telvax doc`"));
            }
            _ => paths.push(a),
        }
    }
    if paths.is_empty() {
        paths.push(".");
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for path in paths {
        if Path::new(path).is_dir() {
            let mut found = Vec::new();
            collect_tvx_files(Path::new(path), &mut found);
            if found.is_empty() {
                return failed(format!("no `.tvx` files in `{path}`"));
            }
            found.sort();
            files.extend(found);
        } else {
            files.push(PathBuf::from(path));
        }
    }
    let mut text = String::from("# Reference\n\n");
    for file in files {
        let name = file.to_string_lossy();
        let src = std::fs::read_to_string(&file)
            .or_else(|e| failed(format!("cannot read `{name}`: {e}")))?;
        let section = telvax_doc::generate(&src, name.trim_start_matches("./"), opts)
            .or_else(|e| failed(format!("`{name}`: {e}")))?;
        text.push_str(&section);
        text.push('\n');
    }
    match output {
        Some(out) => {
            std::fs::write(out, text).or_else(|e| failed(format!("cannot write `{out}`: {e}")))
        }
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

/// Every `.tvx` file under `dir`, skipping hidden folders, `target`, `node_modules` and `.tvpm`.
fn collect_tvx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !name.starts_with('.') && name != "target" && name != "node_modules" {
                collect_tvx_files(&path, out);
            }
        } else if name.ends_with(".tvx") {
            out.push(path);
        }
    }
}

/// `telvax lint <file> [--allow|--warn|--deny rule]... [--deny-warnings]`: type-checks, then reports
/// unused code. Rule levels come from `[lint]` in `telvax.toml`, then the flags.
fn lint(file: &str, flags: &[&str]) -> Result<(), CliError> {
    let mut deny = false;
    let mut from_flags = Vec::new();
    let mut words = flags.iter();
    while let Some(&flag) = words.next() {
        match flag {
            "--deny-warnings" => deny = true,
            "--allow" | "--warn" | "--deny" => {
                let Some(&rule) = words.next() else {
                    return Err(CliError::Usage);
                };
                let level = telvax_lint::Level::parse(&flag[2..]).or_else(failed)?;
                from_flags.push((rule, level));
            }
            _ => return Err(CliError::Usage),
        }
    }
    let path = Path::new(file);
    let project = match telvax_pkg::Project::discover(path) {
        Ok(p) => p,
        Err(e) => return failed(e),
    };
    // `[lint]` in telvax.toml first, then the flags, which win.
    let mut config = telvax_lint::Config::default();
    if let Some(project) = &project {
        for (rule, level) in &project.manifest.lint {
            let rule = telvax_lint::Rule::parse(rule)
                .map_err(|e| format!("telvax.toml [lint]: {e}"))
                .or_else(failed)?;
            let level = telvax_lint::Level::parse(level)
                .map_err(|e| format!("telvax.toml [lint] {}: {e}", rule.name()))
                .or_else(failed)?;
            config.set(rule, level);
        }
    }
    for (rule, level) in from_flags {
        config.set(telvax_lint::Rule::parse(rule).or_else(failed)?, level);
    }
    let deps = match &project {
        Some(project) => project.dependency_packages().or_else(failed)?,
        None => Vec::new(),
    };
    let loaded = telvax_modules::load(path, &deps).or_else(failed)?;
    let (hir, _) = lower_loaded(&loaded)?;
    let warnings = telvax_lint::lint_with(&hir, &config);
    let errors = warnings.iter().filter(|d| d.is_error()).count();
    eprint!(
        "{}",
        loaded
            .map
            .render(&warnings, std::io::stderr().is_terminal())
    );
    if warnings.is_empty() {
        eprintln!("`{file}`: no warnings");
        return Ok(());
    }
    eprintln!(
        "`{file}`: {} warning(s), {errors} error(s)",
        warnings.len() - errors
    );
    if deny || errors > 0 {
        Err(CliError::Failed(String::new()))
    } else {
        Ok(())
    }
}
