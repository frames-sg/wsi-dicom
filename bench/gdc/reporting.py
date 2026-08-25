"""JSONL/CSV persistence, aggregation, and Markdown reporting."""

from __future__ import annotations

import csv
import json
from pathlib import Path
from typing import Iterable, Sequence

def read_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows

def append_jsonl(path: Path, row: dict) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row, sort_keys=True))
        handle.write("\n")

def completed_result_keys(rows: Iterable[dict]) -> set[tuple[str, str, str, str, int]]:
    return {
        (
            row["slide"],
            row["tool"],
            row.get("profile", ""),
            row.get("scope", ""),
            int(row.get("run_index", 1)),
        )
        for row in rows
        if "slide" in row and "tool" in row
    }

def infer_result_label(row: dict, *, result_set: str | None = None) -> str:
    existing = row.get("result_label")
    if existing:
        return str(existing)

    tool = str(row.get("tool", ""))
    if tool == "wsi-dicom-cpu":
        return "wsi-dicom CPU"
    if tool == "wsidicomizer":
        return "wsidicomizer"
    if tool == "wsi-dicom-device":
        hints = [
            row.get("system_label"),
            result_set,
            row.get("result_set"),
            row.get("output_dir"),
            row.get("stdout_path"),
            row.get("stderr_path"),
            row.get("source_path"),
        ]
        hint = " ".join(str(value) for value in hints if value).lower()
        if "cuda" in hint or "nvidia" in hint:
            return "wsi-dicom CUDA"
        if "metal" in hint or "darwin" in hint:
            return "wsi-dicom Metal"
        system_label = row.get("system_label")
        if system_label:
            return f"wsi-dicom Device ({system_label})"
        return "wsi-dicom Device"
    return tool or "unknown"

def attach_result_context(
    row: dict,
    *,
    system_label: str | None = None,
    result_set: str | None = None,
) -> dict:
    if system_label:
        row.setdefault("system_label", system_label)
    if result_set:
        row.setdefault("result_set", result_set)
    row.setdefault("result_label", infer_result_label(row, result_set=result_set))
    return row

def write_json(path: Path, data: object) -> None:
    path.write_text(json.dumps(data, indent=2, sort_keys=True), encoding="utf-8")

def write_csv(path: Path, rows: Sequence[dict]) -> None:
    fields = [
        "slide",
        "display_name",
        "gdc_file_id",
        "tool",
        "result_label",
        "system_label",
        "result_set",
        "profile",
        "scope",
        "run_index",
        "status",
        "returncode",
        "elapsed_secs",
        "produced_files",
        "output_bytes",
        "output_dir",
        "stdout_path",
        "stderr_path",
        "error",
        "preflight_status",
        "preflight_reason",
        "preflight_speedup_vs_cpu",
        "preflight_device_frame_pct",
        "dicom_metadata_error",
    ]
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            csv_row = {field: row.get(field) for field in fields}
            preflight = row.get("preflight") or {}
            csv_row["preflight_status"] = preflight.get("status")
            csv_row["preflight_reason"] = preflight.get("reason")
            csv_row["preflight_speedup_vs_cpu"] = preflight.get("speedup_vs_cpu")
            csv_row["preflight_device_frame_pct"] = preflight.get("device_frame_pct")
            writer.writerow(csv_row)

def average_passed_seconds(
    rows: Sequence[dict],
    *,
    slide: str,
    result_label: str,
    profile: str,
    scope: str,
) -> float | None:
    passed = [
        row
        for row in rows
        if row.get("slide") == slide
        and infer_result_label(row) == result_label
        and row.get("profile") == profile
        and row.get("scope") == scope
        and row.get("status") == "passed"
    ]
    if not passed:
        return None
    return sum(float(row["elapsed_secs"]) for row in passed) / len(passed)

def status_summary(
    rows: Sequence[dict],
    *,
    slide: str,
    result_label: str,
    profile: str,
    scope: str,
) -> str:
    selected = [
        row
        for row in rows
        if row.get("slide") == slide
        and infer_result_label(row) == result_label
        and row.get("profile") == profile
        and row.get("scope") == scope
    ]
    if not selected:
        return "missing"
    passed = sum(1 for row in selected if row.get("status") == "passed")
    if passed == len(selected):
        return "passed"
    if passed:
        return f"partial {passed}/{len(selected)}"
    return str(selected[-1].get("status", "failed"))

def has_rows_for_cell(
    rows: Sequence[dict],
    *,
    slide: str,
    profile: str,
    scope: str,
) -> bool:
    return any(
        row.get("slide") == slide
        and row.get("profile") == profile
        and row.get("scope") == scope
        for row in rows
    )

def format_seconds(value: float | None) -> str:
    return "" if value is None else f"{value:.3f}"

def format_speedup(numerator: float | None, denominator: float | None) -> str:
    if numerator is None or denominator is None or denominator <= 0:
        return ""
    return f"{numerator / denominator:.2f}x"

def result_label_sort_key(label: str) -> tuple[int, str]:
    order = {
        "wsi-dicom CPU": 0,
        "wsi-dicom Metal": 1,
        "wsi-dicom CUDA": 2,
        "wsi-dicom Device": 3,
        "wsidicomizer": 4,
    }
    return order.get(label, 50), label

def render_markdown_summary(rows: Sequence[dict], *, title: str) -> str:
    slides = sorted({row["slide"] for row in rows})
    profiles = sorted({row.get("profile", "") for row in rows})
    scopes = sorted({row.get("scope", "") for row in rows})
    labels = sorted({infer_result_label(row) for row in rows}, key=result_label_sort_key)
    device_labels = [
        label
        for label in labels
        if any(
            row.get("tool") == "wsi-dicom-device" and infer_result_label(row) == label
            for row in rows
        )
    ]
    cpu_label = "wsi-dicom CPU" if "wsi-dicom CPU" in labels else None
    dicomizer_label = "wsidicomizer" if "wsidicomizer" in labels else None

    header = ["Slide", "Profile", "Scope"]
    separator = ["---", "---", "---"]
    for label in labels:
        header.extend([f"{label} status", f"{label} seconds"])
        separator.extend(["---", "---:"])
    for label in device_labels:
        if cpu_label:
            header.append(f"{label} vs CPU")
            separator.append("---:")
        if dicomizer_label:
            header.append(f"{label} vs wsidicomizer")
            separator.append("---:")

    lines = [
        f"# {title}",
        "",
        "| " + " | ".join(header) + " |",
        "| " + " | ".join(separator) + " |",
    ]
    for slide in slides:
        for profile in profiles:
            for scope in scopes:
                if not has_rows_for_cell(rows, slide=slide, profile=profile, scope=scope):
                    continue
                seconds_by_label = {
                    label: average_passed_seconds(
                        rows,
                        slide=slide,
                        result_label=label,
                        profile=profile,
                        scope=scope,
                    )
                    for label in labels
                }
                cells = [slide, profile, scope]
                for label in labels:
                    cells.extend(
                        [
                            status_summary(
                                rows,
                                slide=slide,
                                result_label=label,
                                profile=profile,
                                scope=scope,
                            ),
                            format_seconds(seconds_by_label[label]),
                        ]
                    )
                for label in device_labels:
                    if cpu_label:
                        cells.append(
                            format_speedup(seconds_by_label[cpu_label], seconds_by_label[label])
                        )
                    if dicomizer_label:
                        cells.append(
                            format_speedup(
                                seconds_by_label[dicomizer_label],
                                seconds_by_label[label],
                            )
                        )
                lines.append(
                    "| " + " | ".join(cells) + " |"
                )
    lines.extend(
        [
            "",
            "Speedups above 1.00x mean the device-labeled `wsi-dicom` run was faster than the comparison tool.",
            "Failed, timed-out, and unsupported runs remain in `results.jsonl` and are not used for speedup ratios.",
            "`preflight-failed` rows mean the bounded device route profile did not meet the configured publication threshold, so the full conversion was skipped.",
            "The `htj2k-lossless-rpcl` profile maps wsidicomizer to its HTJ2K setting because RPCL-specific control is not exposed by that CLI.",
        ]
    )
    return "\n".join(lines) + "\n"

def write_planned_commands(path: Path, commands: Sequence[dict]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for command in commands:
            handle.write(json.dumps(command, sort_keys=True))
            handle.write("\n")

def rows_from_result_dirs(result_dirs: Sequence[Path], *, annotate: bool = False) -> list[dict]:
    rows: list[dict] = []
    for result_dir in result_dirs:
        path = result_dir / "results.jsonl"
        if not path.exists():
            raise FileNotFoundError(f"missing results.jsonl in {result_dir}")
        for row in read_jsonl(path):
            if annotate:
                row = attach_result_context(dict(row), result_set=result_dir.name)
            rows.append(row)
    return rows
