#!/usr/bin/env python3
"""Replays one fuzz artifact through the real (non-sanitised) decoder worker and
prints a JSON line with its peak resident size.

    decoder-replay.py WORKER FORMAT ARTIFACT

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
HINT = (8000, 4000)  # 32 megapixels, the largest reservation allowed


def main(worker, fmt, artifact):
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
    started = time.perf_counter()
    done = subprocess.run([worker], input=request, capture_output=True, timeout=30, env={})
    wall = time.perf_counter() - started
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    peak = usage.ru_maxrss
    peak_kib = peak // 1024 if sys.platform == "darwin" else peak
    out = done.stdout
    status = out[4] if len(out) >= 5 and out[:4] == b"LDER" else None
    print(json.dumps({
        "artifact": os.path.basename(artifact),
        "format": fmt,
        "peak_rss_kb": peak_kib,
        "exit": done.returncode,
        "status": status,
        "stdout_bytes": len(out),
        "wall_seconds": round(wall, 3),
        "cpu_seconds": round(usage.ru_utime + usage.ru_stime, 3),
    }))


if __name__ == "__main__":
    main(*sys.argv[1:4])
