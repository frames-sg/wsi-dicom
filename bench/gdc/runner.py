"""Bounded subprocess lifecycle for benchmark operations."""

from __future__ import annotations

from pathlib import Path
from typing import Sequence

from bench.process_evidence import run_bounded_command


def run_command(
    command: Sequence[str],
    *,
    cwd: Path,
    stdout_path: Path,
    stderr_path: Path,
    timeout_secs: int,
) -> dict:
    evidence = run_bounded_command(
        command,
        cwd=cwd,
        stdout_path=stdout_path,
        stderr_path=stderr_path,
        timeout_secs=timeout_secs,
    )
    status = (
        "timeout"
        if evidence["timed_out"]
        else "failed"
        if evidence["launch_error"] is not None
        or evidence["stdout_truncated"]
        or evidence["stderr_truncated"]
        else "passed"
        if evidence["returncode"] == 0
        else "failed"
    )
    return {
        "status": status,
        "returncode": evidence["returncode"],
        "elapsed_secs": evidence["elapsed_seconds"],
        "stdout_path": str(stdout_path),
        "stderr_path": str(stderr_path),
        "stdout_bytes": evidence["stdout_bytes"],
        "stderr_bytes": evidence["stderr_bytes"],
        "stdout_truncated": evidence["stdout_truncated"],
        "stderr_truncated": evidence["stderr_truncated"],
        "launch_error": evidence["launch_error"],
    }
