//! `telvax serve --watch`: runs the server as a child process and replaces it when the program changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, SystemTime};

use crate::{CliError, check_project, failed};

const POLL: Duration = Duration::from_millis(300);

/// What a file looked like when it was last read; a change to either part is a change to the file.
type Stamp = Option<(SystemTime, u64)>;

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// The files that make up the program: the entry file, the modules it imports, the files it embeds
/// with `std.fs.embed`, and the manifest and lockfile of its project. A file that cannot be parsed
/// still names the modules it imported before the error.
fn watched(file: &str) -> Vec<PathBuf> {
    let path = Path::new(file);
    let mut files = vec![path.to_path_buf()];
    let project = telvax_pkg::Project::discover(path).ok().flatten();
    let deps = project
        .as_ref()
        .and_then(|p| p.dependency_packages().ok())
        .unwrap_or_default();
    if let Ok(loaded) = telvax_modules::load(path, &deps) {
        files.extend(loaded.map.files.iter().map(|f| PathBuf::from(&f.name)));
        let dir = path.parent().unwrap_or(Path::new(""));
        files.extend(
            loaded
                .embedded_paths()
                .unwrap_or_default()
                .into_iter()
                .map(|p| dir.join(p)),
        );
    }
    if let Some(project) = project {
        files.push(project.dir.join(telvax_pkg::manifest::MANIFEST));
        files.push(project.dir.join(telvax_pkg::lock::LOCKFILE));
    }
    files.sort();
    files.dedup();
    files
}

fn stamps(files: &[PathBuf]) -> BTreeMap<PathBuf, Stamp> {
    files.iter().map(|f| (f.clone(), stamp(f))).collect()
}

fn start(file: &str, flags: &[&str]) -> Result<Child, CliError> {
    let exe = std::env::current_exe().or_else(|e| failed(format!("cannot find telvax: {e}")))?;
    Command::new(exe)
        .arg("serve")
        .arg(file)
        .args(flags)
        .spawn()
        .or_else(|e| failed(format!("cannot start the server: {e}")))
}

/// Asks a child to stop and waits for it. On Windows the child is ended at once.
fn stop(child: &mut Child, graceful: bool) {
    #[cfg(unix)]
    if graceful {
        let _ = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status();
        let _ = child.wait();
        return;
    }
    let _ = graceful;
    let _ = child.kill();
    let _ = child.wait();
}

/// Serves `file` and again every time it changes. `flags` are the options of `serve`, without
/// `--watch`. A change that does not check leaves the running server as it is.
pub fn run(file: &str, flags: &[&str]) -> Result<(), CliError> {
    let rt = tokio::runtime::Runtime::new().or_else(|e| failed(e.to_string()))?;
    rt.block_on(supervise(file, flags))
}

async fn supervise(file: &str, flags: &[&str]) -> Result<(), CliError> {
    let mut child: Option<Child> = None;
    let mut files = watched(file);
    let mut seen = stamps(&files);
    // Whether `files` has been checked since it last changed.
    let mut pending = true;
    let mut ticker = tokio::time::interval(POLL);
    #[cfg(unix)]
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .or_else(|e| failed(e.to_string()))?;
    eprintln!("watching {} file(s) for changes", files.len());
    loop {
        if pending {
            pending = false;
            // The editor may still be writing; look again before reading.
            tokio::time::sleep(Duration::from_millis(100)).await;
            files = watched(file);
            seen = stamps(&files);
            match check_project(file) {
                Ok(_) => {
                    if let Some(mut old) = child.take() {
                        eprintln!("restarting");
                        stop(&mut old, true);
                    }
                    child = Some(start(file, flags)?);
                }
                Err(CliError::Failed(message)) => {
                    if !message.is_empty() {
                        eprintln!("error: {message}");
                    }
                    eprintln!(
                        "{}",
                        if child.is_some() {
                            "the server that is running is kept; save the file again when it is fixed"
                        } else {
                            "waiting for a change"
                        }
                    );
                }
                Err(other) => return Err(other),
            }
        }
        #[cfg(unix)]
        let terminated = async { term.recv().await };
        #[cfg(not(unix))]
        let terminated = std::future::pending::<Option<()>>();
        tokio::select! {
            _ = ticker.tick() => {}
            _ = tokio::signal::ctrl_c() => {
                // The terminal sent it to the server too, which stops by itself.
                if let Some(mut c) = child.take() { let _ = c.wait(); }
                return Ok(());
            }
            _ = terminated => {
                if let Some(mut c) = child.take() { stop(&mut c, true); }
                return Ok(());
            }
        }
        if let Some(c) = &mut child
            && let Ok(Some(status)) = c.try_wait()
        {
            eprintln!("the server stopped ({status}); waiting for a change");
            child = None;
        }
        let now = stamps(&files);
        if now != seen {
            if let Some((changed, _)) = now.iter().find(|(f, s)| seen.get(*f) != Some(*s)) {
                eprintln!("{} changed", changed.display());
            }
            pending = true;
        }
    }
}
