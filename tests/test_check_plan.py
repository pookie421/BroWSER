"""Planning-contract tests; these do not exercise a browser engine."""

import contextlib
import copy
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "tools" / "check_plan.py"
SPEC = importlib.util.spec_from_file_location("check_plan", MODULE_PATH)
check_plan = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_plan)


class PlanContractTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "planning").mkdir()
        (self.root / "docs").mkdir()
        (self.root / "README.md").write_text("# Plan\n\n[Architecture](docs/architecture.md#scope)\n", encoding="utf-8")
        (self.root / "docs" / "architecture.md").write_text("# Architecture\n\n## Scope\n", encoding="utf-8")
        self.plan = {
            "schema_version": 1,
            "project": "BroWSER",
            "status": "planning",
            "milestones": [
                {
                    "id": "M0", "title": "Planning baseline", "status": "complete", "depends_on": [],
                    "owner_role": "Technical lead", "deliverables": ["README.md"],
                    "acceptance": ["Architecture and measurable gates reviewed"], "evidence": ["README.md"],
                },
                {
                    "id": "M1", "title": "Offline renderer", "status": "planned", "depends_on": ["M0"],
                    "owner_role": "Rendering lead", "deliverables": ["engine/renderer.rs"],
                    "acceptance": ["Pinned static corpus renders deterministically"], "evidence": [],
                },
            ],
            "tasks": [
                {
                    "id": "B001", "title": "Build deterministic fixtures", "milestone": "M1", "priority": "P0",
                    "status": "planned", "depends_on": [], "owner_role": "QA lead",
                    "acceptance": ["Fixture corpus has hashes and provenance"], "evidence": [],
                },
                {
                    "id": "B002", "title": "Render fixture corpus", "milestone": "M1", "priority": "P0",
                    "status": "planned", "depends_on": ["B001"], "owner_role": "Rendering lead",
                    "acceptance": ["Every fixture has a deterministic output"], "evidence": [],
                },
            ],
        }

    def write_plan(self, plan=None):
        (self.root / "planning" / "roadmap.json").write_text(json.dumps(self.plan if plan is None else plan), encoding="utf-8")

    def errors(self):
        return check_plan.validate_plan(self.plan, self.root)

    def assert_error(self, phrase):
        self.assertTrue(any(phrase in error for error in self.errors()), self.errors())

    def test_valid_plan_including_unbuilt_planned_deliverable(self):
        self.write_plan()
        self.assertEqual([], check_plan.validate_repository(self.root))

    def test_unknown_and_missing_keys_are_rejected(self):
        self.plan["milestones"][0]["claim"] = "shipped"
        del self.plan["tasks"][0]["owner_role"]
        self.assert_error("unknown keys: claim")
        self.assert_error("missing keys: owner_role")

    def test_boolean_is_not_schema_version(self):
        self.plan["schema_version"] = True
        self.assert_error("expected integer 1")

    def test_root_schema_values_are_rejected(self):
        self.plan["project"] = "Other"
        self.plan["status"] = "released"
        self.assert_error("expected BroWSER")
        self.assert_error("expected planning")

    def test_empty_collections_rejected(self):
        self.plan["tasks"] = []
        self.assert_error("tasks: expected a nonempty array")

    def test_malformed_records_report_errors_without_crashing(self):
        self.plan["milestones"].append(None)
        self.plan["tasks"][0]["status"] = {}
        self.plan["tasks"][0]["depends_on"] = [None, {}]
        self.plan["tasks"][0]["acceptance"] = [" "]
        self.plan["tasks"][0]["evidence"] = [3]
        self.assert_error("expected an object")
        self.assert_error("expected planned, in_progress, or complete")
        self.assert_error("expected a list of nonempty strings")

    def test_identifier_formats(self):
        self.plan["milestones"][0]["id"] = "phase0"
        self.plan["tasks"][0]["id"] = "B1"
        self.assert_error("expected M[0-9]+")
        self.assert_error("expected B[0-9]{3}")

    def test_duplicate_ids_and_dependencies(self):
        self.plan["tasks"].append(copy.deepcopy(self.plan["tasks"][0]))
        self.plan["tasks"][1]["depends_on"] = ["B001", "B001"]
        self.assert_error("duplicate id B001")
        self.assert_error("duplicate entries")

    def test_unknown_dependency_and_milestone(self):
        self.plan["tasks"][0]["depends_on"] = ["B999"]
        self.plan["tasks"][0]["milestone"] = "M9"
        self.assert_error("unknown dependency B999")
        self.assert_error("unknown milestone M9")

    def test_self_dependency_and_cycle(self):
        self.plan["tasks"][0]["depends_on"] = ["B001", "B002"]
        self.assert_error("cannot depend on itself")
        self.assert_error("dependency cycle")

    def test_milestone_dependency_cycle(self):
        self.plan["milestones"][0]["depends_on"] = ["M1"]
        self.assert_error("dependency cycle")

    def test_in_progress_work_requires_completed_dependencies(self):
        self.plan["tasks"][1]["status"] = "in_progress"
        self.assert_error("in_progress requires completed dependency B001")

    def test_complete_work_requires_completed_dependencies(self):
        self.plan["tasks"][1]["status"] = "complete"
        self.plan["tasks"][1]["evidence"] = ["README.md"]
        self.assert_error("complete requires completed dependency B001")

    def test_completed_task_requires_existing_evidence(self):
        task = self.plan["tasks"][0]
        task["status"] = "complete"
        self.assert_error("evidence: must contain at least one entry")
        task["evidence"] = ["missing.md"]
        self.assert_error("file does not exist: missing.md")

    def test_completed_milestone_requires_existing_deliverables(self):
        self.plan["milestones"][0]["deliverables"] = ["missing.md"]
        self.assert_error("deliverables: file does not exist: missing.md")

    def test_completed_milestone_requires_completed_member_tasks(self):
        task = self.plan["tasks"][0]
        task["milestone"] = "M0"
        for status in ("planned", "in_progress"):
            with self.subTest(status=status):
                task["status"] = status
                self.assert_error("complete milestone has incomplete member task B001")
        task["status"] = "complete"
        task["evidence"] = ["README.md"]
        self.assertEqual([], self.errors())

    def test_task_cannot_depend_on_work_in_a_later_milestone(self):
        task = self.plan["tasks"][0]
        task["milestone"] = "M0"
        task["status"] = "complete"
        task["evidence"] = ["README.md"]
        task["depends_on"] = ["B002"]
        self.plan["tasks"][1]["depends_on"] = []
        self.assert_error("not M0 or one of its prerequisite milestones")

    def test_task_cannot_depend_on_an_unrelated_milestone(self):
        milestone = copy.deepcopy(self.plan["milestones"][1])
        milestone["id"] = "M2"
        milestone["depends_on"] = ["M0"]
        self.plan["milestones"].append(milestone)
        self.plan["tasks"][1]["milestone"] = "M2"
        self.assert_error("not M2 or one of its prerequisite milestones")

    def test_task_can_depend_on_transitive_prerequisite_work(self):
        milestone = copy.deepcopy(self.plan["milestones"][1])
        milestone["id"] = "M2"
        milestone["depends_on"] = ["M1"]
        self.plan["milestones"].append(milestone)
        task = self.plan["tasks"][0]
        task["milestone"] = "M0"
        task["status"] = "complete"
        task["evidence"] = ["README.md"]
        self.plan["tasks"][1]["milestone"] = "M2"
        self.assertEqual([], self.errors())

    def test_empty_acceptance_and_bad_priority(self):
        self.plan["tasks"][0]["acceptance"] = []
        self.plan["tasks"][0]["priority"] = "urgent"
        self.assert_error("acceptance: must contain at least one entry")
        self.assert_error("expected P0, P1, or P2")

    def test_evidence_paths_cannot_traverse_or_be_absolute(self):
        for value in ("../README.md", "/etc/hosts", "C:\\Windows\\file", "docs/../../file"):
            with self.subTest(value=value):
                self.plan["tasks"][0]["evidence"] = [value]
                self.assert_error("unsafe repository-relative path")

    def test_symlink_escape_rejected_for_evidence_and_markdown_links(self):
        with tempfile.TemporaryDirectory() as external:
            outside = Path(external) / "outside.md"
            outside.write_text("outside", encoding="utf-8")
            (self.root / "escape.md").symlink_to(outside)
            self.plan["tasks"][0]["evidence"] = ["escape.md"]
            self.assert_error("path escapes repository")
            (self.root / "README.md").write_text("[Escape](escape.md)", encoding="utf-8")
            self.assertTrue(any("escapes repository" in error for error in check_plan.check_links(self.root)))

    def test_local_links_percent_encoding_references_and_fragments(self):
        (self.root / "docs" / "a b.md").write_text("# Space", encoding="utf-8")
        (self.root / "README.md").write_text(
            "[Space](docs/a%20b.md#space)\n[Architecture][a]\n[a]: docs/architecture.md\n"
            "[Root](/README.md) [Anchor](#plan) [Web](https://example.com/missing)\n",
            encoding="utf-8",
        )
        self.assertEqual([], check_plan.check_links(self.root))

    def test_missing_and_escaping_local_links(self):
        (self.root / "README.md").write_text("[Missing](no.md) [Escape](../outside.md)", encoding="utf-8")
        errors = check_plan.check_links(self.root)
        self.assertTrue(any("broken local link" in error for error in errors))
        self.assertTrue(any("escapes repository" in error for error in errors))

    def test_examples_and_issue_templates_are_not_link_targets(self):
        (self.root / ".github" / "ISSUE_TEMPLATE").mkdir(parents=True)
        (self.root / ".github" / "ISSUE_TEMPLATE" / "bug.md").write_text("[Example](absent.md)", encoding="utf-8")
        (self.root / "README.md").write_text("```md\n[Example](absent.md)\n```\n`[Example](absent.md)`\n", encoding="utf-8")
        self.assertEqual([], check_plan.check_links(self.root))

    def test_duplicate_json_keys_and_bad_json_rejected(self):
        roadmap = self.root / "planning" / "roadmap.json"
        roadmap.write_text('{"project":"BroWSER","project":"Other"}', encoding="utf-8")
        self.assertIn("duplicate JSON key", check_plan.validate_repository(self.root)[0])
        roadmap.write_text("{broken", encoding="utf-8")
        self.assertTrue(check_plan.validate_repository(self.root))

    def test_roadmap_symlink_outside_repository_rejected(self):
        with tempfile.TemporaryDirectory() as external:
            outside = Path(external) / "roadmap.json"
            outside.write_text(json.dumps(self.plan), encoding="utf-8")
            (self.root / "planning" / "roadmap.json").symlink_to(outside)
            self.assertTrue(check_plan.validate_repository(self.root))

    def test_cli_success_and_failure_status(self):
        self.write_plan()
        output, errors = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            self.assertEqual(0, check_plan.main(["--root", str(self.root)]))
        self.assertIn("does not measure browser functionality", output.getvalue())
        self.plan["tasks"][0]["priority"] = "P5"
        self.write_plan()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            self.assertEqual(1, check_plan.main(["--root", str(self.root)]))
        self.assertIn("expected P0, P1, or P2", errors.getvalue())


if __name__ == "__main__":
    unittest.main()
