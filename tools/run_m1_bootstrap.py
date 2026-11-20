#!/usr/bin/env python3
"""Replay frozen bootstrap oracles, verify asset identity, and measure determinism.

Only the stage=bootstrap cases are executed. Full M1, WPT, sustained fuzzing, RSS
containment and security readiness are not inferred from these results.
"""

from __future__ import annotations

import argparse
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
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COMMAND_LOG: list[dict] = []


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def local_path(root: Path, name: str) -> Path:
    path = (root / name).resolve()
    path.relative_to(root.resolve())
    if not path.is_file():
        raise ValueError(f"missing fixture asset: {name}")
    return path


def verify_contract(root: Path, contract: dict) -> None:
    if contract.get("schema_version") != 1 or contract.get("id") != "M1-v1":
        raise ValueError("unsupported cohort identity")
    identifiers = [case["id"] for case in contract["cases"]]
    if len(set(identifiers)) != len(identifiers):
        raise ValueError("duplicate fixture case ID")
    if not any(case["stage"] == "bootstrap" for case in contract["cases"]):
        raise ValueError("bootstrap cohort is empty")
    for asset in contract["assets"]:
        data = local_path(root, asset["path"]).read_bytes()
        if len(data) != asset["bytes"] or digest(data) != asset["sha256"]:
            raise ValueError(f"asset identity mismatch: {asset['path']}")
    for case in contract["cases"]:
        if case["stage"] not in ("bootstrap", "full_m1"):
            raise ValueError(f"invalid stage: {case['id']}")
        if case.get("input") is not None:
            local_path(root, case["input"])


def ppm_pixels(data: bytes, width: int, height: int) -> bytes:
    expected = f"P6\n{width} {height}\n255\n".encode()
    if not data.startswith(expected) or len(data) != len(expected) + width * height * 3:
        raise ValueError("invalid P6 image header or pixel payload length")
    return data[len(expected):]


def assert_render(case: dict, image: bytes, layout: dict, dom: dict) -> None:
    viewport = case["viewport"]
    if layout["viewport"] != viewport:
        raise ValueError("layout viewport differs from the frozen fixture")
    pixels = ppm_pixels(image, viewport["width"], viewport["height"])
    for x, y, red, green, blue in case["expected"]["pixels"]:
        if not (0 <= x < viewport["width"] and 0 <= y < viewport["height"]):
            raise ValueError("oracle pixel lies outside viewport")
        offset = (y * viewport["width"] + x) * 3
        if pixels[offset:offset + 3] != bytes((red, green, blue)):
            raise ValueError(f"pixel oracle mismatch at ({x}, {y})")
    by_id = {}
    for node in dom["nodes"]:
        identifier = node.get("attributes", {}).get("id")
        if identifier:
            if identifier in by_id:
                raise ValueError("geometry fixture contains duplicate element IDs")
            by_id[identifier] = node["id"]
    by_node = {box["node"]: box for box in layout["boxes"]}
    for identifier, expected in case["expected"]["boxes"].items():
        box = by_node[by_id[identifier]]
        for edge, values in expected.items():
            actual = box[{"content": "content_box", "border": "border_box"}[edge]]
            if [actual[key] for key in ("x", "y", "width", "height")] != values:
                raise ValueError(f"geometry oracle mismatch for #{identifier} {edge}")


def invoke(binary: Path, args: list[str], timeout: int) -> subprocess.CompletedProcess:
    result = subprocess.run([str(binary), *args], cwd=ROOT, capture_output=True, timeout=timeout, check=False)
    # Preserve diagnostics without making transient artifact directory names part
    # of the corpus identity. Output paths can be recreated in any fresh directory.
    def normalized(value: str) -> str:
        value = value.replace(str(ROOT), ".")
        return re.sub(r"[^\s]*browser-replay-[^/\s]+", "<run-directory>", value)
    COMMAND_LOG.append({
        "arguments": [normalized(str(binary)), *[normalized(arg) for arg in args]],
        "exit_code": result.returncode,
        "stdout": result.stdout.decode("utf-8", errors="replace"),
        "stderr": result.stderr.decode("utf-8", errors="replace"),
    })
    return result


def source_identity() -> dict:
    def git(*args: str) -> str:
        result = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=False)
        if result.returncode:
            raise ValueError(f"cannot identify Git source state: {result.stderr.strip()}")
        return result.stdout.strip()
    return {"commit": git("rev-parse", "HEAD"), "worktree_dirty": bool(git("status", "--porcelain", "--untracked-files=normal"))}


def compiler_identity() -> str:
    local_compiler = ROOT / "artifacts/toolchains/cargo/bin/rustc"
    environment = os.environ.copy()
    if local_compiler.is_file():
        compiler = str(local_compiler)
        environment["CARGO_HOME"] = str(ROOT / "artifacts/toolchains/cargo")
        environment["RUSTUP_HOME"] = str(ROOT / "artifacts/toolchains/rustup")
    else:
        compiler = shutil.which("rustc")
        if compiler is None:
            raise ValueError("Rust compiler not found; install the pinned toolchain")
    result = subprocess.run([compiler, "-Vv"], cwd=ROOT, capture_output=True, text=True, env=environment, check=False)
    if result.returncode:
        raise ValueError(f"cannot identify Rust compiler: {result.stderr.strip()}")
    return result.stdout.strip()


def execute_case(binary: Path, case: dict, repeat: int, timeout: int) -> dict:
    with tempfile.TemporaryDirectory(prefix="browser-replay-") as temp:
        directory = Path(temp)
        if case["kind"] == "reject":
            image = directory / "reject.ppm"
            layout = directory / "reject.json"
            result = invoke(binary, ["render", "--fixture", case["input"], "--output", str(image), "--layout", str(layout)], timeout)
            diagnostic = result.stderr.decode("utf-8", errors="replace")
            if result.returncode <= 0 or image.exists() or layout.exists():
                raise ValueError("expected explicit rejection without output artifacts")
            if case["expected"].get("diagnostic_contains", "") not in diagnostic:
                raise ValueError(f"expected rejection diagnostic not found: {diagnostic}")
            return {"id": case["id"], "outcome": "pass", "executions": 1, "diagnostic": diagnostic.strip()}
        if case["kind"] != "render":
            raise ValueError(f"no bootstrap executor for kind {case['kind']!r}")
        dom_path = directory / "dom.json"
        result = invoke(binary, ["inspect", "--fixture", case["input"], "--output", str(dom_path)], timeout)
        if result.returncode != 0:
            raise ValueError(result.stderr.decode("utf-8", errors="replace"))
        dom = json.loads(dom_path.read_text())
        first = None
        for iteration in range(repeat):
            image_path, layout_path = directory / f"{iteration}.ppm", directory / f"{iteration}.json"
            viewport = case["viewport"]
            result = invoke(binary, ["render", "--fixture", case["input"], "--output", str(image_path), "--layout", str(layout_path), "--width", str(viewport["width"]), "--height", str(viewport["height"])], timeout)
            if result.returncode != 0:
                raise ValueError(result.stderr.decode("utf-8", errors="replace"))
            image, layout_bytes = image_path.read_bytes(), layout_path.read_bytes()
            assert_render(case, image, json.loads(layout_bytes), dom)
            hashes = {"image_sha256": digest(image), "layout_sha256": digest(layout_bytes)}
            if first is None:
                first = hashes
            elif hashes != first:
                raise ValueError(f"nondeterministic output at repetition {iteration + 1}")
            image_path.unlink()
            layout_path.unlink()
        return {"id": case["id"], "outcome": "pass", "executions": repeat, **first}


def main() -> int:
    started_at = datetime.datetime.now(datetime.timezone.utc).isoformat()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/browser-engine")
    parser.add_argument("--repeat", type=int, default=100)
    parser.add_argument("--report", type=Path, default=ROOT / "artifacts/m1/bootstrap-report.json")
    args = parser.parse_args()
    if not 1 <= args.repeat <= 1000:
        parser.error("--repeat must be between 1 and 1000")
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error("binary not found; run cargo build --locked --offline first")
    manifest = ROOT / "planning/m1-cohort.json"
    try:
        contract = json.loads(manifest.read_text())
        verify_contract(ROOT, contract)
        source = source_identity()
        compiler = compiler_identity()
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.error(f"invalid fixture contract: {error}")
    selected = [case for case in contract["cases"] if case["stage"] == "bootstrap"]
    results = []
    for case in selected:
        try:
            result = execute_case(binary, case, args.repeat, contract["limits"]["wall_time_seconds"])
        except (ValueError, OSError, KeyError, subprocess.TimeoutExpired) as error:
            result = {"id": case["id"], "outcome": "fail", "error": str(error)}
        results.append(result)
    source_paths = [*sorted((ROOT / "src").glob("*.rs")), ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "rust-toolchain.toml"]
    passed = sum(result["outcome"] == "pass" for result in results)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    command_log_path = args.report.with_suffix(".commands.json")
    command_log_path.write_text(json.dumps({"temporary_output_paths_normalized": True, "commands": COMMAND_LOG}, indent=2) + "\n")
    report = {
        "cohort": contract["id"],
        "cohort_sha256": digest(manifest.read_bytes()),
        "binary_sha256": digest(binary.read_bytes()),
        "source_sha256": {str(path.relative_to(ROOT)): digest(path.read_bytes()) for path in source_paths},
        "source": source,
        "runner_sha256": digest(Path(__file__).read_bytes()),
        "case_input_sha256": {case["id"]: digest(local_path(ROOT, case["input"]).read_bytes()) for case in contract["cases"] if case.get("input") is not None},
        "compiler": compiler,
        "invocation": [sys.executable, *sys.argv],
        "started_at_utc": started_at,
        "finished_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "command_log": {"file": command_log_path.name, "sha256": digest(command_log_path.read_bytes())},
        "platform": {"os": platform.system(), "release": platform.release(), "architecture": platform.machine(), "python": platform.python_version(), "cpu_count": os.cpu_count(), "locale": locale.getlocale()},
        "summary": {"bootstrap_selected": len(selected), "bootstrap_passed": passed, "full_m1_not_run": len(contract["cases"]) - len(selected), "wpt_executed": 0, "repetitions_per_render": args.repeat},
        "limits": {"wall_time_seconds_per_invocation": contract["limits"]["wall_time_seconds"], "rss_containment_enforced": False},
        "cases": results,
        "boundary": "Bootstrap fixtures only. Not full M1 completion, WPT conformance, sandbox evidence or sustained fuzzing.",
    }
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["summary"], sort_keys=True))
    for result in results:
        if result["outcome"] != "pass":
            print(f"FAIL {result['id']}: {result['error']}")
    print(f"Evidence: {args.report}")
    return 0 if passed == len(selected) else 1


if __name__ == "__main__":
    raise SystemExit(main())
