#!/usr/bin/env python3
"""The unsafe-code report of the dependency audit gate.

    unsafe-report.py write            regenerate docs/phase-1/unsafe-report.json
    unsafe-report.py check            compare a fresh report with the committed one

For each root crate on the frame, key and parser paths it runs the unsafe-code
counter over the whole dependency tree for the Linux target (so the numbers do not
depend on the host) and keeps, per package: version, where it comes from, the
count of unsafe items in the crate's source (the counter's "used" and "unused" added up: its split depends on the host), and whether the crate
forbids unsafe code. `check` fails on any difference, so a new dependency with
unsafe code, a bumped version, or a crate that stops forbidding unsafe is
looked at by a person before the report is regenerated and committed.
"""
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPORT = os.path.join(ROOT, "docs", "phase-1", "unsafe-report.json")
TARGET = "x86_64-unknown-linux-gnu"
# Frame path, key path, parser path (see docs/phase-1/dependency-audit.md).
ROOTS = ["lumen-detector", "lumen-seal", "lumen-sealed", "lumen-decoder", "lumen-term"]


def scan(crate):
    out = subprocess.run(
        ["cargo", "geiger", "--manifest-path", os.path.join(ROOT, "crates", crate, "Cargo.toml"),
         "--all-features", "--target", TARGET, "--output-format", "Json", "--locked"],
        capture_output=True, text=True, cwd=ROOT,
    )
    if out.returncode != 0 or not out.stdout.strip():
        sys.exit(f"the unsafe counter failed for {crate}:\n{out.stderr[-2000:]}")
    packages = {}
    for p in json.loads(out.stdout)["packages"]:
        ident = p["package"]["id"]
        used, unused = p["unsafety"]["used"], p["unsafety"]["unused"]
        source = ident["source"]
        kind = next(iter(source)) if isinstance(source, dict) else str(source)
        packages[f'{ident["name"]} {ident["version"]}'] = {
            "source": kind,
            # Used plus unused: the split depends on the host the counter runs on,
            # the total (every unsafe item in the crate's source files) does not.
            "unsafe_items": {k: used[k]["unsafe_"] + unused[k]["unsafe_"] for k in sorted(used)},
            "forbids_unsafe": p["unsafety"]["forbids_unsafe"],
        }
    return dict(sorted(packages.items()))


def fresh():
    return {"target": TARGET, "roots": {c: scan(c) for c in ROOTS}}


def differences(old, new):
    """Every difference between two reports: the target, the set of root crates
    (a root dropped from the list is a difference, not a skipped scan), and each
    package of each root."""
    diffs = []
    if old.get("target") != new.get("target"):
        diffs.append(f"target: committed {old.get('target')} -> now {new.get('target')}")
    old_roots, new_roots = old.get("roots", {}), new.get("roots", {})
    for c in sorted(set(old_roots) | set(new_roots)):
        if c not in new_roots:
            diffs.append(f"{c}: committed, not scanned now")
        elif c not in old_roots:
            diffs.append(f"{c}: scanned now, not in the committed report")
        else:
            a, b = old_roots[c], new_roots[c]
            for k in sorted(set(a) | set(b)):
                if a.get(k) != b.get(k):
                    diffs.append(f"{c}: {k}: committed {a.get(k)} -> now {b.get(k)}")
    return diffs


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "write":
        with open(REPORT, "w") as fh:
            json.dump(fresh(), fh, indent=1, sort_keys=True)
            fh.write("\n")
        print("wrote", os.path.relpath(REPORT, ROOT))
    elif mode == "check":
        diffs = differences(json.load(open(REPORT)), fresh())
        if diffs:
            print("the unsafe-code report changed; review each line, then run `scripts/unsafe-report.py write`:")
            print("\n".join(diffs))
            sys.exit(1)
        print("unsafe-code report: unchanged")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
