#!/usr/bin/env python3
"""Replays the timeout artifacts of a fuzz run under the production limits and
writes one triage record per artifact.

    decoder-triage-timeouts.py WORKER FORMAT CRASHES.jsonl ARTIFACTS_DIR OUT.jsonl [JOBS]

Each artifact the run recorded as a hang is replayed through the real worker with
the sandbox's own limits (2 s CPU, 512 MiB) and the host's 4 s wall limit, up to
two tries. A try is *over the limits* when the worker was still running at the
wall limit, was stopped by a signal, or used 2 s of CPU or more. An artifact is a
**confirmed hang** only when both tries are over the limits. Replays run JOBS at a
time; an artifact that looks slow in that pass (wall 1 s or more) is replayed
again on its own, one try at a time, so that load on the machine is not mistaken
for a hang. Every record keeps the measurements of its last two tries.

Exit status 0 if no artifact is a confirmed hang, 1 if any is (their names are
printed), 2 on a usage error.
"""
import concurrent.futures
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
WALL_LIMIT = 4.0
CPU_LIMIT = 2.0
HEAP_MIB = 512
TRIES = 2
SLOW = 1.0


def over_limits(r):
    """True when one replay record breaks a production limit."""
    return bool(
        r.get("timed_out")
        or (r.get("exit") is not None and r["exit"] < 0)  # stopped by a signal
        or r.get("cpu_seconds", 0) >= CPU_LIMIT
        or r.get("wall_seconds", 0) >= WALL_LIMIT
    )


def classify(tries):
    """The verdict for a list of try records: a hang only if every try is over."""
    return len(tries) >= TRIES and all(over_limits(t) for t in tries[:TRIES])


def run_one(worker, fmt, path):
    out = subprocess.run(
        [sys.executable, "-I", os.path.join(HERE, "decoder-replay.py"), worker, fmt, path, str(WALL_LIMIT + 1)],
        capture_output=True, text=True,
    )
    if out.returncode != 0:
        return {"error": out.stderr[-300:], "timed_out": False, "exit": out.returncode, "wall_seconds": 0, "cpu_seconds": 0}
    return json.loads(out.stdout)


def triage(worker, fmt, entry, artifacts, quiet_pass=None):
    path = os.path.join(artifacts, entry["artifact"])
    first = quiet_pass if quiet_pass is not None else run_one(worker, fmt, path)
    tries = [first]
    if over_limits(first) or first.get("wall_seconds", 0) >= SLOW:
        # Slow in the busy pass: replay alone, one at a time, to settle it.
        tries = [run_one(worker, fmt, path) for _ in range(TRIES)]
    return tries


def record(entry, tries):
    last = tries[-TRIES:]
    return {
        **entry,
        "kind": "timeout_raw",
        "replay": {
            "done": True,
            "confirmed_hang": classify(tries),
            "limits": {"wall_ms": int(WALL_LIMIT * 1000), "heap_mib": HEAP_MIB, "tries": TRIES},
            "max_wall_seconds": max(t.get("wall_seconds", 0) for t in last),
            "max_cpu_seconds": max(t.get("cpu_seconds", 0) for t in last),
            "peak_rss_kb": max(t.get("peak_rss_kb", 0) or 0 for t in last),
            "status": last[-1].get("status"),
            "exit": last[-1].get("exit"),
        },
    }


def main(argv):
    if len(argv) not in (5, 6):
        sys.stderr.write(__doc__)
        return 2
    worker, fmt, crashes, artifacts, out_path = argv[:5]
    jobs = int(argv[5]) if len(argv) == 6 else 4
    entries = [json.loads(line) for line in open(crashes) if line.strip()]
    entries = [e for e in entries if e.get("artifact", "").startswith("timeout-")]
    hangs = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool, open(out_path, "w") as out:
        # Pass 1 runs JOBS at a time; slow ones are settled afterwards, alone.
        first = list(pool.map(lambda e: run_one(worker, fmt, os.path.join(artifacts, e["artifact"])), entries))
        for e, f in zip(entries, first):
            tries = triage(worker, fmt, e, artifacts, quiet_pass=f)
            rec = record(e, tries)
            if rec["replay"]["confirmed_hang"]:
                hangs.append(e["artifact"])
            out.write(json.dumps(rec) + "\n")
    print(f"{len(entries)} timeout artifacts replayed; {len(hangs)} confirmed hangs")
    for h in hangs:
        print("HANG", h)
    return 1 if hangs else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
