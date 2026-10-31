#!/usr/bin/env python3
"""Replays one fuzz artifact through the real (non-sanitised) decoder worker and
prints a JSON line with its peak resident size.

    decoder-replay.py WORKER FORMAT ARTIFACT [WALL_LIMIT_SECONDS]

The worker is run as in production: the request has the largest hint the
contract allows, so the reservation is not what limits it, and the worker
enters its sandbox and applies its own limits. The peak is the child's
`ru_maxrss`, normalised to KiB (Linux reports KiB, macOS bytes). It also
records the wall and CPU seconds the worker used.
"""
import json
import os
import resource
import struct
import subprocess
import sys
import time

SIGNATURE = {"png": b"\x89PNG\r\n\x1a\n", "jpeg": b"\xff\xd8\xff", "webp": b"RIFF\0\0\0\0WEBP"}
FORMAT_BYTE = {"png": 1, "jpeg": 2, "webp": 3}
HINT = (4000, 4000)  # 16 megapixels, the largest reservation allowed


def replay(worker, fmt, artifact, wall_limit=30.0):
    """Runs the worker once on one artifact and returns the measurements. With a
    wall limit, a worker still running at the limit is killed and the record says
    `timed_out` (production stops a worker that stalls after 4 s)."""
    data = open(artifact, "rb").read()
    sniffed = {
        "png": data.startswith(SIGNATURE["png"]),
        "jpeg": data.startswith(SIGNATURE["jpeg"]),
        "webp": data[:4] == b"RIFF" and data[8:12] == b"WEBP",
    }
    # The fuzz targets put the signature in front of inputs that lack it.
    if not sniffed[fmt]:
        data = SIGNATURE[fmt] + data
    request = b"LDEC" + bytes([1, FORMAT_BYTE[fmt], 0, 0]) + struct.pack("<IIQ", HINT[0], HINT[1], len(data)) + data
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    started = time.perf_counter()
    timed_out = False
    try:
        done = subprocess.run([worker], input=request, capture_output=True, timeout=wall_limit, env={})
        out, code = done.stdout, done.returncode
    except subprocess.TimeoutExpired as e:
        out, code, timed_out = e.stdout or b"", None, True
    wall = time.perf_counter() - started
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    peak = usage.ru_maxrss
    peak_kib = peak // 1024 if sys.platform == "darwin" else peak
    status = out[4] if len(out) >= 5 and out[:4] == b"LDER" else None
    return {
        "artifact": os.path.basename(artifact),
        "format": fmt,
        "peak_rss_kb": peak_kib,
        "exit": code,
        "status": status,
        "stdout_bytes": len(out),
        "wall_seconds": round(wall, 3),
        # Total over this process's children so far, minus what it was before:
        # exact when replays run one at a time, which is how the CLI uses it.
        "cpu_seconds": round(usage.ru_utime + usage.ru_stime - before.ru_utime - before.ru_stime, 3),
        "timed_out": timed_out,
    }


def main(worker, fmt, artifact, wall_limit="30"):
    print(json.dumps(replay(worker, fmt, artifact, float(wall_limit))))


if __name__ == "__main__":
    main(*sys.argv[1:5])
