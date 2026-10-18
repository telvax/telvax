use std::path::{Path, PathBuf};

use telvax_modules::{Loaded, Package, load};

struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Project {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "telvax-mod-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Project(dir)
    }

    fn file(&self, rel: &str, text: &str) -> &Project {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        self
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dep(name: &str, project: &Project, src: &str) -> Package {
    Package {
        name: name.into(),
        src_dir: project.path(src),
    }
}

/// Loads, type-checks and returns (value of `main` as text, all messages).
fn run(loaded: &Loaded) -> (String, Vec<String>) {
    let mut messages: Vec<String> = loaded
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    let (hir, diags) = telvax_hir::lower(&loaded.program);
    messages.extend(diags.into_iter().map(|d| d.message));
    if !messages.is_empty() {
        return (String::new(), messages);
    }
    let interp = telvax_runtime::Interp::new(hir).unwrap();
    (interp.call("main", vec![]).unwrap().to_string(), messages)
}

fn ok(loaded: &Loaded) -> String {
    let (value, messages) = run(loaded);
    assert!(messages.is_empty(), "{messages:#?}");
    value
}

#[test]
fn sibling_modules_are_imported_by_path() {
    let p = Project::new("sibling");
    p.file(
        "main.tvx",
        r#"
import util.double

fn main() -> i32:
    double(21)
"#,
    )
    .file(
        "util.tvx",
        r#"
fn double(x: i32) -> i32:
    x * 2
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "42");
}

#[test]
fn structs_group_imports_and_nested_modules() {
    let p = Project::new("nested");
    p.file(
        "main.tvx",
        r#"
from crate.geometry import Point, norm2
import crate.geometry.shapes.area

fn main() -> i32:
    let p = Point(x: 3, y: 4)
    norm2(p) + area(2, 5)
"#,
    )
    .file(
        "geometry.tvx",
        r#"
type Point:
    x: i32
    y: i32

fn norm2(p: Point) -> i32:
    p.x * p.x + p.y * p.y
"#,
    )
    .file(
        "geometry/shapes.tvx",
        r#"
fn area(w: i32, h: i32) -> i32:
    w * h
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "35");
}

#[test]
fn mod_rs_style_directories_work() {
    let p = Project::new("modfile");
    p.file(
        "main.tvx",
        r#"
import tools.answer

fn main() -> i32:
    answer()
"#,
    )
    .file(
        "tools/mod.tvx",
        r#"
fn answer() -> i32:
    42
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "42");
}

#[test]
fn dependencies_are_imported_by_package_name() {
    let p = Project::new("deps");
    p.file(
        "app/main.tvx",
        r#"
import mathx.dot
import mathx.vec.scale

fn main() -> i32:
    dot(2, 3) + scale(4)
"#,
    )
    .file(
        "mathx/src/lib.tvx",
        r#"
fn dot(a: i32, b: i32) -> i32:
    a * b
"#,
    )
    .file(
        "mathx/src/vec.tvx",
        r#"
import crate.dot

fn scale(x: i32) -> i32:
    dot(x, 10)
"#,
    );
    let deps = [dep("mathx", &p, "mathx/src")];
    assert_eq!(ok(&load(&p.path("app/main.tvx"), &deps).unwrap()), "46");
}

#[test]
fn same_private_names_in_different_modules_do_not_collide() {
    let p = Project::new("collide");
    p.file(
        "main.tvx",
        r#"
import a.value
import b.other

fn helper() -> i32:
    1

fn main() -> i32:
    helper() + value() + other()
"#,
    )
    .file(
        "a.tvx",
        r#"
fn helper() -> i32:
    100

fn value() -> i32:
    helper()
"#,
    )
    .file(
        "b.tvx",
        r#"
fn helper() -> i32:
    10000

fn other() -> i32:
    helper()
"#,
    );
    assert_eq!(
        ok(&load(&p.path("main.tvx"), &[]).unwrap()),
        "10101",
        "each module keeps its own `helper`"
    );
}

#[test]
fn locals_shadow_imports() {
    let p = Project::new("shadow");
    p.file(
        "main.tvx",
        r#"
import util.double

fn main() -> i32:
    let double = 5
    double + 1
"#,
    )
    .file(
        "util.tvx",
        r#"
fn double(x: i32) -> i32:
    x * 2
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "6");
}

#[test]
fn std_imports_survive_merging() {
    let p = Project::new("std");
    p.file(
        "main.tvx",
        r#"
import util.shout

fn main() -> Text:
    shout("hi")
"#,
    )
    .file(
        "util.tvx",
        r#"
fn shout(s: Text) -> Text:
    std.text.uppercase(s)
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "HI");
}

#[test]
fn import_cycles_between_modules_are_allowed() {
    let p = Project::new("cycle");
    p.file(
        "main.tvx",
        r#"
import even.is_even

fn main() -> Bool:
    is_even(10)
"#,
    )
    .file(
        "even.tvx",
        r#"
import odd.is_odd

fn is_even(n: i32) -> Bool:
    if n == 0:
        True
    else:
        is_odd(n - 1)
"#,
    )
    .file(
        "odd.tvx",
        r#"
import even.is_even

fn is_odd(n: i32) -> Bool:
    if n == 0:
        False
    else:
        is_even(n - 1)
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "True");
}

fn messages(p: &Project, deps: &[Package]) -> Vec<String> {
    run(&load(&p.path("main.tvx"), deps).unwrap()).1
}

#[test]
fn import_problems_are_diagnosed() {
    let p = Project::new("errors");
    p.file(
        "main.tvx",
        r#"
import nope.f

fn main():
    pass
"#,
    );
    let m = messages(&p, &[]);
    assert!(
        m.iter()
            .any(|m| m.contains("cannot find the module `nope`")),
        "{m:?}"
    );

    p.file(
        "main.tvx",
        r#"
import util._hidden

fn main():
    pass
"#,
    )
    .file(
        "util.tvx",
        r#"
fn _hidden():
    pass

fn shown():
    pass
"#,
    );
    assert!(
        messages(&p, &[])
            .iter()
            .any(|m| m.contains("`_hidden` is private"))
    );

    p.file(
        "main.tvx",
        r#"
import util.missing

fn main():
    pass
"#,
    );
    assert!(
        messages(&p, &[])
            .iter()
            .any(|m| m.contains("has no function, type or value `missing`"))
    );

    p.file(
        "main.tvx",
        r#"
import crate

fn main():
    pass
"#,
    );
    assert!(
        messages(&p, &[])
            .iter()
            .any(|m| m.contains("must name an item"))
    );
}

#[test]
fn diagnostics_point_into_the_right_file_with_original_names() {
    let p = Project::new("render");
    p.file(
        "main.tvx",
        r#"
import util.broken

fn main() -> i32:
    broken()
"#,
    )
    .file(
        "util.tvx",
        r#"
fn broken() -> i32:
    "not a number"
"#,
    );
    let loaded = load(&p.path("main.tvx"), &[]).unwrap();
    assert!(loaded.diagnostics.is_empty());
    let (_, diags) = telvax_hir::lower(&loaded.program);
    let text = loaded.map.render(&diags, false);
    assert!(text.contains("util.tvx:3:5"), "{text}");
    assert!(text.contains("expected `i32`, found `Text`"), "{text}");

    // Errors that mention a renamed item show the name the user wrote.
    p.file(
        "main.tvx",
        r#"
import util.broken

fn main() -> Text:
    broken()
"#,
    )
    .file(
        "util.tvx",
        r#"
fn broken() -> i32:
    1
"#,
    );
    let loaded = load(&p.path("main.tvx"), &[]).unwrap();
    let (_, diags) = telvax_hir::lower(&loaded.program);
    assert!(!diags.is_empty());
    let shown = loaded.map.render(&diags, false);
    assert!(shown.contains("main.tvx:5:"), "{shown}");
}

#[test]
fn syntax_errors_in_a_module_are_reported_with_that_file() {
    let p = Project::new("syntax");
    p.file(
        "main.tvx",
        r#"
import util.f

fn main() -> i32:
    f()
"#,
    )
    .file("util.tvx", "pub fn f() -> i32 { let x = ; 1 }\n");
    let loaded = load(&p.path("main.tvx"), &[]).unwrap();
    assert!(!loaded.diagnostics.is_empty());
    assert!(
        loaded
            .map
            .render(&loaded.diagnostics, false)
            .contains("util.tvx:1")
    );
}

#[test]
fn load_text_uses_the_unsaved_buffer() {
    let p = Project::new("buffer");
    p.file(
        "util.tvx",
        r#"
fn one() -> i32:
    1
"#,
    );
    let loaded = telvax_modules::load_text(
        "<buffer>",
        r#"
import util.one

fn main() -> i32:
    one() + 1
"#,
        Some(Path::new(&p.0)),
        &[],
    );
    assert_eq!(ok(&loaded), "2");
}

#[test]
fn bundled_sources_rebuild_the_same_program_without_the_file_system() {
    use telvax_modules::{MemFiles, load_in};
    let p = Project::new("bundle");
    p.file(
        "app/main.tvx",
        r#"
import mathx.dot
import helper.twice

fn main() -> i32:
    twice(dot(2, 3))
"#,
    )
    .file(
        "app/helper.tvx",
        r#"
fn twice(x: i32) -> i32:
    x * 2
"#,
    )
    .file(
        "mathx/src/lib.tvx",
        r#"
fn dot(a: i32, b: i32) -> i32:
    a * b
"#,
    );
    let deps = [dep("mathx", &p, "mathx/src")];
    let entry = p.path("app/main.tvx");
    let first = load(&entry, &deps).unwrap();
    assert_eq!(ok(&first), "12");

    let mem = MemFiles(
        first
            .sources()
            .into_iter()
            .map(|(n, t)| (PathBuf::from(n), t))
            .collect(),
    );
    let text = std::fs::read_to_string(&entry).unwrap();
    drop(p); // the files are gone from disk
    let again = load_in(
        &mem,
        &entry.display().to_string(),
        &text,
        entry.parent(),
        &deps,
    );
    assert!(again.diagnostics.is_empty(), "{:?}", again.diagnostics);
    assert_eq!(ok(&again), "12");
}

#[test]
fn enums_cross_modules_and_do_not_collide() {
    let p = Project::new("enums");
    p.file(
        "main.tvx",
        r#"
from crate.shapes import Shape, area

enum Shape2:
    Dot

fn main() -> i32:
    let s = Shape.Square(3)
    var n = 0
    match s:
        case Shape.Square(k):
            n = k
        case Shape.Dot:
            n = 0
    area(Shape.Square(4)) + n
"#,
    )
    .file(
        "shapes.tvx",
        r#"
enum Shape:
    Square(i32)
    Dot

fn area(s: Shape) -> i32:
    match s:
        case Shape.Square(k):
            k * k
        case Shape.Dot:
            0
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "19");
}

#[test]
fn impl_blocks_travel_with_their_type() {
    let p = Project::new("impls");
    p.file(
        "main.tvx",
        r#"
import crate.shapes.Rect

type Rect2:
    w: i32

    fn area(self) -> i32:
        self.w * self.w

fn area() -> i32:
    1

fn main() -> i32:
    Rect.new(3, 4).area() + Rect2(w: 2).area() + area()
"#,
    )
    .file(
        "shapes.tvx",
        r#"
type Rect:
    w: i32
    h: i32

fn area() -> i32:
    100

extend Rect:
    fn new(w: i32, h: i32) -> Rect:
        Rect(w: w, h: h)

    fn area(self) -> i32:
        self.w * self.h
"#,
    );
    // `area` the method, `area` the function of each module, and the one in main do not clash.
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "17");
}

#[test]
fn imports_can_be_renamed_with_as() {
    let p = Project::new("alias");
    p.file(
        "main.tvx",
        r#"
from util import double as twice, Pt as Point
import util.shout as loud
from std.text import trim as strip

fn main() -> Text:
    let p = Point(x: 4)
    loud(strip(f"{twice(p.x)} "))
"#,
    )
    .file(
        "util.tvx",
        r#"
type Pt:
    x: Int

fn double(x: Int) -> Int:
    x * 2

fn shout(s: Text) -> Text:
    std.text.uppercase(s)
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "8");
}

#[test]
fn import_errors_say_what_to_write() {
    let p = Project::new("errors");
    p.file(
        "main.tvx",
        "import util\nfrom util import nothing\nfrom missing import x\n\nfn main() -> i32:\n    0\n",
    )
    .file("util.tvx", "fn double(x: Int) -> Int:\n    x * 2\n");
    let loaded = load(&p.path("main.tvx"), &[]).unwrap();
    let messages: Vec<&str> = loaded
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages.contains(&"`util` is a module, not a function or type"),
        "{messages:#?}"
    );
    assert!(
        messages.contains(&"module `util` has no function, type or value `nothing`"),
        "{messages:#?}"
    );
    assert!(
        messages.contains(&"cannot find the module `missing`"),
        "{messages:#?}"
    );
    let note = loaded.diagnostics[0].notes.join(" ");
    assert!(note.contains("from util import name"), "{note}");
}

#[test]
fn embedded_paths_are_the_literals_given_to_fs_embed() {
    let p = Project::new("embed");
    p.file(
        "main.tvx",
        r#"
import util.words

let a = std.fs.embed("data/a.txt")
let b = std.fs.embed('data/b.txt')
let again = std.fs.embed("data/a.txt")
# std.fs.embed(name) in a comment is not a call
let other = words.embed(1)

fn main() -> i32:
    0
"#,
    )
    .file(
        "util.tvx",
        r#"
pub fn words() -> i32:
    1
"#,
    );
    let loaded = load(&p.path("main.tvx"), &[]).unwrap();
    assert_eq!(
        loaded.embedded_paths().unwrap(),
        vec!["data/a.txt".to_string(), "data/b.txt".to_string()]
    );
}

#[test]
fn a_top_level_let_is_imported_and_shared() {
    let p = Project::new("lets");
    p.file(
        "main.tvx",
        r#"
from util import LIMIT, names, record

fn main() -> Text:
    record("b")
    f"{LIMIT} {names.len()} {names.get(1).unwrap_or("-")}"
"#,
    )
    .file(
        "util.tvx",
        r#"
let LIMIT = 100
let names: List[Text] = ["a"]
let _secret = 7

fn record(name: Text):
    names.push(name)
"#,
    );
    assert_eq!(ok(&load(&p.path("main.tvx"), &[]).unwrap()), "100 2 b");

    // A let that starts with `_` stays private.
    p.file(
        "main.tvx",
        "from util import _secret\n\nfn main() -> i32:\n    0\n",
    );
    let loaded = load(&p.path("main.tvx"), &[]).unwrap();
    assert!(
        loaded
            .diagnostics
            .iter()
            .any(|d| d.message == "`_secret` is private"),
        "{:?}",
        loaded.diagnostics
    );
}
