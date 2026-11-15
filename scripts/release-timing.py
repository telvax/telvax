#!/usr/bin/env python3
"""Where the time of a release run went.

Reads the jobs of a run (the JSON of `gh api repos/R/actions/runs/ID/jobs`) and the `needs` of
.github/workflows/release.yml, and writes:

* a Markdown summary: the queue time and the run time of each job, longest first, the total time,
  the critical path (the chain of `needs` with the largest sum) and, when the jobs of a previous
  run are given, the same totals for it and the change;
* a JSON file (`release-timing.json`) for a script that compares runs later.

Standard library only, so it runs on any runner and in the tests (scripts/test-release-timing.py).

    release-timing.py --workflow release.yml --jobs jobs.json [--previous prev.json] \
        [--previous-id 123] [--out-json release-timing.json] [--out-md summary.md]
"""

import argparse
import json
import re
import sys
from datetime import datetime


def parse_time(value):
    if not value:
        return None
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def workflow_needs(text):
    """{job id: [job ids it needs]} from the workflow file, without a YAML library.

    A job is a key with two spaces of indentation under `jobs:`; `needs:` is either one id or a
    flow list (`[plan, build]`), which is all this workflow uses.
    """
    needs = {}
    in_jobs = False
    current = None
    for line in text.splitlines():
        if re.match(r"^jobs:\s*$", line):
            in_jobs = True
            continue
        if not in_jobs:
            continue
        if re.match(r"^\S", line) and not line.startswith("#"):
            break  # another top-level key
        job = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if job:
            current = job.group(1)
            needs[current] = []
            continue
        dep = re.match(r"^    needs:\s*(.+?)\s*$", line)
        if dep and current:
            value = dep.group(1).strip("[]")
            needs[current] = [item.strip() for item in value.split(",") if item.strip()]
    return needs


def job_id(name, known):
    """The workflow job id of a job in the run: `build (ubuntu-latest, x)` is `build`, and
    `site / deploy` (a reusable workflow) is `site`."""
    head = re.split(r" \(| / ", name, maxsplit=1)[0]
    return head if head in known else None


def measure(jobs, known):
    """One row per finished job that ran: id, name, queue and run seconds."""
    rows = []
    for job in jobs:
        if job.get("conclusion") == "skipped":
            continue
        created = parse_time(job.get("created_at"))
        started = parse_time(job.get("started_at"))
        completed = parse_time(job.get("completed_at"))
        if not (created and started and completed):
            continue  # still running (this job itself) or never started
        rows.append(
            {
                "id": job_id(job["name"], known),
                "name": job["name"],
                "conclusion": job.get("conclusion"),
                "queue": max(0.0, (started - created).total_seconds()),
                "run": max(0.0, (completed - started).total_seconds()),
                "created": created,
                "completed": completed,
            }
        )
    return rows


def critical_path(rows, needs):
    """The chain of `needs` with the largest sum of (queue + run). A job id with several
    instances (a matrix) counts as its slowest instance. Returns (seconds, [job ids])."""
    own = {}
    for row in rows:
        if row["id"]:
            own[row["id"]] = max(own.get(row["id"], 0.0), row["queue"] + row["run"])
    best = {}

    def longest(job):
        if job in best:
            return best[job]
        tail = (0.0, [])
        for dep in needs.get(job, []):
            if dep in own:
                candidate = longest(dep)
                if candidate[0] > tail[0]:
                    tail = candidate
        best[job] = (tail[0] + own[job], tail[1] + [job])
        return best[job]

    result = (0.0, [])
    for job in own:
        candidate = longest(job)
        if candidate[0] > result[0]:
            result = candidate
    return result


def summarize(jobs, needs):
    known = set(needs)
    rows = measure(jobs, known)
    if not rows:
        return {"total": 0.0, "critical_seconds": 0.0, "critical_path": [], "jobs": []}
    total = (max(r["completed"] for r in rows) - min(r["created"] for r in rows)).total_seconds()
    seconds, path = critical_path(rows, needs)
    ordered = sorted(rows, key=lambda r: r["queue"] + r["run"], reverse=True)
    return {
        "total": total,
        "critical_seconds": seconds,
        "critical_path": path,
        "jobs": [
            {
                "job": r["name"],
                "conclusion": r["conclusion"],
                "queue": r["queue"],
                "run": r["run"],
            }
            for r in ordered
        ],
    }


def fmt(seconds):
    seconds = int(round(seconds))
    minutes, secs = divmod(seconds, 60)
    hours, minutes = divmod(minutes, 60)
    if hours:
        return f"{hours}h {minutes:02d}m {secs:02d}s"
    if minutes:
        return f"{minutes}m {secs:02d}s"
    return f"{secs}s"


def delta(now, before):
    diff = int(round(now - before))
    if diff == 0:
        return "no change"
    sign = "+" if diff > 0 else "-"
    return f"{sign}{fmt(abs(diff))}"


def markdown(current, previous, previous_id):
    lines = ["## Where the time of this release went", ""]
    if not current["jobs"]:
        return "\n".join(lines + ["No finished job to measure.", ""])
    lines += [
        f"Total: **{fmt(current['total'])}**. Critical path: "
        f"**{fmt(current['critical_seconds'])}** ({' → '.join(current['critical_path'])}).",
        "",
        "| Job | Queue | Run | Result |",
        "|---|---:|---:|---|",
    ]
    for job in current["jobs"]:
        lines.append(f"| {job['job']} | {fmt(job['queue'])} | {fmt(job['run'])} | {job['conclusion']} |")
    lines.append("")
    if previous and previous["jobs"]:
        label = f"run {previous_id}" if previous_id else "the previous successful release run"
        lines += [
            f"Against {label}:",
            "",
            "| | Before | Now | Change |",
            "|---|---:|---:|---:|",
            f"| Total | {fmt(previous['total'])} | {fmt(current['total'])} | "
            f"{delta(current['total'], previous['total'])} |",
            f"| Critical path | {fmt(previous['critical_seconds'])} | {fmt(current['critical_seconds'])} | "
            f"{delta(current['critical_seconds'], previous['critical_seconds'])} |",
            "",
        ]
    else:
        lines += ["No previous successful release run to compare with.", ""]
    return "\n".join(lines)


def load_jobs(path):
    with open(path) as handle:
        data = json.load(handle)
    return data["jobs"] if isinstance(data, dict) else data


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--workflow", required=True)
    parser.add_argument("--jobs", required=True)
    parser.add_argument("--previous")
    parser.add_argument("--previous-id")
    parser.add_argument("--out-json", default="release-timing.json")
    parser.add_argument("--out-md", default="-")
    args = parser.parse_args(argv)

    with open(args.workflow) as handle:
        needs = workflow_needs(handle.read())
    current = summarize(load_jobs(args.jobs), needs)
    previous = summarize(load_jobs(args.previous), needs) if args.previous else None

    with open(args.out_json, "w") as handle:
        json.dump({"current": current, "previous": previous, "previous_run": args.previous_id}, handle, indent=2)
        handle.write("\n")
    text = markdown(current, previous, args.previous_id)
    if args.out_md == "-":
        sys.stdout.write(text)
    else:
        with open(args.out_md, "a") as handle:  # appended: $GITHUB_STEP_SUMMARY
            handle.write(text)


if __name__ == "__main__":
    main(sys.argv[1:])
