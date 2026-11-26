//! Registries: where published packages live.
//!
//! The layout works as a plain directory and as a static website (no server logic needed):
//!
//! ```text
//! index/<name>.json              one JSON line per published version
//! packages/<name>-<version>.nvpkg   gzip-compressed tar: telvax.toml + src/**
//! names.json                     every package with its newest version, for `tvpm search` over HTTP
//! owners.json                    who may publish each name (the official registry, see `check`)
//! ```
//!
//! The official registry is `telvax/registry`, served by GitHub Pages at [`DEFAULT_REGISTRY`].

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::manifest::{Dependency, MANIFEST, Manifest, validate_name};

/// The registry `tvpm` uses when neither `--registry` nor `TVPM_REGISTRY` names one: `telvax/registry`, served as static
/// files. The address is permanent.
pub const DEFAULT_REGISTRY: &str = "https://telvax.github.io/registry";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDep {
    pub name: String,
    pub req: VersionReq,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub vers: Version,
    pub deps: Vec<IndexDep>,
    /// Hex SHA-256 of the package archive.
    pub cksum: String,
    #[serde(default)]
    pub yanked: bool,
    /// Why the files of this version were taken out of the registry (malware, a legal order). The line stays, so a lock
    /// file that names the version fails with the reason instead of a missing file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<String>,
    /// The `description` of the package, for `search`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl IndexEntry {
    /// Whether a new resolution may pick this version: not yanked and not removed.
    pub fn usable(&self) -> bool {
        !self.yanked && self.removed.is_none()
    }
}

/// A line of `names.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameEntry {
    pub name: String,
    /// The newest version that is not yanked or removed; `None` when there is none.
    pub version: Option<Version>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The names that match `query` (a part of the name or of the description), sorted by name.
pub fn matching(names: Vec<NameEntry>, query: &str) -> Vec<NameEntry> {
    let q = query.to_lowercase();
    let mut out: Vec<NameEntry> = names
        .into_iter()
        .filter(|n| {
            n.name.contains(&q)
                || n.description
                    .as_deref()
                    .is_some_and(|d| d.to_lowercase().contains(&q))
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The error for a version whose files were removed, if it was.
fn removed(entries: &[IndexEntry], name: &str, version: &Version) -> Result<(), String> {
    match entries
        .iter()
        .find(|e| &e.vers == version)
        .and_then(|e| e.removed.as_ref())
    {
        Some(why) => Err(format!(
            "`{name}` {version} was removed from the registry: {why}"
        )),
        None => Ok(()),
    }
}

pub trait Registry {
    /// Every published version of `name` (empty when the package is unknown).
    fn versions(&self, name: &str) -> Result<Vec<IndexEntry>, String>;
    /// The package archive of `name` at `version`.
    fn download(&self, name: &str, version: &Version) -> Result<Vec<u8>, String>;
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn parse_index(name: &str, text: &str) -> Result<Vec<IndexEntry>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l).map_err(|e| format!("corrupt registry index for `{name}`: {e}"))
        })
        .collect()
}

/// A registry in a local directory.
#[derive(Debug, Clone)]
pub struct DirRegistry {
    pub root: PathBuf,
}

impl DirRegistry {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn index_file(&self, name: &str) -> PathBuf {
        self.root.join("index").join(format!("{name}.json"))
    }

    fn package_file(&self, name: &str, version: &Version) -> PathBuf {
        self.root
            .join("packages")
            .join(format!("{name}-{version}.nvpkg"))
    }

    /// The names of the packages that have an index file, sorted.
    pub fn package_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.root.join("index"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension().is_some_and(|x| x == "json"))
                    .then(|| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
                    .flatten()
            })
            .collect();
        names.sort();
        names
    }

    /// `names.json` as it should be for the index: each package, its newest usable version and its description.
    pub fn names(&self) -> Result<Vec<NameEntry>, String> {
        self.package_names()
            .into_iter()
            .map(|name| {
                let entries = self.versions(&name)?;
                let newest = entries
                    .iter()
                    .filter(|e| e.usable())
                    .max_by(|a, b| a.vers.cmp(&b.vers));
                Ok(NameEntry {
                    description: newest.and_then(|e| e.description.clone()),
                    version: newest.map(|e| e.vers.clone()),
                    name,
                })
            })
            .collect()
    }

    /// Writes `names.json` from the index.
    pub fn write_names(&self) -> Result<(), String> {
        let text = names_json(&self.names()?)?;
        std::fs::write(self.root.join("names.json"), text).map_err(|e| e.to_string())
    }

    /// Names of the published packages that match `query`.
    pub fn search(&self, query: &str) -> Result<Vec<NameEntry>, String> {
        Ok(matching(self.names()?, query))
    }
}

/// The text of `names.json`: an array, one package per line, so a pull request shows one changed line.
pub fn names_json(names: &[NameEntry]) -> Result<String, String> {
    let lines: Vec<String> = names
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    Ok(if lines.is_empty() {
        "[]\n".to_string()
    } else {
        format!("[\n{}\n]\n", lines.join(",\n"))
    })
}

impl Registry for DirRegistry {
    fn versions(&self, name: &str) -> Result<Vec<IndexEntry>, String> {
        match std::fs::read_to_string(self.index_file(name)) {
            Ok(text) => parse_index(name, &text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(format!("cannot read the index for `{name}`: {e}")),
        }
    }

    fn download(&self, name: &str, version: &Version) -> Result<Vec<u8>, String> {
        removed(&self.versions(name)?, name, version)?;
        let file = self.package_file(name, version);
        std::fs::read(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))
    }
}

/// A registry served over HTTP(S) with the same layout (any static file host works).
#[derive(Debug, Clone)]
pub struct HttpRegistry {
    pub base: String,
}

impl HttpRegistry {
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let url = format!("{}/{path}", self.base.trim_end_matches('/'));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut resp = agent
            .get(&url)
            .call()
            .map_err(|e| format!("request to {url} failed: {e}"))?;
        match resp.status().as_u16() {
            200 => resp
                .body_mut()
                .read_to_vec()
                .map(Some)
                .map_err(|e| format!("cannot read {url}: {e}")),
            404 => Ok(None),
            code => Err(format!("{url} answered HTTP {code}")),
        }
    }
}

impl Registry for HttpRegistry {
    fn versions(&self, name: &str) -> Result<Vec<IndexEntry>, String> {
        match self.get(&format!("index/{name}.json"))? {
            Some(bytes) => parse_index(name, &String::from_utf8_lossy(&bytes)),
            None => Ok(Vec::new()),
        }
    }

    fn download(&self, name: &str, version: &Version) -> Result<Vec<u8>, String> {
        removed(&self.versions(name)?, name, version)?;
        self.get(&format!("packages/{name}-{version}.nvpkg"))?
            .ok_or_else(|| format!("package `{name}` {version} is not in the registry"))
    }
}

impl HttpRegistry {
    /// The packages of `names.json` that match `query`.
    pub fn search(&self, query: &str) -> Result<Vec<NameEntry>, String> {
        let bytes = self
            .get("names.json")?
            .ok_or_else(|| format!("{} has no names.json to search", self.base))?;
        let names: Vec<NameEntry> = serde_json::from_slice(&bytes)
            .map_err(|e| format!("the names.json of {} is not valid: {e}", self.base))?;
        Ok(matching(names, query))
    }
}

/// Searches a registry, a directory or a URL.
pub fn search(spec: &str, query: &str) -> Result<Vec<NameEntry>, String> {
    if spec.starts_with("http://") || spec.starts_with("https://") {
        HttpRegistry {
            base: spec.to_string(),
        }
        .search(query)
    } else {
        DirRegistry::new(spec).search(query)
    }
}

/// Opens a registry from a directory path or an `http(s)://` URL.
pub fn open(spec: &str) -> Box<dyn Registry> {
    if spec.starts_with("http://") || spec.starts_with("https://") {
        Box::new(HttpRegistry {
            base: spec.to_string(),
        })
    } else {
        Box::new(DirRegistry::new(spec))
    }
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(&path, root, out)?;
        } else if path.extension().is_some_and(|x| x == "tvx") {
            out.push(path.strip_prefix(root).expect("inside root").to_path_buf());
        }
    }
    Ok(())
}

/// Archives the project at `dir`: its manifest, `src/**/*.tvx` and README / LICENSE files.
pub fn pack(dir: &Path) -> Result<(Manifest, Vec<u8>), String> {
    let manifest = Manifest::read(dir).map_err(|e| e.0)?;
    if let Some((name, _)) = manifest
        .dependencies
        .iter()
        .find(|(_, d)| d.path().is_some())
    {
        return Err(format!(
            "`{name}` is a path dependency; published packages may only depend on registry packages"
        ));
    }
    let src = dir.join("src");
    if !src.is_dir() {
        return Err("the package has no `src/` directory".into());
    }
    let mut files = vec![PathBuf::from(MANIFEST)];
    collect(&src, dir, &mut files).map_err(|e| e.to_string())?;
    if files.len() == 1 {
        return Err("`src/` contains no .tvx files".into());
    }
    for extra in ["README.md", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE"] {
        if dir.join(extra).is_file() {
            files.push(PathBuf::from(extra));
        }
    }
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for f in &files {
        tar.append_path_with_name(dir.join(f), f)
            .map_err(|e| format!("cannot archive {}: {e}", f.display()))?;
    }
    let bytes = tar
        .into_inner()
        .and_then(|gz| gz.finish())
        .map_err(|e| e.to_string())?;
    Ok((manifest, bytes))
}

/// Publishes the project at `project` to the directory registry at `registry`.
pub fn publish(registry: &Path, project: &Path) -> Result<IndexEntry, String> {
    let (manifest, archive) = pack(project)?;
    let reg = DirRegistry::new(registry);
    let name = manifest.package.name.clone();
    let version = manifest.package.version.clone();
    if reg.versions(&name)?.iter().any(|e| e.vers == version) {
        return Err(format!(
            "`{name}` {version} is already published; bump the version"
        ));
    }
    let entry = IndexEntry {
        name: name.clone(),
        vers: version.clone(),
        deps: manifest
            .dependencies
            .iter()
            .map(|(n, d): (&String, &Dependency)| IndexDep {
                name: n.clone(),
                req: d.requirement(),
            })
            .collect(),
        cksum: sha256_hex(&archive),
        yanked: false,
        removed: None,
        description: manifest.package.description.clone(),
    };
    std::fs::create_dir_all(registry.join("index"))
        .and_then(|()| std::fs::create_dir_all(registry.join("packages")))
        .map_err(|e| e.to_string())?;
    std::fs::write(reg.package_file(&name, &version), &archive).map_err(|e| e.to_string())?;
    let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    let index = reg.index_file(&name);
    let mut text = std::fs::read_to_string(&index).unwrap_or_default();
    text.push_str(&line);
    text.push('\n');
    std::fs::write(&index, text).map_err(|e| e.to_string())?;
    reg.write_names()?;
    Ok(entry)
}

/// Marks a published version as yanked: it stays installable from lockfiles but new resolutions
/// skip it.
pub fn yank(registry: &Path, name: &str, version: &Version, yanked: bool) -> Result<(), String> {
    let reg = DirRegistry::new(registry);
    let mut entries = reg.versions(name)?;
    let entry = entries
        .iter_mut()
        .find(|e| &e.vers == version)
        .ok_or_else(|| format!("`{name}` {version} is not published"))?;
    entry.yanked = yanked;
    let text: String = entries
        .iter()
        .map(|e| serde_json::to_string(e).map(|l| l + "\n"))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    std::fs::write(reg.index_file(name), text).map_err(|e| e.to_string())?;
    reg.write_names()
}

/// Extracts `archive` into `dest` after checking it against `cksum`.
pub fn unpack(archive: &[u8], cksum: &str, dest: &Path) -> Result<(), String> {
    let actual = sha256_hex(archive);
    if actual != cksum {
        return Err(format!(
            "checksum mismatch: the index says {cksum}, the download is {actual}"
        ));
    }
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(format!("unsafe path in the archive: {}", path.display()));
        }
        entry.unpack_in(dest).map_err(|e| e.to_string())?;
    }
    let mut buf = String::new();
    let _ = std::fs::File::open(dest.join(MANIFEST))
        .and_then(|mut f| f.read_to_string(&mut buf))
        .map_err(|_| "the package has no telvax.toml".to_string())?;
    Ok(())
}

/// Reads one file of an archive, by its path.
fn archive_file(archive: &[u8], wanted: &str) -> Option<Vec<u8>> {
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    for entry in tar.entries().ok()? {
        let mut entry = entry.ok()?;
        if entry.path().ok()?.to_str() == Some(wanted) {
            let mut out = Vec::new();
            entry.read_to_end(&mut out).ok()?;
            return Some(out);
        }
    }
    None
}

fn archive_has(archive: &[u8], prefix: &str) -> bool {
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    let Ok(entries) = tar.entries() else {
        return false;
    };
    entries.flatten().any(|e| {
        e.path()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.starts_with(prefix)))
            .unwrap_or(false)
    })
}

/// `owners.json`: each package name with the GitHub logins that may publish it.
pub fn owners(root: &Path) -> Result<std::collections::BTreeMap<String, Vec<String>>, String> {
    match std::fs::read_to_string(root.join("owners.json")) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|e| format!("owners.json is not valid: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(format!("cannot read owners.json: {e}")),
    }
}

/// What [`check`] found.
#[derive(Debug, Default)]
pub struct CheckReport {
    /// Each problem, in words; the registry is valid when there is none.
    pub problems: Vec<String>,
    /// The versions that are new against the base: their packages are unpacked and tested next.
    pub new_versions: Vec<(String, Version)>,
}

/// The rules of the official registry, on a directory (a checkout of `telvax/registry`). With `base` (a checkout of
/// the branch the change goes into) it also checks the change: published files never change, a line changes only to
/// be yanked or removed, and `author` (the GitHub login of the pull request) owns every name it touches.
pub fn check(
    root: &Path,
    base: Option<&Path>,
    author: Option<&str>,
) -> Result<CheckReport, String> {
    let reg = DirRegistry::new(root);
    let mut report = CheckReport::default();
    let mut bad = |m: String| report.problems.push(m);
    let names = reg.package_names();

    // The index files and the archives they name.
    let mut referenced = std::collections::BTreeSet::new();
    for file in std::fs::read_dir(root.join("index"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let fname = file.file_name().to_string_lossy().into_owned();
        if fname.starts_with('.') {
            continue;
        }
        let stem = fname.strip_suffix(".json").unwrap_or("");
        if stem.is_empty() || validate_name(stem).is_err() {
            bad(format!("index/{fname}: not `<valid package name>.json`"));
        }
    }
    for name in &names {
        let entries = match reg.versions(name) {
            Ok(e) => e,
            Err(e) => {
                bad(e);
                continue;
            }
        };
        let mut seen = std::collections::BTreeSet::new();
        for e in &entries {
            let at = format!("index/{name}.json, {}", e.vers);
            if e.name != *name {
                bad(format!("{at}: the line names `{}`", e.name));
            }
            if !seen.insert(e.vers.clone()) {
                bad(format!("{at}: the version is listed twice"));
            }
            for d in &e.deps {
                if !names.contains(&d.name) {
                    bad(format!(
                        "{at}: depends on `{}`, which is not in the registry",
                        d.name
                    ));
                }
            }
            let file = format!("packages/{name}-{}.nvpkg", e.vers);
            if e.removed.is_some() {
                if root.join(&file).exists() {
                    bad(format!("{at}: marked removed, but {file} is still there"));
                }
                continue;
            }
            referenced.insert(file.clone());
            let Ok(archive) = std::fs::read(root.join(&file)) else {
                bad(format!("{at}: {file} is missing"));
                continue;
            };
            if sha256_hex(&archive) != e.cksum {
                bad(format!(
                    "{at}: the checksum of {file} is not the one of the index"
                ));
                continue;
            }
            match archive_file(&archive, MANIFEST)
                .map(|b| Manifest::parse(&String::from_utf8_lossy(&b)))
            {
                Some(Ok(m)) => {
                    if m.package.name != *name || m.package.version != e.vers {
                        bad(format!(
                            "{at}: the archive is {} {}, not what the index says",
                            m.package.name, m.package.version
                        ));
                    }
                    if m.package.license.is_none() && !archive_has(&archive, "LICENSE") {
                        bad(format!(
                            "{at}: no license: give `license` in {MANIFEST} or add a LICENSE file"
                        ));
                    }
                }
                _ => bad(format!("{at}: the archive has no valid {MANIFEST}")),
            }
        }
    }
    for file in std::fs::read_dir(root.join("packages"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let rel = format!("packages/{}", file.file_name().to_string_lossy());
        if !rel.starts_with("packages/.") && !referenced.contains(&rel) {
            bad(format!("{rel}: no line of the index names it"));
        }
    }
    // Names that are the same once `_` is left out are one name: `leia_br` and `leiabr` cannot both exist.
    let mut plain: std::collections::BTreeMap<String, &String> = Default::default();
    for name in &names {
        if let Some(other) = plain.insert(name.replace('_', ""), name) {
            bad(format!(
                "`{name}` and `{other}` are too alike; one name per package"
            ));
        }
    }
    // names.json follows the index.
    let want = names_json(&reg.names()?)?;
    if std::fs::read_to_string(root.join("names.json")).unwrap_or_default() != want {
        bad("names.json does not match the index; `tvpm publish` writes it, or run `tvpm registry names`".into());
    }
    let owners_now = owners(root)?;

    // The change against the base.
    if let Some(base) = base {
        let before = DirRegistry::new(base);
        let owners_before = owners(base)?;
        for name in &names {
            let old = before.versions(name)?;
            let new = reg.versions(name)?;
            let mut touched = false;
            for o in &old {
                match new.iter().find(|n| n.vers == o.vers) {
                    None => bad(format!(
                        "`{name}` {}: a published version cannot be deleted; mark it removed",
                        o.vers
                    )),
                    Some(n) => {
                        let same = IndexEntry {
                            yanked: o.yanked,
                            removed: o.removed.clone(),
                            ..n.clone()
                        };
                        if &same != o {
                            bad(format!(
                                "`{name}` {}: only `yanked` and `removed` may change once published",
                                o.vers
                            ));
                        }
                        if n.yanked != o.yanked || n.removed != o.removed {
                            touched = true;
                        }
                        let file = format!("packages/{name}-{}.nvpkg", o.vers);
                        if o.removed.is_none() && n.removed.is_none() {
                            let a = std::fs::read(base.join(&file)).ok();
                            let b = std::fs::read(root.join(&file)).ok();
                            if a != b {
                                bad(format!("{file}: a published archive cannot change"));
                            }
                        }
                    }
                }
            }
            for n in new.iter().filter(|n| !old.iter().any(|o| o.vers == n.vers)) {
                touched = true;
                if n.removed.is_none() {
                    report.new_versions.push((name.clone(), n.vers.clone()));
                }
            }
            if let (true, Some(author)) = (touched, author) {
                let allowed = match owners_before.get(name) {
                    Some(list) => list.iter().any(|l| l.eq_ignore_ascii_case(author)),
                    // A new name: the change itself says who owns it, and must name its author.
                    None => owners_now
                        .get(name)
                        .is_some_and(|l| l.iter().any(|l| l.eq_ignore_ascii_case(author))),
                };
                if !allowed {
                    bad(format!(
                        "`{name}`: {author} is not an owner of it in owners.json"
                    ));
                }
            }
        }
        for name in before.package_names() {
            if !names.contains(&name) {
                bad(format!(
                    "`{name}`: its index was deleted; published names stay"
                ));
            }
        }
        for (name, list) in &owners_before {
            if owners_now.get(name) != Some(list)
                && author.is_some_and(|a| !list.iter().any(|l| l.eq_ignore_ascii_case(a)))
            {
                bad(format!(
                    "owners.json: the owners of `{name}` changed, and only one of them may change them"
                ));
            }
        }
    }
    for name in owners_now.keys() {
        if !names.contains(name)
            && base.is_some_and(|b| !owners(b).unwrap_or_default().contains_key(name))
        {
            // A name may be claimed only together with its first version.
            bad(format!("owners.json: `{name}` has owners but no package"));
        }
    }
    Ok(report)
}
