#!/usr/bin/env python3
"""Tests of scripts/release-timing.py: python3 scripts/test-release-timing.py"""

import importlib.util
import pathlib
import unittest

here = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("release_timing", here / "release-timing.py")
timing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(timing)

WORKFLOW = (here.parent / ".github" / "workflows" / "release.yml").read_text()


def job(name, created, started, completed, conclusion="success"):
    stamp = lambda s: None if s is None else f"2026-11-15T{s}Z"
    return {
        "name": name,
        "conclusion": conclusion,
        "created_at": stamp(created),
        "started_at": stamp(started),
        "completed_at": stamp(completed),
    }


class NeedsTest(unittest.TestCase):
    def test_reads_the_real_workflow(self):
        needs = timing.workflow_needs(WORKFLOW)
        self.assertEqual(needs["plan"], [])
        self.assertEqual(needs["build"], ["plan"])
        self.assertEqual(needs["publish"], ["plan", "build"])
        self.assertEqual(needs["image-manifest"], ["plan", "image"])
        # A job that needs all the others is how `timing` is wired.
        self.assertEqual(set(needs["timing"]), set(needs) - {"timing"})

    def test_job_names_of_matrices_and_reusable_workflows(self):
        known = {"build", "site", "image"}
        self.assertEqual(timing.job_id("build (macos-latest, aarch64-apple-darwin)", known), "build")
        self.assertEqual(timing.job_id("site / deploy", known), "site")
        self.assertIsNone(timing.job_id("something else", known))


class SummaryTest(unittest.TestCase):
    needs = {"plan": [], "build": ["plan"], "publish": ["plan", "build"], "verify": ["plan", "publish"]}

    def jobs(self, extra_minutes=0):
        """A run whose slow build ends at 10:40 (plus `extra_minutes`); what follows shifts with it."""
        end = 40 + extra_minutes
        at = lambda minute, second=0: f"{10 + minute // 60}:{minute % 60:02d}:{second:02d}"
        return [
            job("plan", "10:00:00", "10:00:10", "10:00:40"),
            job("build (fast)", "10:00:40", "10:00:50", "10:20:50"),
            job("build (slow)", "10:00:40", "10:05:40", at(end)),
            job("publish", at(end), at(end), at(end + 1)),
            job("verify (a)", at(end + 1), at(end + 1, 10), at(end + 5, 10)),
            job("site", at(end + 1), at(end + 1), at(end + 1), conclusion="skipped"),
            job("timing", at(end + 5, 10), at(end + 5, 12), None, conclusion=None),  # this job: not finished
        ]

    def test_total_critical_path_and_order(self):
        summary = timing.summarize(self.jobs(), self.needs)
        self.assertEqual(summary["total"], 45 * 60 + 10)
        self.assertEqual(summary["critical_path"], ["plan", "build", "publish", "verify"])
        # plan 40s + slowest build (queue and run) 39m20s + publish 1m + verify 4m10s
        self.assertEqual(summary["critical_seconds"], 40 + (39 * 60 + 20) + 60 + 250)
        names = [j["job"] for j in summary["jobs"]]
        self.assertEqual(names[0], "build (slow)")  # longest first
        self.assertNotIn("site", names)  # skipped
        self.assertNotIn("timing", names)  # not finished

    def test_queue_and_run_are_separate(self):
        summary = timing.summarize(self.jobs(), self.needs)
        slow = next(j for j in summary["jobs"] if j["job"] == "build (slow)")
        self.assertEqual(slow["queue"], 5 * 60)
        self.assertEqual(slow["run"], 34 * 60 + 20)

    def test_markdown_compares_with_the_previous_run(self):
        now = timing.summarize(self.jobs(), self.needs)
        before = timing.summarize(self.jobs(10), self.needs)
        text = timing.markdown(now, before, "123")
        self.assertIn("Against run 123", text)
        self.assertIn("| Total | 55m 10s | 45m 10s | -10m 00s |", text)
        self.assertIn("| Critical path | 55m 10s | 45m 10s | -10m 00s |", text)
        self.assertIn("| build (slow) |", text)

    def test_markdown_without_a_previous_run_or_jobs(self):
        now = timing.summarize(self.jobs(), self.needs)
        self.assertIn("No previous successful release run", timing.markdown(now, None, None))
        empty = timing.summarize([], self.needs)
        self.assertIn("No finished job", timing.markdown(empty, None, None))

    def test_formatting(self):
        self.assertEqual(timing.fmt(59), "59s")
        self.assertEqual(timing.fmt(61), "1m 01s")
        self.assertEqual(timing.fmt(3725), "1h 02m 05s")
        self.assertEqual(timing.delta(100, 100), "no change")


if __name__ == "__main__":
    unittest.main()
