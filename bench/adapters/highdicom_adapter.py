"""Adapter for a manual highdicom + glymur/PIL pipeline.

Reads tiles from openslide, encodes each tile with the requested codec, and
assembles a VLWholeSlideMicroscopyImage per level via highdicom. This is the
most "from scratch" reference; it has the most Python overhead and is
expected to be the slowest tool in the matrix.

Passthrough is intentionally not implemented for this adapter, because writing
a hand-rolled passthrough would essentially reimplement wsi-dicom's encapsulation
path. We mark passthrough as N/A for highdicom in the matrix.
"""
from __future__ import annotations
import io
from pathlib import Path

from adapters.common import (
    RunOutcome,
    build_run_outcome,
    reset_output_dir,
    run_in_process,
)
from manifest import Slide


TILE = 512


def _encode_jpeg(rgb, quality: int = 80) -> bytes:
    from PIL import Image

    buf = io.BytesIO()
    Image.fromarray(rgb).save(buf, format="JPEG", quality=quality, subsampling=2)
    return buf.getvalue()


def _encode_j2k_lossless_rpcl(rgb) -> bytes:
    """Encode an RGB tile to JPEG2000 Part-1 Lossless (RPCL ordering) via glymur.

    NOTE: glymur 0.14 / OpenJPEG 2.5 does not expose HTJ2K (Part-15) encoding,
    so the highdicom + glymur baseline transcodes to JPEG2000 Part-1 Lossless
    instead. This is the closest analog reachable from a pure-Python pipeline
    and reflects a real Python-ecosystem limitation we report in the paper.
    """
    import glymur
    import os
    import tempfile

    with tempfile.TemporaryDirectory(prefix="bench_j2k_") as tmpdir:
        path = Path(tmpdir) / f"tile_{os.getpid()}_{id(rgb)}.j2k"
        glymur.Jp2k(
            str(path),
            data=rgb,
            irreversible=False,        # 5-3 wavelet -> lossless
            mct=True,
            cratios=[1],
            prog="RPCL",
            cbsize=(64, 64),
        )
        return path.read_bytes()


def _build_dicom_for_level(
    slide_path: str,
    level_index: int,
    out_path: Path,
    transfer_syntax: str,
) -> int:
    """Read level via openslide, encode tile-by-tile, assemble DICOM. Returns frames written."""
    import numpy as np
    import openslide
    from pydicom.uid import (
        JPEGBaseline8Bit,
        HTJ2KLossless,
        JPEG2000Lossless,
        generate_uid,
    )

    if transfer_syntax == "jpeg-baseline":
        ts_uid = JPEGBaseline8Bit
    elif transfer_syntax == "htj2k-lossless-rpcl":
        # See note in _encode_j2k_lossless_rpcl: pure-Python HTJ2K not available.
        # We emit JPEG2000-Lossless and report that as the highdicom baseline.
        ts_uid = JPEG2000Lossless
    elif transfer_syntax == "jpeg2000-lossless":
        ts_uid = JPEG2000Lossless
    else:
        raise ValueError(f"highdicom adapter does not support {transfer_syntax}")

    src = openslide.OpenSlide(slide_path)
    try:
        w, h = src.level_dimensions[level_index]
        downsample = src.level_downsamples[level_index]
        n_x = (w + TILE - 1) // TILE
        n_y = (h + TILE - 1) // TILE
        n_frames = n_x * n_y

        # Per-frame encoded bytestreams
        encoded_frames: list[bytes] = []
        for ty in range(n_y):
            for tx in range(n_x):
                x_lvl = tx * TILE
                y_lvl = ty * TILE
                tw = min(TILE, w - x_lvl)
                th = min(TILE, h - y_lvl)
                # read_region needs level-0 coords
                x0 = int(x_lvl * downsample)
                y0 = int(y_lvl * downsample)
                rgba = np.array(src.read_region((x0, y0), level_index, (tw, th)))
                rgb = rgba[..., :3]
                # pad to TILE x TILE if needed
                if (th, tw) != (TILE, TILE):
                    canvas = np.zeros((TILE, TILE, 3), dtype=np.uint8)
                    canvas[:th, :tw, :] = rgb
                    rgb = canvas

                if transfer_syntax == "jpeg-baseline":
                    encoded = _encode_jpeg(rgb)
                else:
                    encoded = _encode_j2k_lossless_rpcl(rgb)
                encoded_frames.append(encoded)

        # Build minimal VL Whole Slide image dataset using highdicom
        # For simplicity we hand-roll a sparse SOP. highdicom's
        # VLWholeSlideMicroscopyImage requires significant metadata.
        from pydicom.dataset import Dataset, FileMetaDataset
        from pydicom.encaps import encapsulate

        ds = Dataset()
        ds.SOPClassUID = "1.2.840.10008.5.1.4.1.1.77.1.6"  # VL WSM Image Storage
        ds.SOPInstanceUID = generate_uid()
        ds.StudyInstanceUID = generate_uid()
        ds.SeriesInstanceUID = generate_uid()
        ds.Modality = "SM"
        ds.PatientID = "BENCH-PATIENT"
        ds.PatientName = "BENCH^TEST"
        ds.StudyDate = ""
        ds.StudyTime = ""
        ds.AccessionNumber = ""
        ds.ReferringPhysicianName = ""
        ds.Manufacturer = "highdicom-bench"
        ds.SeriesNumber = 1
        ds.InstanceNumber = 1
        ds.ImageType = ["DERIVED", "PRIMARY", "VOLUME", "NONE"]
        ds.SamplesPerPixel = 3
        ds.PhotometricInterpretation = (
            "YBR_FULL_422" if transfer_syntax == "jpeg-baseline" else "YBR_RCT"
        )
        ds.PlanarConfiguration = 0
        ds.BitsAllocated = 8
        ds.BitsStored = 8
        ds.HighBit = 7
        ds.PixelRepresentation = 0
        ds.NumberOfFrames = n_frames
        ds.Rows = TILE
        ds.Columns = TILE
        ds.TotalPixelMatrixColumns = w
        ds.TotalPixelMatrixRows = h
        ds.TotalPixelMatrixFocalPlanes = 1
        ds.TotalPixelMatrixOriginSequence = []
        origin = Dataset()
        origin.XOffsetInSlideCoordinateSystem = "0.0"
        origin.YOffsetInSlideCoordinateSystem = "0.0"
        ds.TotalPixelMatrixOriginSequence = [origin]
        ds.SpecimenLabelInImage = "NO"
        ds.BurnedInAnnotation = "NO"
        ds.LossyImageCompression = "01" if transfer_syntax == "jpeg-baseline" else "00"
        ds.VolumetricProperties = "VOLUME"
        ds.ImageOrientationSlide = ["0.0", "1.0", "0.0", "-1.0", "0.0", "0.0"]
        ds.AcquisitionDateTime = "20260101000000"
        ds.DimensionOrganizationType = "TILED_FULL"
        ds.PixelData = encapsulate(encoded_frames)

        meta = FileMetaDataset()
        meta.MediaStorageSOPClassUID = ds.SOPClassUID
        meta.MediaStorageSOPInstanceUID = ds.SOPInstanceUID
        meta.TransferSyntaxUID = ts_uid
        meta.ImplementationClassUID = generate_uid()
        meta.ImplementationVersionName = "highdicom-bench"

        from pydicom.dataset import FileDataset
        fds = FileDataset(str(out_path), ds, file_meta=meta, preamble=b"\0" * 128)
        fds.is_little_endian = True
        fds.is_implicit_VR = False
        fds.save_as(str(out_path), write_like_original=False)

        return n_frames
    finally:
        src.close()


def run(
    slide: Slide,
    transfer_syntax: str,
    level_scope: str,
    out_dir: Path,
) -> RunOutcome:
    if transfer_syntax == "passthrough":
        return build_run_outcome(
            ok=False, wall_seconds=0.0, peak_rss_bytes=None, output_dir=out_dir,
            error="not-implemented: highdicom adapter does not implement passthrough",
        )

    reset_output_dir(out_dir)

    import openslide
    src = openslide.OpenSlide(slide.path)
    n_levels = src.level_count
    src.close()

    levels = [0] if level_scope == "base" else list(range(n_levels))

    def action():
        for li in levels:
            out_path = out_dir / f"level-{li:04d}.dcm"
            _build_dicom_for_level(slide.path, li, out_path, transfer_syntax)
        return None

    return run_in_process(output_dir=out_dir, action=action)
