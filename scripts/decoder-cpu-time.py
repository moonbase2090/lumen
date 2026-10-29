#!/usr/bin/env python3
"""Runs a command, forwards SIGTERM and SIGINT to it, and writes the CPU time
(user plus system, in seconds) of everything it ran and waited for to OUTFILE.

    decoder-cpu-time.py OUTFILE COMMAND [ARGS...]

The campaign counts this instead of workers times wall time: workers that wait
(on the fork-mode parent, on memory) are not CPU-hours of fuzzing. Processes that
were killed before their parent could wait for them are not counted, which errs
on the low side.
"""
import resource
import signal
import subprocess
import sys

outfile, cmd = sys.argv[1], sys.argv[2:]
child = subprocess.Popen(cmd)


def forward(signum, _frame):
    try:
        child.send_signal(signal.SIGTERM)
    except ProcessLookupError:
        pass


signal.signal(signal.SIGTERM, forward)
signal.signal(signal.SIGINT, forward)
status = child.wait()
usage = resource.getrusage(resource.RUSAGE_CHILDREN)
with open(outfile, "w") as f:
    f.write(f"{usage.ru_utime + usage.ru_stime:.3f}\n")
sys.exit(status if status >= 0 else 128 - status)
