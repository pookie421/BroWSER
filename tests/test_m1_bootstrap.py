"""Negative tests for evidence identity and independently specified pixel oracles."""

import copy
import importlib.util
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("m1_bootstrap", Path(__file__).resolve().parents[1] / "tools/run_m1_bootstrap.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class EvidenceTests(unittest.TestCase):
    def test_asset_hash_changes_and_missing_inputs_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "input.html").write_bytes(b"<div></div>")
            contract = {"schema_version": 1, "id": "M1-v1", "assets": [{"path": "input.html", "bytes": 11, "sha256": runner.digest(b"<div></div>")}], "cases": [{"id": "B1", "stage": "bootstrap", "input": "input.html"}, {"id": "R1", "stage": "full_m1", "input": None}]}
            runner.verify_contract(root, contract)
            (root / "input.html").write_bytes(b"<div>changed</div>")
            with self.assertRaisesRegex(ValueError, "identity mismatch"):
                runner.verify_contract(root, contract)

    def test_paths_cannot_escape_fixture_root(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                runner.local_path(Path(directory), "../escape.html")

    def test_malformed_ppm_is_not_valid_pixel_evidence(self):
        with self.assertRaisesRegex(ValueError, "payload"):
            runner.ppm_pixels(b"P6\n1 1\n255\n\xff", 1, 1)

    def test_wrong_pixels_and_geometry_fail_independent_oracle(self):
        case = {"viewport": {"width": 1, "height": 1}, "expected": {"pixels": [[0, 0, 255, 0, 0]], "boxes": {"a": {"content": [0, 0, 1, 1]}}}}
        image = b"P6\n1 1\n255\n\xff\x00\x00"
        layout = {"viewport": case["viewport"], "boxes": [{"node": 1, "content_box": {"x": 0, "y": 0, "width": 1, "height": 1}}]}
        dom = {"nodes": [{"id": 1, "attributes": {"id": "a"}}]}
        runner.assert_render(case, image, layout, dom)
        with self.assertRaisesRegex(ValueError, "pixel oracle mismatch"):
            runner.assert_render(case, b"P6\n1 1\n255\n\x00\x00\x00", layout, dom)
        wrong = copy.deepcopy(layout)
        wrong["boxes"][0]["content_box"]["x"] = 1
        with self.assertRaisesRegex(ValueError, "geometry oracle mismatch"):
            runner.assert_render(case, image, wrong, dom)
