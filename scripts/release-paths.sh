#!/bin/sh
# Says whether a change reaches the release binaries. Reads file names, one per line, from the
# standard input (`git diff --name-only <tag>..HEAD`) and prints "yes" if one of them goes in a
# binary, "no" if none does (the documentation site, the docs, the workflows, the editor
# extension...), and "no" for an empty list.
#   git diff --name-only "$latest..HEAD" | scripts/release-paths.sh
#
# The list errs on the side of including: a path that is listed but does not reach a binary costs
# one release too many, a path that is missing is a release that does not happen. It is what
# `cargo build -p telvax_cli -p telvax_pkg` and the Dockerfile use:
#
#   the workspace and the toolchain   Cargo.toml, Cargo.lock, rust-toolchain.toml, .cargo/
#   the crates                        compiler/, runtime/, server/, cli/, tools/, python/
#   what the binaries embed           examples/ (include_str! in the CLI), stdlib/
#   the version and the image         scripts/set-version.sh, Dockerfile, .dockerignore
#   the install scripts               scripts/install.sh, scripts/install.ps1 (users download them)
#
# benchmarks/ and playground/ are workspace members that no release binary depends on, so a change
# there alone is not released. When the workspace or the build changes, add the path here and to
# scripts/test-release-paths.sh.
set -eu

while IFS= read -r file; do
  case "$file" in
    Cargo.toml | Cargo.lock | rust-toolchain.toml | .cargo/* | \
    compiler/* | runtime/* | server/* | cli/* | tools/* | python/* | \
    examples/* | stdlib/* | \
    scripts/set-version.sh | scripts/install.sh | scripts/install.ps1 | Dockerfile | .dockerignore)
      echo yes
      exit 0
      ;;
  esac
done
echo no
