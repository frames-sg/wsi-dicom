"""Adapter for the wsi-dicom Rust CLI."""
from __future__ import annotations
import json
import os
import signal
import subprocess
import time
from pathlib import Path
from typing import Optional

from manifest import WSI_DICOM_BIN, WSI_DICOM_TS, Slide, passthrough_cli_value
from adapters.common import (
    RunOutcome,
    build_run_outcome,
    parse_time_l_stderr,
    reset_output_dir,
)


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
    reset_output_dir(out_dir)

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
        return build_run_outcome(
            ok=False, wall_seconds=time.monotonic() - t0,
            peak_rss_bytes=None, output_dir=out_dir,
            extra_metrics={}, stderr=e.stderr or "", stderr_tail_chars=2000,
            error=f"timeout: command exceeded {timeout:g}s",
        )
    wall = time.monotonic() - t0
    rss = parse_time_l_stderr(r.stderr)

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

    if r.returncode != 0:
        return build_run_outcome(
            ok=False, wall_seconds=wall, peak_rss_bytes=rss,
            output_dir=out_dir, extra_metrics=extra, stderr=r.stderr,
            stderr_tail_chars=2000,
            error=f"returncode={r.returncode}",
        )
    return build_run_outcome(
        ok=True, wall_seconds=wall, peak_rss_bytes=rss,
        output_dir=out_dir, extra_metrics=extra, stderr=r.stderr,
    )
