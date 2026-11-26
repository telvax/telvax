//! `telvax.toml`.
//!
//! ```toml
//! [package]
//! name = "app"
//! version = "0.1.0"
//! edition = "2026"                     # the syntax the package is written in
//!
//! [dependencies]
//! mathx = "^1.2"                       # from the registry
//! shared = { path = "../shared" }      # from a directory
//!
//! [lint]                               # what `telvax lint` does for each rule
//! unused-variable = "deny"             # allow, warn or deny
//! unused-function = "allow"
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

pub const MANIFEST: &str = "telvax.toml";

/// The syntax editions this compiler reads. A syntax that changes in a way old programs cannot
/// follow is a new edition; the old ones keep working (see `docs/guide/stability.md`).
pub const EDITIONS: &[&str] = &["2026", "2027-preview"];

/// The edition a package without an `edition` line is read as, and the one `tvpm new` writes.
pub const CURRENT_EDITION: &str = "2026";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub package: PackageInfo,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, Dependency>,
    /// The level (`allow`, `warn`, `deny`) of each lint rule, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lint: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: Version,
    /// The syntax edition of the package's source; `CURRENT_EDITION` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The license, as an SPDX expression (`MIT OR Apache-2.0`). The official registry asks for it, or a LICENSE file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// The file that `telvax run` / `serve` start from; defaults to `src/main.tvx`, then `src/lib.tvx`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    /// `name = "^1.2"`
    Version(VersionReq),
    /// `name = { path = "../x" }` or `name = { version = "1" }`
    Detailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<VersionReq>,
    },
}

impl Dependency {
    /// The directory of a path dependency.
    pub fn path(&self) -> Option<&str> {
        match self {
            Dependency::Detailed { path: Some(p), .. } => Some(p),
            _ => None,
        }
    }

    pub fn requirement(&self) -> VersionReq {
        match self {
            Dependency::Version(r) => r.clone(),
            Dependency::Detailed {
                version: Some(r), ..
            } => r.clone(),
            Dependency::Detailed { .. } => VersionReq::STAR,
        }
    }
}

#[derive(Debug)]
pub struct ManifestError(pub String);

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ManifestError {}

/// Package names are `snake_case` identifiers so that `import name::item` parses.
pub fn validate_name(name: &str) -> Result<(), ManifestError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.contains("__")
        && !matches!(name, "std" | "crate" | "self" | "super");
    if ok {
        Ok(())
    } else {
        Err(ManifestError(format!(
            "invalid package name `{name}`: use lowercase letters, digits and single `_`, starting with a letter (not `std` or `crate`)"
        )))
    }
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let m: Manifest = toml::from_str(text)
            .map_err(|e| ManifestError(format!("invalid {MANIFEST}: {}", e.message())))?;
        validate_name(&m.package.name)?;
        if let Some(edition) = &m.package.edition
            && !EDITIONS.contains(&edition.as_str())
        {
            return Err(ManifestError(format!(
                "this telvax reads the syntax edition{} {}, not `{edition}`; \
                 change `edition` in {MANIFEST}, or update telvax if the package is newer",
                if EDITIONS.len() == 1 { "" } else { "s" },
                EDITIONS
                    .iter()
                    .map(|e| format!("`{e}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        for (name, dep) in &m.dependencies {
            validate_name(name).map_err(|e| ManifestError(format!("dependency: {}", e.0)))?;
            if let Dependency::Detailed {
                path: Some(_),
                version: Some(_),
            } = dep
            {
                return Err(ManifestError(format!(
                    "dependency `{name}` has both `path` and `version`; pick one"
                )));
            }
        }
        Ok(m)
    }

    pub fn read(dir: &Path) -> Result<Manifest, ManifestError> {
        let file = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&file)
            .map_err(|e| ManifestError(format!("cannot read {}: {e}", file.display())))?;
        Manifest::parse(&text)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("a manifest always serializes")
    }

    pub fn write(&self, dir: &Path) -> Result<(), ManifestError> {
        std::fs::write(dir.join(MANIFEST), self.to_toml())
            .map_err(|e| ManifestError(format!("cannot write {MANIFEST}: {e}")))
    }
}
