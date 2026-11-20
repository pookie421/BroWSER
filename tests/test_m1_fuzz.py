import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest


SPEC = importlib.util.spec_from_file_location(
    "run_m1_fuzz", Path(__file__).resolve().parents[1] / "tools" / "run_m1_fuzz.py",
)
fuzz = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fuzz)


def checksum(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class FuzzCampaignTests(unittest.TestCase):
    def test_source_hashes_bind_dirty_checkout_and_frozen_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            paths = (
                "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "fuzz/Cargo.toml", "fuzz/Cargo.lock",
                "tools/run_m1_fuzz.py", "src/lib.rs", "fuzz/fuzz_targets/html_parse.rs",
                "fixtures/m1/full/seed.html",
            )
            for name in paths:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(name.encode())
            asset = paths[-1]
            manifest = root / fuzz.MANIFEST
            manifest.parent.mkdir()
            manifest.write_text(json.dumps({"assets": [{"path": asset, "sha256": checksum(asset.encode())}]}))
            first = fuzz.source_hashes(root)
            self.assertEqual(set(first), {*paths, fuzz.MANIFEST.as_posix()})

            (root / "src/lib.rs").write_bytes(b"edited engine")
            second = fuzz.source_hashes(root)
            self.assertNotEqual(first["src/lib.rs"], second["src/lib.rs"])
            (root / asset).write_bytes(b"edited fixture")
            with self.assertRaisesRegex(ValueError, "frozen M1 asset"):
                fuzz.source_hashes(root)

    def test_seeds_are_frozen_and_renderer_headers_match_the_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            sources = {
                "fixtures/m1/bootstrap/seed.html": b"<div></div>",
                "fixtures/m1/full/seed.html": b"<div>Text</div>",
                "fixtures/m1/assets/checker.png": b"unused as an HTML seed",
            }
            for filename, content in sources.items():
                path = root / filename
                path.parent.mkdir(parents=True)
                path.write_bytes(content)
            identities = {name: checksum(data) for name, data in sources.items()}
            rendered = fuzz.seed_corpus(root, root / "renderer", "render_fixture", identities)
            self.assertEqual(rendered, {
                "bootstrap-seed.html": checksum(b"\x00\x40\x40<div></div>"),
                "full-seed.html": checksum(b"\x01\x40\x40<div>Text</div>"),
            })
            parsed = fuzz.seed_corpus(root, root / "parser", "html_parse", identities)
            self.assertEqual(parsed["full-seed.html"], checksum(b"<div>Text</div>"))

    def test_normal_exit_requires_positive_final_coverage_and_requested_duration(self):
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary)
            (target / "failures").mkdir()
            log = target / "fuzzer.log"
            log.write_text(
                "INFO: Loaded 1 modules   (40 inline 8-bit counters)\n"
                "#100\tDONE   cov: 9 ft: 26 corp: 12/1Kb\n"
                "Done 100 runs in 2 second(s)\n"
                "stat::number_of_executed_units: 100\n"
            )
            child = {"metrics": {"exit_code": 0, "status": "running", "peak_rss_bytes": 16 * 1024**2}}
            fuzz.enforce_evidence(child, target, 100, None)
            self.assertEqual(child["metrics"]["status"], "complete")
            self.assertEqual(child["metrics"]["libfuzzer"]["covered_edges"], 9)
            with self.assertRaisesRegex(ValueError, "requested time window"):
                fuzz.enforce_evidence(child, target, None, 3)
            (target / "failures" / "crash-input").write_bytes(b"synthetic test artifact")
            with self.assertRaisesRegex(ValueError, "produced a failure"):
                fuzz.enforce_evidence(child, target, 100, None)
            (target / "failures" / "crash-input").unlink()
            log.write_text("stat::number_of_executed_units: 100\n")
            with self.assertRaisesRegex(ValueError, "no complete positive coverage"):
                fuzz.enforce_evidence(child, target, 100, None)
            child["metrics"]["peak_rss_bytes"] = 513 * 1024**2
            with self.assertRaisesRegex(ValueError, "over-limit process RSS"):
                fuzz.enforce_evidence(child, target, 100, None)

    @unittest.skipUnless(hasattr(fuzz.os, "wait4"), "requires wait4")
    def test_watchdog_child_cleanup_reaps_and_records_signal(self):
        process = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"], start_new_session=True)
        metrics = {}
        started = time.monotonic()
        child = {"process": process, "metrics": metrics, "started": started, "last_seen_alive": started}
        try:
            self.assertFalse(fuzz.collect_process(child, fuzz.os.WNOHANG))
        finally:
            fuzz.stop(process)
            self.assertTrue(fuzz.collect_process(child, 0))
        self.assertEqual(metrics["exit_code"], -fuzz.signal.SIGKILL)
        self.assertGreaterEqual(metrics["cpu_seconds"], 0)
        self.assertLess(metrics["observed_wall_seconds"], 30)

    def test_missing_source_retains_incomplete_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            run = root / "run"
            run.mkdir()
            report = fuzz.run_campaign(root, run, argparse.Namespace(runs=100, seconds=None))
            self.assertEqual(report["status"], "incomplete")
            self.assertFalse(report["seven_day_window_observed"])
            self.assertEqual(json.loads((run / "report.json").read_text()), report)


if __name__ == "__main__":
    unittest.main()
