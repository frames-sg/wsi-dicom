"""Device eligibility and performance preflight policy."""

from __future__ import annotations

from pathlib import Path
from typing import Sequence

from .commands import build_wsi_dicom_profile_command
from .models import Slide
from .reporting import attach_result_context
from .runner import run_command
from .validation import first_text_line, profile_metric, read_profile_report

def evaluate_device_preflight(
    *,
    cpu_result: dict,
    device_result: dict,
    min_speedup: float,
    min_device_frame_pct: float,
) -> dict:
    cpu_report, cpu_error = read_profile_report(Path(cpu_result["stdout_path"]))
    device_report, device_error = read_profile_report(Path(device_result["stdout_path"]))
    total_frames = profile_metric(device_report, "total_frames")
    gpu_encode_frames = profile_metric(device_report, "gpu_encode_frames")
    device_frame_pct = (
        100.0 * gpu_encode_frames / total_frames if total_frames > 0 else None
    )
    device_elapsed_secs = float(device_result["elapsed_secs"])
    speedup_vs_cpu = (
        float(cpu_result["elapsed_secs"]) / device_elapsed_secs
        if device_elapsed_secs > 0
        else None
    )
    status = "passed"
    reason = None
    if cpu_result["status"] != "passed":
        status = "failed"
        reason = "CPU preflight profile did not pass"
        detail = first_text_line(Path(cpu_result["stderr_path"]))
        if detail:
            reason = f"{reason}: {detail}"
    elif device_result["status"] != "passed":
        status = device_result["status"]
        reason = "device preflight profile did not pass"
        detail = first_text_line(Path(device_result["stderr_path"]))
        if detail:
            reason = f"{reason}: {detail}"
    elif cpu_error:
        status = "failed"
        reason = cpu_error
    elif device_error:
        status = "failed"
        reason = device_error
    else:
        if total_frames == 0:
            status = "failed"
            reason = "device preflight reported zero frames"
        elif device_frame_pct is not None and device_frame_pct < min_device_frame_pct:
            status = "failed"
            reason = (
                "device preflight used device encode for "
                f"{device_frame_pct:.1f}% of frames; required {min_device_frame_pct:.1f}%"
            )
        elif (speedup_vs_cpu or 0.0) < min_speedup:
            status = "failed"
            reason = (
                f"device preflight speedup was {(speedup_vs_cpu or 0.0):.3f}x; "
                f"required at least {min_speedup:.3f}x"
            )

    return {
        "status": status,
        "reason": reason,
        "cpu": {
            **cpu_result,
            "report": cpu_report,
            "report_error": cpu_error,
        },
        "device": {
            **device_result,
            "report": device_report,
            "report_error": device_error,
        },
        "cpu_elapsed_secs": cpu_result["elapsed_secs"],
        "device_elapsed_secs": device_result["elapsed_secs"],
        "speedup_vs_cpu": speedup_vs_cpu,
        "device_frame_pct": device_frame_pct,
    }

def run_device_preflight(
    *,
    wsi_dicom_command: Sequence[str],
    slide: Slide,
    artifact_dir: Path,
    cwd: Path,
    profile: str,
    scope: str,
    tile_size: int,
    jpeg_quality: int,
    source_device_decode: bool,
    max_frames: int,
    timeout_secs: int,
    min_speedup: float,
    min_device_frame_pct: float,
) -> dict:
    if profile != "htj2k-lossless-rpcl":
        return {
            "status": "skipped",
            "reason": f"device preflight does not support profile: {profile}",
        }

    preflight_dir = artifact_dir / "device-preflight"
    cpu_command = build_wsi_dicom_profile_command(
        wsi_dicom_command,
        slide.path,
        profile=profile,
        scope=scope,
        tile_size=tile_size,
        jpeg_quality=jpeg_quality,
        backend="cpu",
        source_device_decode=False,
        max_frames=max_frames,
    )
    device_command = build_wsi_dicom_profile_command(
        wsi_dicom_command,
        slide.path,
        profile=profile,
        scope=scope,
        tile_size=tile_size,
        jpeg_quality=jpeg_quality,
        backend="require-device",
        source_device_decode=source_device_decode,
        max_frames=max_frames,
    )
    cpu_result = run_command(
        cpu_command,
        cwd=cwd,
        stdout_path=preflight_dir / "cpu.stdout.json",
        stderr_path=preflight_dir / "cpu.stderr.txt",
        timeout_secs=timeout_secs,
    )
    device_result = run_command(
        device_command,
        cwd=cwd,
        stdout_path=preflight_dir / "device.stdout.json",
        stderr_path=preflight_dir / "device.stderr.txt",
        timeout_secs=timeout_secs,
    )
    preflight = evaluate_device_preflight(
        cpu_result={**cpu_result, "command": cpu_command},
        device_result={**device_result, "command": device_command},
        min_speedup=min_speedup,
        min_device_frame_pct=min_device_frame_pct,
    )
    preflight["max_frames"] = max_frames
    preflight["timeout_secs"] = timeout_secs
    preflight["min_speedup"] = min_speedup
    preflight["min_device_frame_pct"] = min_device_frame_pct
    return preflight

def preflight_failure_row(
    *,
    slide: Slide,
    tool: str,
    command: Sequence[str],
    output_dir: Path,
    profile: str,
    scope: str,
    run_index: int,
    preflight: dict,
    system_label: str | None,
) -> dict:
    status = "preflight-failed"
    if preflight.get("status") == "timeout":
        status = "preflight-timeout"
    return attach_result_context(
        {
            "slide": slide.slide_id,
            "display_name": slide.display_name,
            "gdc_file_id": slide.gdc_file_id,
            "source_path": str(slide.path),
            "tool": tool,
            "profile": profile,
            "scope": scope,
            "run_index": run_index,
            "status": status,
            "returncode": preflight.get("device", {}).get("returncode"),
            "elapsed_secs": preflight.get("device_elapsed_secs", 0.0) or 0.0,
            "output_dir": str(output_dir),
            "produced_files": 0,
            "output_bytes": 0,
            "command": list(command),
            "stdout_path": preflight.get("device", {}).get("stdout_path"),
            "stderr_path": preflight.get("device", {}).get("stderr_path"),
            "preflight": preflight,
            "error": preflight.get("reason") or "device preflight failed",
        },
        system_label=system_label,
    )
