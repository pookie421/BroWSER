"""Check that the worker comparator validates retained results before matching."""

import copy
import json
import tempfile
import unittest
from pathlib import Path

from tools import compare_m1_workers as workers
from tools import run_m1_bootstrap as runner


class WorkerEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = runner.ROOT / "planning/m1-cohort.json"
        cls.contract = runner.verify_contract(runner.ROOT, manifest.read_bytes())

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        self.path = self.root / "full-m1-report.json"
        self.log = self.root / "full-m1-report.commands.json"
        binary_hash = runner.digest(b"test-binary")
        inputs = {"src/example.rs": runner.digest(b"test-input")}
        self.report = {
            "run_id": "first", "worker_id": "development-worker", "stage": "full_m1",
            "build_identity_verified": True, "identity_error": None,
            "evidence_kind": "build_bound_full_m1", "cohort": self.contract["id"],
            "cohort_sha256": runner.REVIEWED_COHORT[1], "binary_sha256": binary_hash,
            "asset_sha256": {asset["path"]: asset["sha256"] for asset in self.contract["assets"]},
            "source": {"commit": "test", "worktree_dirty": False},
            "source_sha256": inputs,
            "runner_sha256": runner.digest(b"runner"),
            "runner_support_sha256": {"tools/m1_limits.py": runner.digest(b"helper")},
            "build": {
                "inputs": inputs, "binary_sha256": binary_hash, "compiler": "rustc",
                "cargo": "cargo", "target": "test",
                "flags": ["build", "--target-dir", "<staging>/test"],
                "cargo_configuration": {}, "compiler_overrides": {"RUSTC": "<home>/rustc"},
            },
            "platform": {"os": "Test"},
            "limits": {"wall_time_seconds_per_invocation": 5, "rss_policy": "sampled",
                       "rss_backend": "test", "fixture_rss_limit_bytes": 536870912},
            "summary": {"repetitions_per_case": 100}, "cases": [],
        }
        self.commands = []
        for case in self.contract["cases"]:
            if case["stage"] != "full_m1":
                continue
            identifier = case["id"]
            filenames, operations = workers.case_signature(case)
            hashes, artifacts = {}, {}
            for filename in filenames:
                name = f"cases/{identifier}/run-0001/{filename}"
                path = self.root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f"{identifier} {filename}", encoding="utf-8")
                hashes[filename] = artifacts[name] = runner.file_digest(path)
            self.report["cases"].append({
                "id": identifier,
                "outcome": "review_pending" if "reference_requirement" in case["expected"] else "pass",
                "executions": 100, "attempts": 100 * len(operations), "output_sha256": hashes,
                "diagnostics_sha256": runner.digest(json.dumps([[code, "", ""] for _, code in operations]).encode()),
                "artifacts": artifacts,
            })
            for repetition in range(1, 101):
                for operation, code in operations:
                    self.commands.append({
                        "case": identifier, "operation": operation, "exit_code": code,
                        "outcome": "exit_nonzero" if code else "success", "repetition": repetition,
                        "peak_rss_bytes": 1024, "kernel_peak_rss_bytes": 1024,
                        "sampled_peak_rss_bytes": 900, "limit_trigger": None,
                        "duration_seconds": 0.01, "stdout": "", "stderr": "",
                    })
                self.commands[-1]["output_sha256"] = hashes
        self.write_bundle()

    def write_bundle(self):
        self.log.write_text(json.dumps({"commands": self.commands}), encoding="utf-8")
        self.report["command_log"] = {"file": self.log.name, "sha256": runner.file_digest(self.log)}
        self.path.write_text(json.dumps(self.report), encoding="utf-8")

    def test_compares_all_cases_and_preserves_pending_review_and_worker_identity(self):
        first = workers.load_bundle(self.path, self.contract)
        second = copy.deepcopy(first)
        second["run_id"] = "second"
        result = workers.compare(first, second)
        self.assertEqual(result["matching_cases"], 23)
        self.assertCountEqual(result["review_pending"], ["M1-T004", "M1-T005"])
        self.assertFalse(result["distinct_declared_worker_ids"])

    def test_detects_changed_artifacts_and_missing_commands(self):
        artifact = next(iter(self.report["cases"][0]["artifacts"]))
        path = self.root / artifact
        original = path.read_bytes()
        path.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "artifact hash mismatch"):
            workers.load_bundle(self.path, self.contract)
        path.write_bytes(original)
        self.commands.pop(4)
        self.write_bundle()
        with self.assertRaisesRegex(ValueError, "incomplete or inconsistent repetition hashes"):
            workers.load_bundle(self.path, self.contract)

    def test_rejects_a_zero_exit_with_an_above_limit_peak(self):
        self.commands[0]["peak_rss_bytes"] = 536870913
        self.write_bundle()
        with self.assertRaisesRegex(ValueError, "above-limit peak RSS"):
            workers.load_bundle(self.path, self.contract)

    def test_checks_logged_kernel_peak_and_rejection_diagnostics(self):
        self.commands[0]["kernel_peak_rss_bytes"] = 536870913
        self.write_bundle()
        with self.assertRaisesRegex(ValueError, "inconsistent, or above-limit peak RSS"):
            workers.load_bundle(self.path, self.contract)
        self.commands[0]["kernel_peak_rss_bytes"] = 1024
        reject = next(command for command in self.commands if command["exit_code"] == 1)
        reject["stderr"] = "unrelated denial"
        self.write_bundle()
        with self.assertRaisesRegex(ValueError, "inconsistent repetition diagnostics"):
            workers.load_bundle(self.path, self.contract)

    def test_cannot_mark_raster_review_passed_under_frozen_pending_contract(self):
        case = next(case for case in self.report["cases"] if case["id"] == "M1-T004")
        case["outcome"] = "pass"
        self.write_bundle()
        with self.assertRaisesRegex(ValueError, "M1-T004 requires outcome review_pending"):
            workers.load_bundle(self.path, self.contract)

    def test_rejects_mismatched_outputs_in_matched_environments(self):
        first = workers.load_bundle(self.path, self.contract)
        second = copy.deepcopy(first)
        second["run_id"] = "second"
        second["outcomes"]["M1-H001"]["diagnostics_sha256"] = runner.digest(b"different")
        with self.assertRaisesRegex(ValueError, "hashes differ: M1-H001"):
            workers.compare(first, second)
        second["platform"] = {"os": "different"}
        with self.assertRaisesRegex(ValueError, "reports differ in platform"):
            workers.compare(first, second)
