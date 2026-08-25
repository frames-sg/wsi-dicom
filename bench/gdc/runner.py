"""Bounded subprocess lifecycle for benchmark operations."""

from __future__ import annotations

import subprocess
import time
from pathlib import Path
from typing import Sequence

def run_command(
    command: Sequence[str],
    *,
    cwd: Path,
    stdout_path: Path,
    stderr_path: Path,
    timeout_secs: int,
) -> dict:
    started = time.perf_counter()
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    with stdout_path.open("w", encoding="utf-8") as stdout_file, stderr_path.open(
        "w", encoding="utf-8"
    ) as stderr_file:
        try:
            completed = subprocess.run(
                list(command),
                cwd=cwd,
                stdout=stdout_file,
                stderr=stderr_file,
                text=True,
                timeout=timeout_secs,
                check=False,
            )
            returncode = completed.returncode
            status = "passed" if returncode == 0 else "failed"
        except subprocess.TimeoutExpired:
            returncode = None
            status = "timeout"
        except OSError as exc:
            returncode = None
            status = "failed"
            stderr_file.write(f"{type(exc).__name__}: {exc}\n")
    return {
        "status": status,
        "returncode": returncode,
        "elapsed_secs": time.perf_counter() - started,
        "stdout_path": str(stdout_path),
        "stderr_path": str(stderr_path),
    }
