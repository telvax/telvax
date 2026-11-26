#!/bin/sh
# Prints the ids of the jobs of a workflow run that ended because no runner took them: GitHub cancels such a job
# with the annotation "The job was not acquired by Runner of type hosted even after multiple attempts". A job
# that a person cancelled, or that failed on its own, has no such annotation and is not printed.
#
#   scripts/unacquired-jobs.sh <owner/repo> <run id> <attempt>
#
# `TELVAX_FIXTURES=dir` reads `jobs.json` and `annotations-<job id>.json` from a folder instead of the API (tests).
set -eu
repo=$1 run=$2 attempt=$3

jobs() {
  if [ -n "${TELVAX_FIXTURES:-}" ]; then cat "$TELVAX_FIXTURES/jobs.json"
  else gh api --paginate "repos/$repo/actions/runs/$run/attempts/$attempt/jobs"; fi
}

annotations() {
  if [ -n "${TELVAX_FIXTURES:-}" ]; then cat "$TELVAX_FIXTURES/annotations-$1.json"
  else gh api "repos/$repo/check-runs/$1/annotations"; fi
}

for id in $(jobs | jq -r '.jobs[] | select(.conclusion == "cancelled" or .conclusion == "failure") | .id'); do
  if annotations "$id" | jq -e 'any(.[]; .message | test("not acquired by Runner"))' > /dev/null; then
    echo "$id"
  fi
done
