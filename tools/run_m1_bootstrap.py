#!/usr/bin/env python3
"""Build and replay a frozen M1 fixture cohort, retaining a bundle per run."""

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

if __package__:
    from . import m1_limits
else:
    import m1_limits

ROOT = Path(__file__).resolve().parents[1]
# The digest pins the exact asset roster as well as case stages, inputs and oracles.
REVIEWED_COHORT = ("M1-v2", "9424c617b2eee264072ab9c52f85a199c819ca481de9700c16bf2b5ec129b06e")
CASE_GROUPS = {"B": 5, "H": 6, "C": 3, "L": 2, "T": 5, "P": 3, "R": 4}
OUTCOMES = ("pass", "fail", "timeout", "crash", "resource_limit", "harness_error", "review_pending")


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
    require(isinstance(layout, dict) and layout.keys() in ({"viewport", "boxes"}, {"viewport", "boxes", "text"})
            and viewport_is_valid(layout["viewport"])
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
    if "text" in layout:
        require(isinstance(layout["text"], list), "invalid inline layout list")
        seen = set()
        for row in layout["text"]:
            require(has_keys(row, {"node", "inline"}) and nonnegative_integer(row["node"])
                    and row["node"] in by_node and row["node"] not in seen, "invalid inline box reference")
            seen.add(row["node"])
            inline = row["inline"]
            require(has_keys(inline, {"collapsed_text", "line_height_px", "content_height_px", "lines"})
                    and isinstance(inline["collapsed_text"], str)
                    and nonnegative_integer(inline["line_height_px"])
                    and nonnegative_integer(inline["content_height_px"])
                    and isinstance(inline["lines"], list), "invalid inline layout")
            for line in inline["lines"]:
                require(has_keys(line, {"x_64", "baseline_64", "run"})
                        and type(line["x_64"]) is int and type(line["baseline_64"]) is int,
                        "invalid inline line position")
                validate_glyph_run(line["run"])
    return by_node


def validate_glyph_run(run) -> None:
    require(has_keys(run, {"text", "direction", "script", "language", "font_scale",
                           "variation", "cluster_units", "glyphs"})
            and all(isinstance(run[field], str) for field in ("text", "direction", "script", "language"))
            and run["direction"] in ("ltr", "rtl") and nonnegative_integer(run["font_scale"])
            and run["font_scale"] > 0 and run["cluster_units"] == "UTF-8 byte offsets"
            and has_keys(run["variation"], {"wdth", "wght"})
            and all(type(value) is int for value in run["variation"].values())
            and isinstance(run["glyphs"], list), "invalid shaped run")
    offsets = set()
    offset = 0
    for character in run["text"]:
        offsets.add(offset)
        offset += len(character.encode("utf-8"))
    for glyph in run["glyphs"]:
        require(has_keys(glyph, {"g", "cl", "dx", "dy", "ax", "ay"})
                and all(type(value) is int for value in glyph.values())
                and glyph["g"] >= 0 and glyph["cl"] in offsets, "invalid glyph or UTF-8 cluster")


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
    case: str, operation: str, repetition: int, env: dict | None = None,
    rss_limit_bytes: int | None = None,
) -> subprocess.CompletedProcess:
    start = time.monotonic()
    record = {"case": case, "operation": operation, "repetition": repetition, "arguments": command}
    try:
        if rss_limit_bytes is not None:
            limited = m1_limits.run_limited(
                command, cwd=ROOT, timeout=timeout, rss_limit_bytes=rss_limit_bytes, env=env
            )
            record.update(
                peak_rss_bytes=limited.peak_rss_bytes,
                kernel_peak_rss_bytes=limited.kernel_peak_rss_bytes,
                sampled_peak_rss_bytes=limited.sampled_peak_rss_bytes,
                limit_trigger=limited.trigger,
            )
            result = subprocess.CompletedProcess(command, limited.returncode, limited.stdout, limited.stderr)
            if limited.termination is not None:
                record.update(
                    outcome=limited.termination, exit_code=result.returncode,
                    stdout=decode_output(result.stdout), stderr=decode_output(result.stderr),
                )
                raise ReplayError(limited.termination, f"{operation} stopped by fixture limits ({limited.trigger})")
        else:
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


def validate_output(check, value):
    try:
        return check(value)
    except ValueError as error:
        raise ReplayError("harness_error", str(error)) from error


def element_children(nodes, parent, name=None):
    return [nodes[child] for child in parent["children"]
            if nodes[child]["type"] == "element" and (name is None or nodes[child]["name"] == name)]


def single_element(nodes, parent, name):
    matches = element_children(nodes, parent, name)
    require(len(matches) == 1, f"expected one <{name}> child")
    return matches[0]


def text_content(nodes, node):
    pending = [node["id"]]
    text = []
    while pending:
        node = nodes[pending.pop()]
        if node["type"] == "text":
            text.append(node["text"])
        pending.extend(reversed(node["children"]))
    return "".join(text)


def assert_html(case, dom):
    nodes, by_id = validate_output(validate_dom, dom)
    root = nodes[dom["root"]]
    expected = case["expected"]
    if case["kind"] == "inspect_fragment":
        elements = element_children(nodes, root)
        require([node["name"] for node in elements] == expected["fragment_children"], "wrong fragment children")
        require(text_content(nodes, root) == expected["text_content"], "wrong fragment text")
        require(elements and elements[-1]["attributes"] == expected["last_child_attributes"],
                "wrong fragment child attributes")
        return
    html = single_element(nodes, root, expected.get("document_element", "html"))
    require(len(element_children(nodes, root)) == 1, "unexpected document element")
    head, body = single_element(nodes, html, "head"), single_element(nodes, html, "body")
    if "doctype" in expected:
        doctypes = [nodes[child]["name"] for child in root["children"] if nodes[child]["type"] == "doctype"]
        require(doctypes == [expected["doctype"]], "wrong document doctype")
    if "head_children" in expected:
        require([node["name"] for node in element_children(nodes, head)] == expected["head_children"], "wrong head children")
    if "title" in expected:
        require(text_content(nodes, single_element(nodes, head, "title")) == expected["title"], "wrong title")
    body_children = element_children(nodes, body)
    if "body_children" in expected:
        require([node["name"] for node in body_children] == expected["body_children"], "wrong body children")
    if "paragraph_texts" in expected:
        paragraphs = [text_content(nodes, node) for node in body_children if node["name"] == "p"]
        require(paragraphs == expected["paragraph_texts"], "wrong paragraph text")
    if "duplicate_attribute_resolution" in expected:
        require(expected["duplicate_attribute_resolution"] == "first-wins"
                and body_children and body_children[0]["attributes"] == expected["first_child_attributes"],
                "wrong duplicate attribute resolution")
    if "style_text" in expected:
        style = single_element(nodes, head, "style")
        require(text_content(nodes, style) == expected["style_text"], "wrong raw style text")
        require(len(element_children(nodes, style)) == expected["style_element_children"], "wrong raw style element children")
    for identifier, oracle in expected.get("elements", {}).items():
        require(identifier in by_id, f"missing element #{identifier}")
        node = nodes[by_id[identifier]]
        require(node["name"] == oracle["tag"] and text_content(nodes, node) == oracle["text_content"],
                f"wrong element #{identifier}")


def assert_style(case, style, dom):
    nodes, by_id = validate_output(validate_dom, dom)
    require(has_keys(style, {"elements"}) and isinstance(style["elements"], list), "invalid style output")
    elements = {}
    for row in style["elements"]:
        require(has_keys(row, {"id", "node", "tag", "properties"}) and nonnegative_integer(row["node"])
                and row["node"] not in elements and isinstance(row["properties"], dict), "invalid computed-style row")
        node = nodes.get(row["node"])
        require(node is not None and node["type"] == "element" and row["tag"] == node["name"]
                and row["id"] == node["attributes"].get("id"), "style row does not match DOM")
        elements[row["node"]] = row["properties"]
    require(elements.keys() == {index for index, node in nodes.items() if node["type"] == "element"},
            "computed style is missing DOM elements")
    expected = case["expected"]
    for identifier, oracle in expected["elements"].items():
        require(identifier in by_id, f"missing computed style for #{identifier}")
        properties = elements[by_id[identifier]]
        for name, value in oracle.items():
            actual = properties.get(name)
            require(type(actual) is type(value) and actual == value,
                    f"computed style mismatch for #{identifier} {name}: {actual!r} != {value!r}")
    if "unknown_property_applied" in expected:
        actual = any("unknown-prop" in properties for properties in elements.values())
        require(actual is expected["unknown_property_applied"], "unknown property was applied")


def glyph_reference(case):
    original = read_json(ROOT / case["expected"]["glyph_reference"])
    return {key: value for key, value in original.items() if key not in ("reference_tool", "command_argv")}


def assert_inline(case, layout, dom, image, reference):
    expected = case["expected"]
    nodes, _ = validate_dom(dom)
    text_rows = layout.get("text", [])
    require(len(text_rows) == 1, "expected one inline formatting result")
    row = text_rows[0]
    inline = row["inline"]
    lines = inline["lines"]
    box = next(box for box in layout["boxes"] if box["node"] == row["node"])["content_box"]
    require(lines and inline["content_height_px"] == inline["line_height_px"] * len(lines),
            "wrong inline lines or line height")
    require(all(current["baseline_64"] - previous["baseline_64"] == inline["line_height_px"] * 64
                for previous, current in zip(lines, lines[1:])), "wrong line baseline spacing")
    for line in lines:
        run = line["run"]
        advance = sum(glyph["ax"] for glyph in run["glyphs"])
        x = box["x"] * 64
        if run["direction"] == "rtl":
            x += box["width"] * 64 - advance
        require(run["glyphs"] and line["x_64"] == x, "wrong inline line origin or missing glyph geometry")
    require(box["y"] * 64 <= lines[0]["baseline_64"] <= (box["y"] + inline["line_height_px"]) * 64,
            "first line baseline is outside its line box")
    observed = inline["collapsed_text"]
    require(observed == re.sub(r"[ \r\n\t\f]+", " ", text_content(nodes, nodes[row["node"]])).strip(" "),
            "collapsed text does not match the inspected DOM")
    if "line_texts" in expected:
        require(observed == expected["collapsed_text"], "wrong collapsed text")
        require([line["run"]["text"] for line in lines] == expected["line_texts"], "wrong wrapped lines")
        require(inline["line_height_px"] == expected["line_height_px"], "wrong observed line height")
        require(inline["content_height_px"] == box["height"] == expected["content_height_px"],
                "wrong observed inline content height")
        for line in lines:
            run = line["run"]
            require(run["variation"] == {"wdth": case["font_width"], "wght": case["font_weight"]},
                    "wrong font variation")
            require(case["font_family"] == "Noto Sans" and run["script"] == "Latn"
                    and all(glyph["g"] for glyph in run["glyphs"]), "wrong inline font or missing glyph")
    if reference is not None:
        require(len(lines) == 1, "expected one shaped line")
        run = lines[0]["run"]
        require(observed == run["text"] == expected["shaped_text"], "wrong shaped text")
        require(run["direction"] == expected["direction"], "wrong inline direction")
        require(sum(glyph["g"] == 0 for glyph in run["glyphs"]) == expected["missing_glyphs"],
                "inline shaping produced a missing glyph")
        require(run == reference, "inline glyph geometry differs from the frozen HarfBuzz oracle")
    viewport = case["viewport"]
    require(any(channel != 255 for channel in ppm_pixels(image, viewport["width"], viewport["height"])),
            "inline render has no visible paint")


def prepare_full_inputs(case, directory):
    if case["kind"] == "shape":
        path = directory / "shape-input.txt"
        path.write_bytes(case["text"].encode("utf-8"))
        return path
    if case["kind"] != "generated_boundary" or case["limit"] == "viewport_max":
        return None
    generator = case["generator"]
    inputs = {}
    for value in case["values"]:
        path = directory / f"input-{value}.html"
        if case["limit"] == "input_bytes":
            prefix = generator["prefix"].encode("utf-8")
            data = prefix + b" " * (value - len(prefix))
        elif case["limit"] == "nodes":
            # The base has document, doctype, html, head and body nodes.
            before, after = generator["base"].split("</body>", 1)
            data = (before + generator["repeat"] * (value - 5) + "</body>" + after).encode("utf-8")
        elif case["limit"] == "depth":
            data = ("<!doctype html><html><head></head><body>" + generator["open"] * (value - 2)
                    + generator["close"] * (value - 2) + "</body></html>").encode("utf-8")
        else:
            raise ReplayError("harness_error", f"unknown boundary limit {case['limit']}")
        path.write_bytes(data)
        inputs[value] = path
    return inputs


def assert_boundary(case, value, result, output):
    limit = case["limit"]
    maximum = case["values"][-2]
    diagnostic = {
        "input_bytes": f"fixture exceeds the {maximum}-byte input limit",
        "nodes": f"node count exceeds {maximum} limit",
        "depth": f"element depth exceeds {maximum} limit",
        "viewport_max": f"--width must be an integer in 1..={maximum}",
    }[limit].encode()
    if value == case["values"][-1]:
        require(result.returncode == 1 and diagnostic in result.stderr and not result.stdout
                and not os.path.lexists(output),
                f"limit exceeded without the expected bounded rejection: {decode_output(result.stderr).strip()}")
        return
    require_success(result, f"{limit} boundary {value}")
    if limit == "viewport_max":
        ppm_pixels(output.read_bytes(), value, case["generator"]["height"])
        return
    dom = read_json(output)
    nodes, _ = validate_output(validate_dom, dom)
    if limit == "input_bytes":
        prefix = case["generator"]["prefix"].encode("utf-8")
        require(text_content(nodes, nodes[dom["root"]]) == " " * (value - len(prefix)),
                "input was truncated at or below the byte limit")
    if limit == "nodes":
        require(len(nodes) == value, "DOM was truncated at or below the node limit")
    if limit == "depth":
        deepest = 0
        pending = [(dom["root"], 0)]
        while pending:
            index, parent_depth = pending.pop()
            depth = parent_depth + (nodes[index]["type"] == "element")
            deepest = max(deepest, depth)
            pending.extend((child, depth) for child in nodes[index]["children"])
        require(deepest == value, "DOM was truncated at or below the depth limit")


def full_attempt(case, repetition, output_dir, input_dir, inputs, reference, launch):
    def run(command, fixture, output, *args):
        return launch(command, repetition, [command, "--fixture", str(fixture), "--output", str(output), *args])

    def json_result(command, fixture, output, *args):
        require_success(run(command, fixture, output, *args), command)
        return read_json(output)

    kind = case["kind"]
    fixture = case["input"]
    if kind in ("inspect", "inspect_fragment"):
        options = ("--fragment-context", case["context_element"]) if kind == "inspect_fragment" else ("--parse-mode", "document")
        # Successful inspection includes the malformed-nesting case's no-fatal-error assertion.
        assert_html(case, json_result("inspect", fixture, output_dir / "dom.json", *options))
    elif kind == "style":
        dom = json_result("inspect", fixture, output_dir / "dom.json", "--parse-mode", "document")
        css = ("--user-css", case["user_stylesheet"]) if "user_stylesheet" in case else ()
        assert_style(case, json_result("style", fixture, output_dir / "style.json", *css), dom)
    elif kind == "shape":
        actual = json_result("shape", inputs, output_dir / "shape.json", "--fixture-root", str(input_dir),
                             "--direction", case["direction"], "--script", case["script"],
                             "--language", case["language"], "--scale", str(reference["font_scale"]))
        validate_output(validate_glyph_run, actual)
        require(actual == reference, "shaped run differs from the frozen HarfBuzz oracle")
    elif kind == "decode_reject":
        output = output_dir / "decode.json"
        result = run("decode-png", case["resource"], output)
        require(result.returncode == 1 and not result.stdout and result.stderr.startswith(b"browser-engine: invalid PNG header: ")
                and not os.path.lexists(output), f"wrong PNG rejection outcome: {decode_output(result.stderr).strip()}")
    elif kind == "generated_boundary":
        for value in case["values"]:
            if case["limit"] == "viewport_max":
                output = output_dir / f"{value}.ppm"
                result = run("render", case["generator"]["input"], output,
                             "--width", str(value), "--height", str(case["generator"]["height"]))
            else:
                output = output_dir / f"{value}.json"
                result = run("inspect", inputs[value], output, "--fixture-root", str(input_dir), "--parse-mode", "document")
            assert_boundary(case, value, result, output)
    elif kind in ("render", "layout"):
        viewport = case["viewport"]
        expected = case["expected"]
        inline = "line_texts" in expected or reference is not None
        dom = json_result("inspect", fixture, output_dir / "dom.json", "--parse-mode", "document")
        image, layout_path = output_dir / "render.ppm", output_dir / "layout.json"
        extra = ["--profile", "m1"]
        for name, path in case.get("resources", {}).items():
            extra.extend(["--resource", f"{name}={path}"])
        require_success(run("render", fixture, image, "--layout", str(layout_path),
                            "--width", str(viewport["width"]), "--height", str(viewport["height"]), *extra), "render")
        image_bytes, layout = image.read_bytes(), read_json(layout_path)
        assert_render(case, image_bytes, layout, dom)
        if inline:
            assert_inline(case, layout, dom, image_bytes, reference)
        if "decoded_width" in expected:
            resources = list(case["resources"].values())
            require(len(resources) == 1, "size oracle requires one PNG resource")
            decoded = json_result("decode-png", resources[0], output_dir / "decode.json")
            require(has_keys(decoded, {"width", "height"}) and nonnegative_integer(decoded["width"])
                    and nonnegative_integer(decoded["height"]) and decoded["width"] == expected["decoded_width"]
                    and decoded["height"] == expected["decoded_height"], "wrong decoded PNG dimensions")
    else:
        raise ReplayError("harness_error", f"unsupported full-M1 case adapter: {kind}")


def execute_full_case(case, repeat, directory, launch, commands):
    inputs = prepare_full_inputs(case, directory)
    reference = glyph_reference(case) if "glyph_reference" in case["expected"] else None
    first = None
    for repetition in range(1, repeat + 1):
        attempt = directory / f"run-{repetition:04}"
        attempt.mkdir()
        start = len(commands)
        full_attempt(case, repetition, attempt, directory, inputs, reference, launch)
        hashes = {path.name: file_digest(path) for path in sorted(attempt.iterdir()) if path.is_file()}
        diagnostic_hash = digest(json.dumps([
            [record["exit_code"], record["stdout"], record["stderr"]] for record in commands[start:]
        ]).encode("utf-8"))
        commands[-1]["output_sha256"] = hashes
        actual = {"outputs": hashes, "diagnostics": diagnostic_hash}
        if first is None:
            first = actual
        else:
            require(actual == first, f"nondeterministic output at repetition {repetition}")
            shutil.rmtree(attempt)
    result = {"output_sha256": first["outputs"], "diagnostics_sha256": first["diagnostics"]}
    if "reference_requirement" in case["expected"]:
        # Independent raster references belong in a reviewed cohort revision.
        result.update(outcome="review_pending", error="Exact glyph geometry matched and paint was produced. The frozen raster review is pending.")
    return result


def execute_case(
    binary: Path, case: dict, repeat: int, timeout: int, directory: Path, commands: list[dict],
    *, rss_limit_bytes: int | None = None,
) -> dict:
    def run(operation, repetition, arguments):
        with tempfile.TemporaryDirectory(prefix=".process-", dir=directory) as scratch:
            env = {"LC_ALL": "C", "TZ": "UTC", "HOME": scratch, "TMPDIR": scratch,
                   "XDG_CONFIG_HOME": scratch, "XDG_DATA_HOME": scratch, "XDG_CACHE_HOME": scratch,
                   "XDG_RUNTIME_DIR": scratch}
            return invoke(commands, [str(binary), *arguments], timeout, case=case["id"],
                          operation=operation, repetition=repetition, rss_limit_bytes=rss_limit_bytes, env=env)

    if case.get("stage") == "full_m1":
        return execute_full_case(case, repeat, directory, run, commands)

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


def execute_cases(
    binary: Path, selected: list[dict], repeat: int, timeout: int, directory: Path, commands: list[dict],
    *, rss_limit_bytes: int | None = None,
) -> list[dict]:
    results = []
    for case in selected:
        outputs = directory / case["id"]
        first_attempt = len(commands)
        result = {"id": case["id"], "outcome": "pass"}
        try:
            outputs.mkdir()
            result.update(execute_case(binary, case, repeat, timeout, outputs, commands,
                                       rss_limit_bytes=rss_limit_bytes))
        except ReplayError as error:
            result.update(outcome=error.outcome, error=str(error))
        except OSError as error:
            result.update(outcome="harness_error", error=str(error))
        except ValueError as error:
            result.update(outcome="fail", error=str(error))
        case_commands = commands[first_attempt:]
        result["attempts"] = len(case_commands)
        peaks = [record["peak_rss_bytes"] for record in case_commands if "peak_rss_bytes" in record]
        if peaks:
            result["peak_rss_bytes"] = max(peaks)
        if case.get("stage") == "full_m1":
            result["executions"] = len({record["repetition"] for record in case_commands})
        else:
            result["executions"] = sum(record["operation"] in ("render", "reject") for record in case_commands)
        result["artifacts"] = {}
        try:
            if outputs.is_dir():
                for path in sorted(outputs.rglob("*")):
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
        "runner_support_sha256": {"tools/m1_limits.py": file_digest(Path(m1_limits.__file__))},
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


def platform_identity() -> dict:
    try:
        physical_memory = os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE")
    except (AttributeError, OSError, ValueError):
        physical_memory = None
    if physical_memory is not None and physical_memory <= 0:
        physical_memory = None
    return {
        "os": platform.system(), "release": platform.release(), "architecture": platform.machine(),
        "processor": platform.processor(), "python": platform.python_version(),
        "cpu_count": os.cpu_count(), "physical_memory_bytes": physical_memory,
        "libc": list(platform.libc_ver()), "locale": list(locale.getlocale()),
    }


def run_bundle(args, contract: dict, manifest: Path, stage: Path, source: dict) -> tuple[dict, bool]:
    started_at = utc_now()
    initial = snapshot(manifest, contract)
    require(initial["cohort_sha256"] == REVIEWED_COHORT[1], "cohort changed before replay")
    require(initial["asset_sha256"] == {asset["path"]: asset["sha256"] for asset in contract["assets"]},
            "fixture assets changed before replay")
    cohort_stage = getattr(args, "stage", "bootstrap").replace("-", "_")
    selected = [case for case in contract["cases"] if case["stage"] == cohort_stage]
    require(bool(selected), "fixture stage does not have any cases")
    commands = []
    artifacts = stage / "cases"
    artifacts.mkdir()
    timeout = contract["limits"]["wall_time_seconds"]
    rss_policy = getattr(args, "rss_policy", None) or ("sampled" if cohort_stage == "full_m1" else "off")
    rss_limit = contract["limits"]["resident_memory_bytes"] if rss_policy == "sampled" else None
    rss_backend = None
    build_record, binary_digest, identity_error = None, None, None
    results = None
    configured_target = os.environ.get("CARGO_TARGET_DIR")
    target_parent = (ROOT / configured_target).resolve() if configured_target else stage
    try:
        if rss_limit is not None:
            rss_backend = m1_limits.rss_backend()
        if args.binary is None:
            target_parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(prefix="browser-replay-target-", dir=target_parent) as target:
                binary, build_record = build(args.cargo, Path(target), initial, commands)
                binary_digest = build_record["binary_sha256"]
                require(snapshot(manifest, contract) == initial, "source, runner or fixtures changed during build")
                results = execute_cases(binary, selected, args.repeat, timeout, artifacts, commands,
                                        rss_limit_bytes=rss_limit)
                require(file_digest(binary) == binary_digest, "tested executable changed during replay")
        else:
            binary = args.binary.resolve()
            binary_digest = file_digest(binary)
            results = execute_cases(binary, selected, args.repeat, timeout, artifacts, commands,
                                    rss_limit_bytes=rss_limit)
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
    summary = {"wpt_executed": 0, "repetitions_per_render": args.repeat}
    for name in ("bootstrap", "full_m1"):
        total = sum(case["stage"] == name for case in contract["cases"])
        executed = name == cohort_stage
        summary.update({
            f"{name}_selected": len(selected) if executed else 0,
            f"{name}_passed": counts["pass"] if executed else 0,
            f"{name}_not_run": 0 if executed else total,
            f"{name}_outcomes": {
                outcome: counts[outcome] if executed else 0
                for outcome in OUTCOMES
            },
        })
    if cohort_stage == "full_m1":
        summary["repetitions_per_case"] = args.repeat
    report_name = cohort_stage.replace("_", "-") + "-report.json"
    command_log_path = stage / report_name.replace(".json", ".commands.json")
    normalize = record_paths(stage, target_parent)
    write_json(command_log_path, normalize({"commands": commands}))
    report = {
        "run_id": args.run_id,
        "stage": cohort_stage,
        "cohort": contract["id"],
        "cohort_sha256": initial["cohort_sha256"],
        "binary_sha256": binary_digest,
        "build": build_record,
        "build_identity_verified": build_record is not None and identity_error is None,
        "evidence_kind": f"build_bound_{cohort_stage}" if build_record is not None and identity_error is None else "exploratory",
        "identity_error": identity_error,
        "source": source,
        "source_sha256": initial["build_inputs"],
        "runner_sha256": initial["runner_sha256"],
        "runner_support_sha256": initial["runner_support_sha256"],
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
        "platform": platform_identity(),
        "worker_id": getattr(args, "worker_id", None),
        "summary": summary,
        "limits": {
            "wall_time_seconds_per_invocation": timeout,
            "rss_policy": rss_policy, "rss_backend": rss_backend,
            "fixture_rss_limit_bytes": rss_limit,
            "fixture_environment": "fixed C locale and private per-invocation home, cache and temporary directory",
        },
        "cases": results,
        "boundary": ("Controlled fixture replay. Bootstrap and full-M1 outcomes have separate denominators. "
                     "RSS sampling with a kernel peak is per fixture child. OS isolation, worker equivalence, "
                     "WPT and sustained fuzzing require separate evidence."),
    }
    report = normalize(report)
    write_json(stage / report_name, report)
    require(file_digest(command_log_path) == report["command_log"]["sha256"], "command log changed during publication")
    return report, counts["pass"] == len(selected) and identity_error is None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--build", action="store_true", help="fresh locked offline build (the default)")
    mode.add_argument("--binary", type=Path, help="replay an existing executable as exploratory evidence")
    parser.add_argument("--cargo", default=os.environ.get("CARGO", "cargo"))
    parser.add_argument("--repeat", type=int, default=100)
    parser.add_argument("--stage", choices=("bootstrap", "full-m1"), default="bootstrap")
    parser.add_argument("--rss-policy", choices=("sampled", "off"),
                        help="fixture RSS policy (sampled for full M1 and off for bootstrap by default)")
    parser.add_argument("--worker-id", help="non-private operator-assigned ID for the execution worker")
    parser.add_argument("--artifacts-dir", type=Path, default=ROOT / "artifacts/m1")
    parser.add_argument("--run-id")
    args = parser.parse_args()
    if args.run_id is None:
        args.run_id = f"{args.stage}-{datetime.datetime.now(datetime.timezone.utc):%Y%m%dT%H%M%SZ}-{uuid4().hex}"
    if not 1 <= args.repeat <= 1000:
        parser.error("--repeat must be between 1 and 1000")
    if re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_-]{0,99}", args.run_id) is None:
        parser.error("--run-id must contain 1–100 letters, digits, hyphens or underscores and start with a letter or digit")
    if args.worker_id is not None and re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_.-]{0,63}", args.worker_id) is None:
        parser.error("--worker-id must contain 1–64 letters, digits, dots, hyphens or underscores and start with a letter or digit")
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
    print(f"Evidence ({report['evidence_kind']}): {destination / (args.stage + '-report.json')}")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
