"""Post-run validation and profile-report inspection."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Sequence

from .runner import run_command

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
        "--strict",
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

def read_profile_report(stdout_path: Path) -> tuple[dict | None, str | None]:
    try:
        text = stdout_path.read_text(encoding="utf-8").strip()
    except OSError as exc:
        return None, f"failed to read profile stdout: {exc}"
    if not text:
        return None, "profile stdout was empty"
    try:
        return json.loads(text), None
    except json.JSONDecodeError as exc:
        return None, f"failed to parse profile JSON: {exc}"

def first_text_line(path: Path) -> str | None:
    try:
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.strip():
                return line.strip()
    except OSError:
        return None
    return None

def validation_failure_message(validation: dict) -> str:
    stderr_path = validation.get("stderr_path")
    if stderr_path:
        detail = first_text_line(Path(stderr_path))
        if detail:
            return f"validation failed: {detail}"
    status = validation.get("status") or "failed"
    return f"validation {status}"

def profile_metric(report: dict | None, name: str, default: int = 0) -> int:
    if not report:
        return default
    try:
        return int(report.get("metrics", {}).get(name, default) or default)
    except (TypeError, ValueError):
        return default
