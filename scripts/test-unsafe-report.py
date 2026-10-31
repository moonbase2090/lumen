#!/usr/bin/env python3
"""Tests the comparison of unsafe-report.py (no counter is run)."""
import copy
import importlib.util
import os
import unittest

spec = importlib.util.spec_from_file_location("unsafe_report", os.path.join(os.path.dirname(__file__), "unsafe-report.py"))
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

PKG = {"source": "Registry", "unsafe_items": {"exprs": 3}, "forbids_unsafe": False}
BASE = {"target": "x86_64-unknown-linux-gnu",
        "roots": {"a": {"p 1.0.0": PKG}, "b": {"q 2.0.0": dict(PKG, forbids_unsafe=True)}}}


class Compare(unittest.TestCase):
    def test_identical_reports_have_no_differences(self):
        self.assertEqual(mod.differences(BASE, copy.deepcopy(BASE)), [])

    def test_a_changed_target_is_a_difference(self):
        new = copy.deepcopy(BASE)
        new["target"] = "aarch64-apple-darwin"
        self.assertEqual(len(mod.differences(BASE, new)), 1)
        self.assertIn("target", mod.differences(BASE, new)[0])

    def test_a_root_dropped_from_the_scan_is_a_difference(self):
        new = copy.deepcopy(BASE)
        del new["roots"]["b"]
        d = mod.differences(BASE, new)
        self.assertEqual(len(d), 1)
        self.assertIn("committed, not scanned now", d[0])

    def test_a_root_added_to_the_scan_is_a_difference(self):
        new = copy.deepcopy(BASE)
        new["roots"]["c"] = {}
        self.assertIn("not in the committed report", mod.differences(BASE, new)[0])

    def test_package_changes_are_differences(self):
        for change in (
            lambda r: r["roots"]["a"].update({"p 1.0.1": PKG}),                  # a new version
            lambda r: r["roots"]["a"].pop("p 1.0.0"),                            # a package gone
            lambda r: r["roots"]["a"]["p 1.0.0"]["unsafe_items"].update(exprs=4),  # more unsafe
            lambda r: r["roots"]["b"]["q 2.0.0"].update(forbids_unsafe=False),   # stops forbidding
        ):
            new = copy.deepcopy(BASE)
            change(new)
            self.assertNotEqual(mod.differences(BASE, new), [])

    def test_the_committed_report_names_exactly_the_configured_roots(self):
        import json
        committed = json.load(open(mod.REPORT))
        self.assertEqual(sorted(committed["roots"]), sorted(mod.ROOTS))
        self.assertEqual(committed["target"], mod.TARGET)


if __name__ == "__main__":
    unittest.main()
