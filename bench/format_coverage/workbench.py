"""Execute the DICOM workbench for one converted format case."""

from __future__ import annotations

import sys
from pathlib import Path

from bench.process_evidence import run_bounded_command

from .captured_output import parse_json_output


def execute_workbench(
    *,
    conversion_output: Path,
    case_root: Path,
    workbench: Path,
    wsi_dicom: Path,
    catalog: Path,
    timeout_secs: int,
) -> dict:
    command = [
        sys.executable,
        str(workbench),
        str(conversion_output),
        "--output",
        str(case_root / "workbench"),
        "--wsi-dicom",
        str(wsi_dicom),
        "--catalog",
        str(catalog),
        "--max-pixel-frames",
        "1",
        "--evaluation-timeout-secs",
        str(timeout_secs),
    ]
    stdout_path = case_root / "workbench.stdout.json"
    evidence = run_bounded_command(
        command,
        stdout_path=stdout_path,
        stderr_path=case_root / "workbench.stderr.txt",
        timeout_secs=timeout_secs + 30,
    )
    evidence["report"] = parse_json_output(stdout_path)
    evidence["status"] = (
        "passed"
        if not evidence.get("timed_out", False)
        and evidence.get("returncode") == 0
        and evidence.get("launch_error") is None
        and not evidence.get("stdout_truncated", False)
        and not evidence.get("stderr_truncated", False)
        and isinstance(evidence["report"], dict)
        and evidence["report"].get("status") == "passed"
        else "failed"
    )
    return evidence
