#!/bin/sh
# Works out the version a merge to main releases, from the latest tag and the title (the first line
# of the squash commit) of every commit since it. Prints the version, or "none" when nothing is released.
#   scripts/next-version.sh <latest tag version or ""> <version declared in Cargo.toml> <commit messages>
# The messages are the commits since the tag, each ended by the character 036 (octal), as
# `git log <tag>..HEAD --format='%B%x1e'` prints them. One message with no separator works too.
# The biggest bump of all the commits wins:
#
#   feat!: / BREAKING CHANGE:   major  (1.2.3 -> 2.0.0; while the major is 0, minor: 0.1.2 -> 0.2.0)
#   feat:                       minor  (0.1.2 -> 0.2.0)
#   fix, perf, refactor, build,
#   revert, or no prefix        patch  (0.1.2 -> 0.1.3)
#   docs, ci, test, style,
#   chore                       no release
#   [skip release]              that commit counts for nothing
# A Cargo.toml version above the latest tag wins, so a version can still be chosen by hand.
set -eu
latest="$1" declared="$2" messages="$3"

if [ -z "$latest" ]; then echo "$declared"; exit 0; fi
if [ "$declared" != "$latest" ] && [ "$(printf '%s\n%s\n' "$latest" "$declared" | sort -V | tail -n 1)" = "$declared" ]; then
  echo "$declared"; exit 0
fi

# 0 none, 1 patch, 2 minor, 3 breaking
bump="$(printf '%s' "$messages" | awk '
  BEGIN { RS = "\036"; max = 0 }
  {
    msg = $0
    sub(/^[ \t\r\n]+/, "", msg)
    if (msg == "" || index(msg, "[skip release]")) next
    subject = msg
    sub(/\n.*/, "", subject)
    type = ""; breaking = 0
    if (match(subject, /^[A-Za-z]+(\([^)]*\))?!?:/)) {
      head = substr(subject, 1, RLENGTH - 1)
      if (head ~ /!$/) breaking = 1
      sub(/[(!].*/, "", head)
      type = tolower(head)
    }
    if (msg ~ /(^|\n)BREAKING[ -]CHANGE:/) breaking = 1
    if (breaking) level = 3
    else if (type == "feat") level = 2
    else if (type ~ /^(docs|ci|test|style|chore)$/) level = 0
    else level = 1
    if (level > max) max = level
  }
  END { print max }')"

major="${latest%%.*}"; rest="${latest#*.}"; minor="${rest%%.*}"; patch="${rest#*.}"
case "$bump" in
  0) echo none ;;
  3) if [ "$major" = 0 ]; then echo "0.$((minor + 1)).0"; else echo "$((major + 1)).0.0"; fi ;;
  2) echo "$major.$((minor + 1)).0" ;;
  *) echo "$major.$minor.$((patch + 1))" ;;
esac
