"""Adapter for wsidicomizer (Python)."""
from __future__ import annotations
import shutil
import time
import resource
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from manifest import Slide


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


def _build_encoding(transfer_syntax: str):
    """Return a wsidicom.codec.Settings instance, or None for passthrough."""
    from wsidicom.codec import settings as cs

    if transfer_syntax == "passthrough":
        return None
    if transfer_syntax == "htj2k-lossless-rpcl":
        # wsidicomizer -> HTJ2K Lossless. (RPCL ordering is a wsi-dicom-only detail;
        # we report wsidicomizer's HTJ2K-Lossless as the closest available comparison.)
        return cs.HTJpeg2000Settings(levels=0)
    if transfer_syntax == "jpeg-baseline":
        return cs.JpegSettings(quality=80)
    if transfer_syntax == "jpeg2000-lossless":
        return cs.Jpeg2kSettings(levels=0)
    raise ValueError(f"unknown transfer syntax: {transfer_syntax}")


def run(
    slide: Slide,
    transfer_syntax: str,
    level_scope: str,
    out_dir: Path,
) -> RunOutcome:
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    from wsidicomizer import WsiDicomizer

    encoding = _build_encoding(transfer_syntax)
    include_levels = [0] if level_scope == "base" else None
    force_transcoding = transfer_syntax != "passthrough"

    rss_start = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    t0 = time.monotonic()
    err = None
    try:
        WsiDicomizer.convert(
            filepath=slide.path,
            output_path=str(out_dir),
            tile_size=512,
            include_levels=include_levels,
            include_label=False,
            include_overview=False,
            include_thumbnail=False,
            encoding=encoding,
            force_transcoding=force_transcoding,
        )
    except Exception as e:
        err = f"{type(e).__name__}: {e}"
    wall = time.monotonic() - t0
    rss_end = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    # macOS getrusage returns bytes; Linux returns kibibytes. We're on macOS.
    peak_rss = max(rss_end, rss_start)

    out_bytes = _bytes_in_dir(out_dir) if out_dir.exists() else 0
    if err is not None:
        return RunOutcome(False, wall, peak_rss, str(out_dir), out_bytes,
                          {}, "", error=err)
    return RunOutcome(True, wall, peak_rss, str(out_dir), out_bytes, {}, "")
