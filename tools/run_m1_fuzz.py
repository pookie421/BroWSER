#!/usr/bin/env python3
"""Run both M1 coverage-guided targets in a bounded foreground campaign."""

from __future__ import annotations

import argparse
from contextlib import ExitStack
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
NIGHTLY = "nightly-2026-11-16"
FUZZ_VERSION = "cargo-fuzz 0.13.2"
MANIFEST = Path("planning/m1-cohort.json")
TARGET_SEEDS = {"html_parse": 1201, "render_fixture": 1202}
OBSERVATION_SECONDS = 7 * 24 * 60 * 60
MAX_INPUT_BYTES = 4096
MAX_RSS_MIB = 512
INPUT_TIMEOUT_SECONDS = 3
MAX_EVIDENCE_BYTES = 256 * 1024 * 1024
MAX_EVIDENCE_FILES = 100_000
END_GRACE_SECONDS = 15
MAX_CHECKPOINT_GAP_SECONDS = 120


def file_digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def write_report(run: Path, report: dict) -> None:
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=run, delete=False) as stream:
        json.dump(report, stream, indent=2, sort_keys=True)
        stream.write("\n")
    os.replace(stream.name, run / "report.json")


def source_hashes(root: Path) -> dict[str, str]:
    contract = json.loads((root / MANIFEST).read_bytes())
    paths = {
        MANIFEST, Path("Cargo.toml"), Path("Cargo.lock"), Path("rust-toolchain.toml"),
        Path("fuzz/Cargo.toml"), Path("fuzz/Cargo.lock"), Path("tools/run_m1_fuzz.py"),
        *(path.relative_to(root) for path in (root / "src").rglob("*.rs")),
        *(path.relative_to(root) for path in (root / "fuzz/fuzz_targets").glob("*.rs")),
    }
    expected = {}
    for asset in contract["assets"]:
        path = Path(asset["path"])
        if path.parts[:2] != ("fixtures", "m1") or ".." in path.parts:
            raise ValueError(f"invalid cohort asset path: {path}")
        paths.add(path)
        expected[path.as_posix()] = asset["sha256"]

    result = {}
    for path in sorted(paths):
        absolute = root / path
        if absolute.resolve() != absolute or not absolute.is_file():
            raise ValueError(f"source must be a regular file inside the checkout: {path}")
        result[path.as_posix()] = file_digest(absolute)
    if any(result[path] != value for path, value in expected.items()):
        raise ValueError("a frozen M1 asset differs from its recorded bytes")
    return result


def seed_corpus(source: Path, directory: Path, target: str, inputs: dict[str, str]) -> dict[str, str]:
    directory.mkdir()
    seeds = {}
    for name in sorted(inputs):
        path = Path(name)
        if len(path.parts) != 4 or path.parts[:2] != ("fixtures", "m1"):
            continue
        profile = path.parent.name
        if profile not in ("bootstrap", "full") or path.suffix != ".html":
            continue
        payload = (source / path).read_bytes()
        if target == "render_fixture":
            payload = bytes((profile == "full", 64, 64)) + payload
        if not 0 < len(payload) <= MAX_INPUT_BYTES:
            raise ValueError(f"fuzz seed has invalid size: {path}")
        filename = f"{profile}-{path.name}"
        (directory / filename).write_bytes(payload)
        seeds[filename] = hashlib.sha256(payload).hexdigest()
    if not seeds:
        raise ValueError("the M1 fuzz seed corpus is empty")
    return seeds


def tool_env() -> dict[str, str]:
    env = {key: os.environ[key] for key in ("HOME", "PATH", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME")
           if key in os.environ}
    env.update(CARGO_NET_OFFLINE="true", LC_ALL="C", TZ="UTC", ASAN_OPTIONS="halt_on_error=1")
    return env


def identify_tools(source: Path, env: dict[str, str]) -> dict:
    def version(command):
        return subprocess.check_output(command, cwd=source, env=env, text=True, timeout=15).strip()

    cargo_fuzz = version(["cargo", f"+{NIGHTLY}", "fuzz", "--version"])
    if cargo_fuzz != FUZZ_VERSION:
        raise ValueError(f"expected {FUZZ_VERSION}, got {cargo_fuzz}")
    rustc = version(["rustc", f"+{NIGHTLY}", "-vV"])
    target = re.search(r"^host: ([a-z0-9_-]+)$", rustc, re.MULTILINE)
    if target is None:
        raise ValueError("cannot determine the pinned rustc host triple")
    binaries = {name: Path(version(["rustup", "which", "--toolchain", NIGHTLY, name]))
                for name in ("rustc", "cargo")}
    cargo_fuzz_path = shutil.which("cargo-fuzz", path=env.get("PATH"))
    if cargo_fuzz_path is None:
        raise ValueError("cargo-fuzz is not on PATH")
    binaries["cargo-fuzz"] = Path(cargo_fuzz_path)
    return {"nightly": NIGHTLY, "cargo_fuzz": cargo_fuzz, "rustc": rustc,
            "cargo": version(["cargo", f"+{NIGHTLY}", "--version"]), "target": target[1],
            "executables_sha256": {name: file_digest(path) for name, path in binaries.items()}}


def cargo_config_hashes(source: Path, env: dict[str, str]) -> dict[str, str]:
    directories = {
        Path(env.get("CARGO_HOME", Path.home() / ".cargo")),
        *(parent / ".cargo" for parent in (source, *source.parents)),
    }
    # Cargo accepts the legacy and TOML names. Record each file present so a
    # lockfile alone cannot conceal a changed local registry or tool wrapper.
    return {str(path): file_digest(path) for directory in sorted(directories)
            for path in (directory / "config", directory / "config.toml") if path.is_file()}


def stop(process: subprocess.Popen) -> None:
    if process.returncode is None:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def build(source: Path, target_dir: Path, run: Path, tools: dict, env: dict, report: dict) -> None:
    cargo = ["cargo", f"+{NIGHTLY}"]
    commands = [
        [*cargo, "fetch", "--offline", "--locked", "--manifest-path", "fuzz/Cargo.toml"],
        [*cargo, "fuzz", "build", "--dev", "--sanitizer", "address",
         "--target", tools["target"], "--target-dir", str(target_dir)],
    ]
    with (run / "build.log").open("wb") as log:
        for command in commands:
            report["build_commands"].append(command)
            process = subprocess.Popen(
                command, cwd=source, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True,
            )
            try:
                status = process.wait(timeout=600)
            except BaseException:
                stop(process)
                process.wait()
                raise
            if status != 0:
                raise ValueError(f"fuzz build or locked dependency check exited {status}. See build.log")


def read_stats(path: Path) -> dict:
    stats = {}
    with path.open(encoding="utf-8", errors="replace") as stream:
        for line in stream:
            if match := re.match(r"stat::([a-z_]+):\s+(\d+)", line):
                stats[match[1]] = int(match[2])
            if match := re.match(r"INFO: Loaded \d+ modules?\s+\((\d+) inline 8-bit counters?\)", line):
                stats["instrumented_counters"] = int(match[1])
            if match := re.match(r"#(\d+)\s+DONE\s+cov: (\d+) ft: (\d+)", line):
                stats.update(final_executions=int(match[1]), covered_edges=int(match[2]), features=int(match[3]))
            if match := re.match(r"Done \d+ runs in (\d+) second\(s\)", line):
                stats["fuzzer_elapsed_seconds"] = int(match[1])
    return stats


def artifacts_size(run: Path) -> bool:
    size, count = 0, 0
    for target in TARGET_SEEDS:
        for path in (run / target).rglob("*"):
            if path.is_file():
                size += path.stat().st_size
                count += 1
                if size > MAX_EVIDENCE_BYTES or count > MAX_EVIDENCE_FILES:
                    return False
    return True


def collect_process(child: dict, flags: int) -> bool:
    process = child["process"]
    if process.returncode is not None:
        return True
    sampled_at = time.monotonic()
    pid, status, usage = os.wait4(process.pid, flags)
    if not pid:
        child["last_seen_alive"] = sampled_at
        return False
    process.returncode = os.waitstatus_to_exitcode(status)
    child["metrics"].update(
        exit_code=process.returncode,
        finished_utc=utc_now(),
        cpu_seconds=usage.ru_utime + usage.ru_stime,
        peak_rss_bytes=usage.ru_maxrss * (1024 if sys.platform == "linux" else 1),
        observed_wall_seconds=time.monotonic() - child["started"],
    )
    return True


def enforce_evidence(child: dict, directory: Path, runs: int | None, seconds: int | None) -> None:
    metrics = child["metrics"]
    stats = read_stats(directory / "fuzzer.log")
    metrics["libfuzzer"] = stats
    if metrics["exit_code"] != 0 or any((directory / "failures").iterdir()):
        metrics["status"] = "failed"
        raise ValueError(f"{directory.name} produced a failure or exited {metrics['exit_code']}")
    if not 0 < metrics["peak_rss_bytes"] <= MAX_RSS_MIB * 1024**2:
        metrics["status"] = "failed"
        raise ValueError(f"{directory.name} has a missing or over-limit process RSS measurement")
    if not (stats.get("instrumented_counters", 0) > 0 and stats.get("covered_edges", 0) > 0
            and stats.get("number_of_executed_units", 0) > 0
            and stats["number_of_executed_units"] == stats.get("final_executions")):
        raise ValueError(f"{directory.name} has no complete positive coverage and execution evidence")
    if runs is not None and stats["number_of_executed_units"] != runs:
        raise ValueError(f"{directory.name} did not complete the requested execution count")
    if seconds is not None and stats.get("fuzzer_elapsed_seconds", -1) < seconds:
        raise ValueError(f"{directory.name} did not complete the requested time window")
    metrics["status"] = "complete"


def run_targets(run: Path, source: Path, target_dir: Path, root: Path, args, report: dict, env: dict) -> None:
    watchdog = (args.seconds or 300) + END_GRACE_SECONDS
    children = {}
    try:
        with ExitStack() as streams:
            for target, seed in TARGET_SEEDS.items():
                directory = run / target
                directory.mkdir()
                (directory / "failures").mkdir()
                seeds = seed_corpus(source, directory / "corpus", target, report["source_sha256"])
                executable = target_dir / report["tools"]["target"] / "debug" / target
                command = [
                    str(executable), f"-seed={seed}", f"-max_len={MAX_INPUT_BYTES}",
                    f"-rss_limit_mb={MAX_RSS_MIB}", f"-timeout={INPUT_TIMEOUT_SECONDS}",
                    "-print_final_stats=1", "-reload=0",
                    f"-artifact_prefix={directory / 'failures'}/", str(directory / "corpus"),
                    f"-max_total_time={args.seconds}" if args.seconds is not None else f"-runs={args.runs}",
                ]
                log = streams.enter_context((directory / "fuzzer.log").open("wb"))
                metrics = {"status": "running", "started_utc": utc_now(), "command": command,
                           "seed_sha256": seeds, "executable_sha256": file_digest(executable)}
                report["targets"][target] = metrics
                process = subprocess.Popen(
                    command, cwd=directory, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True,
                )
                started = time.monotonic()
                children[target] = {
                    "process": process, "metrics": metrics, "started": started, "last_seen_alive": started,
                }
            write_report(run, report)
            next_check = 0.0
            last_sample = time.time()
            while True:
                now_wall = time.time()
                if abs(now_wall - last_sample) > MAX_CHECKPOINT_GAP_SECONDS:
                    raise ValueError("fuzz process observation was interrupted or the wall clock changed")
                last_sample = now_wall
                running = []
                for target, child in children.items():
                    if collect_process(child, os.WNOHANG):
                        if child["metrics"]["status"] == "running":
                            enforce_evidence(child, run / target, args.runs, args.seconds)
                    else:
                        running.append(child)
                if not running:
                    break
                now = time.monotonic()
                if any(now - child["started"] > watchdog for child in running):
                    raise ValueError("external fuzz watchdog expired")
                if now >= next_check:
                    if not artifacts_size(run):
                        raise ValueError("fuzz evidence exceeded the campaign storage limit")
                    if source_hashes(root) != report["source_sha256"]:
                        raise ValueError("checkout source changed during the campaign")
                    next_check = now + 10
                time.sleep(0.1)
    finally:
        for child in children.values():
            stop(child["process"])
        for target, child in children.items():
            collect_process(child, 0)
            metrics = child["metrics"]
            if metrics["status"] == "running":
                metrics.update(status="incomplete", libfuzzer=read_stats(run / target / "fuzzer.log"))

    start = max(child["started"] for child in children.values())
    end = min(child["last_seen_alive"] for child in children.values())
    report["concurrent_window_seconds_lower_bound"] = max(0.0, end - start)


def run_campaign(root: Path, run: Path, args) -> dict:
    report = {
        "schema": 1,
        "status": "running",
        "started_utc": utc_now(),
        "mode": "timed" if args.seconds is not None else "bounded_smoke",
        "requested_runs_per_target": args.runs,
        "requested_seconds_per_target": args.seconds,
        "platform": {"system": platform.system(), "release": platform.release(), "machine": platform.machine()},
        "limits": {"input_bytes": MAX_INPUT_BYTES, "input_timeout_seconds": INPUT_TIMEOUT_SECONDS,
                   "libfuzzer_sampled_rss_mib": MAX_RSS_MIB, "evidence_bytes": MAX_EVIDENCE_BYTES},
        "build_commands": [],
        "targets": {},
    }
    write_report(run, report)
    previous_term = signal.signal(signal.SIGTERM, signal.default_int_handler)
    try:
        report["source_sha256"] = source_hashes(root)
        report["base_commit"] = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True, timeout=15,
        ).strip()
        env = tool_env()
        with tempfile.TemporaryDirectory(dir=run, prefix="build-") as build_dir:
            source = Path(build_dir) / "source"
            target_dir = Path(build_dir) / "target"
            for name in report["source_sha256"]:
                destination = source / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(root / name, destination)
            if source_hashes(source) != report["source_sha256"]:
                raise ValueError("source changed while constructing the campaign snapshot")
            report["tools"] = identify_tools(source, env)
            report["cargo_config_sha256"] = cargo_config_hashes(source, env)
            build(source, target_dir, run, report["tools"], env, report)
            if source_hashes(source) != report["source_sha256"] or source_hashes(root) != report["source_sha256"]:
                raise ValueError("source or dependency lock changed during the fuzz build")
            run_targets(run, source, target_dir, root, args, report, env)
            if source_hashes(source) != report["source_sha256"] or source_hashes(root) != report["source_sha256"]:
                raise ValueError("source or dependency lock changed during the fuzz campaign")
            if cargo_config_hashes(source, env) != report["cargo_config_sha256"]:
                raise ValueError("Cargo configuration changed during the fuzz campaign")
            for target, metrics in report["targets"].items():
                exe = target_dir / report["tools"]["target"] / "debug" / target
                if metrics["executable_sha256"] != file_digest(exe):
                    raise ValueError(f"{target} executable changed during the fuzz campaign")
        if not artifacts_size(run):
            raise ValueError("fuzz evidence exceeded the campaign storage limit")
        report["status"] = "complete"
    except KeyboardInterrupt:
        report.update(status="interrupted", error="campaign interrupted")
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        report.update(status="incomplete", error=str(error))
    finally:
        signal.signal(signal.SIGTERM, previous_term)
        report["finished_utc"] = utc_now()
        report["seven_day_window_observed"] = (
            report["status"] == "complete"
            and (args.seconds or 0) >= OBSERVATION_SECONDS
            and report.get("concurrent_window_seconds_lower_bound", 0) >= OBSERVATION_SECONDS
        )
        # Keep a manifest of all the actual inputs, diagnostics and failures in
        # the run directory. Working build snapshots and binaries are temporary.
        report["evidence_sha256"] = {
            path.relative_to(run).as_posix(): file_digest(path)
            for path in sorted(run.rglob("*")) if path.is_file() and path != run / "report.json"
        }
        write_report(run, report)
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--runs", type=int, help="execution budget per target (default 5000)")
    modes.add_argument("--seconds", type=int, help="concurrent time budget in seconds per target")
    args = parser.parse_args()
    if platform.system() not in ("Darwin", "Linux"):
        parser.error("the campaign requires macOS or Linux")
    if args.runs is None and args.seconds is None:
        args.runs = 5000
    if args.runs is not None and not 1 <= args.runs <= 1_000_000:
        parser.error("--runs must be between 1 and 1000000")
    if args.seconds is not None and not 1 <= args.seconds <= OBSERVATION_SECONDS + 60:
        parser.error(f"--seconds must be between 1 and {OBSERVATION_SECONDS + 60}")
    output = ROOT / "artifacts" / "m1"
    output.mkdir(parents=True, exist_ok=True)
    run = Path(tempfile.mkdtemp(dir=output, prefix="fuzz-"))
    print(f"Fuzz evidence: {run.relative_to(ROOT)}/report.json", flush=True)
    report = run_campaign(ROOT, run, args)
    print(json.dumps({key: report[key] for key in ("status", "seven_day_window_observed")}))
    if "error" in report:
        print(report["error"])
    return 0 if report["status"] == "complete" else 1


if __name__ == "__main__":
    raise SystemExit(main())
