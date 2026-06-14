"""Adapter for wsidicomizer (Python)."""
from __future__ import annotations
from pathlib import Path

from adapters.common import (
    RunOutcome,
    build_run_outcome,
    reset_output_dir,
    run_in_process,
)
from manifest import Slide


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
    reset_output_dir(out_dir)

    from wsidicomizer import WsiDicomizer

    encoding = _build_encoding(transfer_syntax)
    include_levels = [0] if level_scope == "base" else None
    force_transcoding = transfer_syntax != "passthrough"

    def action():
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
        return None

    return run_in_process(output_dir=out_dir, action=action)
