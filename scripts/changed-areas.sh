#!/bin/sh
# Prints which parts of the repository a push or pull request touches, as GITHUB_OUTPUT lines:
#   core=true|false     anything that is not documentation (including the site), release plumbing, the extension or the Python package
#   python=true|false   core, or the Python package
#   vscode=true|false   core, or the VS Code extension
#   docker=true|false   what the container build exercises: the generated Dockerfile, the server, the
#                       dependencies, the toolchain and the example it serves
# CI uses it to skip pipelines a change cannot affect: a documentation-only change runs none of them.
# When the change set cannot be worked out, everything runs.
# Usage (in CI): scripts/changed-areas.sh >> "$GITHUB_OUTPUT"
set -eu

if [ "${GITHUB_EVENT_NAME:-}" = pull_request ]; then
  files="$(git diff --name-only "origin/${GITHUB_BASE_REF}...HEAD" 2>/dev/null || echo __unknown__)"
else
  before="${BEFORE_SHA:-}"
  case "$before" in
    "" | 0000000000000000000000000000000000000000) files=__unknown__ ;;
    *) files="$(git diff --name-only "$before" HEAD 2>/dev/null || echo __unknown__)" ;;
  esac
fi

core=false python=false vscode=false docker=false
if [ "$files" = __unknown__ ] || [ -z "$files" ]; then
  core=true
  docker=true
else
  for f in $files; do
    case "$f" in
      docs/* | rfcs/* | spec/* | site/* | *.md | LICENSE* | .gitattributes) ;;
      # Repository settings and workflows that no test pipeline exercises: the release and
      # auto-merge workflows only run on main, and the secret scan runs on its own.
      .github/FUNDING.yml | .github/CODEOWNERS | .github/dependabot.yml) ;;
      .github/workflows/release.yml | .github/workflows/dependabot-auto-merge.yml | .github/workflows/secrets.yml | .github/workflows/site.yml) ;;
      scripts/set-version.sh | scripts/next-version.sh | scripts/test-next-version.sh) ;;
      python/*) python=true ;;
      editors/vscode/*) vscode=true ;;
      cli/src/bundle.rs | Dockerfile | .dockerignore | cli/Cargo.toml | server/* | Cargo.toml | Cargo.lock | rust-toolchain.toml | examples/rag-service.tvx | .github/workflows/integration.yml | scripts/changed-areas.sh)
        core=true docker=true ;;
      *) core=true ;;
    esac
  done
fi
[ "$core" = true ] && python=true && vscode=true
echo "core=$core"
echo "python=$python"
echo "vscode=$vscode"
echo "docker=$docker"
