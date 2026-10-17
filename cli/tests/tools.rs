use std::process::Command;

const TELVAX: &str = env!("CARGO_BIN_EXE_telvax");

fn scratch(name: &str, text: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("telvax-tools-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.tvx"));
    std::fs::write(&file, text).unwrap();
    file
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(TELVAX).args(args).output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn fmt_rewrites_in_place_and_check_reports() {
    let messy = "fn main() -> i32:\n  let x=1\n  x+2\n";
    let tidy = "fn main() -> i32:\n    let x = 1\n    x + 2\n";
    let file = scratch("messy", messy);
    let path = file.to_str().unwrap();
    let (ok, out, _) = run(&["fmt", "--check", path]);
    assert!(!ok && out.contains("would reformat"), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), messy);

    let (ok, out, err) = run(&["fmt", path]);
    assert!(ok, "{err}");
    assert!(out.contains("formatted"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), tidy);
    let (ok, _, _) = run(&["fmt", "--check", path]);
    assert!(ok, "formatted code must pass --check");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn fmt_takes_directories_and_shows_a_diff() {
    let dir = std::env::temp_dir().join(format!("telvax-tools-{}-fmtdir", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::create_dir_all(dir.join("target")).unwrap();
    let messy = "fn main() -> i32:\n  let x=1\n  x+2\n";
    let tidy = "fn main() -> i32:\n    let x = 1\n    x + 2\n";
    std::fs::write(dir.join("a.tvx"), messy).unwrap();
    std::fs::write(dir.join("sub").join("b.tvx"), messy).unwrap();
    std::fs::write(dir.join("target").join("c.tvx"), messy).unwrap();
    let path = dir.to_str().unwrap();
    let (ok, out, _) = run(&["fmt", "--diff", path]);
    assert!(!ok);
    assert!(
        out.contains("-  let x=1") && out.contains("+    let x = 1"),
        "{out}"
    );
    assert!(
        out.contains("a.tvx") && out.contains("b.tvx") && !out.contains("c.tvx"),
        "{out}"
    );
    assert_eq!(std::fs::read_to_string(dir.join("a.tvx")).unwrap(), messy);
    let (ok, out, _) = run(&["fmt", "--check", path]);
    assert!(!ok && out.contains("would reformat"), "{out}");
    let (ok, _, err) = run(&["fmt", path]);
    assert!(ok, "{err}");
    assert_eq!(std::fs::read_to_string(dir.join("a.tvx")).unwrap(), tidy);
    assert_eq!(
        std::fs::read_to_string(dir.join("sub").join("b.tvx")).unwrap(),
        tidy
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("target").join("c.tvx")).unwrap(),
        messy
    );
    let out = Command::new(TELVAX)
        .args(["fmt", "--check"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "no path means this directory");
    let (ok, _, err) = run(&["fmt", "--wide", path]);
    assert!(!ok && err.contains("unknown option `--wide`"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fmt_refuses_broken_source() {
    let file = scratch("broken", "fn main():\n    let s = \"open\n");
    let (ok, _, err) = run(&["fmt", file.to_str().unwrap()]);
    assert!(!ok && err.contains("cannot format"), "{err}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn lint_reports_unused_code() {
    let file = scratch(
        "lintme",
        r#"
fn _helper() -> i32:
    1

fn main() -> i32:
    let unused = 2
    3
"#,
    );
    let path = file.to_str().unwrap();
    let (ok, _, err) = run(&["lint", path]);
    assert!(ok, "{err}");
    assert!(
        err.contains("unused variable `unused`") && err.contains("`_helper` is never used"),
        "{err}"
    );
    let (ok, _, _) = run(&["lint", path, "--deny-warnings"]);
    assert!(!ok);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn test_runs_the_test_functions() {
    let src = "fn double(x: Int) -> Int:\n    x * 2\n\nfn test_double():\n    assert_eq(double(2), 4)\n\nfn test_wrong():\n    assert_eq(double(3), 7)\n";
    let file = scratch("tests", src);
    let path = file.to_str().unwrap();
    let (ok, out, err) = run(&["test", path]);
    assert!(!ok, "a failing test must fail the run");
    assert!(out.contains("ok      test_double"), "{out}");
    assert!(
        out.contains("FAILED  test_wrong") && out.contains("left is `6`, right is `7`"),
        "{out}"
    );
    assert!(out.contains("1 passed, 1 failed"), "{out}{err}");
    let (ok, out, _) = run(&["test", path, "double"]);
    assert!(ok && out.contains("1 passed, 0 failed"), "{out}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn test_on_a_directory_runs_every_test_file() {
    let dir = std::env::temp_dir().join(format!("telvax-tools-{}-testdir", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let good = "fn test_one():\n    assert_eq(1, 1)\n";
    std::fs::write(dir.join("test_a.tvx"), good).unwrap();
    std::fs::write(dir.join("sub").join("b_test.tvx"), good).unwrap();
    std::fs::write(
        dir.join("helper.tvx"),
        "fn test_ignored():\n    assert_eq(1, 2)\n",
    )
    .unwrap();
    let path = dir.to_str().unwrap();
    let (ok, out, err) = run(&["test", path]);
    assert!(ok, "{out}{err}");
    assert!(
        out.contains("test_a.tvx") && out.contains("b_test.tvx"),
        "{out}"
    );
    assert!(!out.contains("helper.tvx"), "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
    std::fs::write(
        dir.join("test_c.tvx"),
        "fn test_bad():\n    assert_eq(1, 2)\n",
    )
    .unwrap();
    let (ok, out, _) = run(&["test", path]);
    assert!(!ok && out.contains("2 passed, 1 failed"), "{out}");
    let (ok, out, _) = run(&["test", path, "one"]);
    assert!(ok && out.contains("2 passed, 0 failed"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_parameters_are_fixtures_made_for_each_test() {
    let src = r#"fn fixture_base() -> Int:
    10

fn fixture_total(base: Int) -> Int:
    base * 2

fn teardown_total(total: Int):
    std.io.print(f"cleaned {total};")

fn test_uses_both(base: Int, total: Int):
    assert_eq(total, base * 2)

fn test_fails(total: Int):
    assert_eq(total, 0)
"#;
    let file = scratch("fixtures", src);
    let path = file.to_str().unwrap();
    let (ok, out, err) = run(&["test", path]);
    assert!(!ok, "{out}{err}");
    assert!(out.contains("ok      test_uses_both"), "{out}");
    assert!(
        out.contains("FAILED  test_fails: assertion failed: left is `20`, right is `0`"),
        "{out}"
    );
    assert_eq!(
        out.matches("cleaned 20;").count(),
        2,
        "teardown runs after a pass and after a failure: {out}"
    );
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_missing_or_mistyped_fixture_says_what_to_write() {
    let src = "fn fixture_n() -> Int:\n    1\n\nfn test_missing(db: Int):\n    assert_eq(db, 1)\n\nfn test_type(n: Text):\n    assert_eq(n, \"1\")\n";
    let file = scratch("badfixture", src);
    let path = file.to_str().unwrap();
    let (ok, out, _) = run(&["test", path]);
    assert!(!ok);
    assert!(
        out.contains("no fixture: write `fn fixture_db() -> ...`"),
        "{out}"
    );
    assert!(
        out.contains("takes `n: Text` but `fixture_n` returns `Int`"),
        "{out}"
    );
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn lint_does_not_flag_tests_and_fixtures() {
    let src = "fn fixture_n() -> Int:\n    1\n\nfn teardown_n(n: Int):\n    std.io.print(f\"{n}\")\n\nfn cases_m() -> List[Int]:\n    [1]\n\nfn test_m(m: Int, n: Int):\n    assert_eq(m, n)\n";
    let file = scratch("lintfixtures", src);
    let (ok, _, err) = run(&["lint", file.to_str().unwrap(), "--deny-warnings"]);
    assert!(ok, "{err}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn cases_run_a_test_once_for_each_value() {
    let src = r#"fn double(x: Int) -> Int:
    x * 2

fn cases_double() -> List[(Int, Int)]:
    [(1, 2), (2, 4), (3, 7)]

fn test_double(x: Int, want: Int):
    assert_eq(double(x), want)

fn cases_small() -> List[Int]:
    [1, 2]

fn fixture_limit() -> Int:
    5

fn test_small(n: Int, limit: Int):
    assert(n < limit)
"#;
    let file = scratch("cases", src);
    let path = file.to_str().unwrap();
    let (ok, out, err) = run(&["test", path]);
    assert!(!ok, "{out}{err}");
    assert!(out.contains("ok      test_double[(1, 2)]"), "{out}");
    assert!(out.contains("ok      test_double[(2, 4)]"), "{out}");
    assert!(
        out.contains("FAILED  test_double[(3, 7)]: assertion failed: left is `6`, right is `7`"),
        "{out}"
    );
    assert!(
        out.contains("ok      test_small[1]") && out.contains("ok      test_small[2]"),
        "{out}"
    );
    assert!(out.contains("4 passed, 1 failed"), "{out}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn bad_cases_say_what_is_wrong() {
    let src = r#"fn cases_wide() -> List[(Int, Int, Int)]:
    [(1, 2, 3)]

fn test_wide(a: Int, b: Int):
    assert(a < b)

fn cases_typed() -> List[Text]:
    ["a"]

fn test_typed(n: Int):
    assert(n > 0)

fn cases_empty() -> List[Int]:
    []

fn test_empty(n: Int):
    assert(n > 0)
"#;
    let file = scratch("badcases", src);
    let path = file.to_str().unwrap();
    let (ok, out, _) = run(&["test", path]);
    assert!(!ok);
    assert!(
        out.contains("a case of `cases_wide` has 3 value(s) but `test_wide` takes 2 parameter(s)"),
        "{out}"
    );
    assert!(
        out.contains("`test_typed` takes `n: Int` but the cases of `cases_typed` give `Text`"),
        "{out}"
    );
    assert!(out.contains("`cases_empty` has no cases"), "{out}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn mock_replaces_a_function_for_one_test() {
    let src = r#"fn price(item: Text) -> Int:
    100

fn total(item: Text) -> Int:
    price(item) * 2

fn config() -> Text:
    match std.fs.read("/definitely/missing.txt"):
        case Ok(text):
            text
        case Err(_):
            "default"

fn fake_price(item: Text) -> Int:
    7

fn fake_read(path: Text) -> Result[Text, Text]:
    Ok("from mock")

fn test_user_function():
    std.test.mock(price, fake_price)
    assert_eq(total("a"), 14)
    assert_eq(total("b"), 14)
    assert_eq(std.test.calls(price), 2)

fn test_closure():
    std.test.mock(price, item => 1)
    assert_eq(total("a"), 2)

fn test_std_function():
    assert_eq(config(), "default")
    std.test.mock("std.fs.read", fake_read)
    assert_eq(config(), "from mock")

fn test_zz_back_to_the_real_one():
    assert_eq(total("a"), 200)
    assert_eq(config(), "default")
"#;
    let file = scratch("mocks", src);
    let (ok, out, err) = run(&["test", file.to_str().unwrap()]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("4 passed, 0 failed"), "{out}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn mock_checks_what_it_is_given() {
    let src = r#"fn price(item: Text) -> Int:
    100

fn wrong(item: Int) -> Int:
    1

fn test_wrong_type():
    std.test.mock(price, wrong)

fn test_not_a_std_function():
    std.test.mock("std.nowhere.nothing", price)

fn test_calls_without_a_mock():
    std.test.calls(price)
"#;
    let file = scratch("badmocks", src);
    let (ok, out, err) = run(&["test", file.to_str().unwrap()]);
    assert!(!ok);
    assert!(
        err.contains("the replacement must have the type of the function it replaces"),
        "{out}{err}"
    );
    let _ = std::fs::remove_dir_all(file.parent().unwrap());

    let src = r#"fn price(item: Text) -> Int:
    100

fn test_not_a_std_function():
    std.test.mock("std.nowhere.nothing", price)

fn test_calls_without_a_mock():
    assert_eq(std.test.calls(price), 0)
"#;
    let file = scratch("badmocks2", src);
    let (ok, out, _) = run(&["test", file.to_str().unwrap()]);
    assert!(!ok);
    assert!(
        out.contains("`std.nowhere.nothing` is not a std function"),
        "{out}"
    );
    assert!(
        out.contains("`price` is not mocked: call `std.test.mock` first"),
        "{out}"
    );
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn coverage_lists_the_lines_the_tests_did_not_run() {
    let src = r#"fn sign(n: Int) -> Text:
    if n > 0:
        "positive"
    else:
        "other"

fn never_called() -> Int:
    let x = 1
    x + 1

fn fixture_n() -> Int:
    1

fn teardown_n(n: Int):
    print(f"{n}")

fn test_sign(n: Int):
    assert_eq(sign(5), "positive")
    assert_eq(n, 1)
"#;
    let file = scratch("coverage", src);
    let path = file.to_str().unwrap();
    let (ok, out, err) = run(&["test", path, "--coverage"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("coverage (lines that ran"), "{out}");
    // Lines 2, 3, 5, 8 and 9 have code, and 5, 8 and 9 never ran. The test, the fixture and the
    // teardown are not counted.
    assert!(out.contains("2/5  40.0%   missing 5, 8-9"), "{out}");
    assert!(out.contains("total  2/5  40.0%"), "{out}");
    let (ok, out, err) = run(&["test", path, "--fail-under", "40"]);
    assert!(ok, "{out}{err}");
    let (ok, out, err) = run(&["test", path, "--fail-under", "90"]);
    assert!(!ok);
    assert!(
        err.contains("coverage is 40.0%, below the 90% that --fail-under asks for"),
        "{out}{err}"
    );
    let (ok, _, err) = run(&["test", path, "--fail-under", "lots"]);
    assert!(
        !ok && err.contains("--fail-under takes a percentage"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn coverage_adds_up_over_a_directory() {
    let dir = std::env::temp_dir().join(format!("telvax-tools-{}-covdir", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let one = "fn a(n: Int) -> Int:\n    if n > 0:\n        1\n    else:\n        2\n\nfn test_a():\n    assert_eq(a(1), 1)\n";
    let two = "fn b() -> Int:\n    5\n\nfn test_b():\n    assert_eq(b(), 5)\n";
    std::fs::write(dir.join("test_one.tvx"), one).unwrap();
    std::fs::write(dir.join("test_two.tvx"), two).unwrap();
    let (ok, out, err) = run(&["test", dir.to_str().unwrap(), "--coverage"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("missing 5"), "{out}");
    assert!(out.contains("total  3/4  75.0%"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn macros_quote_their_argument_and_dbg_gives_the_value_back() {
    let src = "fn pick(n: Int) -> Int:\n    if n > 0:\n        n\n    else:\n        todo!()\n\nfn test_dbg():\n    let x = dbg!(pick(4))\n    assert!(x == 4)\n\nfn test_quote():\n    let x = 1\n    assert!(x > 3)\n\nfn test_message():\n    assert!(1 > 3, \"one is small\")\n\nfn test_todo():\n    pick(0)\n\nfn test_panic():\n    panic!(\"boom\")\n";
    let file = scratch("macros", src);
    let path = file.to_str().unwrap();
    let (ok, out, err) = run(&["test", path]);
    assert!(!ok);
    assert!(out.contains("ok      test_dbg"), "{out}");
    assert!(err.contains("pick(4) = 4"), "{err}");
    assert!(out.contains("test_quote: assertion failed: x > 3"), "{out}");
    assert!(out.contains("test_message: one is small"), "{out}");
    assert!(out.contains("test_todo: not yet implemented"), "{out}");
    assert!(out.contains("test_panic: boom"), "{out}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn an_unknown_macro_names_the_ones_that_exist() {
    let file = scratch("badmacro", "fn main() -> i32:\n    foo!(1)\n    0\n");
    let (ok, out, err) = run(&["check", file.to_str().unwrap()]);
    assert!(!ok);
    assert!(
        format!("{out}{err}").contains("there is no macro `foo!`"),
        "{out}{err}"
    );
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn allow_limits_what_a_program_may_touch() {
    let src = "import std.fs\n\nfn has(path: Text) -> Bool:\n    std.fs.exists(path)\n\nfn test_has():\n    assert(has(\"/\"))\n\nfn add(a: Int, b: Int) -> Int:\n    a + b\n";
    let file = scratch("caps", src);
    let path = file.to_str().unwrap();
    // Everything is allowed unless `--allow` says otherwise.
    let (ok, out, _) = run(&["run", path, "has", "/"]);
    assert!(ok && out.contains("True"), "{out}");
    let (ok, _, err) = run(&["run", path, "has", "/", "--allow", "none"]);
    assert!(!ok);
    assert!(
        err.contains("`std.fs.exists` needs the `fs.read` capability")
            && err.contains("--allow fs.read"),
        "{err}"
    );
    let (ok, out, _) = run(&["run", path, "has", "/", "--allow", "fs.read"]);
    assert!(ok && out.contains("True"), "{out}");
    // A function that does no I/O is not affected.
    let (ok, out, _) = run(&["run", path, "add", "1", "2", "--allow", "none"]);
    assert!(ok && out.contains('3'), "{out}");
    let (ok, out, _) = run(&["test", path, "--allow", "env"]);
    assert!(
        !ok && out.contains("FAILED  test_has") && out.contains("--allow fs.read"),
        "{out}"
    );
    let (ok, _, err) = run(&["run", path, "add", "1", "2", "--allow", "disk"]);
    assert!(!ok && err.contains("unknown capability `disk`"), "{err}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn doctor_reports_the_build_and_an_unknown_command_is_named() {
    let (ok, out, _) = run(&["doctor"]);
    assert!(ok, "{out}");
    assert!(out.contains("syntax editions read: 2026"), "{out}");
    assert!(out.contains("native backend (build, jit)"), "{out}");
    // A key is reported as set or not, never shown.
    let key = Command::new(TELVAX)
        .arg("doctor")
        .env("OPENAI_API_KEY", "sk-secret-value")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&key.stdout);
    assert!(
        text.contains("OPENAI_API_KEY        set") && !text.contains("sk-secret-value"),
        "{text}"
    );
    let out = Command::new(TELVAX).arg("chek").output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        err.contains("unknown command `chek`; did you mean `check`?"),
        "{err}"
    );
    let out = Command::new(TELVAX).arg("frobnicate").output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("unknown command `frobnicate`") && !err.contains("did you mean"),
        "{err}"
    );
}

#[test]
fn run_defaults_to_main_and_reads_arguments_by_their_declared_type() {
    let src = "fn main() -> i32:\n    7\n\nfn add(a: Int, b: Int) -> Int:\n    a + b\n\nfn shout(s: Text) -> Text:\n    s\n\nfn total(xs: List[Int]) -> Int:\n    var t = 0\n    for x in xs:\n        t += x\n    t\n";
    let file = scratch("runargs", src);
    let path = file.to_str().unwrap();
    let (ok, out, err) = run(&["run", path]);
    assert!(ok && out.trim() == "7", "{out}{err}");
    // The check that comes before is silent: only `telvax check` says "ok".
    assert!(!err.contains(" ok"), "{err}");
    let (ok, out, _) = run(&["run", path, "--allow", "none"]);
    assert!(ok && out.trim() == "7", "{out}");
    // A `Text` parameter keeps the word as it is.
    let (ok, out, _) = run(&["run", path, "shout", "123"]);
    assert!(ok && out.trim() == "123", "{out}");
    let (ok, out, _) = run(&["run", path, "total", "[1,2,3]"]);
    assert!(ok && out.trim() == "6", "{out}");
    let (ok, _, err) = run(&["run", path, "add", "1", "x"]);
    assert!(
        !ok && err.contains("argument `b` of `add` is Int, and `x` is not a value"),
        "{err}"
    );
    let (ok, _, err) = run(&["run", path, "add", "1"]);
    assert!(
        !ok && err.contains("takes 2 argument(s) (a: Int, b: Int), got 1"),
        "{err}"
    );
    let (ok, _, err) = run(&["run", path, "nope"]);
    assert!(
        !ok && err.contains("has no function `nope`; it has: add, main, shout, total"),
        "{err}"
    );
    let (ok, _, err) = run(&["check", path]);
    assert!(ok && err.contains(" ok"), "{err}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn check_takes_many_files_a_package_and_json() {
    let good = scratch("check_good", "fn main() -> i32:\n    0\n");
    let bad = scratch(
        "check_bad",
        "fn main() -> i32:\n    let x: Int = \"a\"\n    0\n",
    );
    let (good_path, bad_path) = (good.to_str().unwrap(), bad.to_str().unwrap());
    let (ok, _, err) = run(&["check", good_path, bad_path]);
    assert!(!ok);
    assert!(
        err.contains("2 file(s) checked, 1 with errors") && err.contains("check_good.tvx` ok"),
        "{err}"
    );
    // JSON: one array on stdout, the places as line and column, and the exit code still says failed.
    let (ok, out, _) = run(&["check", "--json", bad_path]);
    assert!(!ok);
    let records: serde_json::Value = serde_json::from_str(&out).unwrap();
    let first = &records[0];
    assert_eq!(first["severity"], "error");
    assert_eq!(
        (first["line"].as_u64(), first["column"].as_u64()),
        (Some(2), Some(18))
    );
    assert_eq!(first["end_column"].as_u64(), Some(21));
    assert!(
        first["message"]
            .as_str()
            .unwrap()
            .contains("expected `Int`, found `Text`")
    );
    assert!(first["file"].as_str().unwrap().ends_with("check_bad.tvx"));
    let (ok, out, _) = run(&["check", "--json", good_path]);
    assert!(ok && out.trim() == "[]", "{out}");
    let (ok, out, _) = run(&["check", "--json", "no-such-file.tvx"]);
    assert!(
        !ok && out.contains("cannot read `no-such-file.tvx`") && out.contains("\"line\":null"),
        "{out}"
    );
    let (ok, _, err) = run(&["check", "--bogus"]);
    assert!(!ok && err.contains("no option `--bogus`"), "{err}");
    // Without a file, the package around the current directory.
    let pkg = std::env::temp_dir().join(format!("telvax-tools-{}-checkpkg", std::process::id()));
    std::fs::create_dir_all(pkg.join("src")).unwrap();
    std::fs::write(
        pkg.join("telvax.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(pkg.join("src/main.tvx"), "fn main() -> i32:\n    0\n").unwrap();
    let out = Command::new(TELVAX)
        .arg("check")
        .current_dir(&pkg)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && err.contains("main.tvx` ok"),
        "{err}"
    );
    let outside = Command::new(TELVAX)
        .arg("check")
        .current_dir(good.parent().unwrap())
        .output()
        .unwrap();
    assert!(!outside.status.success());
    assert!(String::from_utf8_lossy(&outside.stderr).contains("inside a package"));
    let _ = std::fs::remove_dir_all(&pkg);
    let _ = std::fs::remove_dir_all(good.parent().unwrap());
    let _ = std::fs::remove_dir_all(bad.parent().unwrap());
}

#[test]
fn deeply_nested_programs_are_checked_not_crashed() {
    // Calls inside calls, within the limit on brackets, and a sum of many terms: the checker
    // recurses once per level and used to overflow the 8 MB stack of a main thread.
    let calls = format!(
        "fn f(x: Int) -> Int:\n    x\n\nfn main() -> i32:\n    let y = {}1{}\n    0\n",
        "f(".repeat(190),
        ")".repeat(190)
    );
    let sum = format!(
        "fn main() -> i32:\n    let y = {}1\n    0\n",
        "1 + ".repeat(5_000)
    );
    for (name, src) in [("calls", calls), ("sum", sum)] {
        let file = scratch(&format!("deep-{name}"), &src);
        let (ok, _, err) = run(&["check", file.to_str().unwrap()]);
        assert!(ok, "{name}: {err}");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }
}

#[test]
fn nesting_past_the_limit_is_an_error_not_a_crash() {
    let blocks = {
        let mut s = String::from("fn f():\n");
        for level in 0..5000 {
            s.push_str(&format!("{}if True:\n", "    ".repeat(level + 1)));
        }
        s.push_str(&format!("{}pass\n", "    ".repeat(5001)));
        s
    };
    let brackets = format!("let x = {}1{}\n", "(".repeat(5000), ")".repeat(5000));
    for (name, src, needle) in [
        ("blocks", blocks, "blocks are nested too deeply"),
        ("brackets", brackets, "brackets are nested too deeply"),
    ] {
        let file = scratch(&format!("limit-{name}"), &src);
        let out = Command::new(TELVAX)
            .args(["check", file.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{name}: it must fail, not crash"
        );
        let shown = String::from_utf8_lossy(&out.stderr).into_owned()
            + &String::from_utf8_lossy(&out.stdout);
        assert!(shown.contains(needle), "{name}: {shown}");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }
}

#[test]
fn lint_levels_come_from_the_manifest_and_the_flags() {
    let dir = std::env::temp_dir().join(format!("telvax-lintcfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let manifest = |lint: &str| {
        std::fs::write(
            dir.join("telvax.toml"),
            format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[lint]\n{lint}"),
        )
        .unwrap();
    };
    let src = dir.join("src/main.tvx");
    std::fs::write(&src, "fn main():\n    let dead = 5\n").unwrap();
    let path = src.to_str().unwrap();

    manifest("unused-variable = \"deny\"\n");
    let (ok, _, err) = run(&["lint", path]);
    assert!(!ok && err.contains("unused variable `dead`"), "{err}");

    // a flag wins over the manifest
    let (ok, _, err) = run(&["lint", path, "--warn", "unused-variable"]);
    assert!(ok && err.contains("unused variable `dead`"), "{err}");

    manifest("unused-variable = \"allow\"\n");
    let (ok, _, err) = run(&["lint", path, "--deny-warnings"]);
    assert!(ok && err.contains("no warnings"), "{err}");

    manifest("unused-var = \"allow\"\n");
    let (ok, _, err) = run(&["lint", path]);
    assert!(!ok && err.contains("no lint rule `unused-var`"), "{err}");

    let (ok, out, _) = run(&["lint", "--list-rules"]);
    assert!(ok && out.contains("unused-function"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn doc_writes_markdown_from_signatures_and_comments() {
    let file = scratch(
        "documented",
        "# Adds one.\nfn bump(n: Int) -> Int:\n    n + 1\n\nfn _hidden() -> Int:\n    0\n",
    );
    let path = file.to_str().unwrap();
    let (ok, out, err) = run(&["doc", path]);
    assert!(ok, "{err}");
    assert!(
        out.contains("fn bump(n: Int) -> Int") && out.contains("Adds one."),
        "{out}"
    );
    assert!(out.contains("_hidden"), "{out}");
    let (ok, out, _) = run(&["doc", "--public", path]);
    assert!(ok && !out.contains("_hidden"), "{out}");
    let (ok, _, err) = run(&["doc", "--nope", path]);
    assert!(!ok && err.contains("unknown option"), "{err}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn schema_prints_the_type_of_a_csv() {
    let file = scratch("sales_2024", "");
    let csv = file.with_extension("csv");
    std::fs::write(
        &csv,
        "id,region,price,paid,note\n1,north,2.5,true,a\n2,south,3,false,\n",
    )
    .unwrap();
    let path = csv.to_str().unwrap();
    let (ok, out, err) = run(&["schema", path]);
    assert!(ok, "{err}");
    assert_eq!(
        out,
        "type Sales2024:\n    id: Int\n    region: Text\n    price: Float\n    paid: Bool\n    note: Text?\n"
    );
    let (ok, out, _) = run(&["schema", path, "--name", "Row"]);
    assert!(ok && out.starts_with("type Row:\n"), "{out}");

    std::fs::write(&csv, "unit price\n1\n").unwrap();
    let (ok, _, err) = run(&["schema", path]);
    assert!(
        !ok && err.contains("`unit price` cannot be a field name"),
        "{err}"
    );
    let (ok, _, err) = run(&["schema", "data.txt"]);
    assert!(!ok && err.contains("cannot tell the format"), "{err}");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[cfg(feature = "tables")] // writing Parquet needs the full build
#[test]
fn schema_reads_parquet_and_arrow_files() {
    let program = scratch(
        "write_both",
        "type Row:\n    id: Int\n    note: Text?\n\nfn main():\n    let t: Table[Row] = read_csv(\"in.csv\")\n    std.table.write_parquet(t, \"out.parquet\")\n    std.table.write_ipc(t, \"out.arrow\")\n",
    );
    let dir = program.parent().unwrap();
    std::fs::write(dir.join("in.csv"), "id,note\n1,a\n2,\n").unwrap();
    let out = Command::new(TELVAX)
        .current_dir(dir)
        .args(["run", "write_both.tvx"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for file in ["out.parquet", "out.arrow"] {
        let (ok, text, err) = run(&["schema", dir.join(file).to_str().unwrap(), "--name", "Row"]);
        assert!(ok, "{file}: {err}");
        assert_eq!(text, "type Row:\n    id: Int\n    note: Text?\n", "{file}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
