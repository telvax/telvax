//! The files `tvpm new --template <name>` writes next to the manifest, as text kept in the program.
//!
//! They are plain strings so a test can write them to a directory and run `telvax check`, `fmt --check`,
//! `lint` and `test` on them: a template that stops passing its own checks fails the build, so it
//! does not rot.

/// The names `--template` takes.
pub const NAMES: &[&str] = &["lib"];

/// The files of a library package, as `(path, text)`, without the manifest and `.gitignore` that
/// `tvpm new` always writes. `name` is the package name.
pub fn library(name: &str) -> Vec<(String, String)> {
    let fill = |text: &str| text.replace("{name}", name);
    vec![
        (
            "src/lib.tvx".into(),
            "# Greets `who` by name.\nfn greet(who: Text) -> Text:\n    f\"hello, {who}\"\n".into(),
        ),
        (
            "src/test_lib.tvx".into(),
            "from lib import greet\n\nfn test_greet():\n    assert_eq(greet(\"world\"), \"hello, world\")\n".into(),
        ),
        ("README.md".into(), fill(README)),
        ("CHANGELOG.md".into(), fill(CHANGELOG)),
        ("Makefile".into(), MAKEFILE.into()),
        (".pre-commit-config.yaml".into(), PRE_COMMIT.into()),
        (".github/workflows/ci.yml".into(), CI.into()),
    ]
}

const README: &str = "# {name}

A Telvax library.

## Use it

```sh
tvpm add {name}
```

```telvax ignore
from {name} import greet
```

## Work on it

```sh
make check    # format, lint, type check and tests
make doc      # the documentation of the public functions, from their signatures and comments
```

`make` runs `telvax fmt --check`, `telvax lint`, `telvax check` and `telvax test` on `src/`. The tests are the
`test_*` functions of the files named `test_*.tvx` in `src/`.

## Release

Put the version in `telvax.toml`, add a section to `CHANGELOG.md`, and push a tag `v<version>`. The workflow in
`.github/workflows/ci.yml` runs the checks and, when the repository has the variable `TVPM_REGISTRY` (a registry
directory), runs `tvpm publish` on the tag.
";

const CHANGELOG: &str = "# Changelog

## Unreleased

* The first version of `{name}`.
";

const MAKEFILE: &str = "# The checks of the CI, to run before a push. `make` runs them all.
.PHONY: all check fmt lint test doc

all: check

check:
\ttelvax fmt --check src/
\ttelvax lint src/lib.tvx
\ttelvax check src/lib.tvx
\ttelvax test src/

fmt:
\ttelvax fmt src/

lint:
\ttelvax lint src/lib.tvx

test:
\ttelvax test src/

doc:
\ttelvax doc --public src/lib.tvx
";

const PRE_COMMIT: &str =
    "# https://pre-commit.com: `pre-commit install` runs these before each commit.
repos:
  - repo: local
    hooks:
      - id: telvax-fmt
        name: telvax fmt --check
        entry: telvax fmt --check src/
        language: system
        pass_filenames: false
        files: \\.tvx$
      - id: telvax-lint
        name: telvax lint
        entry: telvax lint src/lib.tvx
        language: system
        pass_filenames: false
        files: \\.tvx$
";

const CI: &str = "name: CI

on:
  push:
    branches: [main]
    tags: [\"v*\"]
  pull_request:

permissions:
  contents: read

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - name: Install telvax
        run: |
          curl -fsSL https://raw.githubusercontent.com/telvax/telvax/main/scripts/install.sh | sh
          echo \"$HOME/.telvax/bin\" >> \"$GITHUB_PATH\"
      - run: telvax fmt --check src/
      - run: telvax lint src/lib.tvx
      - run: telvax check src/lib.tvx
      - run: telvax test src/

  # Only on a tag, and only when the repository has the variable TVPM_REGISTRY (a registry directory).
  publish:
    needs: check
    if: startsWith(github.ref, 'refs/tags/v') && vars.TVPM_REGISTRY != ''
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - name: Install telvax
        run: |
          curl -fsSL https://raw.githubusercontent.com/telvax/telvax/main/scripts/install.sh | sh
          echo \"$HOME/.telvax/bin\" >> \"$GITHUB_PATH\"
      - run: tvpm publish --registry \"$TVPM_REGISTRY\"
        env:
          TVPM_REGISTRY: ${{ vars.TVPM_REGISTRY }}
";
