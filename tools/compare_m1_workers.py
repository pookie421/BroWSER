#!/usr/bin/env python3
"""Compare full-M1 replay artifacts and the recorded worker environments."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

if __package__:
    from . import run_m1_bootstrap as runner
else:
    import run_m1_bootstrap as runner


def sha256_value(value) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[a-f0-9]{64}", value) is not None


def check_file(bundle: Path, name: str, expected: str) -> None:
    runner.require(isinstance(name, str) and not Path(name).is_absolute()
                   and ".." not in Path(name).parts and sha256_value(expected),
                   "invalid relative artifact name or SHA-256")
    path = runner.local_path(bundle, name)
    runner.require(runner.file_digest(path) == expected, f"artifact hash mismatch: {name}")


def build_signature(build: dict) -> dict:
    # Cargo generates an isolated, uniquely named target directory per replay.
    flags = list(build["flags"])
    directory = flags.index("--target-dir")
    del flags[directory:directory + 2]
    return {
        "compiler": build["compiler"], "cargo": build["cargo"], "target": build["target"],
        "flags": flags, "cargo_configuration": build["cargo_configuration"],
        "compiler_overrides": {
            key: value for key, value in build["compiler_overrides"].items() if key != "RUSTC"
        },
    }


def case_signature(case: dict) -> tuple[set[str], list[tuple[str, int]]]:
    kind = case["kind"]
    if kind in ("inspect", "inspect_fragment"):
        return {"dom.json"}, [("inspect", 0)]
    if kind == "style":
        return {"dom.json", "style.json"}, [("inspect", 0), ("style", 0)]
    if kind == "shape":
        return {"shape.json"}, [("shape", 0)]
    if kind == "decode_reject":
        return set(), [("decode-png", 1)]
    if kind == "generated_boundary":
        viewport = case["limit"] == "viewport_max"
        command, extension = ("render", "ppm") if viewport else ("inspect", "json")
        return {f"{value}.{extension}" for value in case["values"][:-1]}, [
            (command, int(index == len(case["values"]) - 1)) for index in range(len(case["values"]))
        ]
    if kind in ("render", "layout"):
        outputs = {"dom.json", "render.ppm", "layout.json"}
        commands = [("inspect", 0), ("render", 0)]
        if "decoded_width" in case["expected"]:
            outputs.add("decode.json")
            commands.append(("decode-png", 0))
        return outputs, commands
    raise ValueError(f"unsupported full-M1 case kind: {kind}")


def load_bundle(path: Path, contract: dict) -> dict:
    report = runner.strict_json(path.read_bytes())
    runner.require(isinstance(report, dict) and report.get("stage") == "full_m1"
                   and report.get("build_identity_verified") is True and report.get("identity_error") is None
                   and report.get("evidence_kind") == "build_bound_full_m1",
                   "expected a build-bound full-M1 report")
    runner.require(report["cohort"] == contract["id"]
                   and report["cohort_sha256"] == runner.REVIEWED_COHORT[1]
                   and report["asset_sha256"] == {asset["path"]: asset["sha256"] for asset in contract["assets"]},
                   "report uses a different frozen cohort or corpus")
    runner.require(report["source_sha256"] == report["build"]["inputs"]
                   and report["binary_sha256"] == report["build"]["binary_sha256"]
                   and sha256_value(report["binary_sha256"]),
                   "report and build identities disagree")

    bundle = path.parent.resolve()
    command_log = report["command_log"]
    check_file(bundle, command_log["file"], command_log["sha256"])
    commands = runner.strict_json((bundle / command_log["file"]).read_bytes())["commands"]
    frozen = {case["id"]: case for case in contract["cases"] if case["stage"] == "full_m1"}
    cases = {case["id"]: case for case in report["cases"]}
    runner.require(cases.keys() == frozen.keys() and len(cases) == len(report["cases"]),
                   "report uses a different case roster")

    repetitions = report["summary"]["repetitions_per_case"]
    runner.require(type(repetitions) is int and 100 <= repetitions <= 1000,
                   "expected at least 100 repetitions of each full-M1 case")
    rows = {identifier: {} for identifier in frozen}
    invocations = {identifier: {} for identifier in frozen}
    limits = report["limits"]
    rss_limit = limits["fixture_rss_limit_bytes"]
    timeout = limits["wall_time_seconds_per_invocation"]
    runner.require(timeout == contract["limits"]["wall_time_seconds"],
                   "report uses a different fixture deadline")
    if limits["rss_policy"] == "sampled":
        runner.require(rss_limit == contract["limits"]["resident_memory_bytes"] and bool(limits["rss_backend"]),
                       "report uses a different RSS limit or lacks its backend")
    else:
        runner.require(limits["rss_policy"] == "off" and rss_limit is None,
                       "invalid report RSS policy")
    for command in commands:
        identifier = command["case"]
        if identifier == "build":
            continue
        runner.require(identifier in rows and command["outcome"] in ("success", "exit_nonzero"),
                       "command log contains a failed or unknown fixture invocation")
        if rss_limit is not None:
            peak = command.get("peak_rss_bytes")
            kernel = command.get("kernel_peak_rss_bytes")
            sampled = command.get("sampled_peak_rss_bytes")
            runner.require(type(peak) is int and type(kernel) is int and type(sampled) is int
                           and kernel > 0 and sampled >= 0 and peak == max(kernel, sampled)
                           and peak <= rss_limit and command["limit_trigger"] is None,
                           f"{identifier} has missing, inconsistent, or above-limit peak RSS")
        repetition = command["repetition"]
        runner.require(type(repetition) is int and 1 <= repetition <= repetitions,
                       f"{identifier} has an invalid repetition index")
        duration = command["duration_seconds"]
        runner.require(type(duration) in (int, float) and 0 <= duration <= timeout,
                       f"{identifier} has a missing or above-limit duration")
        invocations[identifier].setdefault(repetition, []).append(command)
        if "output_sha256" in command:
            runner.require(repetition not in rows[identifier], f"{identifier} has duplicate repetition results")
            rows[identifier][repetition] = command["output_sha256"]

    expected_repetitions = set(range(1, repetitions + 1))
    outcomes = {}
    for identifier, case in cases.items():
        outcome = case["outcome"]
        expected_outcome = ("review_pending" if "reference_requirement" in frozen[identifier]["expected"]
                            else "pass")
        runner.require(outcome == expected_outcome, f"{identifier} requires outcome {expected_outcome}, got {outcome}")
        expected_files, expected_commands = case_signature(frozen[identifier])
        hashes = case["output_sha256"]
        runner.require(isinstance(hashes, dict) and hashes.keys() == expected_files,
                       f"{identifier} has an incomplete output roster")
        runner.require(case["executions"] == repetitions and case["attempts"] == repetitions * len(expected_commands)
                       and rows[identifier].keys() == expected_repetitions
                       and invocations[identifier].keys() == expected_repetitions
                       and all([(record["operation"], record["exit_code"]) for record in commands] == expected_commands
                               for commands in invocations[identifier].values())
                       and all(recorded == hashes for recorded in rows[identifier].values()),
                       f"{identifier} has incomplete or inconsistent repetition hashes")
        for commands in invocations[identifier].values():
            diagnostics = [[record["exit_code"], record["stdout"], record["stderr"]] for record in commands]
            observed = runner.digest(json.dumps(diagnostics).encode("utf-8"))
            runner.require(observed == case["diagnostics_sha256"],
                           f"{identifier} has inconsistent repetition diagnostics")
        for name, checksum in case["artifacts"].items():
            runner.require(name.startswith(f"cases/{identifier}/"), f"{identifier} artifact is outside its case directory")
            check_file(bundle, name, checksum)
        for name, checksum in hashes.items():
            runner.require(case["artifacts"].get(f"cases/{identifier}/run-0001/{name}") == checksum,
                           f"{identifier} is missing its first retained output: {name}")
        outcomes[identifier] = {
            "outcome": outcome, "outputs": hashes, "diagnostics_sha256": case["diagnostics_sha256"],
        }
    return {
        "run_id": report["run_id"], "worker_id": report.get("worker_id"),
        "platform": report["platform"], "source": report["source"],
        "source_sha256": report["source_sha256"], "runner_sha256": report["runner_sha256"],
        "runner_support_sha256": report["runner_support_sha256"],
        "build": build_signature(report["build"]), "limits": limits, "repetitions": repetitions,
        "outcomes": outcomes,
    }


def compare(first: dict, second: dict) -> dict:
    runner.require(first["run_id"] != second["run_id"], "provide two distinct run IDs")
    for key in ("source", "source_sha256", "runner_sha256", "runner_support_sha256",
                "build", "platform", "limits", "repetitions"):
        runner.require(first[key] == second[key], f"worker reports differ in {key}")
    differences = sorted(case for case, result in first["outcomes"].items()
                         if result != second["outcomes"].get(case))
    runner.require(not differences, f"fixture outcomes or hashes differ: {', '.join(differences)}")
    ids = [first["worker_id"], second["worker_id"]]
    return {
        "runs": [first["run_id"], second["run_id"]], "worker_ids": ids,
        "distinct_declared_worker_ids": bool(all(ids) and ids[0] != ids[1]),
        "matching_cases": len(first["outcomes"]), "repetitions_per_case": first["repetitions"],
        "review_pending": [case for case, result in first["outcomes"].items()
                           if result["outcome"] == "review_pending"],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("first", type=Path)
    parser.add_argument("second", type=Path)
    args = parser.parse_args(argv)
    try:
        manifest = runner.ROOT / "planning/m1-cohort.json"
        contract = runner.verify_contract(runner.ROOT, manifest.read_bytes())
        first, second = (load_bundle(path, contract) for path in (args.first, args.second))
        result = compare(first, second)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, IndexError, RuntimeError) as error:
        print(f"M1 comparison failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
