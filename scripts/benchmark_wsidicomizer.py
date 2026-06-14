#!/usr/bin/env python3
"""Benchmark wsi-dicom against wsidicomizer on a local slide corpus."""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import json
import os
import platform
import shlex
import subprocess
import sys
import time
from pathlib import Path
from typing import Iterable, NamedTuple, Sequence


SUPPORTED_SUFFIXES = {
    ".bif",
    ".czi",
    ".mrxs",
    ".ndpi",
    ".scn",
    ".svs",
    ".tif",
    ".tiff",
    ".vms",
    ".vmu",
}

PROFILE_CHOICES = ("fast-jpeg", "htj2k-rpcl")


class Slide(NamedTuple):
    path: Path
    relative_path: str


def discover_slides(root: Path, suffixes: Iterable[str] = SUPPORTED_SUFFIXES) -> list[Slide]:
    root = root.resolve()
    normalized_suffixes = {suffix.lower() for suffix in suffixes}
    slides = [
        Slide(path=path, relative_path=path.relative_to(root).as_posix())
        for path in root.rglob("*")
        if path.is_file() and path.suffix.lower() in normalized_suffixes
    ]
    return sorted(slides, key=lambda slide: slide.relative_path.lower())


def split_command(command: str) -> list[str]:
    parts = shlex.split(command)
    if not parts:
        raise ValueError("command cannot be empty")
    return parts


def safe_slug(value: str) -> str:
    slug = []
    for character in value:
        if character.isalnum():
            slug.append(character.lower())
        elif character in {".", "-", "_"}:
            slug.append(character.lower())
        else:
            slug.append("-")
    compacted = "".join(slug).strip("-")
    while "--" in compacted:
        compacted = compacted.replace("--", "-")
    return compacted or "slide"


def build_wsi_dicom_command(
    base_command: Sequence[str],
    source: Path,
    output_dir: Path,
    *,
    tile_size: int,
    level: int,
    profile: str,
) -> list[str]:
    command = [
        *base_command,
        "convert",
        str(source),
        "--out",
        str(output_dir),
        "--research-placeholder",
        "--tile-size",
        str(tile_size),
        "--level",
        str(level),
        "--json",
    ]
    if profile == "fast-jpeg":
        command.extend(["--preset", "fast-jpeg", "--jpeg-quality", "80"])
    elif profile == "htj2k-rpcl":
        command.extend(["--transfer-syntax", "htj2k-lossless-rpcl"])
    else:
        raise ValueError(f"unsupported profile: {profile}")
    return command


def build_wsidicomizer_command(
    base_command: Sequence[str],
    source: Path,
    output_dir: Path,
    *,
    tile_size: int,
    level: int,
    workers: int,
    offset_table: str,
    profile: str,
) -> list[str]:
    command = [
        *base_command,
        "--input",
        str(source),
        "--output",
        str(output_dir),
        "--tile-size",
        str(tile_size),
        "--levels",
        str(level),
        "--workers",
        str(workers),
        "--offset-table",
        offset_table,
        "--no-confidential",
    ]
    if profile == "fast-jpeg":
        command.extend(["--format", "jpeg", "--quality", "80"])
    elif profile == "htj2k-rpcl":
        command.extend(["--format", "htjpeg2000"])
    else:
        raise ValueError(f"unsupported profile: {profile}")
    return command


def run_command(
    command: Sequence[str],
    *,
    cwd: Path,
    stdout_path: Path,
    stderr_path: Path,
    timeout_secs: int,
) -> dict:
    started = time.perf_counter()
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    with stdout_path.open("w", encoding="utf-8") as stdout_file, stderr_path.open(
        "w", encoding="utf-8"
    ) as stderr_file:
        try:
            completed = subprocess.run(
                list(command),
                cwd=cwd,
                stdout=stdout_file,
                stderr=stderr_file,
                text=True,
                timeout=timeout_secs,
                check=False,
            )
            returncode = completed.returncode
            status = "passed" if returncode == 0 else "failed"
        except subprocess.TimeoutExpired:
            returncode = None
            status = "timeout"
    elapsed_secs = time.perf_counter() - started
    return {
        "status": status,
        "returncode": returncode,
        "elapsed_secs": elapsed_secs,
        "stdout_path": str(stdout_path),
        "stderr_path": str(stderr_path),
    }


def count_output_files(output_dir: Path) -> tuple[int, int]:
    if not output_dir.exists():
        return 0, 0
    produced_files = 0
    output_bytes = 0
    for path in output_dir.rglob("*"):
        if path.is_file():
            produced_files += 1
            output_bytes += path.stat().st_size
    return produced_files, output_bytes


def dicom_metadata_from_dataset(path: Path, dataset) -> dict:
    return {
        "file": path.name,
        "transfer_syntax_uid": str(getattr(dataset.file_meta, "TransferSyntaxUID", "")),
        "rows": int(getattr(dataset, "Rows", 0) or 0),
        "columns": int(getattr(dataset, "Columns", 0) or 0),
        "number_of_frames": int(getattr(dataset, "NumberOfFrames", 0) or 0),
        "total_pixel_matrix_columns": int(
            getattr(dataset, "TotalPixelMatrixColumns", 0) or 0
        ),
        "total_pixel_matrix_rows": int(getattr(dataset, "TotalPixelMatrixRows", 0) or 0),
    }


def collect_dicom_outputs(output_dir: Path) -> tuple[list[dict], str | None]:
    if not output_dir.exists():
        return [], None
    try:
        import pydicom
    except ImportError as err:
        return [], f"pydicom unavailable: {err}"

    outputs = []
    for path in sorted(output_dir.rglob("*.dcm")):
        try:
            dataset = pydicom.dcmread(str(path), stop_before_pixels=True)
        except Exception as err:  # noqa: BLE001 - evidence collection should not hide benchmark result
            return outputs, f"failed to read DICOM metadata from {path}: {err}"
        outputs.append(dicom_metadata_from_dataset(path, dataset))
    return outputs, None


def command_output(command: Sequence[str], *, cwd: Path, timeout_secs: int = 30) -> str | None:
    try:
        completed = subprocess.run(
            list(command),
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=timeout_secs,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if completed.returncode != 0:
        return None
    output = completed.stdout.strip()
    return output or None


def parse_cargo_pkgid_version(pkgid: str) -> str | None:
    package = pkgid.strip().rsplit("#", maxsplit=1)[-1]
    if "@" in package:
        return package.rsplit("@", maxsplit=1)[-1] or None
    return package or None


def cargo_package_version(*, cwd: Path, package: str) -> str | None:
    output = command_output(["cargo", "pkgid", "-p", package], cwd=cwd)
    return parse_cargo_pkgid_version(output) if output else None


def package_version(python_command: Sequence[str], package: str, *, cwd: Path) -> str | None:
    output = command_output(
        [
            *python_command,
            "-c",
            (
                "import importlib.metadata as m; "
                f"print(m.version({package!r}))"
            ),
        ],
        cwd=cwd,
    )
    return output.splitlines()[-1] if output else None


def collect_environment(
    *,
    cwd: Path,
    wsi_dicom_command: Sequence[str],
    wsidicomizer_command: Sequence[str],
    python_command: Sequence[str],
) -> dict:
    return {
        "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "platform": platform.platform(),
        "python": sys.version.replace("\n", " "),
        "wsi_dicom_version": command_output([*wsi_dicom_command, "--version"], cwd=cwd)
        or cargo_package_version(cwd=cwd, package="wsi-dicom"),
        "wsidicomizer_version": package_version(python_command, "wsidicomizer", cwd=cwd),
        "wsidicom_version": package_version(python_command, "wsidicom", cwd=cwd),
        "openslide_python_version": package_version(
            python_command, "openslide-python", cwd=cwd
        ),
    }


def validate_output(
    *,
    wsi_dicom_command: Sequence[str],
    output_dir: Path,
    artifact_dir: Path,
    cwd: Path,
    timeout_secs: int,
) -> dict:
    command = [
        *wsi_dicom_command,
        "validate",
        str(output_dir),
        "--json",
        "--command-timeout-secs",
        str(max(1, min(timeout_secs, 300))),
    ]
    result = run_command(
        command,
        cwd=cwd,
        stdout_path=artifact_dir / "validate.stdout.json",
        stderr_path=artifact_dir / "validate.stderr.txt",
        timeout_secs=timeout_secs,
    )
    return {
        "command": command,
        "status": result["status"],
        "returncode": result["returncode"],
        "elapsed_secs": result["elapsed_secs"],
        "stdout_path": result["stdout_path"],
        "stderr_path": result["stderr_path"],
    }


def benchmark_tool(
    *,
    tool: str,
    command: Sequence[str],
    slide: Slide,
    output_dir: Path,
    artifact_dir: Path,
    cwd: Path,
    timeout_secs: int,
    run_index: int,
    validate: bool,
    wsi_dicom_command: Sequence[str],
) -> dict:
    result = run_command(
        command,
        cwd=cwd,
        stdout_path=artifact_dir / "stdout.txt",
        stderr_path=artifact_dir / "stderr.txt",
        timeout_secs=timeout_secs,
    )
    produced_files, output_bytes = count_output_files(output_dir)
    dicom_outputs, dicom_metadata_error = collect_dicom_outputs(output_dir)
    row = {
        "slide": slide.relative_path,
        "source_path": str(slide.path),
        "tool": tool,
        "run_index": run_index,
        "status": result["status"],
        "returncode": result["returncode"],
        "elapsed_secs": result["elapsed_secs"],
        "output_dir": str(output_dir),
        "produced_files": produced_files,
        "output_bytes": output_bytes,
        "command": list(command),
        "stdout_path": result["stdout_path"],
        "stderr_path": result["stderr_path"],
        "dicom_outputs": dicom_outputs,
    }
    if dicom_metadata_error:
        row["dicom_metadata_error"] = dicom_metadata_error
    if validate and result["status"] == "passed":
        row["validation"] = validate_output(
            wsi_dicom_command=wsi_dicom_command,
            output_dir=output_dir,
            artifact_dir=artifact_dir,
            cwd=cwd,
            timeout_secs=timeout_secs,
        )
    return row


def write_jsonl(path: Path, rows: Iterable[dict]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True))
            handle.write("\n")


def read_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def completed_result_keys(rows: Iterable[dict]) -> set[tuple[str, str, int]]:
    return {
        (row["slide"], row["tool"], int(row.get("run_index", 1)))
        for row in rows
        if "slide" in row and "tool" in row
    }


def next_available_path(path: Path) -> Path:
    if not path.exists():
        return path
    for index in range(1, 10_000):
        candidate = path.with_name(f"{path.name}-resume-{index}")
        if not candidate.exists():
            return candidate
    raise RuntimeError(f"could not find available resume path for {path}")


def write_csv(path: Path, rows: Sequence[dict]) -> None:
    fields = [
        "slide",
        "tool",
        "run_index",
        "status",
        "returncode",
        "elapsed_secs",
        "produced_files",
        "output_bytes",
        "output_dir",
        "stdout_path",
        "stderr_path",
        "dicom_outputs",
        "dicom_metadata_error",
    ]
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field) for field in fields})


def aggregate_tool(rows: Sequence[dict], slide: str, tool: str) -> dict | None:
    tool_rows = [row for row in rows if row["slide"] == slide and row["tool"] == tool]
    if not tool_rows:
        return None
    passed_rows = [row for row in tool_rows if row["status"] == "passed"]
    if len(passed_rows) == len(tool_rows):
        status = "passed"
    elif passed_rows:
        status = "partial"
    else:
        status = tool_rows[-1]["status"]
    elapsed_values = [row["elapsed_secs"] for row in passed_rows or tool_rows]
    output_values = [row.get("output_bytes", 0) for row in passed_rows or tool_rows]
    file_values = [row.get("produced_files", 0) for row in passed_rows or tool_rows]
    evidence_row = (passed_rows or tool_rows)[0]
    return {
        "status": status,
        "elapsed_secs": sum(elapsed_values) / len(elapsed_values),
        "output_bytes": int(sum(output_values) / len(output_values)),
        "produced_files": int(sum(file_values) / len(file_values)),
        "dicom_outputs": evidence_row.get("dicom_outputs", []),
        "dicom_metadata_error": evidence_row.get("dicom_metadata_error"),
    }


def target_signature(row: dict | None) -> tuple[tuple[tuple[str, object], ...], ...] | None:
    if not row:
        return None
    outputs = row.get("dicom_outputs") or []
    if not outputs:
        return None
    normalized = []
    for output in outputs:
        normalized.append(
            tuple(
                sorted(
                    (key, value)
                    for key, value in output.items()
                    if key != "file"
                )
            )
        )
    return tuple(sorted(normalized))


def target_comparison(wsi: dict | None, dicomizer: dict | None) -> str:
    if not wsi or not dicomizer:
        return ""
    if wsi.get("status") != "passed" or dicomizer.get("status") != "passed":
        return ""
    wsi_signature = target_signature(wsi)
    dicomizer_signature = target_signature(dicomizer)
    if wsi_signature is None or dicomizer_signature is None:
        return "unknown-target"
    if wsi_signature == dicomizer_signature:
        return "equal-target"
    return "mixed-target"


def render_markdown_summary(rows: Sequence[dict], *, title: str) -> str:
    slides = sorted({row["slide"] for row in rows})
    lines = [
        f"# {title}",
        "",
        "| Slide | wsi-dicom status | wsi-dicom seconds | wsidicomizer status | wsidicomizer seconds | wsi-dicom speedup | Target |",
        "| --- | --- | ---: | --- | ---: | ---: | --- |",
    ]
    for slide in slides:
        wsi = aggregate_tool(rows, slide, "wsi-dicom")
        dicomizer = aggregate_tool(rows, slide, "wsidicomizer")
        wsi_status = wsi["status"] if wsi else "missing"
        dicomizer_status = dicomizer["status"] if dicomizer else "missing"
        wsi_elapsed = f"{wsi['elapsed_secs']:.3f}" if wsi else ""
        dicomizer_elapsed = f"{dicomizer['elapsed_secs']:.3f}" if dicomizer else ""
        if wsi and dicomizer and wsi_status == "passed" and dicomizer_status == "passed":
            speedup = f"{dicomizer['elapsed_secs'] / wsi['elapsed_secs']:.2f}x"
        else:
            speedup = ""
        target = target_comparison(wsi, dicomizer)
        lines.append(
            f"| {slide} | {wsi_status} | {wsi_elapsed} | {dicomizer_status} | {dicomizer_elapsed} | {speedup} | {target} |"
        )
    lines.extend(
        [
            "",
            "A speedup above 1.00x means `wsi-dicom` completed faster than `wsidicomizer` for the same slide and profile.",
            "`equal-target` means both tools emitted matching transfer syntax and frame geometry; `mixed-target` means speed is not codec/geometry-normalized.",
            "Failed and timed-out runs are retained in the JSONL evidence and omitted from speedup calculations.",
        ]
    )
    return "\n".join(lines) + "\n"


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Benchmark wsi-dicom against wsidicomizer on a local corpus."
    )
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=Path("target/wsidicomizer-benchmark"))
    parser.add_argument("--run-label")
    parser.add_argument("--profile", choices=PROFILE_CHOICES, default="fast-jpeg")
    parser.add_argument("--tile-size", type=int, default=256)
    parser.add_argument("--level", type=int, default=0)
    parser.add_argument("--workers", type=int, default=min(os.cpu_count() or 1, 8))
    parser.add_argument(
        "--offset-table", choices=("basic", "extended", "empty"), default="extended"
    )
    parser.add_argument("--timeout-secs", type=int, default=3600)
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--max-slides", type=int)
    parser.add_argument(
        "--only",
        action="append",
        default=[],
        help="Case-insensitive substring filter; can be repeated.",
    )
    parser.add_argument(
        "--wsi-dicom-command",
        default="target/release/wsi-dicom",
        help="Command used to invoke wsi-dicom. Shell-style quoting is supported.",
    )
    parser.add_argument(
        "--wsidicomizer-command",
        default="./.venv/bin/wsidicomizer",
        help="Command used to invoke wsidicomizer. Shell-style quoting is supported.",
    )
    parser.add_argument(
        "--python-command",
        default="./.venv/bin/python",
        help="Python command used to read wsidicomizer package versions.",
    )
    parser.add_argument("--validate", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument(
        "--resume",
        action="store_true",
        help="Resume an existing run directory and skip slide/tool pairs already present in results.jsonl.",
    )
    return parser.parse_args(argv)


def select_slides(
    slides: Sequence[Slide], *, only_filters: Sequence[str], max_slides: int | None
) -> list[Slide]:
    selected = list(slides)
    for only in only_filters:
        needle = only.lower()
        selected = [slide for slide in selected if needle in slide.relative_path.lower()]
    if max_slides is not None:
        selected = selected[:max_slides]
    return selected


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    repo_root = Path.cwd()
    slides = select_slides(
        discover_slides(args.corpus_root),
        only_filters=args.only,
        max_slides=args.max_slides,
    )
    if not slides:
        print("No supported slide files found for the selected corpus/filter.", file=sys.stderr)
        return 2

    run_label = args.run_label or dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    run_dir = args.out / run_label
    if run_dir.exists():
        if not args.resume:
            print(f"Output run directory already exists: {run_dir}", file=sys.stderr)
            return 2
    else:
        run_dir.mkdir(parents=True)

    wsi_dicom_command = split_command(args.wsi_dicom_command)
    wsidicomizer_command = split_command(args.wsidicomizer_command)
    python_command = split_command(args.python_command)
    environment = collect_environment(
        cwd=repo_root,
        wsi_dicom_command=wsi_dicom_command,
        wsidicomizer_command=wsidicomizer_command,
        python_command=python_command,
    )
    results_path = run_dir / "results.jsonl"
    rows: list[dict] = read_jsonl(results_path) if args.resume else []
    metadata = {
        "profile": args.profile,
        "tile_size": args.tile_size,
        "level": args.level,
        "workers": args.workers,
        "offset_table": args.offset_table,
        "timeout_secs": args.timeout_secs,
        "runs": args.runs,
        "validate": args.validate,
        "corpus_root": str(args.corpus_root),
        "slide_count": len(slides),
        "resume": args.resume,
        "existing_result_rows": len(rows),
        "environment": environment,
        "slides": [slide.relative_path for slide in slides],
    }
    (run_dir / "benchmark-run.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    if args.dry_run:
        print(f"Discovered {len(slides)} slide(s). Evidence directory: {run_dir}")
        return 0

    completed = completed_result_keys(rows)
    for run_index in range(1, args.runs + 1):
        for slide in slides:
            slide_slug = safe_slug(slide.relative_path)
            for tool in ("wsi-dicom", "wsidicomizer"):
                key = (slide.relative_path, tool, run_index)
                if key in completed:
                    print(f"[{run_index}/{args.runs}] skip {tool}: {slide.relative_path}", flush=True)
                    continue
                artifact_dir = run_dir / "artifacts" / f"run-{run_index:02d}" / slide_slug / tool
                output_dir = run_dir / "outputs" / f"run-{run_index:02d}" / slide_slug / tool
                if args.resume:
                    artifact_dir = next_available_path(artifact_dir)
                    output_dir = next_available_path(output_dir)
                if tool == "wsi-dicom":
                    command = build_wsi_dicom_command(
                        wsi_dicom_command,
                        slide.path,
                        output_dir,
                        tile_size=args.tile_size,
                        level=args.level,
                        profile=args.profile,
                    )
                else:
                    command = build_wsidicomizer_command(
                        wsidicomizer_command,
                        slide.path,
                        output_dir,
                        tile_size=args.tile_size,
                        level=args.level,
                        workers=args.workers,
                        offset_table=args.offset_table,
                        profile=args.profile,
                    )
                print(f"[{run_index}/{args.runs}] {tool}: {slide.relative_path}", flush=True)
                row = benchmark_tool(
                    tool=tool,
                    command=command,
                    slide=slide,
                    output_dir=output_dir,
                    artifact_dir=artifact_dir,
                    cwd=repo_root,
                    timeout_secs=args.timeout_secs,
                    run_index=run_index,
                    validate=args.validate,
                    wsi_dicom_command=wsi_dicom_command,
                )
                rows.append(row)
                write_jsonl(results_path, rows)
                write_csv(run_dir / "summary.csv", rows)
                (run_dir / "summary.md").write_text(
                    render_markdown_summary(rows, title="wsi-dicom vs wsidicomizer Benchmark"),
                    encoding="utf-8",
                )
    print(f"Benchmark evidence written to {run_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
