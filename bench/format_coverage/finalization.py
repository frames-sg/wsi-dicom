"""Finalize format-coverage case and run reports without executing cases."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

from bench.file_digest import sha256_file
from bench.json_document import write_json
from bench.process_evidence import run_bounded_command

from .manifest import ALLOWED_ROUTE_CLASSIFICATIONS, FormatCoverageError, SCHEMA_VERSION


def command_version(command: list[str]) -> str:
    with tempfile.TemporaryDirectory(prefix="wsi-format-version-") as temporary:
        root = Path(temporary)
        stdout_path = root / "stdout.txt"
        evidence = run_bounded_command(
            command,
            stdout_path=stdout_path,
            stderr_path=root / "stderr.txt",
            timeout_secs=30,
            max_output_bytes=64 * 1024,
        )
        text = stdout_path.read_text(encoding="utf-8", errors="replace").strip()
    if (
        evidence["returncode"] != 0
        or evidence["timed_out"]
        or evidence["launch_error"] is not None
        or evidence["stdout_truncated"]
        or evidence["stderr_truncated"]
        or not text
    ):
        raise FormatCoverageError(f"version command failed: {command}")
    return text


def finalize_case(
    case: dict,
    *,
    backend: str,
    evaluation_mode: str,
    source_record: dict,
    conversion: dict,
    workbench: dict | None,
    case_root: Path,
) -> dict:
    expected_route = case.get("expected_route", {}).get(backend)
    route_matches = (
        conversion["route_classification"] == expected_route
        if expected_route is not None
        else conversion["route_classification"] in ALLOWED_ROUTE_CLASSIFICATIONS
    )
    status = (
        "passed"
        if route_matches
        and (
            conversion["status"] in {"expected_rejection", "profiled"}
            or (
                conversion["status"] == "converted"
                and workbench
                and workbench["status"] == "passed"
            )
        )
        else "failed"
    )
    result = {
        "id": case["id"],
        "format": case["format"],
        "level": case["level"],
        "transfer_syntax": case["transfer_syntax"],
        "backend": backend,
        "evaluation_mode": evaluation_mode,
        "expected_route": expected_route,
        "expected": case["expected"],
        "status": status,
        "source": source_record,
        "conversion": conversion,
        "workbench": workbench,
    }
    write_json(case_root / "case-report.json", result)
    return result


def build_run_report(
    *,
    manifest_path: Path,
    catalog_path: Path,
    catalog_provenance: dict | None,
    wsi_dicom: Path,
    conversion_timeout_secs: int,
    workbench_timeout_secs: int,
    backend: str,
    cases: list[dict],
) -> dict:
    return {
        "schema_version": SCHEMA_VERSION,
        "status": (
            "passed"
            if all(case["status"] == "passed" for case in cases)
            else "failed"
        ),
        "manifest": {
            "path": str(manifest_path),
            "sha256": sha256_file(manifest_path),
        },
        "rule_catalog": (
            {
                **catalog_provenance,
                "configured_path": str(catalog_path),
            }
            if catalog_provenance is not None
            else None
        ),
        "software": {
            "wsi_dicom": {
                "path": str(wsi_dicom),
                "sha256": sha256_file(wsi_dicom),
                "version": command_version([str(wsi_dicom), "--version"]),
            },
            "python": sys.version.splitlines()[0],
        },
        "policy": {
            "conversion_timeout_secs": conversion_timeout_secs,
            "workbench_timeout_secs": workbench_timeout_secs,
            "backend": backend,
            "codec_validation": "round-trip",
            "uid_policy": "deterministic",
            "strict_workbench": True,
        },
        "cases": cases,
        "limitations": [
            "bounded native levels rather than complete source pyramids",
            "source-to-DICOM pixel equality not evaluated by this run",
            "scanner calibration accuracy not evaluated",
            "DICOMweb receiver and viewer behavior not evaluated",
            (
                "one pinned artifact per source family does not establish "
                "all-variant support"
            ),
        ],
    }
