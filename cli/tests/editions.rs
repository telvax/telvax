//! The programs of a released syntax edition keep working. `tests/editions/<edition>/` holds
//! them, frozen: each file's hash is in `CHECKSUMS`, so changing one is a visible, deliberate act.
//! See `docs/guide/stability.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

const TELVAX: &str = env!("CARGO_BIN_EXE_telvax");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/editions")
}

/// FNV-1a: small, and the same on every platform and Rust version.
fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(TELVAX).args(args).output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

fn editions() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn programs(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "tvx"))
        .collect();
    files.sort();
    files
}

#[test]
fn every_edition_directory_is_one_the_compiler_reads() {
    for dir in editions() {
        let name = dir.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            telvax_pkg::manifest::EDITIONS.contains(&name.as_str()),
            "tests/editions/{name} is not in EDITIONS"
        );
    }
    for edition in telvax_pkg::manifest::EDITIONS {
        assert!(
            root().join(edition).is_dir(),
            "edition {edition} has no frozen programs in tests/editions/{edition}"
        );
    }
}

#[test]
fn frozen_programs_are_not_edited() {
    for dir in editions() {
        let listed = std::fs::read_to_string(dir.join("CHECKSUMS")).unwrap();
        let mut expected: Vec<(String, String)> = listed
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let (sum, name) = l.split_once("  ").expect("`<hash>  <file>`");
                (name.to_string(), sum.to_string())
            })
            .collect();
        expected.sort();
        let actual: Vec<(String, String)> = programs(&dir)
            .iter()
            .map(|p| {
                (
                    p.file_name().unwrap().to_str().unwrap().to_string(),
                    hash(&std::fs::read(p).unwrap()),
                )
            })
            .collect();
        assert_eq!(
            actual,
            expected,
            "a program of edition {} changed, or one was added without a line in CHECKSUMS. \
             Existing programs are frozen: a change that breaks one needs a new edition",
            dir.display()
        );
    }
}

#[test]
fn frozen_programs_check_format_and_pass_their_tests() {
    for dir in editions() {
        for file in programs(&dir) {
            let path = file.to_str().unwrap();
            let (ok, out) = run(&["check", path]);
            assert!(ok, "{path} no longer checks:\n{out}");
            let (ok, out) = run(&["fmt", "--check", path]);
            assert!(ok, "{path} is no longer formatted as is:\n{out}");
            if std::fs::read_to_string(&file).unwrap().contains("fn test_") {
                let (ok, out) = run(&["test", path]);
                assert!(ok, "{path}: a test fails:\n{out}");
            }
        }
    }
}

#[test]
fn a_2026_program_moves_to_the_next_edition_with_migrate() {
    let dir = std::env::temp_dir().join(format!("telvax-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let main = dir.join("src/main.tvx");
    let program = "fn main() -> Text:\n    std.string.join([\"a\", \"b\"], \"-\") + f\"{std.string.repeat(\"x\", 2)}\"\n";
    std::fs::write(&main, program).unwrap();
    let manifest = |edition: &str| {
        format!("[package]\nname = \"moved\"\nversion = \"0.1.0\"\nedition = \"{edition}\"\n")
    };
    let path = main.to_str().unwrap();
    // In 2026 the old form still checks, with a warning that says what to write.
    std::fs::write(dir.join("telvax.toml"), manifest("2026")).unwrap();
    let (ok, out) = run(&["check", path]);
    assert!(ok, "{out}");
    assert!(
        out.contains("`std.string.join` is deprecated: write `std.text.join`"),
        "{out}"
    );
    // In the next edition it is an error that names the edition.
    std::fs::write(dir.join("telvax.toml"), manifest("2026")).unwrap();
    let (ok, out) = run(&["migrate", "--check", dir.to_str().unwrap()]);
    assert!(
        !ok && out.contains("would rewrite") && out.contains("would set"),
        "{out}"
    );
    assert_eq!(
        std::fs::read_to_string(&main).unwrap(),
        program,
        "--check writes nothing"
    );
    std::fs::write(dir.join("telvax.toml"), manifest("2027-preview")).unwrap();
    let (ok, out) = run(&["check", path]);
    assert!(
        !ok && out.contains("is not in edition 2027-preview: write `std.text.join`"),
        "{out}"
    );
    // `migrate` rewrites it, the result checks and runs the same, and says so when there is nothing left.
    std::fs::write(dir.join("telvax.toml"), manifest("2026")).unwrap();
    let (ok, out) = run(&["migrate", dir.to_str().unwrap()]);
    assert!(
        ok && out.contains("2 place(s) of `std.string` to `std.text`"),
        "{out}"
    );
    assert!(
        std::fs::read_to_string(dir.join("telvax.toml"))
            .unwrap()
            .contains("edition = \"2027-preview\"")
    );
    let (ok, out) = run(&["check", path]);
    assert!(ok && !out.contains("deprecated"), "{out}");
    let (ok, out) = run(&["run", path]);
    assert!(ok && out.trim() == "a-bxx", "{out}");
    let (ok, out) = run(&["migrate", dir.to_str().unwrap()]);
    assert!(ok && out.contains("nothing to migrate"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
