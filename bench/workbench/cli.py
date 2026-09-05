"""Single entry point for a complete WSI-DICOM Bench DICOM evaluation."""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

from bench.cli_values import positive_int
from bench.workbench.tool_provenance import python_validator_inventory
from bench.workbench.contract import validation_contract_errors
from bench.core_profile import CoreProfileError, load_profile
from bench.json_document import write_json
from bench.process_evidence import read_bounded_text, run_bounded_command

from .catalog import CatalogError, load_catalog
from .report import (
    inspect_dicom_set,
    map_findings,
    sha256_file,
    summarize_domains,
    validator_comparison,
)


DEFAULT_CATALOG = (
    Path(__file__).resolve().parents[2]
    / "rules"
    / "wsi-dicom-bench-rules-2026c-v4.json"
)


class WorkbenchError(RuntimeError):
    """A complete workbench evaluation could not be produced."""


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run intrinsic, decoder, and independent-validator checks and emit one unified per-slide report."
    )
    parser.add_argument("dicom", type=Path, help="DICOM file or directory to evaluate")
    parser.add_argument("--output", type=Path, required=True, help="new evidence directory")
    parser.add_argument(
        "--wsi-dicom",
        type=Path,
        default=Path("target/release/wsi-dicom"),
        help="wsi-dicom executable",
    )
    parser.add_argument("--catalog", type=Path, default=DEFAULT_CATALOG)
    parser.add_argument(
        "--profile",
        type=Path,
        help="machine-readable VL WSI core profile; required by versioned core-profile benchmark runs",
    )
    parser.add_argument("--allow-missing-tools", action="store_true")
    parser.add_argument("--dcmvalidate-iod", type=Path)
    parser.add_argument("--htj2k-decoder")
    parser.add_argument("--max-pixel-frames", type=positive_int, default=1)
    parser.add_argument("--command-timeout-secs", type=positive_int, default=300)
    parser.add_argument("--evaluation-timeout-secs", type=positive_int, default=3600)
    parser.add_argument("--max-json-bytes", type=positive_int, default=256 * 1024 * 1024)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        result = run_workbench(args)
    except (WorkbenchError, CatalogError, CoreProfileError, OSError, ValueError) as exc:
        print(f"workbench failed: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return {"passed": 0, "failed": 1, "execution_error": 2}[result["status"]]


def run_workbench(args: argparse.Namespace) -> dict:
    if args.output.exists():
        raise WorkbenchError(f"output path already exists: {args.output}")
    if not args.dicom.exists():
        raise WorkbenchError(f"DICOM input does not exist: {args.dicom}")
    if not args.wsi_dicom.is_file():
        raise WorkbenchError(f"wsi-dicom executable does not exist: {args.wsi_dicom}")
    catalog = load_catalog(args.catalog)
    if (
        any("intrinsic-wsi-dicom-2026c-core-image-profile" in rule["check_names"] for rule in catalog["rules"])
        and args.profile is None
    ):
        raise WorkbenchError("--profile is required for the DICOM 2026c core-profile catalog")
    profile = load_profile(args.profile) if args.profile else None
    expected_profile = {
        "wsi-dicom-bench-rules-2026c-v4": "wsi-dicom-core-profile-2026c-v2",
    }.get(catalog["catalog_version"])
    if profile and profile["profile_id"] != expected_profile:
        raise WorkbenchError("catalog and selected profile version are incompatible")
    if profile and profile["dicom_edition"] != catalog["dicom_edition"]:
        raise WorkbenchError("profile and rule catalog use different DICOM editions")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=f".{args.output.name}.staging-", dir=args.output.parent
    ) as temporary:
        staging = Path(temporary)
        doctor_execution, validation_execution = _run_evaluations(args, staging)
        doctor = doctor_execution.pop("document")
        validation = validation_execution.pop("document")
        report = _build_report(
            args,
            catalog,
            profile,
            doctor,
            validation,
            doctor_execution,
            validation_execution,
        )
        write_json(staging / "doctor.json", doctor)
        write_json(staging / "validation.json", validation)
        write_json(staging / "workbench-report.json", report)
        (staging / "summary.md").write_text(render_summary(report), encoding="utf-8")
        os.replace(staging, args.output)
    return {
        "status": report["status"],
        "output": str(args.output),
        "report": str(args.output / "workbench-report.json"),
        "summary": str(args.output / "summary.md"),
    }


def _run_evaluations(
    args: argparse.Namespace, staging: Path
) -> tuple[dict, dict]:
    doctor_execution = run_json_command(
        build_doctor_command(args),
        staging / "doctor.stdout.json",
        staging / "doctor.stderr.txt",
        args.evaluation_timeout_secs,
        args.max_json_bytes,
    )
    validation_execution = run_json_command(
        build_validation_command(args),
        staging / "validation.stdout.json",
        staging / "validation.stderr.txt",
        args.evaluation_timeout_secs,
        args.max_json_bytes,
    )
    return doctor_execution, validation_execution


def _build_report(
    args: argparse.Namespace,
    catalog: dict,
    profile: dict | None,
    doctor: dict,
    validation: dict,
    doctor_execution: dict,
    validation_execution: dict,
) -> dict:
    contract_errors = validation_contract_errors(validation, catalog, profile is not None)
    findings = map_findings(validation, catalog)
    domains = summarize_domains(findings)
    comparison = validator_comparison(findings)
    dicom_paths = [Path(path) for path in validation.get("files", [])]
    if not dicom_paths:
        raise WorkbenchError("validation report did not contain any DICOM files")
    failed_findings = sum(finding["status"] == "failed" for finding in findings)
    unmapped_findings = sum(
        finding["catalog_status"] == "unmapped" for finding in findings
    )
    status = (
        "passed"
        if doctor_execution["returncode"] == 0
        and validation_execution["returncode"] == 0
        and failed_findings == 0
        and unmapped_findings == 0
        else "failed"
    )
    if any(finding["status"] == "execution_error" for finding in findings) or doctor_execution["returncode"] != 0 or contract_errors or validation_execution["returncode"] not in {0, 1}:
        status = "execution_error"
    return {
        "schema_version": "wsi-dicom-bench-workbench-report-v2",
        "contract_errors": contract_errors,
        "status": status,
        "catalog": {
            "version": catalog["catalog_version"],
            "dicom_edition": catalog["dicom_edition"],
            "path": str(args.catalog),
            "sha256": sha256_file(args.catalog),
        },
        "profile": (
            {
                "id": profile["profile_id"],
                "version": profile["profile_version"],
                "dicom_edition": profile["dicom_edition"],
                "path": str(args.profile),
                "sha256": sha256_file(args.profile),
            }
            if profile is not None
            else None
        ),
        "evaluation_policy": {
            "strict_external_tools": not args.allow_missing_tools,
            "max_pixel_frames_per_transfer_syntax": args.max_pixel_frames,
            "command_timeout_secs": args.command_timeout_secs,
            "evaluation_timeout_secs": args.evaluation_timeout_secs,
        },
        "software": software_inventory(args, doctor),
        "slide": inspect_dicom_set(dicom_paths),
        "domains": domains,
        "findings": findings,
        "validator_comparison": comparison,
        "doctor": doctor,
        "execution": {
            "doctor": doctor_execution,
            "validation": validation_execution,
        },
        "limitations": _limitations(doctor, findings),
    }


def _limitations(doctor: dict, findings: list[dict]) -> dict:
    return {
        "unavailable_tools": [
            {
                "name": tool.get("name"),
                "status": tool.get("status"),
                "required": tool.get("required"),
                "message": tool.get("message"),
            }
            for tool in doctor.get("tools", [])
            if tool.get("status") != "available"
        ],
        "skipped_checks": [
            {
                "check_name": finding["check_name"],
                "path": finding["path"],
                "message": finding["message"],
            }
            for finding in findings
            if finding["status"] == "skipped"
        ],
        "unmapped_checks": sorted(
            {
                finding["check_name"]
                for finding in findings
                if finding["catalog_status"] == "unmapped"
            }
        ),
        "source_pixel_fidelity": "not_evaluated_by_this_dicom_only_entry_point",
        "dicomweb_receiver_behavior": "not_evaluated_by_this_entry_point",
    }


def build_doctor_command(args: argparse.Namespace) -> list[str]:
    command = [str(args.wsi_dicom), "doctor", "--json"]
    if not args.allow_missing_tools:
        command.append("--strict")
    append_validator_options(command, args)
    return command


def build_validation_command(args: argparse.Namespace) -> list[str]:
    command = [
        str(args.wsi_dicom),
        "validate",
        str(args.dicom),
        "--profile",
        "core-2026c" if args.profile else "general",
        "--json",
        "--max-pixel-frames",
        str(args.max_pixel_frames),
        "--command-timeout-secs",
        str(args.command_timeout_secs),
    ]
    if not args.allow_missing_tools:
        command.append("--strict")
    append_validator_options(command, args)
    return command


def append_validator_options(command: list[str], args: argparse.Namespace) -> None:
    if args.dcmvalidate_iod:
        command.extend(["--dcmvalidate-iod", str(args.dcmvalidate_iod)])
    if args.htj2k_decoder:
        command.extend(["--htj2k-decoder", args.htj2k_decoder])


def run_json_command(
    command: list[str],
    stdout_path: Path,
    stderr_path: Path,
    timeout_secs: int,
    max_json_bytes: int,
) -> dict:
    execution = run_bounded_command(
        command,
        stdout_path=stdout_path,
        stderr_path=stderr_path,
        timeout_secs=timeout_secs,
        max_output_bytes=max_json_bytes,
    )
    if execution["timed_out"]:
        raise WorkbenchError(f"command timed out after {timeout_secs}s: {command}")
    if execution["launch_error"] is not None:
        raise WorkbenchError(
            f"command could not be launched: {command}: {execution['launch_error']}"
        )
    if execution["stdout_truncated"] or execution["stderr_truncated"]:
        raise WorkbenchError(
            f"command output exceeds max-json-bytes={max_json_bytes}: {command}"
        )
    size = stdout_path.stat().st_size
    if size == 0:
        detail = read_bounded_text(stderr_path, 4096)
        raise WorkbenchError(f"command produced no JSON: {command}; stderr={detail!r}")
    try:
        document = json.loads(stdout_path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise WorkbenchError(f"command produced invalid JSON: {command}: {exc}") from exc
    return {
        **execution,
        "stderr": read_bounded_text(stderr_path),
        "document": document,
    }


def software_inventory(args: argparse.Namespace, doctor: dict) -> dict:
    version = run_text_command(
        [str(args.wsi_dicom), "--version"],
        timeout_secs=min(args.evaluation_timeout_secs, 30),
        max_output_bytes=4096,
    )
    validators = []
    for tool in doctor.get("tools", []):
        path_text = tool.get("path")
        path = Path(path_text) if path_text else None
        validators.append(
            {
                "name": tool.get("name"),
                "status": tool.get("status"),
                "path": path_text,
                "sha256": sha256_file(path) if path and path.is_file() else None,
                "probe_command": tool.get("command") or [],
                "probe_stdout": tool.get("probe_stdout"),
                "probe_stderr": tool.get("probe_stderr"),
                "runtime": python_validator_inventory(path, run_text_command) if tool.get("name") == "validate_iods" and path and path.is_file() else None,
            }
        )
    return {
        "wsi_dicom": {
            "path": str(args.wsi_dicom),
            "version": version,
            "sha256": sha256_file(args.wsi_dicom),
        },
        "validators_and_decoders": validators,
        "python": sys.version.splitlines()[0],
    }


def run_text_command(
    command: list[str], timeout_secs: int, max_output_bytes: int
) -> str:
    with tempfile.TemporaryDirectory(prefix="wsi-workbench-text-") as temporary:
        root = Path(temporary)
        stdout_path = root / "stdout.txt"
        stderr_path = root / "stderr.txt"
        execution = run_bounded_command(
            command,
            stdout_path=stdout_path,
            stderr_path=stderr_path,
            timeout_secs=timeout_secs,
            max_output_bytes=max_output_bytes,
        )
        combined = stdout_path.read_bytes() + stderr_path.read_bytes()
    if execution["timed_out"]:
        raise WorkbenchError(f"command timed out after {timeout_secs}s: {command}")
    if execution["launch_error"] is not None:
        raise WorkbenchError(
            f"command could not be launched: {command}: {execution['launch_error']}"
        )
    if (
        execution["stdout_truncated"]
        or execution["stderr_truncated"]
        or execution["stdout_bytes"] + execution["stderr_bytes"] > max_output_bytes
    ):
        raise WorkbenchError(f"command output exceeds {max_output_bytes} bytes: {command}")
    text = combined.decode("utf-8", errors="replace").strip()
    if execution["returncode"] != 0 or not text:
        raise WorkbenchError(
            f"command failed to report a version: {command}; returncode={execution['returncode']}"
        )
    return text


def render_summary(report: dict) -> str:
    slide = report["slide"]
    lines = [
        "# WSI-DICOM Bench workbench report",
        "",
        f"- Status: `{report['status']}`",
        f"- Slide: `{slide['slide_id']}`",
        f"- DICOM instances: {slide['instance_count']}",
        f"- Rule catalog: `{report['catalog']['version']}`",
        f"- DICOM edition: `{report['catalog']['dicom_edition']}`",
        f"- Core profile: `{report['profile']['id'] if report['profile'] else 'not selected'}`",
        "",
        "## Domain results",
        "",
        "| Domain | Status | Passed | Failed | Skipped | Unmapped |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    for domain, summary in sorted(report["domains"].items()):
        lines.append(
            f"| {domain} | {summary['status']} | {summary['passed']} | "
            f"{summary['failed']} | {summary['skipped']} | {summary['unmapped']} |"
        )
    comparison = report["validator_comparison"]
    lines.extend(
        [
            "",
            "## Independent validator comparison",
            "",
            f"- Intrinsic/decoder aggregate: `{comparison['intrinsic']}`",
            f"- Aggregate disagreement: `{str(comparison['disagreement']).lower()}`",
            "",
            "| Validator | Status |",
            "| --- | --- |",
        ]
    )
    for validator, status in comparison["external"].items():
        lines.append(f"| {validator} | {status} |")
    lines.extend(
        [
            "",
            "## Explicit limitations",
            "",
            f"- Source pixel fidelity: `{report['limitations']['source_pixel_fidelity']}`",
            f"- DICOMweb receiver behavior: `{report['limitations']['dicomweb_receiver_behavior']}`",
            f"- Unavailable or unconfigured tools: {len(report['limitations']['unavailable_tools'])}",
            f"- Skipped checks: {len(report['limitations']['skipped_checks'])}",
            f"- Unmapped checks: {len(report['limitations']['unmapped_checks'])}",
            "",
        ]
    )
    return "\n".join(lines)


if __name__ == "__main__":
    raise SystemExit(main())
