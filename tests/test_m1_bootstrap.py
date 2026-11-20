"""Regression coverage for bootstrap oracles and retained evidence."""

import argparse
import copy
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from tools import run_m1_bootstrap as runner
MANIFEST = runner.ROOT / "planning/m1-cohort.json"
RED_IMAGE = b"P6\n1 1\n255\n\xff\x00\x00"
DOM = {
    "root": 0,
    "nodes": [
        {"id": 0, "parent": None, "type": "document", "children": [1, 2]},
        {"id": 1, "parent": 0, "type": "doctype", "name": "html", "children": []},
        {"id": 2, "parent": 0, "type": "element", "name": "div", "children": [], "attributes": {"id": "a"}},
    ],
}
RECT = {"x": 0, "y": 0, "width": 1, "height": 1}
LAYOUT = {
    "viewport": {"width": 1, "height": 1},
    "boxes": [{"node": 2, "tag": "div", "content_box": RECT.copy(), "border_box": RECT.copy(),
               "clip": RECT.copy(), "background": "#ff0000"}],
}
RENDER_CASE = {
    "id": "M1-X001", "kind": "render", "input": "render.html", "viewport": LAYOUT["viewport"],
    "expected": {"pixels": [[0, 0, 255, 0, 0]], "boxes": {"a": {"content": [0, 0, 1, 1]}}},
}
REJECTION_CASE = {
    "id": "M1-X002", "kind": "reject", "input": "reject.html", "control": "control.html",
    "viewport": LAYOUT["viewport"], "expected": {"exit": 1, "stderr": "browser-engine: intended diagnostic"},
}


def completed(code=0, stderr=b"", stdout=b""):
    return subprocess.CompletedProcess([], code, stdout, stderr)


def produce_artifacts(command, *, image=RED_IMAGE, layout=LAYOUT, dom=DOM):
    output = Path(command[command.index("--output") + 1])
    if command[1] == "inspect":
        output.write_text(json.dumps(dom))
    else:
        output.write_bytes(image)
        Path(command[command.index("--layout") + 1]).write_text(json.dumps(layout))
    return completed()


class ContractTests(unittest.TestCase):
    def test_current_cohort_has_full_expected_roster(self):
        contract = runner.verify_contract(runner.ROOT, MANIFEST.read_bytes())
        self.assertEqual(contract["id"], "M1-v2")
        self.assertEqual(len(contract["assets"]), 37)
        self.assertEqual(sum(case["stage"] == "bootstrap" for case in contract["cases"]), 5)
        self.assertEqual(sum(case["stage"] == "full_m1" for case in contract["cases"]), 23)

    def test_changed_case_asset_stage_or_oracle_fails_under_same_revision(self):
        for mutation in (
            lambda contract: contract["cases"].pop(),
            lambda contract: contract["assets"].pop(),
            lambda contract: contract["cases"][0].update(stage="full_m1"),
            lambda contract: contract["cases"][3]["expected"].update(exit=2),
        ):
            contract = json.loads(MANIFEST.read_bytes())
            mutation(contract)
            with self.assertRaisesRegex(ValueError, "differ from the reviewed revision"):
                runner.verify_contract(runner.ROOT, json.dumps(contract).encode())

    def test_duplicate_json_key_and_boolean_version_fail(self):
        duplicated = MANIFEST.read_bytes().replace(b'"schema_version": 1,', b'"schema_version": 1, "schema_version": 1,', 1)
        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            runner.verify_contract(runner.ROOT, duplicated)
        wrong = MANIFEST.read_bytes().replace(b'"schema_version": 1,', b'"schema_version": true,', 1)
        with self.assertRaisesRegex(ValueError, "unsupported cohort identity"):
            runner.verify_contract(runner.ROOT, wrong)

    def test_missing_hash_coverage_fails_even_with_new_digest(self):
        contract = json.loads(MANIFEST.read_bytes())
        control = contract["cases"][3]["control"]
        contract["assets"] = [asset for asset in contract["assets"] if asset["path"] != control]
        data = json.dumps(contract).encode()
        with patch.object(runner, "REVIEWED_COHORT", (contract["id"], runner.digest(data))):
            with self.assertRaisesRegex(ValueError, "unhashed fixture inputs"):
                runner.verify_contract(runner.ROOT, data)

    def test_changed_asset_bytes_fail_even_with_reviewed_manifest(self):
        data = MANIFEST.read_bytes()
        path = json.loads(data)["assets"][0]["path"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / path).parent.mkdir(parents=True)
            (root / path).write_bytes(b"<div>changed</div>")
            with self.assertRaisesRegex(ValueError, "asset identity mismatch"):
                runner.verify_contract(root, data)

    def test_paths_cannot_escape_fixture_root(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                runner.local_path(Path(directory), "../escape.html")


class OracleTests(unittest.TestCase):
    def test_rss_violation_remains_a_failure_after_a_zero_exit(self):
        limited = SimpleNamespace(
            returncode=0, stdout=b"output", stderr=b"", termination="resource_limit",
            trigger="peak_rss", peak_rss_bytes=33, kernel_peak_rss_bytes=33,
            sampled_peak_rss_bytes=30,
        )
        commands = []
        with patch.object(runner.m1_limits, "run_limited", return_value=limited) as invoke:
            with self.assertRaisesRegex(runner.ReplayError, "stopped by fixture limits") as error:
                runner.invoke(commands, ["engine"], 5, case="M1-B001", operation="render",
                              repetition=1, rss_limit_bytes=32)
        self.assertEqual(error.exception.outcome, "resource_limit")
        self.assertEqual(invoke.call_args.kwargs["rss_limit_bytes"], 32)
        record, = commands
        self.assertEqual(record["exit_code"], 0)
        self.assertEqual(record["outcome"], "resource_limit")
        self.assertEqual(record["kernel_peak_rss_bytes"], 33)
        self.assertEqual(record["stdout"], "output")

    def test_fixture_process_gets_a_fresh_allowlisted_environment(self):
        homes = []

        def invoke(command, **kwargs):
            env = kwargs["env"]
            self.assertEqual(env["LC_ALL"], "C")
            self.assertEqual(env["TZ"], "UTC")
            self.assertNotIn("M1_AMBIENT_TEST_VALUE", env)
            home = Path(env["HOME"])
            self.assertEqual(home, Path(env["TMPDIR"]))
            self.assertEqual(list(home.iterdir()), [])
            homes.append(home)
            (home / "transient").write_text("process-local")
            return produce_artifacts(command)

        with tempfile.TemporaryDirectory() as directory:
            with patch.dict(runner.os.environ, {"M1_AMBIENT_TEST_VALUE": "outside"}):
                with patch.object(runner.subprocess, "run", side_effect=invoke):
                    runner.execute_case(Path("engine"), RENDER_CASE, 2, 5, Path(directory), [])
            self.assertEqual(len(set(homes)), 3)
            self.assertTrue(all(not home.exists() for home in homes))

    def test_malformed_ppm_is_not_valid_pixel_evidence(self):
        with self.assertRaisesRegex(ValueError, "payload"):
            runner.ppm_pixels(b"P6\n1 1\n255\n\xff", 1, 1)

    def test_pixel_and_geometry_assertions_are_independent(self):
        runner.assert_render(RENDER_CASE, RED_IMAGE, LAYOUT, DOM)
        with self.assertRaisesRegex(ValueError, "pixel oracle mismatch"):
            runner.assert_render(RENDER_CASE, b"P6\n1 1\n255\n\x00\x00\x00", LAYOUT, DOM)
        wrong = copy.deepcopy(LAYOUT)
        wrong["boxes"][0]["content_box"]["x"] = 1
        with self.assertRaisesRegex(ValueError, "geometry oracle mismatch"):
            runner.assert_render(RENDER_CASE, RED_IMAGE, wrong, DOM)
        pixel_only = dict(RENDER_CASE, expected={"pixels": RENDER_CASE["expected"]["pixels"]})
        runner.assert_render(pixel_only, RED_IMAGE, LAYOUT, DOM)

    def test_layout_rejects_duplicates_bool_fields_and_bad_references(self):
        for expected, mutation in (
            ("duplicate boxes", lambda layout: layout["boxes"].append(copy.deepcopy(layout["boxes"][0]))),
            ("rectangle", lambda layout: layout["boxes"][0]["content_box"].update(x=False)),
            ("rectangle", lambda layout: layout["boxes"][0]["content_box"].pop("width")),
            ("invalid DOM element", lambda layout: layout["boxes"][0].update(node=1)),
            ("invalid DOM element", lambda layout: layout["boxes"][0].update(node=99)),
            ("invalid DOM element", lambda layout: layout["boxes"][0].update(tag="p")),
            ("invalid layout viewport", lambda layout: layout["viewport"].update(width=True)),
        ):
            with self.subTest(expected=expected):
                layout = copy.deepcopy(LAYOUT)
                mutation(layout)
                with self.assertRaisesRegex(runner.ReplayError, expected) as error:
                    runner.assert_render(RENDER_CASE, RED_IMAGE, layout, DOM)
                self.assertEqual(error.exception.outcome, "harness_error")

    def test_dom_rejects_duplicate_nodes_and_bad_relationships(self):
        for expected, mutation in (
            ("duplicate node ID", lambda dom: dom["nodes"].append(copy.deepcopy(dom["nodes"][1]))),
            ("invalid DOM root", lambda dom: dom.update(root=False)),
            ("reference", lambda dom: dom["nodes"][0]["children"].append(999)),
            ("reference", lambda dom: dom["nodes"][2].update(parent=2)),
            ("duplicate child", lambda dom: dom["nodes"][0]["children"].append(2)),
            ("unreachable", lambda dom: dom["nodes"][0]["children"].pop()),
        ):
            with self.subTest(expected=expected):
                dom = copy.deepcopy(DOM)
                mutation(dom)
                with self.assertRaisesRegex(runner.ReplayError, expected):
                    runner.assert_render(RENDER_CASE, RED_IMAGE, LAYOUT, dom)

    def test_reject_requires_exact_outcome_and_control(self):
        exact = b"browser-engine: intended diagnostic\n"
        for result in (
            completed(1, b"browser-engine: unsupported feature X\n"),
            completed(1, b"browser-engine: cannot resolve fixture: missing\n"),
            completed(1, b"browser-engine: --width must be an integer in 1..=2048\n"),
            completed(1, b"unexpected prefix\n" + exact),
            completed(1, exact, b"unexpected stdout"),
            completed(2, exact),
            completed(0),
        ):
            with self.subTest(result=result), tempfile.TemporaryDirectory() as directory:
                with patch.object(runner.subprocess, "run", return_value=result) as invoke:
                    with self.assertRaisesRegex(ValueError, "wrong rejection outcome"):
                        runner.execute_case(Path("engine"), REJECTION_CASE, 1, 5, Path(directory), [])
                    self.assertEqual(invoke.call_count, 1)
        with tempfile.TemporaryDirectory() as directory:
            def run(command, **kwargs):
                if "reject.html" in command:
                    return completed(1, exact)
                return produce_artifacts(command)

            with patch.object(runner.subprocess, "run", side_effect=run):
                actual = runner.execute_case(Path("engine"), REJECTION_CASE, 1, 5, Path(directory), [])
                self.assertEqual(actual["control"], "pass")
            with patch.object(runner.subprocess, "run", side_effect=[completed(1, exact), completed(1, b"bad control")]):
                with self.assertRaisesRegex(ValueError, "control-inspect exited 1"):
                    runner.execute_case(Path("engine"), REJECTION_CASE, 1, 5, Path(directory), [])

    def test_timeout_crash_harness_error_and_failure_retain_other_cases(self):
        variants = ("timeout", "crash", "harness_error", "fail", "pass")
        cases = [dict(RENDER_CASE, id=f"M1-X{index:03}", input=variant) for index, variant in enumerate(variants, start=1)]

        def run(command, **kwargs):
            self.assertEqual(kwargs["timeout"], 5)
            fixture = command[command.index("--fixture") + 1]
            if command[1] == "inspect":
                return produce_artifacts(command)
            if fixture == "timeout":
                produce_artifacts(command)
                raise subprocess.TimeoutExpired(command, 5, output=b"progress", stderr=b"partial diagnostic")
            if fixture == "crash":
                return completed(101, b"panic")
            if fixture == "harness_error":
                return produce_artifacts(command, layout=[])
            if fixture == "fail":
                return produce_artifacts(command, image=b"P6\n1 1\n255\n\x00\x00\x00")
            return produce_artifacts(command)

        with tempfile.TemporaryDirectory() as directory:
            case_directory = Path(directory) / "cases"
            case_directory.mkdir()
            commands = []
            with patch.object(runner.subprocess, "run", side_effect=run):
                results = runner.execute_cases(Path("engine"), cases, 2, 5, case_directory, commands)
            self.assertEqual([case["outcome"] for case in results], list(variants))
            self.assertEqual([case["attempts"] for case in results], [2, 2, 2, 2, 3])
            self.assertEqual(commands[1]["stdout"], "progress")
            self.assertEqual(commands[1]["stderr"], "partial diagnostic")
            self.assertEqual(commands[1]["repetition"], 1)
            self.assertEqual(commands[-1]["repetition"], 2)
            self.assertTrue(all(command["duration_seconds"] >= 0 for command in commands))
            self.assertIn("cases/M1-X001/1.ppm", results[0]["artifacts"])
            self.assertIn("cases/M1-X002/dom.json", results[1]["artifacts"])
            self.assertIn("cases/M1-X003/1.json", results[2]["artifacts"])
            self.assertFalse((case_directory / "M1-X005/2.ppm").exists())
            for result in results:
                for path, checksum in result["artifacts"].items():
                    self.assertEqual(runner.file_digest(Path(directory) / path), checksum)

    def test_missing_subprocess_is_a_recorded_harness_error(self):
        commands = []
        with patch.object(runner.subprocess, "run", side_effect=FileNotFoundError("missing")):
            with self.assertRaisesRegex(runner.ReplayError, "cannot start inspect"):
                runner.invoke(commands, ["engine", "inspect"], 5, case="M1-B001", operation="inspect", repetition=1)
        self.assertEqual(commands[0]["outcome"], "harness_error")
        self.assertEqual(commands[0]["stderr"], "missing")


class FullCohortTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        contract = runner.verify_contract(runner.ROOT, MANIFEST.read_bytes())
        cls.cases = {case["id"]: case for case in contract["cases"]}

    def test_full_shape_requires_utf8_clusters_and_exact_frozen_glyphs(self):
        case = self.cases["M1-T002"]
        reference = runner.glyph_reference(case)
        outcomes = {
            "pass": reference,
            "fail": dict(reference, glyphs=[dict(reference["glyphs"][0], ax=1), *reference["glyphs"][1:]]),
            "harness_error": dict(reference, glyphs=[dict(reference["glyphs"][0], cl=True), *reference["glyphs"][1:]]),
        }
        for outcome, response in outcomes.items():
            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as directory:
                cases = Path(directory)
                commands = []

                def invoke(command, **kwargs):
                    self.assertEqual(command[1], "shape")
                    self.assertEqual(Path(command[command.index("--fixture") + 1]).read_bytes(), case["text"].encode())
                    self.assertEqual(command[command.index("--direction") + 1], case["direction"])
                    self.assertEqual(command[command.index("--scale") + 1], str(reference["font_scale"]))
                    Path(command[command.index("--output") + 1]).write_text(json.dumps(response))
                    return completed()

                with patch.object(runner.subprocess, "run", side_effect=invoke):
                    result, = runner.execute_cases(Path("engine"), [case], 2, 5, cases, commands)
                self.assertEqual(result["outcome"], outcome)
                self.assertEqual(result["executions"], 2 if outcome == "pass" else 1)
                self.assertTrue((cases / "M1-T002/run-0001/shape.json").is_file())
                self.assertFalse((cases / "M1-T002/run-0002").exists())

    def test_over_limit_reject_requires_matching_diagnostic_and_no_artifact(self):
        case = self.cases["M1-R002"]
        largest = case["values"][-1]
        right = completed(1, b"browser-engine: byte 110000: node count exceeds 10000 limit\n")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "dom.json"
            runner.assert_boundary(case, largest, right, output)
            for result in (completed(0), completed(1, b"cannot resolve fixture"), completed(2, right.stderr)):
                with self.assertRaisesRegex(ValueError, "expected bounded rejection"):
                    runner.assert_boundary(case, largest, result, output)
            output.write_bytes(b"{}")
            with self.assertRaisesRegex(ValueError, "expected bounded rejection"):
                runner.assert_boundary(case, largest, right, output)

    def test_full_replay_retains_well_formed_nondeterministic_output(self):
        case = self.cases["M1-T001"]
        ref = runner.glyph_reference(case)
        n = 0

        def invoke(command, **kwargs):
            nonlocal n
            n += 1
            Path(command[command.index("--output") + 1]).write_text(json.dumps(ref, indent=n))
            return completed()

        with tempfile.TemporaryDirectory() as directory:
            with patch.object(runner.subprocess, "run", side_effect=invoke):
                result, = runner.execute_cases(Path("engine"), [case], 2, 5, Path(directory), [])
            self.assertEqual(result["outcome"], "fail")
            self.assertIn("nondeterministic output at repetition 2", result["error"])
            self.assertTrue((Path(directory) / "M1-T001/run-0002/shape.json").is_file())

    def test_text_raster_with_correct_glyphs_still_requires_review(self):
        case = dict(self.cases["M1-T004"], viewport=LAYOUT["viewport"])
        ref = runner.glyph_reference(case)
        dom = copy.deepcopy(DOM)
        dom["nodes"][2]["children"] = [3]
        dom["nodes"].append({"id": 3, "type": "text", "parent": 2, "children": [], "text": ref["text"]})
        rtl_origin = 64 - sum(glyph["ax"] for glyph in ref["glyphs"])
        layout = dict(LAYOUT, text=[{"node": 2, "inline": {
            "collapsed_text": ref["text"], "line_height_px": 1, "content_height_px": 1,
            "lines": [{"x_64": rtl_origin, "baseline_64": 64, "run": ref}],
        }}])
        with tempfile.TemporaryDirectory() as directory:
            commands = []
            with patch.object(runner.subprocess, "run",
                              side_effect=lambda command, **kwargs: produce_artifacts(command, layout=layout, dom=dom)) as invoke:
                result, = runner.execute_cases(Path("engine"), [case], 2, 5, Path(directory), commands)
            self.assertEqual(result["outcome"], "review_pending")
            self.assertEqual(result["executions"], 2)
            self.assertEqual(invoke.call_count, 4)
            self.assertTrue((Path(directory) / "M1-T004/run-0001/render.ppm").is_file())
            self.assertFalse((Path(directory) / "M1-T004/run-0002").exists())

    def test_full_stage_uses_separate_denominator_and_review_is_not_a_pass(self):
        contract = runner.verify_contract(runner.ROOT, MANIFEST.read_bytes())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "binary"
            binary.write_bytes(b"binary")
            args = argparse.Namespace(binary=binary, cargo="cargo", run_id="full", repeat=2, stage="full-m1", rss_policy="off")

            def execute(binary, selected, repeat, timeout, directory, commands, **kwargs):
                self.assertEqual(len(selected), 23)
                self.assertTrue(all(case["stage"] == "full_m1" for case in selected))
                return [{"id": case["id"],
                         "outcome": "review_pending" if case["id"] in ("M1-T004", "M1-T005") else "pass"}
                        for case in selected]

            with patch.object(runner, "execute_cases", side_effect=execute):
                report, passed = runner.run_bundle(args, contract, MANIFEST, root, runner.source_identity())
            self.assertFalse(passed)
            self.assertEqual(report["summary"]["full_m1_passed"], 21)
            self.assertEqual(report["summary"]["full_m1_outcomes"]["review_pending"], 2)
            self.assertEqual(report["summary"]["bootstrap_not_run"], 5)
            self.assertTrue((root / "full-m1-report.json").is_file())
            self.assertTrue((root / "full-m1-report.commands.json").is_file())


class BundleTests(unittest.TestCase):
    def test_publication_is_complete_unique_and_never_replaces_existing_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for run_id in ("one", "two"):
                with runner.stage_bundle(root, run_id) as (stage, destination):
                    self.assertFalse(destination.exists())
                    (stage / "report.json").write_text(run_id)
                    (stage / "commands.json").write_text("commands")
                self.assertEqual((destination / "report.json").read_text(), run_id)
                self.assertEqual((destination / "commands.json").read_text(), "commands")
            with self.assertRaises(FileExistsError):
                with runner.stage_bundle(root, "one"):
                    self.fail("must not stage over an existing bundle")
            self.assertEqual((root / "one/report.json").read_text(), "one")
            self.assertEqual(sorted(path.name for path in root.iterdir()), ["one", "two"])

    def test_concurrent_reservation_leaves_primary_run_intact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with runner.stage_bundle(root, "same") as (stage, destination):
                (stage / "report.json").write_text("first")
                with self.assertRaises(FileExistsError):
                    with runner.stage_bundle(root, "same"):
                        self.fail("the first run holds the reservation")
            self.assertEqual((destination / "report.json").read_text(), "first")
            self.assertEqual(list(root.iterdir()), [destination])

    def test_recorded_paths_retain_meaningful_components_without_account_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence_root = Path(directory)
            stage = evidence_root / ".staging"
            target = runner.ROOT / "artifacts/custom-target"
            executable = target / "fresh/debug/browser-engine"
            normalize = runner.record_paths(stage, target)
            record = normalize({
                "arguments": [str(executable), str(stage / "cases/fixture.json")],
                "stderr": f"compiler: {Path.home()}/.cargo/bin/rustc",
                "configuration": {str(Path.home() / ".cargo/config.toml"): "digest"},
            })
            self.assertEqual(record["arguments"], [
                "<checkout>/artifacts/custom-target/fresh/debug/browser-engine",
                "<staging>/cases/fixture.json",
            ])
            self.assertEqual(record["stderr"], "compiler: <home>/.cargo/bin/rustc")
            self.assertEqual(record["configuration"], {"<home>/.cargo/config.toml": "digest"})

    def test_interrupted_or_failed_publication_keeps_existing_bundles(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = root / "original"
            original.mkdir()
            with self.assertRaisesRegex(OSError, "write failed"):
                with runner.stage_bundle(root, "new") as (stage, _):
                    (stage / "commands.json").write_text("partial")
                    raise OSError("write failed")
            with patch.object(Path, "rename", side_effect=OSError("publish failed")):
                with self.assertRaisesRegex(OSError, "publish failed"):
                    with runner.stage_bundle(root, "new") as (stage, _):
                        (stage / "commands.json").write_text("partial")
            self.assertEqual(list(root.iterdir()), [original])

    def test_externally_supplied_binary_is_exploratory_and_swap_invalidates_run(self):
        contract = runner.verify_contract(runner.ROOT, MANIFEST.read_bytes())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "binary"
            binary.write_bytes(b"initial bytes")
            args = argparse.Namespace(binary=binary, cargo="cargo", run_id="exploration", repeat=1)
            for swap in (False, True):
                with self.subTest(swap=swap):
                    stage = root / str(swap)
                    stage.mkdir()

                    def execute(binary, selected, repeat, timeout, directory, commands, **kwargs):
                        if swap:
                            binary.write_bytes(b"replacement bytes")
                        return [{"id": case["id"], "outcome": "pass"} for case in selected]

                    with patch.object(runner, "execute_cases", side_effect=execute):
                        report, passed = runner.run_bundle(args, contract, MANIFEST, stage, runner.source_identity())
                    self.assertEqual(passed, not swap)
                    self.assertFalse(report["build_identity_verified"])
                    self.assertEqual(report["evidence_kind"], "exploratory")
                    self.assertIsNone(report["build"])
                    self.assertEqual(report["command_log"]["sha256"], runner.file_digest(stage / report["command_log"]["file"]))


if __name__ == "__main__":
    unittest.main()
