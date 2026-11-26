//! Dependency resolution.
//!
//! Every package name resolves to exactly one version. Path dependencies are fixed; registry
//! dependencies are chosen newest-first and the search backs up when a later requirement cannot
//! be met, so `a = "^1"` needing `c = "<2"` and `b = "^1"` needing `c = "^1.5"` settles on the
//! newest `c` that satisfies both.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};

use crate::lock::{Lock, Locked};
use crate::manifest::Manifest;
use crate::registry::{IndexEntry, Registry};

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Registry,
    Path(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub name: String,
    pub version: Version,
    pub source: Source,
    pub checksum: Option<String>,
    /// Names of the packages this one depends on.
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Resolution {
    pub packages: Vec<Resolved>,
}

impl Resolution {
    pub fn to_lock(&self, root: &Manifest) -> Lock {
        let mut packages = vec![Locked {
            name: root.package.name.clone(),
            version: root.package.version.clone(),
            source: None,
            checksum: None,
            dependencies: root.dependencies.keys().cloned().collect(),
        }];
        for p in &self.packages {
            packages.push(Locked {
                name: p.name.clone(),
                version: p.version.clone(),
                source: Some(match &p.source {
                    Source::Registry => "registry".to_string(),
                    Source::Path(dir) => format!("path+{}", dir.display()),
                }),
                checksum: p.checksum.clone(),
                dependencies: p.deps.clone(),
            });
        }
        Lock { packages }
    }
}

#[derive(Clone)]
struct Req {
    name: String,
    req: VersionReq,
    by: String,
}

struct Fixed {
    version: Version,
    dir: PathBuf,
    deps: Vec<String>,
}

struct Solver<'a> {
    registry: Option<&'a dyn Registry>,
    /// Versions to keep when they still satisfy every requirement (the locked ones, in `update`).
    prefer: &'a HashMap<String, Version>,
    fixed: HashMap<String, Fixed>,
    cache: RefCell<HashMap<String, Vec<IndexEntry>>>,
}

#[derive(Clone)]
struct Choice {
    entry: IndexEntry,
    by: String,
}

impl Solver<'_> {
    fn versions(&self, name: &str) -> Result<Vec<IndexEntry>, String> {
        if let Some(v) = self.cache.borrow().get(name) {
            return Ok(v.clone());
        }
        let reg = self.registry.ok_or_else(|| format!("`{name}` is a registry dependency but no registry is configured (use --registry or TVPM_REGISTRY)"))?;
        let v = reg.versions(name)?;
        self.cache.borrow_mut().insert(name.to_string(), v.clone());
        Ok(v)
    }

    fn go(
        &self,
        mut pending: Vec<Req>,
        selected: BTreeMap<String, Choice>,
    ) -> Result<BTreeMap<String, Choice>, String> {
        let Some(next) = pending.pop() else {
            return Ok(selected);
        };

        if let Some(fixed) = self.fixed.get(&next.name) {
            if !next.req.matches(&fixed.version) {
                return Err(format!(
                    "`{}` requires `{}` {}, but the path dependency `{}` is version {}",
                    next.by, next.name, next.req, next.name, fixed.version
                ));
            }
            return self.go(pending, selected);
        }
        if let Some(chosen) = selected.get(&next.name) {
            if next.req.matches(&chosen.entry.vers) {
                return self.go(pending, selected);
            }
            return Err(format!(
                "`{}` requires `{}` {}, but {} was chosen to satisfy `{}`",
                next.by, next.name, next.req, chosen.entry.vers, chosen.by
            ));
        }

        let all = self.versions(&next.name)?;
        if all.is_empty() {
            return Err(format!(
                "package `{}` (required by `{}`) is not in the registry",
                next.name, next.by
            ));
        }
        let mut candidates: Vec<IndexEntry> = all
            .iter()
            .filter(|e| e.usable() && next.req.matches(&e.vers))
            .cloned()
            .collect();
        // Newest first, except that a preferred version goes in front of the rest.
        let keep = self.prefer.get(&next.name);
        candidates.sort_by(|a, b| {
            (keep != Some(&a.vers))
                .cmp(&(keep != Some(&b.vers)))
                .then(b.vers.cmp(&a.vers))
        });
        if candidates.is_empty() {
            let mut available: Vec<String> = all
                .iter()
                .filter(|e| e.usable())
                .map(|e| e.vers.to_string())
                .collect();
            available.sort();
            return Err(format!(
                "no version of `{}` matches {} (required by `{}`); available: {}",
                next.name,
                next.req,
                next.by,
                if available.is_empty() {
                    "none (all yanked)".to_string()
                } else {
                    available.join(", ")
                }
            ));
        }
        let mut last = String::new();
        for cand in candidates {
            let mut sel = selected.clone();
            sel.insert(
                next.name.clone(),
                Choice {
                    entry: cand.clone(),
                    by: next.by.clone(),
                },
            );
            let mut pend = pending.clone();
            let by = format!("{} {}", cand.name, cand.vers);
            pend.extend(cand.deps.iter().map(|d| Req {
                name: d.name.clone(),
                req: d.req.clone(),
                by: by.clone(),
            }));
            match self.go(pend, sel) {
                Ok(done) => return Ok(done),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

/// Loads the path dependencies of `manifest` (located in `dir`), transitively.
fn collect_fixed(
    dir: &Path,
    manifest: &Manifest,
    fixed: &mut HashMap<String, Fixed>,
    regs: &mut Vec<Req>,
) -> Result<(), String> {
    for (name, dep) in &manifest.dependencies {
        let by = manifest.package.name.clone();
        let Some(rel) = dep.path() else {
            regs.push(Req {
                name: name.clone(),
                req: dep.requirement(),
                by,
            });
            continue;
        };
        let dep_dir = dir.join(rel);
        let dep_dir = std::fs::canonicalize(&dep_dir).map_err(|e| {
            format!(
                "path dependency `{name}`: cannot open {}: {e}",
                dep_dir.display()
            )
        })?;
        let m =
            Manifest::read(&dep_dir).map_err(|e| format!("path dependency `{name}`: {}", e.0))?;
        if m.package.name != *name {
            return Err(format!(
                "path dependency `{name}` points to a package named `{}`",
                m.package.name
            ));
        }
        if let Some(existing) = fixed.get(name) {
            if existing.dir != dep_dir {
                return Err(format!(
                    "`{name}` is a path dependency in two different places: {} and {}",
                    existing.dir.display(),
                    dep_dir.display()
                ));
            }
            continue;
        }
        fixed.insert(
            name.clone(),
            Fixed {
                version: m.package.version.clone(),
                dir: dep_dir.clone(),
                deps: m.dependencies.keys().cloned().collect(),
            },
        );
        collect_fixed(&dep_dir, &m, fixed, regs)?;
    }
    Ok(())
}

pub fn resolve(
    root_dir: &Path,
    root: &Manifest,
    registry: Option<&dyn Registry>,
) -> Result<Resolution, String> {
    resolve_preferring(root_dir, root, registry, &HashMap::new())
}

/// Like [`resolve`], but a package in `prefer` stays at that version unless a requirement rules it
/// out. With nothing preferred every package gets the newest version that fits.
pub fn resolve_preferring(
    root_dir: &Path,
    root: &Manifest,
    registry: Option<&dyn Registry>,
    prefer: &HashMap<String, Version>,
) -> Result<Resolution, String> {
    let mut fixed = HashMap::new();
    let mut reqs = Vec::new();
    collect_fixed(root_dir, root, &mut fixed, &mut reqs)?;
    // Requirements stated against path packages are checked like any other.
    for (name, dep) in &root.dependencies {
        if let Some(f) = fixed.get(name) {
            let req = dep.requirement();
            if !req.matches(&f.version) {
                return Err(format!(
                    "`{name}` is version {} but {} is required",
                    f.version, req
                ));
            }
        }
    }
    let solver = Solver {
        registry,
        prefer,
        fixed,
        cache: RefCell::new(HashMap::new()),
    };
    // Requirements are popped from the end, so reverse to resolve in declaration order.
    reqs.reverse();
    let chosen = solver.go(reqs, BTreeMap::new())?;

    let mut packages: Vec<Resolved> = chosen
        .into_values()
        .map(|c| Resolved {
            name: c.entry.name.clone(),
            version: c.entry.vers.clone(),
            source: Source::Registry,
            checksum: Some(c.entry.cksum.clone()),
            deps: c.entry.deps.iter().map(|d| d.name.clone()).collect(),
        })
        .collect();
    for (name, f) in &solver.fixed {
        packages.push(Resolved {
            name: name.clone(),
            version: f.version.clone(),
            source: Source::Path(f.dir.clone()),
            checksum: None,
            deps: f.deps.clone(),
        });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Resolution { packages })
}
