#!/bin/sh
# Tests of scripts/release-paths.sh. Run it from the root of the repository: sh scripts/test-release-paths.sh
set -eu
failed=0

# check <expected> <file>...
check() {
  expected="$1"; shift
  got="$(printf '%s\n' "$@" | sh scripts/release-paths.sh)"
  if [ "$got" != "$expected" ]; then
    echo "FAIL: expected $expected, got $got, for: $*"
    failed=1
  fi
}

# Merges that released a version with the same binaries: the site, the docs, the workflows.
check no site/src/content/docs/index.mdx site/package-lock.json
check no docs/guide/deploy.md CHANGELOG.md README.md spec/grammar.md
check no .github/workflows/release.yml .github/workflows/release-verify.yml .github/workflows/site.yml .github/dependabot.yml
check no editors/vscode/package.json editors/vscode/src/extension.ts
check no scripts/next-version.sh scripts/test-next-version.sh scripts/release-timing.py
check no benchmarks/src/main.rs playground/src/lib.rs
check no tests/integration.rs LICENSE-MIT deny.toml clippy.toml rustfmt.toml

# Anything that goes in a binary.
check yes runtime/src/lib.rs
check yes compiler/semantic/src/stdlib_docs.rs
check yes server/src/main.rs cli/src/main.rs
check yes tools/linter/src/lib.rs
check yes Cargo.toml
check yes Cargo.lock
check yes rust-toolchain.toml
check yes examples/rag-service.tvx
check yes stdlib/core/option.tvx
check yes scripts/install.sh
check yes scripts/install.ps1
check yes scripts/set-version.sh
check yes Dockerfile

# One file that goes in a binary is enough, wherever it is in the list.
check yes site/index.md docs/a.md runtime/src/lib.rs
check yes runtime/src/lib.rs site/index.md
# Names that only look like the listed ones.
check no site/runtime/notes.md docs/cli/README.md docs/examples/intro.md
# Nothing changed.
[ "$(printf '' | sh scripts/release-paths.sh)" = no ] || { echo "FAIL: empty list"; failed=1; }

[ "$failed" = 0 ] && echo "release-paths: all cases pass"
exit "$failed"
