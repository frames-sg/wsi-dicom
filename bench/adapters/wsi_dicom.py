"""Adapter for the wsi-dicom Rust CLI."""
from __future__ import annotations
import json
import os
import signal
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from manifest import WSI_DICOM_BIN, WSI_DICOM_TS, Slide, passthrough_cli_value


@dataclass
class RunOutcome:
    ok: bool
    wall_seconds: float
    peak_rss_bytes: Optional[int]
    output_dir: str
    output_size_bytes: int
    extra_metrics: dict
    stderr_tail: str
    error: Optional[str] = None


def _bytes_in_dir(p: Path) -> int:
    total = 0
    for f in p.rglob("*"):
        if f.is_file():
            total += f.stat().st_size
    return total


def _parse_time_l_stderr(stderr: str) -> Optional[int]:
    """macOS /usr/bin/time -l prints maximum resident set size in bytes."""
    for line in stderr.splitlines():
        line = line.strip()
        if line.endswith("maximum resident set size"):
            tok = line.split()
            try:
                return int(tok[0])
            except ValueError:
                return None
    return None


def _run_command(cmd: list[str], timeout_seconds: float) -> subprocess.CompletedProcess[str]:
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    try:
        stdout, stderr = proc.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as e:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            stdout, stderr = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            stdout, stderr = proc.communicate()
        raise subprocess.TimeoutExpired(
            e.cmd, e.timeout, output=stdout, stderr=stderr
        ) from e

    return subprocess.CompletedProcess(cmd, proc.returncode, stdout, stderr)


def run(
    slide: Slide,
    transfer_syntax: str,
    level_scope: str,
    backend: str,           # "cpu" | "prefer-device" | "require-device" | "auto"
    out_dir: Path,
    timeout_seconds: Optional[float] = None,
) -> RunOutcome:
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    if transfer_syntax == "passthrough":
        ts_cli = passthrough_cli_value(slide)
    else:
        ts_cli = WSI_DICOM_TS[transfer_syntax]
    effective_backend = (
        "auto"
        if transfer_syntax == "jpeg-baseline" and backend == "prefer-device"
        else backend
    )

    cmd = [
        "/usr/bin/time", "-l",
        str(WSI_DICOM_BIN), "convert",
        slide.path,
        "--out", str(out_dir),
        "--transfer-syntax", ts_cli,
        "--backend", effective_backend,
        "--json",
    ]
    if level_scope == "base":
        cmd += ["--level", "0"]
    if effective_backend in ("prefer-device", "require-device"):
        cmd.append("--source-device-decode")
    if transfer_syntax == "jpeg-baseline":
        cmd += ["--tile-size", "256", "--jpeg-quality", "80"]

    t0 = time.monotonic()
    timeout = timeout_seconds if timeout_seconds is not None and timeout_seconds > 0 else 60 * 60 * 2
    try:
        r = _run_command(cmd, timeout_seconds=timeout)
    except subprocess.TimeoutExpired as e:
        return RunOutcome(
            ok=False, wall_seconds=time.monotonic() - t0,
            peak_rss_bytes=None, output_dir=str(out_dir),
            output_size_bytes=_bytes_in_dir(out_dir) if out_dir.exists() else 0,
            extra_metrics={}, stderr_tail=(e.stderr or "")[-2000:],
            error=f"timeout: command exceeded {timeout:g}s",
        )
    wall = time.monotonic() - t0
    rss = _parse_time_l_stderr(r.stderr)

    extra = {}
    # Try to parse the wsi-dicom JSON report from stdout
    if r.stdout.strip():
        try:
            extra = json.loads(r.stdout.strip().splitlines()[-1])
        except Exception:
            for line in reversed(r.stdout.strip().splitlines()):
                line = line.strip()
                if line.startswith("{") and line.endswith("}"):
                    try:
                        extra = json.loads(line)
                        break
                    except Exception:
                        continue

    out_bytes = _bytes_in_dir(out_dir) if out_dir.exists() else 0
    if r.returncode != 0:
        return RunOutcome(
            ok=False, wall_seconds=wall, peak_rss_bytes=rss,
            output_dir=str(out_dir), output_size_bytes=out_bytes,
            extra_metrics=extra, stderr_tail=r.stderr[-2000:],
            error=f"returncode={r.returncode}",
        )
    return RunOutcome(
        ok=True, wall_seconds=wall, peak_rss_bytes=rss,
        output_dir=str(out_dir), output_size_bytes=out_bytes,
        extra_metrics=extra, stderr_tail=r.stderr[-1000:],
    )
