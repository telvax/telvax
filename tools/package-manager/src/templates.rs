//! The files `tvpm new --template <name>` writes next to the manifest, as text kept in the program.
//!
//! They are plain strings so a test can write them to a directory and run `telvax check`, `fmt --check`,
//! `lint` and `test` on them: a template that stops passing its own checks fails the build, so it
//! does not rot.

/// The names `--template` takes.
pub const NAMES: &[&str] = &["lib", "job", "service"];

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

/// The files of a batch job: a `main` with flags and defaults (`telvax run`), its test, and a Dockerfile that runs it.
pub fn job(name: &str) -> Vec<(String, String)> {
    let mut files = app(name, "job");
    files.push((
        "src/main.tvx".into(),
        "# Greets `who`, `times` times. Run it: telvax run src/main.tvx main --who you --times 2\nfn greet(who: Text, times: Int) -> Text:\n    std.string.repeat(f\"hello, {who}\\n\", times)\n\nfn main(who: Text = \"world\", times: Int = 1) -> Text:\n    greet(who, times)\n".into(),
    ));
    files.push((
        "src/test_main.tvx".into(),
        "from main import greet\n\nfn test_greet():\n    assert_eq(greet(\"world\", 2), \"hello, world\\nhello, world\\n\")\n".into(),
    ));
    files.push((
        "Dockerfile".into(),
        "# The job in the published Telvax image. Arguments after the image name are the flags of `main`:\n#   docker run --rm IMAGE --who you --times 2\nFROM ghcr.io/telvax/telvax:latest\nWORKDIR /app\nCOPY . .\nENTRYPOINT [\"telvax\", \"run\", \"src/main.tvx\", \"main\"]\n".into(),
    ));
    files
}

/// The files of a service: an `@endpoint`, its test, a Dockerfile that serves it and `.env.example`.
pub fn service(name: &str) -> Vec<(String, String)> {
    let mut files = app(name, "service");
    files.push((
        "src/main.tvx".into(),
        "import std.http.Status\n\ntype Greeting:\n    message: Text\n\n# The text of the greeting.\nfn greeting(who: Text) -> Text:\n    f\"hello, {who}\"\n\n@endpoint(GET, \"/v1/hello/{who}\")\nfn hello(who: Text) -> Result[Greeting, Status]:\n    Ok(Greeting(message: greeting(who)))\n".into(),
    ));
    files.push((
        "src/test_main.tvx".into(),
        "from main import greeting\n\nfn test_greeting():\n    assert_eq(greeting(\"world\"), \"hello, world\")\n".into(),
    ));
    files.push((
        "Dockerfile".into(),
        "# The service in the published Telvax image. Run it: docker run --rm -p 8080:8080 IMAGE\nFROM ghcr.io/telvax/telvax:latest\nWORKDIR /app\nCOPY . .\nEXPOSE 8080\nENTRYPOINT [\"telvax\", \"serve\", \"src/main.tvx\", \"--host\", \"0.0.0.0\", \"--port\", \"8080\"]\n".into(),
    ));
    files.push((
        ".env.example".into(),
        "# Copy to .env and pass it with `docker run --env-file .env`, or export it before `telvax serve`.\n# TELVAX_LIVE=1\n".into(),
    ));
    files
}

/// What a job and a service share: the README, the CHANGELOG, the Makefile, pre-commit, the CI workflow and
/// `.dockerignore`. `kind` is `job` or `service`.
fn app(name: &str, kind: &str) -> Vec<(String, String)> {
    let fill = |text: &str| text.replace("{name}", name).replace("{kind}", kind);
    let (run, what) = if kind == "job" {
        (
            "telvax run src/main.tvx main",
            "A Telvax batch job: it runs, writes what it makes and ends.",
        )
    } else {
        (
            "telvax serve src/main.tvx",
            "A Telvax service: HTTP endpoints, served from this package.",
        )
    };
    vec![
        (
            "README.md".into(),
            fill(APP_README)
                .replace("{run}", run)
                .replace("{what}", what),
        ),
        ("CHANGELOG.md".into(), fill(CHANGELOG)),
        ("Makefile".into(), fill(APP_MAKEFILE).replace("{run}", run)),
        (".pre-commit-config.yaml".into(), APP_PRE_COMMIT.into()),
        (".github/workflows/ci.yml".into(), fill(APP_CI)),
        (".dockerignore".into(), ".git\n.tvpm\ntarget\n.env\n".into()),
    ]
}

const APP_README: &str = "# {name}

{what}

## Run it

```sh
{run}
```

## Work on it

```sh
make check          # format, lint, type check and tests
make docker-build   # the image, from the published Telvax image
make docker-run     # run it
```

`make check` runs `telvax fmt --check`, `telvax lint`, `telvax check` and `telvax test` on `src/`. The tests are the
`test_*` functions of the files named `test_*.tvx` in `src/`. The Dockerfile starts from
`ghcr.io/telvax/telvax`, so the image has the same `telvax` as the one that checks the code.
";

const APP_MAKEFILE: &str = "# The checks of the CI, to run before a push. `make` runs them all.
IMAGE ?= {name}
.PHONY: all check fmt lint test run docker-build docker-run

all: check

check:
\ttelvax fmt --check src/
\ttelvax lint src/main.tvx
\ttelvax check src/main.tvx
\ttelvax test src/

fmt:
\ttelvax fmt src/

lint:
\ttelvax lint src/main.tvx

test:
\ttelvax test src/

run:
\t{run}

docker-build:
\tdocker build -t $(IMAGE) .

docker-run:
\tdocker run --rm -p 8080:8080 $(IMAGE)
";

const APP_PRE_COMMIT: &str =
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
        entry: telvax lint src/main.tvx
        language: system
        pass_filenames: false
        files: \\.tvx$
";

const APP_CI: &str = "name: CI

on:
  push:
    branches: [main]
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
      - run: telvax lint src/main.tvx
      - run: telvax check src/main.tvx
      - run: telvax test src/

  image:
    needs: check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - run: docker build -t {name} .
";

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
