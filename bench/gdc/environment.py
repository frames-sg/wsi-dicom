"""Reproducible benchmark environment evidence."""

from __future__ import annotations

import datetime as dt
import platform
import socket
import subprocess
import sys
from pathlib import Path
from typing import Sequence

def command_output(command: Sequence[str], *, cwd: Path, timeout_secs: int = 30) -> str | None:
    try:
        completed = subprocess.run(
            list(command),
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=timeout_secs,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if completed.returncode != 0:
        return None
    output = completed.stdout.strip()
    return output or None

def python_package_version(python_command: Sequence[str], package: str, *, cwd: Path) -> str | None:
    return command_output(
        [
            *python_command,
            "-c",
            "import importlib.metadata as m; print(m.version(%r))" % package,
        ],
        cwd=cwd,
    )

def cargo_package_version(*, cwd: Path) -> str | None:
    output = command_output(["cargo", "pkgid", "-p", "wsi-dicom"], cwd=cwd)
    if not output:
        return None
    package = output.rsplit("#", maxsplit=1)[-1]
    return package.rsplit("@", maxsplit=1)[-1] if "@" in package else package

def host_accelerator_info() -> dict:
    info: dict[str, object] = {}
    if platform.system() == "Darwin":
        output = command_output(
            ["system_profiler", "SPDisplaysDataType"],
            cwd=Path.cwd(),
            timeout_secs=15,
        )
        if output:
            info["system_profiler_displays"] = output
    if platform.system() == "Linux":
        output = command_output(
            [
                "nvidia-smi",
                "--query-gpu=name,driver_version,cuda_version,memory.total",
                "--format=csv,noheader",
            ],
            cwd=Path.cwd(),
            timeout_secs=15,
        )
        if output:
            info["nvidia_smi"] = output
    return info

def collect_environment(
    *,
    cwd: Path,
    wsi_dicom_command: Sequence[str],
    python_command: Sequence[str],
) -> dict:
    return {
        "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "hostname": socket.gethostname(),
        "platform": platform.platform(),
        "system": platform.system(),
        "machine": platform.machine(),
        "processor": platform.processor(),
        "python": sys.version.replace("\n", " "),
        "cwd": str(cwd),
        "git_commit": command_output(["git", "rev-parse", "HEAD"], cwd=cwd),
        "git_status": command_output(["git", "status", "--short"], cwd=cwd),
        "wsi_dicom_help": command_output([*wsi_dicom_command, "--help"], cwd=cwd),
        "wsi_dicom_package_version": cargo_package_version(cwd=cwd),
        "wsidicomizer_version": python_package_version(
            python_command, "wsidicomizer", cwd=cwd
        ),
        "wsidicom_version": python_package_version(python_command, "wsidicom", cwd=cwd),
        "openslide_python_version": python_package_version(
            python_command, "openslide-python", cwd=cwd
        ),
        "accelerator": host_accelerator_info(),
    }
