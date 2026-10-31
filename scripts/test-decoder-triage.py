#!/usr/bin/env python3
"""Tests scripts/decoder-triage-timeouts.py with a stand-in worker (no fuzzing)."""
import importlib.util
import json
import os
import stat
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("triage", os.path.join(HERE, "decoder-triage-timeouts.py"))
triage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(triage)
triage.WALL_LIMIT = 1.0  # the stand-in is fast or it sleeps for 30 s
triage.SLOW = 0.5

WORKER = """#!{python}
import os, sys, time
data = sys.stdin.buffer.read()
if b"SLEEP" in data:
    time.sleep(30)
elif b"ONCE" in data:
    flag = os.environ.get("FLAG", "") or "{flag}"
    if not os.path.exists(flag):
        open(flag, "w").close()
        time.sleep(30)
elif b"SPIN" in data:
    end = time.process_time() + 3
    while time.process_time() < end:
        pass
sys.stdout.buffer.write(b"LDER" + bytes([0]) + b"\\0" * 8)
"""


class Triage(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.flag = os.path.join(self.dir, "flag")
        self.worker = os.path.join(self.dir, "worker")
        with open(self.worker, "w") as fh:
            fh.write(WORKER.format(python=sys.executable, flag=self.flag))
        os.chmod(self.worker, os.stat(self.worker).st_mode | stat.S_IXUSR)
        self.art = os.path.join(self.dir, "arts")
        os.mkdir(self.art)

    def put(self, name, body):
        with open(os.path.join(self.art, name), "wb") as fh:
            fh.write(b"\xff\xd8\xff" + body)

    def run_triage(self, names):
        crashes = os.path.join(self.dir, "crashes.jsonl")
        with open(crashes, "w") as fh:
            for n in names:
                fh.write(json.dumps({"at_cpu_hour": 1.0, "kind": "hang", "artifact": n}) + "\n")
            fh.write(json.dumps({"kind": "oom", "artifact": "oom-1"}) + "\n")  # not a timeout: ignored
        out = os.path.join(self.dir, "out.jsonl")
        rc = triage.main([self.worker, "jpeg", crashes, self.art, out, "2"])
        return rc, [json.loads(l) for l in open(out)]

    def test_a_fast_artifact_is_not_a_hang(self):
        self.put("timeout-fast", b"plain")
        rc, recs = self.run_triage(["timeout-fast"])
        self.assertEqual(rc, 0)
        self.assertEqual(len(recs), 1)
        self.assertFalse(recs[0]["replay"]["confirmed_hang"])
        self.assertEqual(recs[0]["replay"]["limits"], {"wall_ms": 1000, "heap_mib": 512, "tries": 2})

    def test_an_artifact_that_never_finishes_is_a_confirmed_hang(self):
        self.put("timeout-stuck", b"SLEEP")
        rc, recs = self.run_triage(["timeout-stuck"])
        self.assertEqual(rc, 1)
        self.assertTrue(recs[0]["replay"]["confirmed_hang"])

    def test_one_slow_try_is_not_enough(self):
        # Slow the first time only (a loaded machine), fast on the second.
        self.put("timeout-once", b"ONCE")
        rc, recs = self.run_triage(["timeout-once"])
        self.assertFalse(recs[0]["replay"]["confirmed_hang"], recs[0])
        self.assertEqual(rc, 0)

    def test_cpu_over_the_limit_counts(self):
        self.assertTrue(triage.over_limits({"cpu_seconds": 2.0, "wall_seconds": 0.1, "exit": 0}))
        self.assertFalse(triage.over_limits({"cpu_seconds": 1.99, "wall_seconds": 0.1, "exit": 0}))
        self.assertTrue(triage.over_limits({"exit": -9, "cpu_seconds": 0, "wall_seconds": 0.1}))
        self.assertTrue(triage.over_limits({"timed_out": True}))

    def test_two_tries_both_over_are_required(self):
        over, fine = {"timed_out": True}, {"cpu_seconds": 0.1, "wall_seconds": 0.1, "exit": 0}
        self.assertTrue(triage.classify([over, over]))
        self.assertFalse(triage.classify([over, fine]))
        self.assertFalse(triage.classify([over]))

    def test_only_timeout_artifacts_are_replayed(self):
        self.put("timeout-a", b"plain")
        rc, recs = self.run_triage(["timeout-a"])
        self.assertEqual([r["artifact"] for r in recs], ["timeout-a"])


if __name__ == "__main__":
    unittest.main()
