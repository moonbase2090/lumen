#!/usr/bin/env python3
"""Runs a command with a time limit, in its own process group, and kills the whole
group when the limit passes.

    decoder-timebox.py SECONDS COMMAND [ARGS...]

Exits with the command's status, or 124 if it was killed for taking too long.
"""
import os
import signal
import subprocess
import sys

seconds, cmd = float(sys.argv[1]), sys.argv[2:]
child = subprocess.Popen(cmd, start_new_session=True)
try:
    sys.exit(child.wait(timeout=seconds))
except subprocess.TimeoutExpired:
    os.killpg(child.pid, signal.SIGTERM)
    try:
        child.wait(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.wait()
    sys.exit(124)
