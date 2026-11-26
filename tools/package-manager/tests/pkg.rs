use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use semver::Version;
use telvax_pkg::manifest::Manifest;
use telvax_pkg::project::Change;
use telvax_pkg::registry::{self, DirRegistry, Registry};
use telvax_pkg::{Lock, Project};

/// `TVPM_HOME` is process-wide; tests that install packages take turns.
static HOME: Mutex<()> = Mutex::new(());

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "tvpm-{}-{}-{tag}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn write(&self, rel: &str, text: &str) -> &Dir {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
        self
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A package with one module `src/lib.tvx`, published to `registry`.
fn publish(registry: &Dir, name: &str, version: &str, deps: &[(&str, &str)], lib: &str) {
    let work = Dir::new(&format!("pub-{name}-{version}"));
    let dep_lines: String = deps
        .iter()
        .map(|(n, r)| format!("{n} = \"{r}\"\n"))
        .collect();
    work.write(
        "telvax.toml",
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"{version}\"\n\n[dependencies]\n{dep_lines}"
        ),
    );
    work.write("src/lib.tvx", lib);
    registry::publish(&registry.0, &work.0)
        .unwrap_or_else(|e| panic!("publish {name} {version}: {e}"));
}

fn app(deps: &[(&str, &str)]) -> Manifest {
    let lines: String = deps
        .iter()
        .map(|(n, r)| format!("{n} = \"{r}\"\n"))
        .collect();
    Manifest::parse(&format!(
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n{lines}"
    ))
    .unwrap()
}

fn versions(res: &telvax_pkg::resolver::Resolution) -> Vec<String> {
    res.packages
        .iter()
        .map(|p| format!("{} {}", p.name, p.version))
        .collect()
}

fn resolve(deps: &[(&str, &str)], reg: &Dir) -> Result<Vec<String>, String> {
    let r = DirRegistry::new(&reg.0);
    telvax_pkg::resolver::resolve(Path::new("."), &app(deps), Some(&r)).map(|res| versions(&res))
}

// ---- manifests -------------------------------------------------------------------------------

#[test]
fn manifests_round_trip_and_validate() {
    let m = Manifest::parse("[package]\nname = \"app\"\nversion = \"1.2.3\"\n[dependencies]\na = \"^1.2\"\nb = { path = \"../b\" }\n").unwrap();
    assert_eq!(m.package.version, Version::new(1, 2, 3));
    assert!(m.dependencies["b"].path().is_some());
    assert_eq!(Manifest::parse(&m.to_toml()).unwrap(), m);

    for (text, needle) in [
        (
            "[package]\nname = \"Bad-Name\"\nversion = \"1.0.0\"\n",
            "invalid package name",
        ),
        (
            "[package]\nname = \"std\"\nversion = \"1.0.0\"\n",
            "invalid package name",
        ),
        (
            "[package]\nname = \"a\"\nversion = \"one\"\n",
            "invalid telvax.toml",
        ),
        (
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nb = { path = \"x\", version = \"1\" }\n",
            "pick one",
        ),
        (
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nBad = \"1\"\n",
            "invalid package name",
        ),
        (
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nb = \"not a req\"\n",
            "invalid telvax.toml",
        ),
    ] {
        let e = Manifest::parse(text).unwrap_err().0;
        assert!(e.contains(needle), "{text}: {e}");
    }
}

// ---- registry --------------------------------------------------------------------------------

#[test]
fn the_edition_is_checked_and_defaults_to_the_current_one() {
    let plain = Manifest::parse("[package]\nname = \"app\"\nversion = \"1.0.0\"\n").unwrap();
    assert_eq!(plain.package.edition, None);
    let pinned = Manifest::parse(&format!(
        "[package]\nname = \"app\"\nversion = \"1.0.0\"\nedition = \"{}\"\n",
        telvax_pkg::manifest::CURRENT_EDITION
    ))
    .unwrap();
    assert_eq!(Manifest::parse(&pinned.to_toml()).unwrap(), pinned);
    let e = Manifest::parse("[package]\nname = \"app\"\nversion = \"1.0.0\"\nedition = \"2031\"\n")
        .unwrap_err()
        .0;
    assert!(
        e.contains("`2026`, `2027-preview`, not `2031`") && e.contains("update telvax"),
        "{e}"
    );
}

#[test]
fn update_moves_only_what_it_is_asked_to() {
    let reg = Dir::new("update-reg");
    for name in ["a", "b"] {
        publish(&reg, name, "1.0.0", &[], "fn f() -> Int:\n    1\n");
    }
    let work = Dir::new("update-app");
    work.write(
        "telvax.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\na = \"^1\"\nb = \"^1\"\n",
    );
    let project = Project::discover(&work.0).unwrap().unwrap();
    let registry = DirRegistry::new(&reg.0);
    let at = |name: &str| {
        Lock::read(&work.0)
            .unwrap()
            .unwrap()
            .get(name)
            .unwrap()
            .version
            .to_string()
    };
    let v = |s: &str| Some(Version::parse(s).unwrap());
    // No lock yet: everything is added.
    let changes = project.update(Some(&registry), &[]).unwrap();
    let change = |name: &str, from: Option<Version>, to: Option<Version>| Change {
        name: name.into(),
        from,
        to,
    };
    assert_eq!(
        changes,
        [change("a", None, v("1.0.0")), change("b", None, v("1.0.0"))]
    );
    assert!(project.update(Some(&registry), &[]).unwrap().is_empty());
    for name in ["a", "b"] {
        publish(&reg, name, "1.1.0", &[], "fn f() -> Int:\n    1\n");
    }
    publish(&reg, "a", "2.0.0", &[], "fn f() -> Int:\n    1\n");
    // Naming `a` moves `a` (to the newest that `^1` allows) and leaves `b` at its locked version.
    let changes = project.update(Some(&registry), &["a".to_string()]).unwrap();
    assert_eq!(changes, [change("a", v("1.0.0"), v("1.1.0"))]);
    assert_eq!((at("a"), at("b")), ("1.1.0".into(), "1.0.0".into()));
    // Without names everything moves.
    let changes = project.update(Some(&registry), &[]).unwrap();
    assert_eq!(changes, [change("b", v("1.0.0"), v("1.1.0"))]);
    // A name that is not locked says what is.
    let e = project
        .update(Some(&registry), &["zed".to_string()])
        .unwrap_err();
    assert!(
        e.contains("`zed` is not a registry package") && e.contains("a, b"),
        "{e}"
    );
}

#[test]
fn publishing_writes_an_index_and_a_checksummed_archive() {
    let reg = Dir::new("reg");
    publish(
        &reg,
        "mathx",
        "1.0.0",
        &[],
        r#"
fn one() -> i32:
    1
"#,
    );
    publish(
        &reg,
        "mathx",
        "1.1.0",
        &[],
        r#"
fn one() -> i32:
    1
"#,
    );
    let r = DirRegistry::new(&reg.0);
    let v = r.versions("mathx").unwrap();
    assert_eq!(
        v.iter().map(|e| e.vers.to_string()).collect::<Vec<_>>(),
        ["1.0.0", "1.1.0"]
    );
    let archive = r.download("mathx", &Version::new(1, 0, 0)).unwrap();
    assert_eq!(registry::sha256_hex(&archive), v[0].cksum);
    assert!(r.versions("unknown").unwrap().is_empty());
    assert_eq!(r.search("ath"), ["mathx"]);
}

#[test]
fn publishing_rules() {
    let reg = Dir::new("rules");
    publish(
        &reg,
        "a",
        "1.0.0",
        &[],
        r#"
fn f():
    pass
"#,
    );
    let work = Dir::new("again");
    work.write(
        "telvax.toml",
        "[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
    )
    .write(
        "src/lib.tvx",
        r#"
fn f():
    pass
"#,
    );
    assert!(
        registry::publish(&reg.0, &work.0)
            .unwrap_err()
            .contains("already published")
    );

    let local = Dir::new("pathdep");
    local.write("telvax.toml", "[package]\nname = \"b\"\nversion = \"1.0.0\"\n[dependencies]\na = { path = \"../a\" }\n").write("src/lib.tvx", r#"
fn g():
    pass
"#);
    assert!(
        registry::publish(&reg.0, &local.0)
            .unwrap_err()
            .contains("path dependency")
    );

    let empty = Dir::new("empty");
    empty.write(
        "telvax.toml",
        "[package]\nname = \"c\"\nversion = \"1.0.0\"\n",
    );
    assert!(
        registry::publish(&reg.0, &empty.0)
            .unwrap_err()
            .contains("src/")
    );
}

#[test]
fn tampered_archives_are_rejected() {
    let reg = Dir::new("tamper");
    publish(
        &reg,
        "a",
        "1.0.0",
        &[],
        r#"
fn f():
    pass
"#,
    );
    let r = DirRegistry::new(&reg.0);
    let entry = r.versions("a").unwrap().remove(0);
    let mut archive = r.download("a", &entry.vers).unwrap();
    let dest = Dir::new("dest");
    registry::unpack(&archive, &entry.cksum, &dest.p("ok")).unwrap();
    assert!(dest.p("ok/src/lib.tvx").is_file() && dest.p("ok/telvax.toml").is_file());
    *archive.last_mut().unwrap() ^= 0xff;
    assert!(
        registry::unpack(&archive, &entry.cksum, &dest.p("bad"))
            .unwrap_err()
            .contains("checksum mismatch")
    );
}

// ---- resolution ------------------------------------------------------------------------------

#[test]
fn the_newest_compatible_version_wins_transitively() {
    let reg = Dir::new("newest");
    publish(
        &reg,
        "c",
        "1.0.0",
        &[],
        r#"
fn c():
    pass
"#,
    );
    publish(
        &reg,
        "c",
        "1.4.0",
        &[],
        r#"
fn c():
    pass
"#,
    );
    publish(
        &reg,
        "c",
        "2.0.0",
        &[],
        r#"
fn c():
    pass
"#,
    );
    publish(
        &reg,
        "a",
        "1.0.0",
        &[("c", "^1")],
        r#"
fn a():
    pass
"#,
    );
    publish(
        &reg,
        "a",
        "1.2.0",
        &[("c", "^1")],
        r#"
fn a():
    pass
"#,
    );
    assert_eq!(
        resolve(&[("a", "^1")], &reg).unwrap(),
        ["a 1.2.0", "c 1.4.0"]
    );
    assert_eq!(
        resolve(&[("a", "=1.0.0")], &reg).unwrap(),
        ["a 1.0.0", "c 1.4.0"]
    );
}

#[test]
fn the_search_backs_up_when_a_newer_choice_conflicts() {
    let reg = Dir::new("backtrack");
    publish(
        &reg,
        "c",
        "1.0.0",
        &[],
        r#"
fn c():
    pass
"#,
    );
    publish(
        &reg,
        "c",
        "2.0.0",
        &[],
        r#"
fn c():
    pass
"#,
    );
    // The newest `a` wants c 2, but `b` needs c 1, so `a` must fall back to its older release.
    publish(
        &reg,
        "a",
        "1.0.0",
        &[("c", "^1")],
        r#"
fn a():
    pass
"#,
    );
    publish(
        &reg,
        "a",
        "1.1.0",
        &[("c", "^2")],
        r#"
fn a():
    pass
"#,
    );
    publish(
        &reg,
        "b",
        "1.0.0",
        &[("c", "^1")],
        r#"
fn b():
    pass
"#,
    );
    let got = resolve(&[("a", "^1"), ("b", "^1")], &reg).unwrap();
    assert_eq!(got, ["a 1.0.0", "b 1.0.0", "c 1.0.0"]);
}

#[test]
fn unsatisfiable_graphs_explain_themselves() {
    let reg = Dir::new("unsat");
    publish(
        &reg,
        "c",
        "1.0.0",
        &[],
        r#"
fn c():
    pass
"#,
    );
    publish(
        &reg,
        "a",
        "1.0.0",
        &[("c", "^2")],
        r#"
fn a():
    pass
"#,
    );
    let e = resolve(&[("a", "^1")], &reg).unwrap_err();
    assert!(
        e.contains("no version of `c` matches ^2")
            && e.contains("required by `a 1.0.0`")
            && e.contains("available: 1.0.0"),
        "{e}"
    );
    let e = resolve(&[("nope", "*")], &reg).unwrap_err();
    assert!(
        e.contains("`nope`") && e.contains("not in the registry"),
        "{e}"
    );
    let e = resolve(&[("a", "^9")], &reg).unwrap_err();
    assert!(e.contains("no version of `a` matches ^9"), "{e}");
}

#[test]
fn yanked_versions_are_skipped_by_new_resolutions() {
    let reg = Dir::new("yank");
    publish(
        &reg,
        "a",
        "1.0.0",
        &[],
        r#"
fn a():
    pass
"#,
    );
    publish(
        &reg,
        "a",
        "1.1.0",
        &[],
        r#"
fn a():
    pass
"#,
    );
    registry::yank(&reg.0, "a", &Version::new(1, 1, 0), true).unwrap();
    assert_eq!(resolve(&[("a", "^1")], &reg).unwrap(), ["a 1.0.0"]);
    registry::yank(&reg.0, "a", &Version::new(1, 1, 0), false).unwrap();
    assert_eq!(resolve(&[("a", "^1")], &reg).unwrap(), ["a 1.1.0"]);
}

#[test]
fn path_dependencies_are_fixed_and_can_have_registry_dependencies() {
    let reg = Dir::new("pathreg");
    publish(
        &reg,
        "c",
        "1.0.0",
        &[],
        r#"
fn c():
    pass
"#,
    );
    publish(
        &reg,
        "c",
        "1.5.0",
        &[],
        r#"
fn c():
    pass
"#,
    );
    let ws = Dir::new("ws");
    ws.write(
        "shared/telvax.toml",
        "[package]\nname = \"shared\"\nversion = \"0.3.0\"\n[dependencies]\nc = \"^1\"\n",
    )
    .write(
        "shared/src/lib.tvx",
        r#"
fn s():
    pass
"#,
    );
    let m = Manifest::parse("[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nshared = { path = \"shared\" }\n").unwrap();
    let r = DirRegistry::new(&reg.0);
    let res = telvax_pkg::resolver::resolve(&ws.0, &m, Some(&r)).unwrap();
    assert_eq!(versions(&res), ["c 1.5.0", "shared 0.3.0"]);

    let wrong = Manifest::parse("[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nother = { path = \"shared\" }\n").unwrap();
    assert!(
        telvax_pkg::resolver::resolve(&ws.0, &wrong, Some(&r))
            .unwrap_err()
            .contains("points to a package named `shared`")
    );
    let strict = Manifest::parse(
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nshared = { path = \"shared\", version = \"9\" }\n",
    );
    assert!(strict.is_err(), "path and version together are rejected");
}

#[test]
fn registry_dependencies_need_a_registry() {
    let e = telvax_pkg::resolver::resolve(Path::new("."), &app(&[("a", "1")]), None).unwrap_err();
    assert!(e.contains("no registry is configured"), "{e}");
}

// ---- projects and the compiler ---------------------------------------------------------------

#[test]
fn locking_installing_and_compiling_against_dependencies() {
    let _g = HOME.lock().unwrap_or_else(|e| e.into_inner());
    let home = Dir::new("home");
    unsafe { std::env::set_var("TVPM_HOME", &home.0) };
    let reg = Dir::new("e2e");
    publish(
        &reg,
        "mathx",
        "1.2.0",
        &[],
        r#"
fn double(x: i32) -> i32:
    x * 2
"#,
    );
    publish(
        &reg,
        "calc",
        "0.5.0",
        &[("mathx", "^1")],
        r#"
import mathx.double

fn quad(x: i32) -> i32:
    double(double(x))
"#,
    );

    let proj = Dir::new("proj");
    proj.write(
        "telvax.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\ncalc = \"^0.5\"\n",
    )
    .write(
        "src/main.tvx",
        r#"
import calc.quad

fn main() -> i32:
    quad(5)
"#,
    );
    let project = Project::open(&proj.0).unwrap();
    assert!(!project.lock_is_fresh());
    assert!(
        project
            .dependency_packages()
            .unwrap_err()
            .contains("tvpm install")
    );

    let r = DirRegistry::new(&reg.0);
    let (res, lock) = project.lock(Some(&r)).unwrap();
    assert_eq!(versions(&res), ["calc 0.5.0", "mathx 1.2.0"]);
    assert!(project.lock_is_fresh());
    assert_eq!(
        Lock::read(&proj.0).unwrap().unwrap(),
        lock,
        "the lockfile on disk matches"
    );
    assert!(
        project
            .dependency_packages()
            .unwrap_err()
            .contains("not installed")
    );

    let fetched = project.install(Some(&r)).unwrap();
    assert_eq!(fetched, ["calc 0.5.0", "mathx 1.2.0"]);
    assert!(
        project.install(Some(&r)).unwrap().is_empty(),
        "a second install downloads nothing"
    );

    let deps = project.dependency_packages().unwrap();
    let loaded = telvax_modules::load(&project.entry().unwrap(), &deps).unwrap();
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    let (hir, diags) = telvax_hir_check(&loaded.program);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(run_main(hir), "20");

    let tree = project.tree().unwrap();
    assert!(
        tree.contains("└── calc 0.5.0") && tree.contains("    └── mathx 1.2.0"),
        "{tree}"
    );
}

fn telvax_hir_check(p: &telvax_ast_program::Program) -> (telvax_hir::hir::Program, Vec<String>) {
    let (hir, d) = telvax_hir::lower(p);
    (hir, d.into_iter().map(|d| d.message).collect())
}

fn run_main(hir: telvax_hir::hir::Program) -> String {
    telvax_runtime::Interp::new(hir)
        .unwrap()
        .call("main", vec![])
        .unwrap()
        .to_string()
}

mod telvax_ast_program {
    pub use telvax_ast::Program;
}

#[test]
fn a_stale_lock_is_detected_and_checksums_are_enforced() {
    let _g = HOME.lock().unwrap_or_else(|e| e.into_inner());
    let home = Dir::new("home2");
    unsafe { std::env::set_var("TVPM_HOME", &home.0) };
    let reg = Dir::new("stale");
    publish(
        &reg,
        "a",
        "1.0.0",
        &[],
        r#"
fn a():
    pass
"#,
    );
    let proj = Dir::new("proj2");
    proj.write(
        "telvax.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\na = \"^1\"\n",
    )
    .write(
        "src/main.tvx",
        r#"
fn main():
    pass
"#,
    );
    let project = Project::open(&proj.0).unwrap();
    let r = DirRegistry::new(&reg.0);
    project.lock(Some(&r)).unwrap();
    assert!(project.lock_is_fresh());

    // Changing the requirement so the locked version no longer fits makes the lock stale.
    proj.write(
        "telvax.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\na = \"^2\"\n",
    );
    assert!(!Project::open(&proj.0).unwrap().lock_is_fresh());

    // A lockfile whose checksum disagrees with the download refuses to install.
    proj.write(
        "telvax.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\na = \"^1\"\n",
    );
    let mut lock = Lock::read(&proj.0).unwrap().unwrap();
    lock.packages
        .iter_mut()
        .find(|p| p.name == "a")
        .unwrap()
        .checksum = Some("0".repeat(64));
    lock.write(&proj.0).unwrap();
    let e = Project::open(&proj.0)
        .unwrap()
        .install(Some(&r))
        .unwrap_err();
    assert!(e.contains("checksum mismatch"), "{e}");
}

#[test]
fn discovery_walks_up_to_the_manifest() {
    let proj = Dir::new("discover");
    proj.write(
        "telvax.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .write(
        "src/deep/x.tvx",
        r#"
fn f():
    pass
"#,
    )
    .write(
        "src/main.tvx",
        r#"
fn main():
    pass
"#,
    );
    let found = Project::discover(&proj.p("src/deep/x.tvx"))
        .unwrap()
        .unwrap();
    assert_eq!(found.manifest.package.name, "app");
    assert!(found.entry().unwrap().ends_with("src/main.tvx"));
    let nowhere = Dir::new("nothing");
    assert!(Project::discover(&nowhere.0).unwrap().is_none());
}

// ---- HTTP registry ---------------------------------------------------------------------------

/// Serves `root` over HTTP on a random port (just enough of HTTP for GET).
fn serve_static(root: PathBuf) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .trim_start_matches('/')
                    .to_string();
                let (status, body) = match std::fs::read(root.join(&path)) {
                    Ok(b) if !path.contains("..") => ("200 OK", b),
                    _ => ("404 Not Found", b"not found".to_vec()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    url
}

#[test]
fn registries_work_over_http() {
    let _g = HOME.lock().unwrap_or_else(|e| e.into_inner());
    let home = Dir::new("home3");
    unsafe { std::env::set_var("TVPM_HOME", &home.0) };
    let reg = Dir::new("http");
    publish(
        &reg,
        "webdep",
        "2.1.0",
        &[],
        r#"
fn w() -> i32:
    7
"#,
    );
    let url = serve_static(reg.0.clone());
    let remote = registry::open(&url);
    assert_eq!(remote.versions("webdep").unwrap().len(), 1);
    assert!(
        remote.versions("missing").unwrap().is_empty(),
        "404 means unknown package"
    );

    let proj = Dir::new("proj3");
    proj.write(
        "telvax.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nwebdep = \"2\"\n",
    )
    .write(
        "src/main.tvx",
        r#"
import webdep.w

fn main() -> i32:
    w()
"#,
    );
    let project = Project::open(&proj.0).unwrap();
    project.lock(Some(remote.as_ref())).unwrap();
    assert_eq!(
        project.install(Some(remote.as_ref())).unwrap(),
        ["webdep 2.1.0"]
    );
    let deps = project.dependency_packages().unwrap();
    let loaded = telvax_modules::load(&project.entry().unwrap(), &deps).unwrap();
    let (hir, diags) = telvax_hir_check(&loaded.program);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(run_main(hir), "7");
}
