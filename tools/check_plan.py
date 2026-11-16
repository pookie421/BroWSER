#!/usr/bin/env python3
"""Validate BroWSER's planning contracts and local Markdown file links.

This checks repository planning data, not browser implementation or conformance.
Only the Python standard library is required (Python 3.11 or newer).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path, PurePosixPath
from urllib.parse import unquote, urlsplit


ROOT_KEYS = {"schema_version", "project", "status", "milestones", "tasks"}
COMMON_KEYS = {"id", "title", "status", "depends_on", "owner_role", "acceptance", "evidence"}
MILESTONE_KEYS = COMMON_KEYS | {"deliverables"}
TASK_KEYS = COMMON_KEYS | {"milestone", "priority"}
STATUSES = {"planned", "in_progress", "complete"}
IGNORED_DIRECTORIES = {".git", ".venv", "venv", "node_modules", "target", "dist", "build", "__pycache__"}
LINK = re.compile(r"!?\[[^\]\n]*\]\((<[^>\n]+>|[^\s)]+)(?:\s+[\"'][^\n]*?[\"'])?\)")
REFERENCE = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*(<[^>\n]+>|\S+)", re.MULTILINE)


def nonempty(value: object) -> bool:
    return isinstance(value, str) and bool(value.strip())


def local_file(root: Path, value: object, label: str, errors: list[str], *, required: bool) -> None:
    """Check lexical and resolved containment, including symlink escapes."""
    if not nonempty(value):
        errors.append(f"{label}: expected a nonempty repository-relative file path")
        return
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or "\\" in value or ":" in value or value in {".", "./"}:
        errors.append(f"{label}: unsafe repository-relative path {value!r}")
        return
    target = root / value
    try:
        target.resolve().relative_to(root.resolve())
    except (ValueError, OSError, RuntimeError):
        errors.append(f"{label}: path escapes repository or cannot be resolved: {value!r}")
        return
    if required and not target.is_file():
        errors.append(f"{label}: file does not exist: {value}")


def string_list(record: dict, key: str, label: str, errors: list[str], *, allow_empty: bool) -> list[str]:
    value = record.get(key)
    if not isinstance(value, list) or any(not nonempty(item) for item in value):
        errors.append(f"{label}.{key}: expected a list of nonempty strings")
        return []
    if not allow_empty and not value:
        errors.append(f"{label}.{key}: must contain at least one entry")
    if len(value) != len(set(value)):
        errors.append(f"{label}.{key}: duplicate entries are not allowed")
    return value


def validate_records(records: object, kind: str, root: Path, errors: list[str]) -> dict[str, dict]:
    expected = MILESTONE_KEYS if kind == "milestones" else TASK_KEYS
    pattern = r"M[0-9]+" if kind == "milestones" else r"B[0-9]{3}"
    by_id: dict[str, dict] = {}
    if not isinstance(records, list) or not records:
        errors.append(f"{kind}: expected a nonempty array")
        return by_id
    for index, record in enumerate(records):
        label = f"{kind}[{index}]"
        if not isinstance(record, dict):
            errors.append(f"{label}: expected an object")
            continue
        missing, extra = expected - record.keys(), record.keys() - expected
        if missing:
            errors.append(f"{label}: missing keys: {', '.join(sorted(missing))}")
        if extra:
            errors.append(f"{label}: unknown keys: {', '.join(sorted(extra))}")
        identifier = record.get("id")
        if not isinstance(identifier, str) or not re.fullmatch(pattern, identifier):
            errors.append(f"{label}.id: expected {pattern}")
        elif identifier in by_id:
            errors.append(f"{label}.id: duplicate id {identifier}")
        else:
            by_id[identifier] = record
        for key in ("title", "owner_role"):
            if not nonempty(record.get(key)):
                errors.append(f"{label}.{key}: expected a nonempty string")
        status = record.get("status")
        if not isinstance(status, str) or status not in STATUSES:
            errors.append(f"{label}.status: expected planned, in_progress, or complete")
        string_list(record, "depends_on", label, errors, allow_empty=True)
        string_list(record, "acceptance", label, errors, allow_empty=False)
        evidence = string_list(record, "evidence", label, errors, allow_empty=status != "complete")
        for path in evidence:
            local_file(root, path, f"{label}.evidence", errors, required=True)
        if kind == "milestones":
            deliverables = string_list(record, "deliverables", label, errors, allow_empty=False)
            for path in deliverables:
                local_file(root, path, f"{label}.deliverables", errors, required=status == "complete")
        else:
            if not isinstance(record.get("milestone"), str) or not re.fullmatch(r"M[0-9]+", record["milestone"]):
                errors.append(f"{label}.milestone: expected a milestone id")
            if not isinstance(record.get("priority"), str) or record["priority"] not in {"P0", "P1", "P2"}:
                errors.append(f"{label}.priority: expected P0, P1, or P2")
    return by_id


def check_dependencies(records: dict[str, dict], kind: str, errors: list[str]) -> None:
    graph: dict[str, list[str]] = {}
    for identifier, record in records.items():
        deps = record.get("depends_on")
        graph[identifier] = [dep for dep in deps if isinstance(dep, str) and dep in records] if isinstance(deps, list) else []
        if not isinstance(deps, list):
            continue
        for dep in deps:
            if not isinstance(dep, str):
                continue
            if dep not in records:
                errors.append(f"{kind}.{identifier}: unknown dependency {dep}")
            elif dep == identifier:
                errors.append(f"{kind}.{identifier}: cannot depend on itself")
            elif record.get("status") in ("complete", "in_progress") and records[dep].get("status") != "complete":
                errors.append(f"{kind}.{identifier}: {record['status']} requires completed dependency {dep}")
    # Iterative depth-first traversal avoids recursion limits on large backlogs.
    colors: dict[str, int] = {}
    for start in graph:
        if colors.get(start):
            continue
        stack = [(start, iter(graph[start]))]
        colors[start] = 1
        while stack:
            node, neighbors = stack[-1]
            child = next(neighbors, None)
            if child is None:
                colors[node] = 2
                stack.pop()
            elif colors.get(child) == 1:
                errors.append(f"{kind}: dependency cycle includes {node} -> {child}")
            elif not colors.get(child):
                colors[child] = 1
                stack.append((child, iter(graph[child])))


def validate_plan(plan: object, root: Path) -> list[str]:
    """Return all detectable schema, dependency, and evidence failures."""
    errors: list[str] = []
    if not isinstance(plan, dict):
        return ["roadmap: expected an object"]
    missing, extra = ROOT_KEYS - plan.keys(), plan.keys() - ROOT_KEYS
    if missing:
        errors.append(f"roadmap: missing keys: {', '.join(sorted(missing))}")
    if extra:
        errors.append(f"roadmap: unknown keys: {', '.join(sorted(extra))}")
    if type(plan.get("schema_version")) is not int or plan.get("schema_version") != 1:
        errors.append("roadmap.schema_version: expected integer 1")
    if plan.get("project") != "BroWSER":
        errors.append("roadmap.project: expected BroWSER")
    if plan.get("status") != "planning":
        errors.append("roadmap.status: expected planning")
    milestones = validate_records(plan.get("milestones"), "milestones", root, errors)
    tasks = validate_records(plan.get("tasks"), "tasks", root, errors)
    check_dependencies(milestones, "milestones", errors)
    check_dependencies(tasks, "tasks", errors)

    def prerequisite_milestones(identifier: str) -> set[str]:
        """Find transitive prerequisites even when another error reports a cycle."""
        found: set[str] = set()
        pending = [identifier]
        while pending:
            milestone = milestones[pending.pop()]
            dependencies = milestone.get("depends_on")
            if not isinstance(dependencies, list):
                continue
            for dependency in dependencies:
                if isinstance(dependency, str) and dependency in milestones and dependency not in found:
                    found.add(dependency)
                    pending.append(dependency)
        return found

    for identifier, task in tasks.items():
        milestone_id = task.get("milestone")
        if not isinstance(milestone_id, str):
            continue
        if milestone_id not in milestones:
            errors.append(f"tasks.{identifier}: unknown milestone {milestone_id}")
            continue
        if milestones[milestone_id].get("status") == "complete" and task.get("status") != "complete":
            errors.append(f"milestones.{milestone_id}: complete milestone has incomplete member task {identifier}")
        allowed = prerequisite_milestones(milestone_id) | {milestone_id}
        dependencies = task.get("depends_on")
        if not isinstance(dependencies, list):
            continue
        for dependency in dependencies:
            if not isinstance(dependency, str) or dependency not in tasks:
                continue
            dependency_milestone = tasks[dependency].get("milestone")
            if isinstance(dependency_milestone, str) and dependency_milestone in milestones and dependency_milestone not in allowed:
                errors.append(
                    f"tasks.{identifier}: dependency {dependency} belongs to {dependency_milestone}, "
                    f"which is not {milestone_id} or one of its prerequisite milestones"
                )
    return errors


def markdown_files(root: Path):
    """Yield authored Markdown, excluding generated and template directories."""
    for path in sorted(root.rglob("*.md")):
        relative = path.relative_to(root)
        if any(part in IGNORED_DIRECTORIES for part in relative.parts) or "ISSUE_TEMPLATE" in relative.parts:
            continue
        yield path


def check_links(root: Path) -> list[str]:
    errors: list[str] = []
    for path in markdown_files(root):
        label = str(path.relative_to(root))
        try:
            path.resolve().relative_to(root.resolve())
            content = path.read_text(encoding="utf-8")
        except (ValueError, OSError, UnicodeError, RuntimeError) as exc:
            errors.append(f"{label}: cannot read repository Markdown: {exc}")
            continue
        # Code examples are not active Markdown links.
        content = re.sub(r"^\s*(```|~~~)[\s\S]*?^\s*\1[^\n]*$", "", content, flags=re.MULTILINE)
        content = re.sub(r"`[^`\n]+`", "", content)
        targets = [match.group(1) for match in LINK.finditer(content)]
        targets.extend(match.group(1) for match in REFERENCE.finditer(content))
        for target in targets:
            target = target.removeprefix("<").removesuffix(">")
            try:
                parsed = urlsplit(target)
            except ValueError as exc:
                errors.append(f"{label}: malformed link {target!r}: {exc}")
                continue
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            decoded = unquote(parsed.path)
            if decoded.startswith("/"):
                candidate = root / decoded.lstrip("/")
            else:
                candidate = path.parent / decoded
            try:
                relative_target = candidate.resolve().relative_to(root.resolve())
            except (ValueError, OSError, RuntimeError):
                errors.append(f"{label}: local link escapes repository: {target}")
                continue
            if not candidate.exists():
                errors.append(f"{label}: broken local link: {target} ({relative_target})")
    return errors


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def validate_repository(root: Path) -> list[str]:
    roadmap = root / "planning" / "roadmap.json"
    try:
        roadmap.resolve().relative_to(root.resolve())
        plan = json.loads(roadmap.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    except (OSError, UnicodeError, ValueError, RuntimeError) as exc:
        return [f"planning/roadmap.json: {exc}"]
    return validate_plan(plan, root) + check_links(root)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1], help="repository root")
    args = parser.parse_args(argv)
    errors = validate_repository(args.root.resolve())
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        print(f"Plan validation failed: {len(errors)} error(s).", file=sys.stderr)
        return 1
    print("Plan validation passed: schema, dependencies, evidence paths, and local Markdown file links.")
    print("This result does not measure browser functionality or standards conformance.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
