#!/usr/bin/env python3
"""Writes WORKDIR/miri-record.json from the logs and runs.tsv that
scripts/decoder-miri.sh leaves in WORKDIR. It can be re-run on existing logs.

    decoder-miri-record.py WORKDIR REPO_ROOT UPSTREAM_COMMIT NIGHTLY
"""
import datetime
import json
import re
import subprocess
import sys

work, root, upstream, nightly = sys.argv[1:5]


def sh(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout.strip()


runs = []
for line in open(f"{work}/runs.tsv"):
    name, status, cmd = line.rstrip("\n").split("\t", 2)
    log = open(f"{work}/{name}.log", errors="replace").read().splitlines()
    # Timings are removed so a later run can be compared with this one.
    results = [re.sub(r"; finished in [0-9.]+s", "", l.strip()) for l in log if l.startswith("test result:")]
    # Miri prints its error on the same line as the test name, so search inside lines.
    errors = [re.sub(r"^.*?error: ", "error: ", l.strip()) for l in log
              if "error: unsupported operation" in l or "Undefined Behavior" in l]
    if not errors:
        # An ordinary test failure: keep the panic line and the assertion after it.
        for n, l in enumerate(log):
            if "panicked at" in l:
                errors = [(l.strip().split(") panicked at ", 1)[-1] + " | " + " ".join(x.strip() for x in log[n + 1:n + 4]))]
                break
    if status == "TIMEOUT":
        errors = [f"did not finish within its time box ({cmd.split('timebox ')[1].split('s:')[0]} s); Miri is slow, and this says nothing about the code"]
    runs.append({
        "name": name,
        "status": status,
        "command": cmd.replace("\t", " "),
        "results": results,
        "first_error": errors[0][:300] if status != "ok" and errors else None,
    })

record = {
    "toolchain": sh("rustc", f"+{nightly}", "-V"),
    "miri": sh("cargo", f"+{nightly}", "miri", "--version"),
    "host": sh("uname", "-srm"),
    "date": datetime.date.today().isoformat(),
    "repo_commit": sh("git", "-C", root, "rev-parse", "HEAD"),
    "upstream_commit": upstream,
    "zune_jpeg_version": open(f"{work}/zune-image/crates/zune-jpeg/Cargo.toml").read().split('version = "')[1].split('"')[0],
    "runs": runs,
}
with open(f"{work}/miri-record.json", "w") as f:
    json.dump(record, f, indent=2)
    f.write("\n")
print(f"record: {work}/miri-record.json")
