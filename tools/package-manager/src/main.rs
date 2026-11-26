use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use semver::{Version, VersionReq};
use telvax_pkg::manifest::{Dependency, MANIFEST, validate_name};
use telvax_pkg::project::Change;
use telvax_pkg::registry::{self, DirRegistry, Registry};
use telvax_pkg::{Manifest, Project};

const USAGE: &str = "\
tvpm: the Telvax package manager

usage:
  tvpm new <name> [--lib | --template lib|job|service]
                                     create a project in ./<name> (a template adds tests, a README,
                                     a CHANGELOG, a Makefile, pre-commit, a CI workflow, and a Dockerfile
                                     for job and service)
  tvpm init                          create telvax.toml in the current directory
  tvpm add <name>[@req] [--path DIR] add a dependency (registry, or a local directory)
  tvpm remove <name>                 remove a dependency
  tvpm lock                          resolve versions and write telvax.lock
  tvpm install                       lock (if needed) and download what is missing
  tvpm update [name...]              move locked versions to the newest that fit (all, or just the named)
  tvpm tree                          print the dependency tree
  tvpm search <text>                 find packages by name or description
  tvpm publish                       publish this package to a registry directory (a clone of telvax/registry)
  tvpm yank <name> <version> [--undo]
  tvpm registry check [dir] [--base DIR] [--author LOGIN]
                                     check a registry directory, and the change against --base
  tvpm registry names [dir]          write names.json from the index
  tvpm check | run [fn] | serve [flags] | build [flags]
                                     install, then run the matching `telvax` command

The registry is a directory or an http(s) URL: --registry <where> or TVPM_REGISTRY, and
https://telvax.github.io/registry (telvax/registry) when neither is given.";

type Res<T> = Result<T, String>;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let registry = take_flag(&mut args, "--registry")
        .or_else(|| std::env::var("TVPM_REGISTRY").ok())
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| registry::DEFAULT_REGISTRY.to_string());
    match run(args, registry) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) if msg.is_empty() => ExitCode::from(2),
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

/// Removes `--flag value` from `args` and returns the value.
fn take_flag(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    args.remove(i);
    (i < args.len()).then(|| args.remove(i))
}

fn take_switch(args: &mut Vec<String>, flag: &str) -> bool {
    match args.iter().position(|a| a == flag) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    }
}

/// The directory of a command that writes to a registry: the official one is changed by a pull request to a clone.
fn registry_dir(spec: &str, what: &str) -> Res<PathBuf> {
    if spec.starts_with("http://") || spec.starts_with("https://") {
        return Err(format!(
            "{what} writes to a registry directory, not to {spec}; clone https://github.com/telvax/registry, \
             run `tvpm {what} --registry <clone>` and open a pull request with the result"
        ));
    }
    Ok(PathBuf::from(spec))
}

fn run(mut args: Vec<String>, registry_spec: String) -> Res<()> {
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        println!("{USAGE}");
        return Ok(());
    }
    let cmd = args.remove(0);
    let reg = Some(registry::open(&registry_spec));
    let reg_ref = reg.as_deref();
    match cmd.as_str() {
        "new" => {
            let mut lib = take_switch(&mut args, "--lib");
            let template = take_flag(&mut args, "--template");
            if let Some(t) = &template {
                if !telvax_pkg::templates::NAMES.contains(&t.as_str()) {
                    return Err(format!(
                        "unknown template `{t}`; the one that exists is: {}",
                        telvax_pkg::templates::NAMES.join(", ")
                    ));
                }
                lib = t == "lib";
            }
            let name = args
                .first()
                .ok_or_else(|| String::from("usage: tvpm new <name>"))?;
            new_project(Path::new(name), name, lib)?;
            if let Some(t) = &template {
                write_template(Path::new(name), name, t)?;
            }
            Ok(())
        }
        "init" => {
            let dir = std::env::current_dir().map_err(|e| e.to_string())?;
            let name = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("app")
                .replace('-', "_")
                .to_lowercase();
            new_project(&dir, &name, false)
        }
        "add" => add(&mut args, reg_ref),
        "remove" => {
            let name = args
                .first()
                .ok_or_else(|| String::from("usage: tvpm remove <name>"))?;
            let project = current_project()?;
            let mut m = project.manifest.clone();
            if m.dependencies.remove(name).is_none() {
                return Err(format!("`{name}` is not a dependency"));
            }
            m.write(&project.dir).map_err(|e| e.0)?;
            println!("removed {name}");
            Ok(())
        }
        "lock" => {
            let project = current_project()?;
            lock(&project, reg_ref)
        }
        "install" => install(&current_project()?, reg_ref),
        "update" => update(&current_project()?, &args, reg_ref),
        "tree" => {
            print!("{}", current_project()?.tree()?);
            Ok(())
        }
        "search" => {
            let q = args.first().map_or("", String::as_str);
            let found = registry::search(&registry_spec, q)?;
            if found.is_empty() {
                println!("no packages match `{q}`");
            }
            for n in found {
                let version = n
                    .version
                    .map_or("(no usable version)".to_string(), |v| v.to_string());
                match n.description {
                    Some(d) => println!("{} {version}  {d}", n.name),
                    None => println!("{} {version}", n.name),
                }
            }
            Ok(())
        }
        "registry" => registry_command(&mut args),
        "publish" => {
            let spec = registry_dir(&registry_spec, "publish")?;
            let project = current_project()?;
            prepare(&project, reg_ref)?;
            // A package that does not compile is not worth anyone's download.
            if let Err(report) = project.check() {
                eprint!("{report}");
                return Err(
                    "the package does not pass `telvax check`, so it was not published".into(),
                );
            }
            let entry = registry::publish(&spec, &project.dir)?;
            println!(
                "published {} {} (sha256 {})",
                entry.name,
                entry.vers,
                &entry.cksum[..16]
            );
            Ok(())
        }
        "yank" => {
            let undo = take_switch(&mut args, "--undo");
            let spec = registry_dir(&registry_spec, "yank")?;
            let [name, version] = args.as_slice() else {
                return Err("usage: tvpm yank <name> <version> [--undo]".into());
            };
            let version: Version = version
                .parse()
                .map_err(|e| format!("bad version `{version}`: {e}"))?;
            registry::yank(&spec, name, &version, !undo)?;
            println!(
                "{} {name} {version}",
                if undo { "unyanked" } else { "yanked" }
            );
            Ok(())
        }
        "check" | "run" | "serve" | "build" => {
            let project = current_project()?;
            prepare(&project, reg_ref)?;
            let entry = project.entry()?;
            let mut command = Command::new(telvax_binary());
            command.arg(&cmd);
            if cmd == "build" && args.first().is_some_and(|a| a == "--serve") {
                args.remove(0);
                command.arg("--serve");
            }
            command.arg(&entry);
            if cmd == "run" && args.is_empty() {
                args.push("main".into());
            }
            let status = command
                .args(&args)
                .status()
                .map_err(|e| format!("cannot run `telvax` (set TELVAX_BIN): {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err(String::new())
            }
        }
        other => {
            eprintln!("unknown command `{other}`\n\n{USAGE}");
            Err(String::new())
        }
    }
}

/// Locks (when the lock is missing or out of date) and installs the dependencies. A package
/// without dependencies has nothing to lock.
fn prepare(project: &Project, reg: Option<&dyn Registry>) -> Res<()> {
    if project.manifest.dependencies.is_empty() {
        return Ok(());
    }
    if !project.lock_is_fresh() {
        lock(project, reg)?;
    }
    for fetched in project.install(reg)? {
        eprintln!("downloaded {fetched}");
    }
    Ok(())
}

fn telvax_binary() -> PathBuf {
    if let Some(p) = std::env::var_os("TELVAX_BIN") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.with_file_name("telvax");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("telvax")
}

fn current_project() -> Res<Project> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    Project::discover(&cwd)?
        .ok_or_else(|| format!("no {MANIFEST} here or in a parent directory; run `tvpm init`"))
}

fn new_project(dir: &Path, name: &str, lib: bool) -> Res<()> {
    validate_name(name).map_err(|e| e.0)?;
    if dir.join(MANIFEST).exists() {
        return Err(format!("{} already exists", dir.join(MANIFEST).display()));
    }
    std::fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;
    Manifest {
        package: telvax_pkg::manifest::PackageInfo {
            license: None,
            name: name.to_string(),
            version: Version::new(0, 1, 0),
            edition: Some(telvax_pkg::manifest::CURRENT_EDITION.to_string()),
            description: None,
            entry: None,
        },
        dependencies: Default::default(),
        lint: Default::default(),
    }
    .write(dir)
    .map_err(|e| e.0)?;
    let (file, body) = if lib {
        (
            "lib.tvx",
            "fn hello() -> Text:\n    \"hello from a library\"\n",
        )
    } else {
        (
            "main.tvx",
            "fn main() -> i32:\n    print(\"hello, world\")\n    0\n",
        )
    };
    let path = dir.join("src").join(file);
    if !path.exists() {
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
    }
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, "target/\n.tvpm/\n").map_err(|e| e.to_string())?;
    }
    println!(
        "created {} `{name}`",
        if lib { "library" } else { "binary package" }
    );
    Ok(())
}

/// Writes the files of the library template next to the ones `new_project` made. The `src/lib.tvx` that
/// `new_project` wrote is the plain one, so the template replaces it; any other file that is already there
/// is left alone.
fn write_template(dir: &Path, name: &str, template: &str) -> Res<()> {
    let files = match template {
        "job" => telvax_pkg::templates::job(name),
        "service" => telvax_pkg::templates::service(name),
        _ => telvax_pkg::templates::library(name),
    };
    for (path, text) in files {
        let target = dir.join(&path);
        if target.exists() && path != "src/lib.tvx" && path != "src/main.tvx" {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(())
}

fn add(args: &mut Vec<String>, reg: Option<&dyn Registry>) -> Res<()> {
    let path = take_flag(args, "--path");
    let spec = args
        .first()
        .ok_or_else(|| String::from("usage: tvpm add <name>[@req] [--path DIR]"))?
        .clone();
    let (name, req) = match spec.split_once('@') {
        Some((n, r)) => (n.to_string(), Some(r.to_string())),
        None => (spec, None),
    };
    validate_name(&name).map_err(|e| e.0)?;
    let project = current_project()?;
    let mut manifest = project.manifest.clone();
    let dep = match (path, req) {
        (Some(p), _) => {
            let dep_dir = project.dir.join(&p);
            let m =
                Manifest::read(&dep_dir).map_err(|e| format!("{}: {}", dep_dir.display(), e.0))?;
            if m.package.name != name {
                return Err(format!(
                    "{} is the package `{}`, not `{name}`",
                    dep_dir.display(),
                    m.package.name
                ));
            }
            Dependency::Detailed {
                path: Some(p),
                version: None,
            }
        }
        (None, Some(r)) => Dependency::Version(
            r.parse::<VersionReq>()
                .map_err(|e| format!("bad version requirement `{r}`: {e}"))?,
        ),
        (None, None) => {
            // No requirement given: depend on the newest published version, compatibly.
            let reg =
                reg.ok_or("give a version (`tvpm add name@1.2`) or a --registry to look it up")?;
            let newest = reg
                .versions(&name)?
                .into_iter()
                .filter(|e| e.usable())
                .map(|e| e.vers)
                .max()
                .ok_or_else(|| format!("`{name}` is not in the registry"))?;
            Dependency::Version(format!("^{newest}").parse().map_err(|e| format!("{e}"))?)
        }
    };
    manifest.dependencies.insert(name.clone(), dep);
    manifest.write(&project.dir).map_err(|e| e.0)?;
    println!("added {name}");
    Ok(())
}

fn lock(project: &Project, reg: Option<&dyn Registry>) -> Res<()> {
    let (resolution, _) = project.lock(reg)?;
    println!("locked {} package(s)", resolution.packages.len());
    for p in &resolution.packages {
        println!("  {} {}", p.name, p.version);
    }
    Ok(())
}

fn update(project: &Project, names: &[String], reg: Option<&dyn Registry>) -> Res<()> {
    let changes = project.update(reg, names)?;
    if changes.is_empty() {
        println!("everything is up to date");
    }
    for Change { name, from, to } in changes {
        match (from, to) {
            (Some(from), Some(to)) => println!("updated {name} {from} -> {to}"),
            (None, Some(to)) => println!("added {name} {to}"),
            (Some(from), None) => println!("removed {name} {from}"),
            (None, None) => {}
        }
    }
    for fetched in project.install(reg)? {
        println!("downloaded {fetched}");
    }
    Ok(())
}

fn install(project: &Project, reg: Option<&dyn Registry>) -> Res<()> {
    if !project.lock_is_fresh() {
        lock(project, reg)?;
    }
    let fetched = project.install(reg)?;
    if fetched.is_empty() {
        println!("everything is installed");
    }
    for f in fetched {
        println!("downloaded {f}");
    }
    Ok(())
}

/// `tvpm registry check|names`: the tools of the official registry (`telvax/registry`), run by its workflow on every pull
/// request and by anyone before opening one.
fn registry_command(args: &mut Vec<String>) -> Res<()> {
    let base = take_flag(args, "--base");
    let author = take_flag(args, "--author");
    let Some(sub) = (!args.is_empty()).then(|| args.remove(0)) else {
        return Err("usage: tvpm registry check|names [dir]".into());
    };
    let root = PathBuf::from(args.first().map_or(".", String::as_str));
    match sub.as_str() {
        "names" => {
            DirRegistry::new(&root).write_names()?;
            println!("wrote {}", root.join("names.json").display());
            Ok(())
        }
        "check" => {
            let report = registry::check(&root, base.as_deref().map(Path::new), author.as_deref())?;
            let mut problems = report.problems;
            // Each new version is unpacked and must check and pass its tests, with its dependencies from this registry.
            let reg = DirRegistry::new(&root);
            for (name, version) in &report.new_versions {
                let label = format!("`{name}` {version}");
                match try_package(&reg, name, version) {
                    Ok(()) => println!("{label}: unpacks, checks and passes its tests"),
                    Err(e) => problems.push(format!("{label}: {e}")),
                }
            }
            for p in &problems {
                eprintln!("problem: {p}");
            }
            if problems.is_empty() {
                println!(
                    "the registry is valid ({} new version(s))",
                    report.new_versions.len()
                );
                Ok(())
            } else {
                Err(format!("{} problem(s) in the registry", problems.len()))
            }
        }
        other => Err(format!(
            "`tvpm registry` has no command `{other}`; it has `check` and `names`"
        )),
    }
}

/// Unpacks a published version in a temporary directory, installs its dependencies from `reg`, and runs `telvax check`
/// and `telvax test` on it.
fn try_package(reg: &DirRegistry, name: &str, version: &Version) -> Res<()> {
    let entry = reg
        .versions(name)?
        .into_iter()
        .find(|e| &e.vers == version)
        .ok_or("not in the index")?;
    let dir = std::env::temp_dir().join(format!(
        "tvpm-check-{}-{name}-{version}",
        std::process::id()
    ));
    registry::unpack(&reg.download(name, version)?, &entry.cksum, &dir)?;
    let project = Project::discover(&dir)?.ok_or("the archive has no telvax.toml")?;
    prepare(&project, Some(reg))?;
    let result = (|| {
        project
            .check()
            .map_err(|report| format!("does not pass `telvax check`:\n{report}"))?;
        // Every file of the package that has `test_` functions, as `telvax test <file>` runs them.
        for file in test_files(&dir) {
            let out = Command::new(telvax_binary())
                .arg("test")
                .arg(&file)
                .current_dir(&dir)
                .output()
                .map_err(|e| format!("cannot run telvax: {e}"))?;
            if !out.status.success() {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                let shown = file
                    .strip_prefix(&dir)
                    .unwrap_or(&file)
                    .display()
                    .to_string();
                return Err(format!("`telvax test {shown}` fails:\n{text}"));
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The `.tvx` files under `dir` that define a `test_` function.
fn test_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "tvx")
                && std::fs::read_to_string(&p).is_ok_and(|t| t.contains("fn test_"))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
