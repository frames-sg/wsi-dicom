"""Argument parsing and top-level benchmark orchestration."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
from pathlib import Path
from typing import Sequence

from .commands import command_for_tool
from .discovery import discover_gdc_slides, select_slides, slide_to_json
from .environment import collect_environment
from .models import (
    PROFILE_CHOICES,
    SCOPE_CHOICES,
    TOOL_CHOICES,
    split_command,
)
from .preflight import preflight_failure_row, run_device_preflight
from .reporting import (
    append_jsonl,
    completed_result_keys,
    infer_result_label,
    read_jsonl,
    render_markdown_summary,
    rows_from_result_dirs,
    write_csv,
    write_json,
    write_planned_commands,
)
from .trial import benchmark_trial

def default_python_command() -> str:
    venv_python = Path("./.venv/bin/python")
    return str(venv_python) if venv_python.exists() else sys.executable

def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Benchmark wsi-dicom and comparable converters on local GDC downloads."
    )
    parser.add_argument("--downloads-root", type=Path, default=Path.home() / "Downloads")
    parser.add_argument("--gdc-glob", default="gdc_download*")
    parser.add_argument("--out", type=Path, default=Path("bench/results"))
    parser.add_argument("--run-label")
    parser.add_argument(
        "--merge-results",
        nargs="+",
        type=Path,
        help="Merge existing benchmark result directories instead of running conversions.",
    )
    parser.add_argument("--profile", action="append", choices=PROFILE_CHOICES)
    parser.add_argument("--scope", action="append", choices=SCOPE_CHOICES)
    parser.add_argument("--tools", nargs="+", choices=TOOL_CHOICES, default=list(TOOL_CHOICES))
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--tile-size", type=int, default=512)
    parser.add_argument("--jpeg-quality", type=int, default=80)
    parser.add_argument("--workers", type=int, default=min(os.cpu_count() or 1, 8))
    parser.add_argument(
        "--offset-table", choices=("basic", "extended", "empty"), default="extended"
    )
    parser.add_argument("--timeout-secs", type=int, default=7200)
    parser.add_argument("--max-slides", type=int)
    parser.add_argument(
        "--only",
        action="append",
        default=[],
        help="Case-insensitive substring filter against slide id, display name, path, or GDC id.",
    )
    parser.add_argument("--wsi-dicom-command", default="target/release/wsi-dicom")
    parser.add_argument("--wsidicomizer-command", default="./.venv/bin/wsidicomizer")
    parser.add_argument("--python-command", default=default_python_command())
    parser.add_argument(
        "--system-label",
        help="Human-readable host/backend label to preserve in result rows, e.g. macos-metal or cuda-rtx4070.",
    )
    parser.add_argument(
        "--device-source-decode",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Add --source-device-decode to wsi-dicom-device runs.",
    )
    parser.add_argument(
        "--device-preflight",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Run bounded CPU/device route profiles before wsi-dicom-device conversions.",
    )
    parser.add_argument("--device-preflight-frames", type=int, default=64)
    parser.add_argument("--device-preflight-timeout-secs", type=int, default=120)
    parser.add_argument(
        "--device-preflight-min-speedup",
        type=float,
        default=1.0,
        help="Require the device preflight wall time to be at least this fast versus CPU.",
    )
    parser.add_argument(
        "--device-preflight-min-device-frame-pct",
        type=float,
        default=100.0,
        help="Require at least this percent of preflight frames to use device encode.",
    )
    parser.add_argument("--probe-slide-metadata", action="store_true")
    parser.add_argument("--validate", action="store_true")
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--list-slides", action="store_true")
    return parser.parse_args(argv)

def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    if args.merge_results:
        run_label = args.run_label or dt.datetime.now().strftime("merged-%Y%m%d-%H%M%S")
        run_dir = args.out / run_label
        if run_dir.exists() and not args.resume:
            print(f"Output run directory already exists: {run_dir}", file=sys.stderr)
            return 2
        run_dir.mkdir(parents=True, exist_ok=True)
        rows = rows_from_result_dirs(args.merge_results, annotate=True)
        for row in rows:
            append_jsonl(run_dir / "results.jsonl", row)
        write_csv(run_dir / "results.csv", rows)
        write_json(
            run_dir / "merged-runs.json",
            {"inputs": [str(path) for path in args.merge_results], "rows": len(rows)},
        )
        (run_dir / "summary.md").write_text(
            render_markdown_summary(rows, title=f"GDC WSI benchmark {run_label}"),
            encoding="utf-8",
        )
        print(f"Merged {len(rows)} rows into {run_dir}")
        return 0

    if args.runs < 1:
        print("--runs must be at least 1", file=sys.stderr)
        return 2
    if args.device_preflight_frames < 1:
        print("--device-preflight-frames must be at least 1", file=sys.stderr)
        return 2
    if args.device_preflight_timeout_secs < 1:
        print("--device-preflight-timeout-secs must be at least 1", file=sys.stderr)
        return 2
    if args.device_preflight_min_speedup < 0:
        print("--device-preflight-min-speedup must be non-negative", file=sys.stderr)
        return 2
    if not 0 <= args.device_preflight_min_device_frame_pct <= 100:
        print(
            "--device-preflight-min-device-frame-pct must be between 0 and 100",
            file=sys.stderr,
        )
        return 2

    repo_root = Path.cwd()
    profiles = args.profile or ["htj2k-lossless-rpcl"]
    scopes = args.scope or ["base"]
    slides = select_slides(
        discover_gdc_slides(
            args.downloads_root,
            gdc_glob=args.gdc_glob,
            probe_metadata=args.probe_slide_metadata,
        ),
        only_filters=args.only,
        max_slides=args.max_slides,
    )
    if args.list_slides:
        for slide in slides:
            print(
                json.dumps(
                    {
                        "slide_id": slide.slide_id,
                        "display_name": slide.display_name,
                        "path": str(slide.path),
                        "gdc_file_id": slide.gdc_file_id,
                        "bytes_on_disk": slide.bytes_on_disk,
                    },
                    sort_keys=True,
                )
            )
        return 0
    if not slides:
        print("No supported GDC slide files found for the selected filters.", file=sys.stderr)
        return 2

    run_label = args.run_label or dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    run_dir = args.out / run_label
    if run_dir.exists() and not args.resume:
        print(f"Output run directory already exists: {run_dir}", file=sys.stderr)
        return 2
    run_dir.mkdir(parents=True, exist_ok=True)

    wsi_dicom_command = split_command(args.wsi_dicom_command)
    wsidicomizer_command = split_command(args.wsidicomizer_command)
    python_command = split_command(args.python_command)

    write_json(run_dir / "slides.json", [slide_to_json(slide) for slide in slides])
    write_json(
        run_dir / "benchmark-run.json",
        {
            "profiles": profiles,
            "scopes": scopes,
            "tools": args.tools,
            "runs": args.runs,
            "tile_size": args.tile_size,
            "jpeg_quality": args.jpeg_quality,
            "workers": args.workers,
            "offset_table": args.offset_table,
            "timeout_secs": args.timeout_secs,
            "device_source_decode": args.device_source_decode,
            "device_preflight": args.device_preflight,
            "device_preflight_frames": args.device_preflight_frames,
            "device_preflight_timeout_secs": args.device_preflight_timeout_secs,
            "device_preflight_min_speedup": args.device_preflight_min_speedup,
            "device_preflight_min_device_frame_pct": (
                args.device_preflight_min_device_frame_pct
            ),
            "system_label": args.system_label,
            "validate": args.validate,
            "downloads_root": str(args.downloads_root.expanduser()),
            "gdc_glob": args.gdc_glob,
        },
    )
    write_json(
        run_dir / "environment.json",
        collect_environment(
            cwd=repo_root,
            wsi_dicom_command=wsi_dicom_command,
            python_command=python_command,
        ),
    )

    results_path = run_dir / "results.jsonl"
    rows = read_jsonl(results_path) if args.resume else []
    completed = completed_result_keys(rows)
    planned_commands = []

    for slide in slides:
        for profile in profiles:
            for scope in scopes:
                for tool in args.tools:
                    for run_index in range(1, args.runs + 1):
                        output_dir = (
                            run_dir
                            / "outputs"
                            / slide.slide_id
                            / profile
                            / scope
                            / tool
                            / f"run-{run_index}"
                        )
                        artifact_dir = (
                            run_dir
                            / "artifacts"
                            / slide.slide_id
                            / profile
                            / scope
                            / tool
                            / f"run-{run_index}"
                        )
                        command = command_for_tool(
                            tool,
                            wsi_dicom_command=wsi_dicom_command,
                            wsidicomizer_command=wsidicomizer_command,
                            source=slide.path,
                            output_dir=output_dir,
                            profile=profile,
                            scope=scope,
                            tile_size=args.tile_size,
                            jpeg_quality=args.jpeg_quality,
                            workers=args.workers,
                            offset_table=args.offset_table,
                            device_source_decode=args.device_source_decode,
                        )
                        key = (slide.slide_id, tool, profile, scope, run_index)
                        planned = {
                            "slide": slide.slide_id,
                            "display_name": slide.display_name,
                            "tool": tool,
                            "profile": profile,
                            "scope": scope,
                            "run_index": run_index,
                            "command": command,
                            "output_dir": str(output_dir),
                            "system_label": args.system_label,
                            "result_label": infer_result_label(
                                {
                                    "tool": tool,
                                    "system_label": args.system_label,
                                    "output_dir": str(output_dir),
                                },
                                result_set=run_label,
                            ),
                        }
                        if args.device_preflight and tool == "wsi-dicom-device":
                            planned["device_preflight"] = {
                                "frames": args.device_preflight_frames,
                                "timeout_secs": args.device_preflight_timeout_secs,
                                "min_speedup": args.device_preflight_min_speedup,
                                "min_device_frame_pct": (
                                    args.device_preflight_min_device_frame_pct
                                ),
                            }
                        planned_commands.append(planned)
                        if args.dry_run or key in completed:
                            continue
                        print(
                            f"{slide.slide_id} {profile} {scope} {tool} run {run_index}",
                            flush=True,
                        )
                        preflight = None
                        if args.device_preflight and tool == "wsi-dicom-device":
                            preflight = run_device_preflight(
                                wsi_dicom_command=wsi_dicom_command,
                                slide=slide,
                                artifact_dir=artifact_dir,
                                cwd=repo_root,
                                profile=profile,
                                scope=scope,
                                tile_size=args.tile_size,
                                jpeg_quality=args.jpeg_quality,
                                source_device_decode=args.device_source_decode,
                                max_frames=args.device_preflight_frames,
                                timeout_secs=args.device_preflight_timeout_secs,
                                min_speedup=args.device_preflight_min_speedup,
                                min_device_frame_pct=(
                                    args.device_preflight_min_device_frame_pct
                                ),
                            )
                            if preflight.get("status") not in {"passed", "skipped"}:
                                row = preflight_failure_row(
                                    slide=slide,
                                    tool=tool,
                                    command=command,
                                    output_dir=output_dir,
                                    profile=profile,
                                    scope=scope,
                                    run_index=run_index,
                                    preflight=preflight,
                                    system_label=args.system_label,
                                )
                                rows.append(row)
                                append_jsonl(results_path, row)
                                continue
                        row = benchmark_trial(
                            slide=slide,
                            tool=tool,
                            command=command,
                            output_dir=output_dir,
                            artifact_dir=artifact_dir,
                            cwd=repo_root,
                            timeout_secs=args.timeout_secs,
                            run_index=run_index,
                            profile=profile,
                            scope=scope,
                            validate=args.validate,
                            wsi_dicom_command=wsi_dicom_command,
                            system_label=args.system_label,
                            preflight=preflight,
                        )
                        rows.append(row)
                        append_jsonl(results_path, row)

    write_planned_commands(run_dir / "planned_commands.jsonl", planned_commands)
    if rows:
        write_csv(run_dir / "results.csv", rows)
        (run_dir / "summary.md").write_text(
            render_markdown_summary(rows, title=f"GDC WSI benchmark {run_label}"),
            encoding="utf-8",
        )
    if args.dry_run:
        print(f"Dry run wrote {len(planned_commands)} planned commands to {run_dir}")
    else:
        print(f"Wrote benchmark results to {run_dir}")
    return 0
