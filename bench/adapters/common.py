"""Shared benchmark adapter result and filesystem helpers."""
from __future__ import annotations

import resource
import shutil
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional


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


def reset_output_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def bytes_in_dir(path: Path) -> int:
    total = 0
    for child in path.rglob("*"):
        if child.is_file():
            total += child.stat().st_size
    return total


def parse_time_l_stderr(stderr: str) -> Optional[int]:
    """macOS /usr/bin/time -l prints maximum resident set size in bytes."""
    for line in stderr.splitlines():
        line = line.strip()
        if line.endswith("maximum resident set size"):
            token = line.split()
            try:
                return int(token[0])
            except ValueError:
                return None
    return None


def current_process_peak_rss_bytes() -> int:
    # macOS getrusage returns bytes; Linux returns kibibytes.
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss


def build_run_outcome(
    *,
    ok: bool,
    wall_seconds: float,
    peak_rss_bytes: Optional[int],
    output_dir: Path,
    extra_metrics: Optional[dict] = None,
    stderr: str = "",
    error: Optional[str] = None,
    stderr_tail_chars: int = 1000,
) -> RunOutcome:
    output_size_bytes = bytes_in_dir(output_dir) if output_dir.exists() else 0
    return RunOutcome(
        ok=ok,
        wall_seconds=wall_seconds,
        peak_rss_bytes=peak_rss_bytes,
        output_dir=str(output_dir),
        output_size_bytes=output_size_bytes,
        extra_metrics=extra_metrics or {},
        stderr_tail=stderr[-stderr_tail_chars:],
        error=error,
    )


def run_in_process(
    *,
    output_dir: Path,
    action: Callable[[], Optional[dict]],
) -> RunOutcome:
    rss_start = current_process_peak_rss_bytes()
    started = time.monotonic()
    error = None
    extra_metrics: Optional[dict] = None
    try:
        extra_metrics = action()
    except Exception as exc:
        error = f"{type(exc).__name__}: {exc}"
    wall_seconds = time.monotonic() - started
    peak_rss = max(current_process_peak_rss_bytes(), rss_start)

    return build_run_outcome(
        ok=error is None,
        wall_seconds=wall_seconds,
        peak_rss_bytes=peak_rss,
        output_dir=output_dir,
        extra_metrics=extra_metrics,
        error=error,
    )
