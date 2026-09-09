"""Run pinned vendor-format conversions and the DICOM workbench."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

from bench.cli_values import positive_int
from bench.json_document import write_json
from bench.format_coverage.conversion import execute_conversion
from bench.format_coverage.finalization import build_run_report, finalize_case
from bench.format_coverage.manifest import (
    ALLOWED_BACKENDS,
    DEFAULT_MANIFEST,
    DEFAULT_WORKBENCH_COMMAND,
    SCHEMA_VERSION,
    FormatCoverageError,
    installed_catalog_path,
    load_manifest,
    validate_rule_catalog_provenance,
)
from bench.format_coverage.report import render_summary, write_checksums
from bench.format_coverage.source_evidence import collect_source_evidence
from bench.format_coverage.workbench import execute_workbench


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Convert a pinned multi-format WSI corpus and run WSI-DICOM Bench."
    )
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument(
        "--catalog",
        type=Path,
        help="rule catalog (defaults to the installed benchmark's v2 catalog)",
    )
    parser.add_argument(
        "--wsi-dicom", type=Path, default=Path("target/release/wsi-dicom")
    )
    parser.add_argument(
        "--workbench-command",
        default=DEFAULT_WORKBENCH_COMMAND,
        help="installed WSI-DICOM Bench workbench command",
    )
    parser.add_argument(
        "--backend", choices=sorted(ALLOWED_BACKENDS), default="cpu"
    )
    parser.add_argument("--conversion-timeout-secs", type=positive_int, default=3600)
    parser.add_argument("--workbench-timeout-secs", type=positive_int, default=3600)
    return parser.parse_args(argv)


def run_case(case: dict, args: argparse.Namespace, staging: Path) -> dict:
    case_root = staging / "cases" / case["id"]
    case_root.mkdir(parents=True)
    materialized, source_record = collect_source_evidence(
        case, args.corpus_root, case_root
    )
    conversion = execute_conversion(
        case,
        wsi_dicom=args.wsi_dicom,
        source=materialized.entry_path,
        case_root=case_root,
        backend=args.backend,
        timeout_secs=args.conversion_timeout_secs,
    )

    workbench = None
    if conversion.status == "converted":
        workbench = execute_workbench(
            conversion_output=conversion.output_path,
            case_root=case_root,
            workbench_command=args.workbench_command,
            wsi_dicom=args.wsi_dicom,
            catalog=args.catalog,
            timeout_secs=args.workbench_timeout_secs,
        )
    return finalize_case(
        case,
        backend=args.backend,
        evaluation_mode=conversion.evaluation_mode,
        source_record=source_record,
        conversion=conversion.evidence,
        workbench=workbench,
        case_root=case_root,
    )


def run(args: argparse.Namespace) -> dict:
    args.corpus_root = args.corpus_root.resolve()
    args.output = args.output.resolve()
    args.manifest = args.manifest.resolve()
    args.catalog = (args.catalog or installed_catalog_path()).resolve()
    args.wsi_dicom = args.wsi_dicom.resolve()
    workbench_command = shutil.which(args.workbench_command)
    if args.output.exists():
        raise FormatCoverageError(f"output already exists: {args.output}")
    if not args.corpus_root.is_dir():
        raise FormatCoverageError(f"corpus root does not exist: {args.corpus_root}")
    if not args.wsi_dicom.is_file():
        raise FormatCoverageError(f"wsi-dicom executable does not exist: {args.wsi_dicom}")
    if workbench_command is None:
        raise FormatCoverageError(
            f"workbench command is not installed or executable: {args.workbench_command}"
        )
    args.workbench_command = workbench_command
    manifest = load_manifest(args.manifest)
    catalog_provenance = (
        validate_rule_catalog_provenance(manifest, args.catalog)
        if manifest["schema_version"] == SCHEMA_VERSION
        else None
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=f".{args.output.name}.staging-", dir=args.output.parent
    ) as temporary:
        staging = Path(temporary)
        write_json(staging / "manifest.json", manifest)
        cases = []
        for case in manifest["cases"]:
            print(f"format-coverage start {case['id']}", file=sys.stderr, flush=True)
            result = run_case(case, args, staging)
            cases.append(result)
            print(
                f"format-coverage {result['status']} {case['id']} conversion={result['conversion']['status']}",
                file=sys.stderr,
                flush=True,
            )
        report = build_run_report(
            manifest_path=args.manifest,
            catalog_path=args.catalog,
            catalog_provenance=catalog_provenance,
            wsi_dicom=args.wsi_dicom,
            conversion_timeout_secs=args.conversion_timeout_secs,
            workbench_timeout_secs=args.workbench_timeout_secs,
            backend=args.backend,
            cases=cases,
        )
        write_json(staging / "format-coverage-report.json", report)
        (staging / "summary.md").write_text(render_summary(report), encoding="utf-8")
        write_checksums(staging)
        os.replace(staging, args.output)
    return report


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        report = run(args)
    except (FormatCoverageError, OSError) as exc:
        print(f"format coverage failed: {exc}", file=sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "status": report["status"],
                "output": str(args.output.resolve()),
            },
            sort_keys=True,
        )
    )
    return 0 if report["status"] == "passed" else 1
