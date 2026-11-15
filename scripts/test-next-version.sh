#!/bin/sh
# Tests of scripts/next-version.sh. Run it from the root of the repository: sh scripts/test-next-version.sh
set -eu
rs="$(printf '\036')"
failed=0

# check <expected> <latest> <declared> <commit>...
check() {
  expected="$1" latest="$2" declared="$3"; shift 3
  messages=""
  for commit in "$@"; do messages="$messages$commit$rs"; done
  got="$(sh scripts/next-version.sh "$latest" "$declared" "$messages")"
  if [ "$got" != "$expected" ]; then
    echo "FAIL: expected $expected, got $got, for: $*"
    failed=1
  fi
}

check 0.48.0 0.47.7 0.47.7 "feat: a" "fix: b"
check 0.48.0 0.47.7 0.47.7 "fix: b" "feat: a"
check 0.47.8 0.47.7 0.47.7 "fix: a" "docs: b"
check 0.47.8 0.47.7 0.47.7 "docs: b" "fix: a"
check none 0.47.7 0.47.7 "docs: a" "ci: b" "test: c" "style: d" "chore: e"
check 0.48.0 0.47.7 0.47.7 "docs: a" "feat!: b" "fix: c"
check 1.0.0 0.47.7 1.0.0 "feat: a"
check 2.0.0 1.4.2 1.4.2 "fix: a" "feat(api)!: b" "feat: c"
check 0.48.0 0.47.7 0.47.7 "docs: a

BREAKING CHANGE: the pipe operator is gone"
check 0.47.8 0.47.7 0.47.7 "feat: a [skip release]" "fix: b"
check none 0.47.7 0.47.7 "feat: a [skip release]" "docs: b"
check 0.47.8 0.47.7 0.47.7 "update the readme"
check 0.47.8 0.47.7 0.47.7 "Fix: Capitalized"
check 0.48.0 0.47.7 0.47.7 "feat: one

* fix: a line of the body"
check 0.47.8 0.47.7 0.47.7 "perf: a" "refactor: b"
check 0.48.0 0.47.7 0.47.7 "feat(scope): a"
# one message with no separator
[ "$(sh scripts/next-version.sh 0.47.7 0.47.7 'feat: a')" = 0.48.0 ] || { echo "FAIL: one message"; failed=1; }
# a version chosen by hand wins, and the first release is the declared one
check 0.50.0 0.47.7 0.50.0 "fix: a"
check 0.1.0 "" 0.1.0 "fix: a"

[ "$failed" = 0 ] && echo "next-version: all cases pass"
exit "$failed"
