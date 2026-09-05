"""Build, execute, and classify one format conversion."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from bench.process_evidence import read_bounded_text, run_bounded_command

from .captured_output import parse_json_output


@dataclass(frozen=True)
class ConversionOutcome:
    evaluation_mode: str
    output_path: Path
    status: str
    evidence: dict


def build_conversion_command(
    case: dict,
    wsi_dicom: Path,
    source: Path,
    output: Path,
    backend: str = "cpu",
) -> list[str]:
    command = [
        str(wsi_dicom),
        "convert",
        str(source),
        "--out",
        str(output),
        "--research-placeholder",
        "--backend",
        backend,
        "--transfer-syntax",
        case["transfer_syntax"],
        "--codec-validation",
        "round-trip",
        "--icc",
        "source-or-srgb",
        "--uid-policy",
        "deterministic",
        "--level",
        str(case["level"]),
        "--json",
    ]
    spacing = case.get("source_pixel_spacing_mm")
    if spacing is not None:
        command.extend(
            [
                "--source-pixel-spacing-mm",
                f"{spacing[0]},{spacing[1]}",
            ]
        )
    return command


def build_profile_command(
    case: dict, wsi_dicom: Path, source: Path, backend: str
) -> list[str]:
    return [
        str(wsi_dicom),
        "profile",
        str(source),
        "--backend",
        backend,
        "--transfer-syntax",
        case["transfer_syntax"],
        "--level",
        str(case["level"]),
        "--max-frames",
        str(case.get("max_frames", 1)),
        "--json",
    ]


def classify_conversion(case: dict, returncode: int, stdout: str, stderr: str) -> str:
    expected = case["expected"]
    if expected["outcome"] == "success":
        return "converted" if returncode == 0 else "failed"
    message = expected["message_contains"]
    if returncode != 0 and message in f"{stdout}\n{stderr}":
        return "expected_rejection"
    return "failed"


def classify_route(
    conversion_status: str, report: object | None, backend: str
) -> str:
    if conversion_status == "expected_rejection":
        return "unsupported"
    if conversion_status not in {"converted", "profiled"} or not isinstance(
        report, dict
    ):
        return "failed"
    metrics = report.get("metrics")
    if not isinstance(metrics, dict):
        return "unclassified"
    total = int(metrics.get("total_frames") or 0)
    if total > 0 and int(metrics.get("jpeg_passthrough_frames") or 0) == total:
        return "passthrough"
    if total > 0 and int(metrics.get("j2k_passthrough_frames") or 0) == total:
        return "passthrough"
    if total > 0 and int(metrics.get("gpu_encode_frames") or 0) == total:
        return "metal_encode"
    if backend == "cpu" and total > 0:
        return "cpu_encode"
    return "cpu_only_source_stage"


def execute_conversion(
    case: dict,
    *,
    wsi_dicom: Path,
    source: Path,
    case_root: Path,
    backend: str,
    timeout_secs: int,
) -> ConversionOutcome:
    output_path = case_root / "dicom"
    evaluation_mode = case.get("evaluation_mode", "convert")
    command = (
        build_profile_command(case, wsi_dicom, source, backend)
        if evaluation_mode == "route-profile"
        else build_conversion_command(case, wsi_dicom, source, output_path, backend)
    )
    stdout_path = case_root / "conversion.stdout.json"
    stderr_path = case_root / "conversion.stderr.txt"
    evidence = run_bounded_command(
        command,
        stdout_path=stdout_path,
        stderr_path=stderr_path,
        timeout_secs=timeout_secs,
        measure_resources=True,
    )
    stdout = read_bounded_text(stdout_path)
    stderr = read_bounded_text(stderr_path)
    process_failed = (
        evidence.get("timed_out", False)
        or evidence.get("launch_error") is not None
        or evidence.get("stdout_truncated", False)
        or evidence.get("stderr_truncated", False)
        or evidence.get("returncode") is None
    )
    if process_failed:
        status = "failed"
    elif evaluation_mode == "route-profile" and evidence["returncode"] == 0:
        status = "profiled"
    else:
        status = classify_conversion(
            case,
            int(evidence["returncode"]),
            stdout,
            stderr,
        )
    evidence["status"] = status
    evidence["report"] = parse_json_output(stdout_path)
    evidence["route_classification"] = classify_route(
        status, evidence["report"], backend
    )
    metrics = (
        evidence["report"].get("metrics", {})
        if isinstance(evidence["report"], dict)
        else {}
    )
    evidence["source_stage"] = {
        "cpu_input_frames": int(metrics.get("cpu_input_frames") or 0),
        "gpu_input_decode_frames": int(metrics.get("gpu_input_decode_frames") or 0),
        "input_decode_micros": int(metrics.get("input_decode_micros") or 0),
        "compose_micros": int(metrics.get("compose_micros") or 0),
        "classification": (
            "cpu_decode_or_composition"
            if int(metrics.get("cpu_input_frames") or 0) > 0
            else "no_cpu_pixel_stage_observed"
        ),
    }
    return ConversionOutcome(
        evaluation_mode=evaluation_mode,
        output_path=output_path,
        status=status,
        evidence=evidence,
    )
