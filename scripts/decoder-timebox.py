#!/usr/bin/env python3
"""Runs a command with a time limit, in its own process group, and kills the whole
group when the limit passes.

    decoder-timebox.py SECONDS COMMAND [ARGS...]

Exits with the command's status, or 124 if it was killed for taking too long. When
the limit passes the group gets TERM and a grace period of 10 seconds; whatever is
still in the group after that, including descendants of a parent that has already
exited, gets KILL.
"""
import os
import signal
import subprocess
import sys
import time

GRACE_SECONDS = float(os.environ.get("TIMEBOX_GRACE_SECONDS", "10"))


def group_alive(pgid):
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def main():
    seconds, cmd = float(sys.argv[1]), sys.argv[2:]
    child = subprocess.Popen(cmd, start_new_session=True)
    try:
        return child.wait(timeout=seconds)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(child.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    deadline = time.monotonic() + GRACE_SECONDS
    while time.monotonic() < deadline:
        if child.poll() is not None:
            child.wait()
        # A zombie child still counts as alive to killpg until it is reaped.
        if child.poll() is not None and not group_alive(child.pid):
            return 124
        time.sleep(0.05)
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    child.wait()
    return 124


if __name__ == "__main__":
    sys.exit(main())
