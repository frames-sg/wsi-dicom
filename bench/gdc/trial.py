"""One benchmark trial and its evidence record."""

from __future__ import annotations

from pathlib import Path
from typing import Sequence

from .models import Slide
from .outputs import collect_dicom_outputs, count_output_files
from .reporting import attach_result_context
from .runner import run_command
from .validation import validate_output, validation_failure_message

def benchmark_trial(
    *,
    slide: Slide,
    tool: str,
    command: Sequence[str],
    output_dir: Path,
    artifact_dir: Path,
    cwd: Path,
    timeout_secs: int,
    run_index: int,
    profile: str,
    scope: str,
    validate: bool,
    wsi_dicom_command: Sequence[str],
    system_label: str | None = None,
    preflight: dict | None = None,
) -> dict:
    if output_dir.exists():
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
                "status": "failed",
                "returncode": None,
                "elapsed_secs": 0.0,
                "output_dir": str(output_dir),
                "produced_files": 0,
                "output_bytes": 0,
                "command": list(command),
                "error": "output directory already exists; use --resume to skip completed trials or a new --run-label",
                "preflight": preflight,
            },
            system_label=system_label,
        )
    output_dir.parent.mkdir(parents=True, exist_ok=True)
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
        "slide": slide.slide_id,
        "display_name": slide.display_name,
        "gdc_file_id": slide.gdc_file_id,
        "source_path": str(slide.path),
        "tool": tool,
        "profile": profile,
        "scope": scope,
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
        validation = validate_output(
            wsi_dicom_command=wsi_dicom_command,
            output_dir=output_dir,
            artifact_dir=artifact_dir,
            cwd=cwd,
            timeout_secs=timeout_secs,
        )
        row["validation"] = validation
        if validation["status"] != "passed":
            row["status"] = "failed"
            row["error"] = validation_failure_message(validation)
    if preflight:
        row["preflight"] = preflight
    return attach_result_context(row, system_label=system_label)
