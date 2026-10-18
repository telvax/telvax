//! Multi-file Telvax programs.
//!
//! A *package* is a directory of `.tvx` files. Its root module is the entry file (or `lib.tvx` for
//! a dependency); `a/b.tvx` or `a/b/mod.tvx` is the module `a::b`. Other modules are used with
//! `import` or `from ... import`:
//!
//! ```text
//! from util import clamp              # ./util.tvx, relative to the package root
//! from geometry import Point, norm as length
//! from mathx.vec import dot           # package `mathx`, file vec.tvx
//! ```
//!
//! Every item can be imported except those whose name starts with `_`. Loading merges every module into one
//! [`telvax_ast::Program`], renaming items from other modules to unique names (`mathx__vec__dot`)
//! and rewriting every reference, so the type checker and backends never see modules. All files
//! share one offset space (see [`SourceMap`]) so diagnostics keep pointing at the right file.

mod rewrite;
mod source_map;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use telvax_ast::{Import, Item, Program};
use telvax_diagnostics::Diagnostic;

pub use source_map::{SourceFile, SourceMap};

/// A whole program as plain data: every source file that went into it, so it can be rebuilt
/// anywhere (inside a binary, on a worker machine) without the original source tree.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Image {
    /// Path of the entry file, as it was loaded.
    pub entry: String,
    /// `(path, text)` of every module, entry first.
    pub files: Vec<(String, String)>,
    /// `(package name, source directory)` of every dependency.
    pub deps: Vec<(String, String)>,
    /// `(path, text)` of every file named by `std.fs.embed`, the path as written in the program.
    #[serde(default)]
    pub data: Vec<(String, String)>,
}

impl Image {
    pub fn new(entry: &str, loaded: &Loaded, deps: &[Package]) -> Image {
        Image {
            entry: entry.to_string(),
            files: loaded.sources(),
            deps: deps
                .iter()
                .map(|d| (d.name.clone(), d.src_dir.display().to_string()))
                .collect(),
            data: Vec::new(),
        }
    }

    pub fn packages(&self) -> Vec<Package> {
        self.deps
            .iter()
            .map(|(name, dir)| Package {
                name: name.clone(),
                src_dir: PathBuf::from(dir),
            })
            .collect()
    }

    /// Loads the program from the stored files alone.
    pub fn load(&self) -> Loaded {
        let files = MemFiles(
            self.files
                .iter()
                .map(|(p, t)| (PathBuf::from(p), t.clone()))
                .collect(),
        );
        let entry_text = self.files.first().map_or("", |(_, t)| t.as_str());
        load_in(
            &files,
            &self.entry,
            entry_text,
            Path::new(&self.entry).parent(),
            &self.packages(),
        )
    }
}

impl Loaded {
    /// The paths of every `std.fs.embed("...")` in the program, in order of appearance and without
    /// repeats. An argument that is not a plain text literal is an error: the file is read when the
    /// program is built, so its name has to be known then.
    pub fn embedded_paths(&self) -> Result<Vec<String>, String> {
        use telvax_lexer::Token;
        let mut paths: Vec<String> = Vec::new();
        for file in &self.map.files {
            let (tokens, _) = telvax_lexer::tokenize(&file.text);
            for (i, (tok, _)) in tokens.iter().enumerate() {
                if *tok != Token::Ident("embed")
                    || !matches!(tokens.get(i + 1), Some((Token::LParen, _)))
                {
                    continue;
                }
                let after_dot = i > 0 && tokens[i - 1].0 == Token::Dot;
                let after_fs = i > 1 && tokens[i - 2].0 == Token::Ident("fs");
                if after_dot && !after_fs {
                    continue;
                }
                match (tokens.get(i + 2), tokens.get(i + 3)) {
                    (Some((Token::Str(lit), _)), Some((Token::RParen, _)))
                        if !lit.contains('\\') =>
                    {
                        let path = lit[1..lit.len() - 1].to_string();
                        if !paths.contains(&path) {
                            paths.push(path);
                        }
                    }
                    _ => {
                        return Err(format!(
                            "{}: `std.fs.embed` takes a text literal, such as `std.fs.embed(\"data/words.txt\")`, \
                             because the file is included when the program is built",
                            file.name
                        ));
                    }
                }
            }
        }
        Ok(paths)
    }

    /// Every file that was read, as `(path, text)`, entry first. Enough to rebuild the program
    /// later with [`MemFiles`].
    pub fn sources(&self) -> Vec<(String, String)> {
        self.map
            .files
            .iter()
            .map(|f| (f.name.clone(), f.text.clone()))
            .collect()
    }
}

/// Where module files come from. The real file system normally; an in-memory table when a
/// program has been bundled into a binary.
pub trait Files {
    fn read(&self, path: &Path) -> std::io::Result<String>;
    fn is_file(&self, path: &Path) -> bool;
}

/// The operating system's files.
#[derive(Debug, Clone, Copy)]
pub struct OsFiles;

impl Files for OsFiles {
    fn read(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// Files held in memory, keyed by the same paths the sources were loaded from.
#[derive(Debug, Clone, Default)]
pub struct MemFiles(pub HashMap<PathBuf, String>);

impl Files for MemFiles {
    fn read(&self, path: &Path) -> std::io::Result<String> {
        self.0.get(path).cloned().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} is not bundled", path.display()),
            )
        })
    }

    fn is_file(&self, path: &Path) -> bool {
        self.0.contains_key(path)
    }
}

/// A dependency: the name used in `import name::...` and the directory with its modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub src_dir: PathBuf,
}

#[derive(Debug)]
pub struct Loaded {
    /// Every module merged into one program (dependencies first, the entry module last).
    pub program: Program,
    pub map: SourceMap,
    /// Parse errors and module / import problems. Type errors come from checking `program`.
    pub diagnostics: Vec<Diagnostic>,
    /// False when the entry file could not be parsed at all (not even with error recovery).
    pub entry_parsed: bool,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct ModKey {
    /// 0 is the package being built, `n` is `deps[n - 1]`.
    pkg: usize,
    path: Vec<String>,
}

struct Module {
    key: ModKey,
    program: Program,
    parsed: bool,
    /// `(local name, module index, item name, import span)`
    imports: Vec<(String, usize, String)>,
}

struct Loader<'a> {
    files: &'a dyn Files,
    deps: &'a [Package],
    root_dir: Option<PathBuf>,
    map: SourceMap,
    modules: Vec<Module>,
    index: HashMap<ModKey, usize>,
    order: Vec<usize>,
    diags: Vec<Diagnostic>,
}

/// Loads the program whose entry file is `entry`.
pub fn load(entry: &Path, deps: &[Package]) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(entry)
        .map_err(|e| format!("cannot read `{}`: {e}", entry.display()))?;
    let root = entry
        .parent()
        .map(Path::to_path_buf)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(load_text(
        &entry.display().to_string(),
        &text,
        Some(&root),
        deps,
    ))
}

/// Like [`load`], with the entry file's text supplied by the caller (an unsaved editor buffer).
/// `root_dir` is where sibling modules are looked up.
pub fn load_text(
    entry_name: &str,
    text: &str,
    root_dir: Option<&Path>,
    deps: &[Package],
) -> Loaded {
    load_in(&OsFiles, entry_name, text, root_dir, deps)
}

/// Like [`load_text`], reading modules through `files`.
pub fn load_in(
    files: &dyn Files,
    entry_name: &str,
    text: &str,
    root_dir: Option<&Path>,
    deps: &[Package],
) -> Loaded {
    let mut l = Loader {
        files,
        deps,
        root_dir: root_dir.map(Path::to_path_buf),
        map: SourceMap::default(),
        modules: Vec::new(),
        index: HashMap::new(),
        order: Vec::new(),
        diags: Vec::new(),
    };
    let entry = l.add_module(
        ModKey {
            pkg: 0,
            path: vec![],
        },
        entry_name,
        text,
    );
    l.process(entry);
    l.order.push(entry);
    l.link()
}

impl Loader<'_> {
    fn package_name(&self, pkg: usize) -> String {
        if pkg == 0 {
            "crate".into()
        } else {
            self.deps[pkg - 1].name.clone()
        }
    }

    fn add_module(&mut self, key: ModKey, name: &str, text: &str) -> usize {
        let base = self.map.add(name, text);
        let (program, errors) = telvax_parser::parse_at(text, base);
        self.diags.extend(errors);
        let idx = self.modules.len();
        self.modules.push(Module {
            key: key.clone(),
            parsed: program.is_some(),
            program: program.unwrap_or_default(),
            imports: Vec::new(),
        });
        self.index.insert(key, idx);
        idx
    }

    fn module_file(&self, key: &ModKey) -> Option<PathBuf> {
        let dir = if key.pkg == 0 {
            self.root_dir.clone()?
        } else {
            self.deps[key.pkg - 1].src_dir.clone()
        };
        if key.path.is_empty() {
            return (key.pkg != 0).then(|| dir.join("lib.tvx"));
        }
        let mut base = dir;
        for seg in &key.path {
            base.push(seg);
        }
        let file = base.with_extension("tvx");
        if self.files.is_file(&file) {
            return Some(file);
        }
        let nested = base.join("mod.tvx");
        self.files.is_file(&nested).then_some(nested)
    }

    /// Loads (once) the module `key`; `None` when no such file exists.
    fn load_module(&mut self, key: ModKey) -> Option<usize> {
        if let Some(&idx) = self.index.get(&key) {
            return Some(idx);
        }
        let file = self.module_file(&key)?;
        let text = self.files.read(&file).ok()?;
        let idx = self.add_module(key, &file.display().to_string(), &text);
        self.process(idx);
        self.order.push(idx);
        Some(idx)
    }

    fn exports(&self, idx: usize) -> HashMap<String, bool> {
        let mut out = HashMap::new();
        for item in &self.modules[idx].program.items {
            match item {
                Item::Fn(f) => {
                    out.insert(f.name.node.clone(), f.is_pub);
                }
                Item::Struct(s) => {
                    out.insert(s.name.node.clone(), s.is_pub);
                }
                Item::Enum(e) => {
                    out.insert(e.name.node.clone(), e.is_pub);
                }
                Item::Alias(a) => {
                    out.insert(a.name.node.clone(), a.is_pub);
                }
                Item::Trait(t) => {
                    out.insert(t.name.node.clone(), t.is_pub);
                }
                Item::Let(l) => {
                    out.insert(l.name.node.clone(), !l.name.node.starts_with('_'));
                }
                _ => {}
            }
        }
        out
    }

    /// How a program writes the module: `util`, `mathx.vec` (the project's own modules have no
    /// prefix).
    fn describe(&self, key: &ModKey) -> String {
        let own = key.pkg == 0;
        (!own)
            .then(|| self.package_name(key.pkg))
            .into_iter()
            .chain(key.path.iter().cloned())
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Resolves the `import`s of module `idx`, loading what they refer to.
    fn process(&mut self, idx: usize) {
        let imports: Vec<Import> = self.modules[idx]
            .program
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Import(imp) if imp.path.first().is_some_and(|s| s.node != "std") => {
                    Some(imp.clone())
                }
                _ => None,
            })
            .collect();
        let current_pkg = self.modules[idx].key.pkg;
        for imp in imports {
            let segs: Vec<String> = imp.path.iter().map(|s| s.node.clone()).collect();
            let (pkg, rest) = match segs[0].as_str() {
                "crate" => (current_pkg, &segs[1..]),
                name => match self.deps.iter().position(|d| d.name == name) {
                    Some(i) => (i + 1, &segs[1..]),
                    None => (current_pkg, &segs[..]),
                },
            };
            let items: Vec<String> = if imp.group.is_empty() {
                vec![rest.last().cloned().unwrap_or_default()]
            } else {
                imp.group.iter().map(|g| g.node.clone()).collect()
            };
            let module_path: Vec<String> = if imp.group.is_empty() {
                rest[..rest.len().saturating_sub(1)].to_vec()
            } else {
                rest.to_vec()
            };
            if rest.is_empty() && imp.group.is_empty() {
                self.diags.push(Diagnostic::error(
                    imp.span.clone(),
                    "an import must name an item, e.g. `from util import helper`",
                ));
                continue;
            }
            if imp.group.is_empty() {
                let whole = ModKey {
                    pkg,
                    path: rest.to_vec(),
                };
                if self.module_file(&whole).is_some() {
                    let name = self.describe(&whole);
                    self.diags.push(
                        Diagnostic::error(
                            imp.span.clone(),
                            format!("`{name}` is a module, not a function or type"),
                        )
                        .with_note(format!(
                            "import the names you use from it: `from {name} import name`"
                        )),
                    );
                    continue;
                }
            }
            let key = ModKey {
                pkg,
                path: module_path,
            };
            let Some(target) = self.load_module(key.clone()) else {
                self.diags.push(
                    Diagnostic::error(
                        imp.span.clone(),
                        format!("cannot find the module `{}`", self.describe(&key)),
                    )
                    .with_note(match self.module_file(&key) {
                        Some(f) => format!("expected the file {}", f.display()),
                        None => "no such file; module `a.b` lives in `a/b.tvx` or `a/b/mod.tvx`"
                            .to_string(),
                    }),
                );
                continue;
            };
            let exports = self.exports(target);
            for (i, (name, ident)) in items
                .iter()
                .zip(if imp.group.is_empty() {
                    vec![imp.path.last().unwrap().clone()]
                } else {
                    imp.group.clone()
                })
                .enumerate()
            {
                match exports.get(name) {
                    Some(true) => {
                        let local = imp.local_name(i, &ident).node;
                        self.modules[idx]
                            .imports
                            .push((local, target, name.clone()))
                    }
                    Some(false) => self.diags.push(
                        Diagnostic::error(ident.span.clone(), format!("`{name}` is private"))
                            .with_note(format!(
                                "names that start with `_` are private to `{}`",
                                self.describe(&key)
                            )),
                    ),
                    None => self.diags.push(Diagnostic::error(
                        ident.span.clone(),
                        format!(
                            "module `{}` has no function or type `{name}`",
                            self.describe(&key)
                        ),
                    )),
                }
            }
        }
    }

    /// The program-wide name of `item` defined in module `idx`.
    fn mangled(&self, idx: usize, item: &str) -> String {
        let key = &self.modules[idx].key;
        if idx == 0 {
            return item.to_string();
        }
        let mut parts = vec![self.package_name(key.pkg)];
        parts.extend(key.path.iter().cloned());
        parts.push(item.to_string());
        parts.join("__")
    }

    /// The name as the user wrote it: `util.double`, `mathx.vec.dot`.
    fn display(&self, idx: usize, item: &str) -> String {
        let key = &self.modules[idx].key;
        let mut parts = Vec::new();
        if key.pkg != 0 {
            parts.push(self.package_name(key.pkg));
        }
        parts.extend(key.path.iter().cloned());
        parts.push(item.to_string());
        parts.join(".")
    }

    fn link(mut self) -> Loaded {
        let entry_parsed = self.modules[0].parsed;
        let order = std::mem::take(&mut self.order);
        let mut merged = Program::default();
        let mut std_imports: Vec<Item> = Vec::new();
        let mut seen_std: HashMap<String, String> = HashMap::new();

        for &idx in &order {
            // Names visible at the top level of this module.
            let mut names: HashMap<String, String> = HashMap::new();
            let mut declared: HashSet<String> = HashSet::new();
            for item in &self.modules[idx].program.items {
                let n = match item {
                    Item::Fn(f) => &f.name,
                    Item::Struct(s) => &s.name,
                    Item::Enum(e) => &e.name,
                    Item::Alias(a) => &a.name,
                    Item::Trait(t) => &t.name,
                    Item::Impl(_) => continue,
                    Item::Let(l) => &l.name,
                    Item::Import(_) => continue,
                };
                declared.insert(n.node.clone());
                let mangled = self.mangled(idx, &n.node);
                if mangled != n.node {
                    self.map
                        .display_names
                        .insert(mangled.clone(), self.display(idx, &n.node));
                }
                names.insert(n.node.clone(), mangled);
            }
            let imports = self.modules[idx].imports.clone();
            for (local, target, item) in imports {
                if declared.contains(&local) {
                    self.diags.push(Diagnostic::error(
                        0..0,
                        format!(
                            "`{local}` is both defined and imported in `{}`",
                            self.display(idx, "")
                        ),
                    ));
                }
                let mangled = self.mangled(target, &item);
                if mangled != item {
                    self.map
                        .display_names
                        .insert(mangled.clone(), self.display(target, &item));
                }
                names.insert(local, mangled);
            }

            let items = std::mem::take(&mut self.modules[idx].program.items);
            let mut rewriter = rewrite::Rewriter::new(&names);
            for mut item in items {
                match &item {
                    Item::Import(imp) => {
                        if imp.path.first().is_some_and(|s| s.node == "std") {
                            let full: Vec<String> = imp
                                .path
                                .iter()
                                .chain(&imp.group)
                                .map(|s| s.node.clone())
                                .collect();
                            let key = format!(
                                "{}|{}",
                                imp.path
                                    .iter()
                                    .map(|s| s.node.as_str())
                                    .collect::<Vec<_>>()
                                    .join("::"),
                                imp.group
                                    .iter()
                                    .map(|s| s.node.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            );
                            if let Some(last) = full.last() {
                                if let Some(prev) = seen_std.get(last)
                                    && *prev != key
                                    && imp.group.is_empty()
                                {
                                    self.diags.push(Diagnostic::error(
                                        imp.span.clone(),
                                        format!(
                                            "`{last}` is already imported from a different std path"
                                        ),
                                    ));
                                }
                                seen_std.insert(last.clone(), key);
                            }
                            std_imports.push(item.clone());
                        }
                    }
                    _ => {
                        rewriter.item(&mut item);
                        merged.items.push(item);
                    }
                }
            }
        }

        let mut items = std_imports;
        items.extend(merged.items);
        Loaded {
            program: Program { items },
            map: self.map,
            diagnostics: self.diags,
            entry_parsed,
        }
    }
}
