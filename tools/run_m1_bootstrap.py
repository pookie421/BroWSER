#!/usr/bin/env python3
"""Build and replay the frozen offline bootstrap, retaining a bundle per run."""

from __future__ import annotations

import argparse
from collections import Counter
from contextlib import contextmanager
import datetime
import hashlib
import json
import locale
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
# The digest pins the exact asset roster as well as case stages, inputs and oracles.
REVIEWED_COHORT = ("M1-v2", "9424c617b2eee264072ab9c52f85a199c819ca481de9700c16bf2b5ec129b06e")
CASE_GROUPS = {"B": 5, "H": 6, "C": 3, "L": 2, "T": 5, "P": 3, "R": 4}


class ReplayError(ValueError):
    def __init__(self, outcome: str, message: str):
        super().__init__(message)
        self.outcome = outcome


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def strict_json(data: bytes) -> object:
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    def invalid_constant(value):
        raise ValueError(f"invalid JSON number: {value}")

    return json.loads(data, object_pairs_hook=unique_object, parse_constant=invalid_constant)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def has_keys(value, keys: set[str]) -> bool:
    return isinstance(value, dict) and value.keys() == keys


def local_path(root: Path, name: str) -> Path:
    path = (root / name).resolve()
    path.relative_to(root.resolve())
    if not path.is_file():
        raise ValueError(f"missing fixture asset: {name}")
    return path


def asset_references(value):
    if isinstance(value, str) and value.startswith("fixtures/m1/"):
        yield value
    elif isinstance(value, list):
        for item in value:
            yield from asset_references(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from asset_references(item)


def verify_contract(root: Path, data: bytes) -> dict:
    contract = strict_json(data)
    require(isinstance(contract, dict), "cohort must be a JSON object")
    require(type(contract.get("schema_version")) is int and contract["schema_version"] == 1
            and contract.get("id") == REVIEWED_COHORT[0], "unsupported cohort identity")
    require(digest(data) == REVIEWED_COHORT[1], "cohort bytes differ from the reviewed revision")
    for key in ("assets", "cases"):
        require(isinstance(contract.get(key), list), f"cohort {key} must be an array")
    limits = contract.get("limits")
    require(isinstance(limits, dict) and all(type(n) is int and n > 0 for n in limits.values()),
            "cohort limits must be positive integers")
    assets = set()
    for asset in contract["assets"]:
        require(isinstance(asset, dict), "asset entry must be an object")
        name, size, checksum = asset.get("path"), asset.get("bytes"), asset.get("sha256")
        require(isinstance(name, str) and name.startswith("fixtures/m1/") and name not in assets,
                "missing, duplicate or invalid fixture asset path")
        require(type(size) is int and size > 0 and isinstance(checksum, str)
                and re.fullmatch(r"[a-f0-9]{64}", checksum) is not None, f"invalid asset identity: {name}")
        path = local_path(root, name)
        if path.stat().st_size != size or file_digest(path) != checksum:
            raise ValueError(f"asset identity mismatch: {name}")
        assets.add(name)
    expected_stages = {
        f"M1-{group}{index:03}": "bootstrap" if group == "B" else "full_m1"
        for group, count in CASE_GROUPS.items()
        for index in range(1, count + 1)
    }
    stages = {}
    for case in contract["cases"]:
        require(isinstance(case, dict), "fixture case must be an object")
        case_id = case.get("id")
        require(isinstance(case_id, str) and case_id not in stages, "missing or duplicate fixture case ID")
        require(isinstance(case.get("kind"), str) and isinstance(case.get("expected"), dict)
                and (case.get("input") is None or isinstance(case["input"], str)),
                f"invalid fixture case fields: {case_id}")
        stages[case_id] = case.get("stage")
        missing = set(asset_references(case)) - assets
        if missing:
            raise ValueError(f"unhashed fixture inputs for {case_id}: {', '.join(sorted(missing))}")
        for key in ("input", "control"):
            if case.get(key) is not None:
                require(case[key] in assets, f"unhashed {key} for {case_id}")
        if case.get("stage") == "bootstrap":
            expected = case["expected"]
            require(case["input"] in assets and viewport_is_valid(case.get("viewport")), f"invalid bootstrap input: {case_id}")
            if case["kind"] == "reject":
                require(case.get("control") in assets and type(expected.get("exit")) is int
                        and expected["exit"] == 1 and isinstance(expected.get("stderr"), str)
                        and expected["stderr"].startswith("browser-engine: "),
                        f"{case_id} requires a control and exact exit and stderr")
            else:
                require(case["kind"] == "render" and isinstance(expected.get("pixels"), list)
                        and isinstance(expected.get("boxes"), dict), f"invalid bootstrap render oracle: {case_id}")
    require(stages == expected_stages, "case roster or stage differs from the reviewed cohort")
    return contract


def ppm_pixels(data: bytes, width: int, height: int) -> bytes:
    expected = f"P6\n{width} {height}\n255\n".encode()
    if not data.startswith(expected) or len(data) != len(expected) + width * height * 3:
        raise ValueError("invalid P6 image header or pixel payload length")
    return data[len(expected):]


def nonnegative_integer(value) -> bool:
    return type(value) is int and value >= 0


def viewport_is_valid(value) -> bool:
    return has_keys(value, {"width", "height"}) and all(
        type(dimension) is int and 0 < dimension <= 2048 for dimension in value.values()
    )


def validate_dom(dom) -> tuple[dict, dict]:
    require(has_keys(dom, {"root", "nodes"}) and nonnegative_integer(dom["root"])
            and isinstance(dom["nodes"], list), "invalid DOM root or node list")
    root = dom["root"]
    nodes = {}
    by_element_id = {}
    fields = {"document": set(), "doctype": {"name"}, "element": {"name", "attributes"}, "text": {"text"}}
    for node in dom["nodes"]:
        require(isinstance(node, dict) and isinstance(node.get("type"), str)
                and node["type"] in fields, "invalid DOM node type")
        kind = node["type"]
        require(has_keys(node, {"id", "type", "parent", "children"} | fields[kind])
                and nonnegative_integer(node["id"]) and node["id"] not in nodes
                and (node["parent"] is None or nonnegative_integer(node["parent"]))
                and isinstance(node["children"], list)
                and all(nonnegative_integer(child) for child in node["children"]),
                "invalid DOM node fields or duplicate node ID")
        nodes[node["id"]] = node
        if kind == "element":
            require(isinstance(node["name"], str) and bool(node["name"])
                    and isinstance(node["attributes"], dict)
                    and all(isinstance(value, str) for value in node["attributes"].values()),
                    "invalid DOM element name or attribute")
            identifier = node["attributes"].get("id")
            if identifier:
                require(identifier not in by_element_id, "geometry fixture contains duplicate element IDs")
                by_element_id[identifier] = node["id"]
        elif kind == "doctype":
            require(isinstance(node["name"], str) and node["parent"] == root
                    and not node["children"], "invalid DOM doctype")
        elif kind == "text":
            require(isinstance(node["text"], str) and not node["children"], "invalid DOM text node")
        else:
            require(node["id"] == root and node["parent"] is None, "invalid DOM document node")
    require(root in nodes and nodes[root]["type"] == "document", "DOM root must reference the document node")
    seen = set()
    pending = [root]
    while pending:
        node_id = pending.pop()
        require(node_id not in seen, "DOM contains a cycle or duplicate child")
        seen.add(node_id)
        for child in nodes[node_id]["children"]:
            require(child in nodes and nodes[child]["parent"] == node_id, "invalid DOM child or parent reference")
            pending.append(child)
    require(len(seen) == len(nodes), "DOM contains unreachable nodes")
    return nodes, by_element_id


def validate_layout(layout, nodes: dict) -> dict:
    require(has_keys(layout, {"viewport", "boxes"}) and viewport_is_valid(layout["viewport"])
            and isinstance(layout["boxes"], list), "invalid layout viewport or box list")
    by_node = {}
    for box in layout["boxes"]:
        require(has_keys(box, {"node", "tag", "border_box", "content_box", "clip", "background"})
                and nonnegative_integer(box["node"]), "invalid layout box fields")
        node_id = box["node"]
        require(node_id not in by_node, "layout contains duplicate boxes for one node")
        require(node_id in nodes and nodes[node_id]["type"] == "element"
                and nodes[node_id]["name"] == box["tag"], "layout box refers to an invalid DOM element")
        for key in ("border_box", "content_box", "clip"):
            rect = box[key]
            require(has_keys(rect, {"x", "y", "width", "height"})
                    and all(type(value) is int for value in rect.values())
                    and rect["width"] >= 0 and rect["height"] >= 0, "invalid layout rectangle")
        color = box["background"]
        require(color is None or (isinstance(color, str) and re.fullmatch(r"#[a-fA-F0-9]{6}", color) is not None),
                "invalid layout background")
        by_node[node_id] = box
    return by_node


def assert_render(case: dict, image: bytes, layout, dom) -> None:
    try:
        nodes, by_id = validate_dom(dom)
        by_node = validate_layout(layout, nodes)
        viewport = case["viewport"]
        pixels = ppm_pixels(image, viewport["width"], viewport["height"])
    except ValueError as error:
        raise ReplayError("harness_error", str(error)) from error
    require(layout["viewport"] == viewport, "layout viewport differs from the frozen fixture")
    for x, y, red, green, blue in case["expected"].get("pixels", []):
        require(0 <= x < viewport["width"] and 0 <= y < viewport["height"], "oracle pixel lies outside viewport")
        offset = (y * viewport["width"] + x) * 3
        require(pixels[offset:offset + 3] == bytes((red, green, blue)), f"pixel oracle mismatch at ({x}, {y})")
    for identifier, expected in case["expected"].get("boxes", {}).items():
        require(identifier in by_id and by_id[identifier] in by_node, f"missing layout box for #{identifier}")
        box = by_node[by_id[identifier]]
        for edge, values in expected.items():
            actual = box[{"content": "content_box", "border": "border_box"}[edge]]
            require([actual[key] for key in ("x", "y", "width", "height")] == values,
                    f"geometry oracle mismatch for #{identifier} {edge}")


def decode_output(data: bytes | None) -> str:
    return (data or b"").decode("utf-8", errors="replace")


def invoke(
    commands: list[dict], command: list[str], timeout: int, *,
    case: str, operation: str, repetition: int, env: dict | None = None
) -> subprocess.CompletedProcess:
    start = time.monotonic()
    record = {"case": case, "operation": operation, "repetition": repetition, "arguments": command}
    try:
        result = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=timeout, check=False, env=env)
        crash = result.returncode < 0 or (result.returncode == 101 and case != "build")
        record.update(
            outcome="crash" if crash else "exit_nonzero" if result.returncode else "success",
            exit_code=result.returncode, stdout=decode_output(result.stdout), stderr=decode_output(result.stderr),
        )
        if record["outcome"] == "crash":
            raise ReplayError("crash", f"{operation} crashed with exit {result.returncode}")
        return result
    except subprocess.TimeoutExpired as error:
        record.update(outcome="timeout", exit_code=None, stdout=decode_output(error.stdout),
                      stderr=decode_output(error.stderr))
        raise ReplayError("timeout", f"{operation} exceeded the {timeout}-second invocation limit") from error
    except OSError as error:
        record.update(outcome="harness_error", exit_code=None, stdout="", stderr=str(error))
        raise ReplayError("harness_error", f"cannot start {operation}: {error}") from error
    finally:
        record["duration_seconds"] = time.monotonic() - start
        commands.append(record)


def require_success(result: subprocess.CompletedProcess, operation: str) -> None:
    require(result.returncode == 0, f"{operation} exited {result.returncode}: {decode_output(result.stderr).strip()}")


def read_json(path: Path):
    try:
        return strict_json(path.read_bytes())
    except (OSError, ValueError, RecursionError) as error:
        raise ReplayError("harness_error", f"cannot read {path.name}: {error}") from error


def execute_case(
    binary: Path, case: dict, repeat: int, timeout: int, directory: Path, commands: list[dict]
) -> dict:
    def run(operation, repetition, arguments):
        return invoke(commands, [str(binary), *arguments], timeout, case=case["id"],
                      operation=operation, repetition=repetition)

    viewport = case["viewport"]

    def render_args(fixture, image, layout):
        return [
            "render", "--fixture", fixture, "--output", str(image), "--layout", str(layout),
            "--width", str(viewport["width"]), "--height", str(viewport["height"]),
        ]

    def inspect(fixture, path, operation):
        result = run(operation, 1, ["inspect", "--fixture", fixture, "--output", str(path)])
        require_success(result, operation)
        return read_json(path)

    if case["kind"] == "reject":
        image, layout = directory / "reject.ppm", directory / "reject.json"
        result = run("reject", 1, render_args(case["input"], image, layout))
        expected = case["expected"]
        diagnostic = decode_output(result.stderr)
        require(not os.path.lexists(image) and not os.path.lexists(layout), "unexpected rejection output artifacts")
        require(result.returncode == expected["exit"] and result.stdout == b""
                and result.stderr == (expected["stderr"] + "\n").encode(),
                f"wrong rejection outcome (exit {result.returncode}): {diagnostic.strip()}")
        control = case["control"]
        dom = inspect(control, directory / "control-dom.json", "control-inspect")
        image, layout = directory / "control.ppm", directory / "control.json"
        result = run("control-render", 1, render_args(control, image, layout))
        require_success(result, "control render")
        assert_render({"viewport": viewport, "expected": {}}, image.read_bytes(), read_json(layout), dom)
        return {"diagnostic": diagnostic.strip(), "control": "pass"}

    dom = inspect(case["input"], directory / "dom.json", "inspect")
    first = None
    for iteration in range(1, repeat + 1):
        image_path, layout_path = directory / f"{iteration}.ppm", directory / f"{iteration}.json"
        result = run("render", iteration, render_args(case["input"], image_path, layout_path))
        require_success(result, "render")
        image, layout_data = image_path.read_bytes(), layout_path.read_bytes()
        hashes = {"image_sha256": digest(image), "layout_sha256": digest(layout_data)}
        commands[-1]["output_sha256"] = hashes
        assert_render(case, image, read_json(layout_path), dom)
        if first is None:
            first = hashes
        else:
            require(hashes == first, f"nondeterministic output at repetition {iteration}")
            image_path.unlink()
            layout_path.unlink()
    return first


def execute_cases(binary: Path, selected: list[dict], repeat: int, timeout: int, directory: Path, commands: list) -> list[dict]:
    results = []
    for case in selected:
        outputs = directory / case["id"]
        first_attempt = len(commands)
        result = {"id": case["id"], "outcome": "pass"}
        try:
            outputs.mkdir()
            result.update(execute_case(binary, case, repeat, timeout, outputs, commands))
        except ReplayError as error:
            result.update(outcome=error.outcome, error=str(error))
        except OSError as error:
            result.update(outcome="harness_error", error=str(error))
        except ValueError as error:
            result.update(outcome="fail", error=str(error))
        result["attempts"] = len(commands) - first_attempt
        result["executions"] = sum(record["operation"] in ("render", "reject") for record in commands[first_attempt:])
        result["artifacts"] = {}
        try:
            if outputs.is_dir():
                for path in sorted(outputs.iterdir()):
                    if path.is_file() and not path.is_symlink():
                        result["artifacts"][path.relative_to(directory.parent).as_posix()] = file_digest(path)
        except OSError as error:
            result.update(outcome="harness_error", error=f"{result.get('error', 'case completed')}; cannot retain artifacts: {error}")
        results.append(result)
    return results


def snapshot(manifest: Path, contract: dict) -> dict:
    build_inputs = sorted((ROOT / "src").rglob("*.rs")) + [
        ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "rust-toolchain.toml"
    ]
    return {
        "build_inputs": {path.relative_to(ROOT).as_posix(): file_digest(path) for path in build_inputs},
        "cohort_sha256": file_digest(manifest),
        "runner_sha256": file_digest(Path(__file__)),
        "asset_sha256": {asset["path"]: file_digest(local_path(ROOT, asset["path"])) for asset in contract["assets"]},
    }


def cargo_configuration() -> dict:
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")).resolve()
    config_dirs = {cargo_home, *(parent / ".cargo" for parent in [ROOT, *ROOT.parents])}
    return {
        str(path): file_digest(path)
        for directory in sorted(config_dirs)
        for path in (directory / "config", directory / "config.toml")
        if path.is_file()
    }


def build(
    cargo: str, target_dir: Path, identity: dict, commands: list[dict]
) -> tuple[Path, dict]:
    compiler = shutil.which("rustc")
    if compiler is None:
        raise ValueError("Rust compiler not found; install the pinned toolchain")
    env = os.environ.copy()
    env.update(RUSTC=compiler, RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER="", CARGO_ENCODED_RUSTFLAGS="")
    version = invoke(commands, [compiler, "-Vv"], 30, case="build", operation="rustc-version", repetition=1, env=env)
    require_success(version, "rustc identity")
    compiler_version = decode_output(version.stdout).strip()
    fields = dict(line.split(": ", 1) for line in compiler_version.splitlines()[1:] if ": " in line)
    toolchain = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    require(fields.get("release") == toolchain and bool(fields.get("host")), "Rust compiler differs from the pinned toolchain")
    host = fields["host"]
    cargo_version = invoke(commands, [cargo, "-Vv"], 30, case="build", operation="cargo-version", repetition=1, env=env)
    require_success(cargo_version, "cargo identity")
    flags = ["build", "--locked", "--offline", "--bin", "browser-engine", "--target", host,
             "--target-dir", str(target_dir), "--message-format=json", "--verbose"]
    configuration = cargo_configuration()
    result = invoke(commands, [cargo, *flags], 120, case="build", operation="cargo-build", repetition=1, env=env)
    require_success(result, "Cargo build")
    require(cargo_configuration() == configuration, "Cargo configuration changed during build")
    candidates = []
    for line in result.stdout.splitlines():
        event = strict_json(line)
        require(isinstance(event, dict), "Cargo emitted a malformed build event")
        if (event.get("reason") == "compiler-artifact" and event.get("executable")
                and event["target"]["name"] == "browser-engine" and "bin" in event["target"]["kind"]):
            require(event["fresh"] is False, "Cargo unexpectedly reused an existing executable")
            candidates.append(Path(event["executable"]).resolve())
    require(len(candidates) == 1, "Cargo did not emit exactly one browser-engine executable")
    executable = candidates[0]
    require(executable.is_relative_to(target_dir.resolve()) and executable.is_file(),
            "Cargo executable is outside the isolated target directory")
    return executable, {
        "executable": str(executable), "binary_sha256": file_digest(executable),
        "inputs": identity["build_inputs"], "compiler": compiler_version, "target": host,
        "cargo": decode_output(cargo_version.stdout).strip(), "flags": flags,
        "cargo_configuration": configuration,
        "compiler_overrides": {key: env[key] for key in ("RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_ENCODED_RUSTFLAGS")},
    }


def source_identity() -> dict:
    def git(*args: str) -> str:
        result = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=False, timeout=30)
        if result.returncode:
            raise ValueError(f"cannot identify Git source state: {result.stderr.strip()}")
        return result.stdout.strip()
    return {"commit": git("rev-parse", "HEAD"), "worktree_dirty": bool(git("status", "--porcelain", "--untracked-files=normal"))}


@contextmanager
def stage_bundle(root: Path, run_id: str):
    root.mkdir(parents=True, exist_ok=True)
    destination = root / run_id
    reservation = root / f".{run_id}.reserved"
    with reservation.open("x"):
        pass
    try:
        if os.path.lexists(destination):
            raise FileExistsError(f"evidence bundle already exists: {destination}")
        with tempfile.TemporaryDirectory(prefix=f".{run_id}-", dir=root) as staging:
            stage = Path(staging)
            yield stage, destination
            if os.path.lexists(destination):
                raise FileExistsError(f"evidence bundle already exists: {destination}")
            stage.rename(destination)
    finally:
        reservation.unlink()


def write_json(path: Path, obj: dict) -> None:
    path.write_text(json.dumps(obj, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def record_paths(stage: Path, target_parent: Path):
    """Keep reproducible path components without retaining account directories."""
    paths = {
        ROOT: "<checkout>",
        stage: "<staging>",
        Path.home(): "<home>",
        Path(tempfile.gettempdir()): "<temp>",
        Path(tempfile.gettempdir()).resolve(): "<temp>",
    }
    if not stage.parent.is_relative_to(ROOT):
        paths.setdefault(stage.parent, "<artifacts-dir>")
    if not target_parent.is_relative_to(ROOT):
        paths.setdefault(target_parent, "<target-dir>")
    pairs = sorted(
        ((str(path), label) for path, label in paths.items() if path != path.parent),
        key=lambda pair: len(pair[0]), reverse=True,
    )

    def normalize(value):
        if isinstance(value, str):
            for path, label in pairs:
                value = value.replace(path, label)
            return value
        if isinstance(value, list):
            return [normalize(item) for item in value]
        if isinstance(value, dict):
            return {normalize(key): normalize(item) for key, item in value.items()}
        return value

    return normalize


def utc_now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def run_bundle(args, contract: dict, manifest: Path, stage: Path, source: dict) -> tuple[dict, bool]:
    started_at = utc_now()
    initial = snapshot(manifest, contract)
    require(initial["cohort_sha256"] == REVIEWED_COHORT[1], "cohort changed before replay")
    require(initial["asset_sha256"] == {asset["path"]: asset["sha256"] for asset in contract["assets"]},
            "fixture assets changed before replay")
    selected = [case for case in contract["cases"] if case["stage"] == "bootstrap"]
    commands = []
    artifacts = stage / "cases"
    artifacts.mkdir()
    timeout = contract["limits"]["wall_time_seconds"]
    build_record, binary_digest, identity_error = None, None, None
    results = None
    configured_target = os.environ.get("CARGO_TARGET_DIR")
    target_parent = (ROOT / configured_target).resolve() if configured_target else stage
    try:
        if args.binary is None:
            target_parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(prefix="browser-replay-target-", dir=target_parent) as target:
                binary, build_record = build(args.cargo, Path(target), initial, commands)
                binary_digest = build_record["binary_sha256"]
                require(snapshot(manifest, contract) == initial, "source, runner or fixtures changed during build")
                results = execute_cases(binary, selected, args.repeat, timeout, artifacts, commands)
                require(file_digest(binary) == binary_digest, "tested executable changed during replay")
        else:
            binary = args.binary.resolve()
            binary_digest = file_digest(binary)
            results = execute_cases(binary, selected, args.repeat, timeout, artifacts, commands)
            require(file_digest(binary) == binary_digest, "tested executable changed during replay")
        require(snapshot(manifest, contract) == initial and source_identity() == source,
                "source, runner or fixtures changed during replay")
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        identity_error = str(error)
        if results is None:
            results = [
                {"id": case["id"], "outcome": "harness_error", "attempts": 0, "executions": 0, "error": identity_error}
                for case in selected
            ]

    counts = Counter(result["outcome"] for result in results)
    summary = {
        "bootstrap_selected": len(selected), "bootstrap_passed": counts["pass"],
        "full_m1_not_run": len(contract["cases"]) - len(selected), "wpt_executed": 0,
        "repetitions_per_render": args.repeat,
        "bootstrap_outcomes": {outcome: counts[outcome] for outcome in ("pass", "fail", "timeout", "crash", "harness_error")},
    }
    command_log_path = stage / "bootstrap-report.commands.json"
    normalize = record_paths(stage, target_parent)
    write_json(command_log_path, normalize({"commands": commands}))
    report = {
        "run_id": args.run_id,
        "cohort": contract["id"],
        "cohort_sha256": initial["cohort_sha256"],
        "binary_sha256": binary_digest,
        "build": build_record,
        "build_identity_verified": build_record is not None and identity_error is None,
        "evidence_kind": "build_bound_bootstrap" if build_record is not None and identity_error is None else "exploratory",
        "identity_error": identity_error,
        "source": source,
        "source_sha256": initial["build_inputs"],
        "runner_sha256": initial["runner_sha256"],
        "asset_sha256": initial["asset_sha256"],
        "case_input_sha256": {case["id"]: initial["asset_sha256"][case["input"]] for case in contract["cases"] if case.get("input") is not None},
        "invocation": [sys.executable, *sys.argv],
        "started_at_utc": started_at,
        "finished_at_utc": utc_now(),
        "command_log": {"file": command_log_path.name, "sha256": file_digest(command_log_path)},
        "path_labels": {
            "<checkout>": "repository root", "<staging>": "temporary run bundle before publication",
            "<home>": "account home", "<temp>": "system temporary directory",
            "<artifacts-dir>": "external evidence root", "<target-dir>": "external Cargo target root",
        },
        "platform": {"os": platform.system(), "release": platform.release(), "architecture": platform.machine(), "python": platform.python_version(), "cpu_count": os.cpu_count(), "locale": locale.getlocale()},
        "summary": summary,
        "limits": {"wall_time_seconds_per_invocation": timeout, "rss_containment_enforced": False},
        "cases": results,
        "boundary": "Development bootstrap for controlled fixtures. Full M1, WPT, RSS containment and sustained fuzzing require separate evidence.",
    }
    report = normalize(report)
    write_json(stage / "bootstrap-report.json", report)
    require(file_digest(command_log_path) == report["command_log"]["sha256"], "command log changed during publication")
    return report, counts["pass"] == len(selected) and identity_error is None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--build", action="store_true", help="fresh locked offline build (the default)")
    mode.add_argument("--binary", type=Path, help="replay an existing executable as exploratory evidence")
    parser.add_argument("--cargo", default=os.environ.get("CARGO", "cargo"))
    parser.add_argument("--repeat", type=int, default=100)
    parser.add_argument("--artifacts-dir", type=Path, default=ROOT / "artifacts/m1")
    parser.add_argument("--run-id", default=f"bootstrap-{datetime.datetime.now(datetime.timezone.utc):%Y%m%dT%H%M%SZ}-{uuid4().hex}")
    args = parser.parse_args()
    if not 1 <= args.repeat <= 1000:
        parser.error("--repeat must be between 1 and 1000")
    if re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_-]{0,99}", args.run_id) is None:
        parser.error("--run-id must contain 1–100 letters, digits, hyphens or underscores and start with a letter or digit")
    manifest = ROOT / "planning/m1-cohort.json"
    try:
        contract = verify_contract(ROOT, manifest.read_bytes())
        with stage_bundle(args.artifacts_dir.resolve(), args.run_id) as (stage, destination):
            report, passed = run_bundle(args, contract, manifest, stage, source_identity())
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        parser.error(str(error))
    print(json.dumps(report["summary"], sort_keys=True))
    for result in report["cases"]:
        if result["outcome"] != "pass":
            print(f"{result['outcome'].upper()} {result['id']}: {result['error']}")
    if report["identity_error"] is not None:
        print(f"Identity verification failed: {report['identity_error']}")
    print(f"Evidence ({report['evidence_kind']}): {destination / 'bootstrap-report.json'}")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
