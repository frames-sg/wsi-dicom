"""Finalize repository-generated synthetic WSI controls atomically."""

from __future__ import annotations

import argparse
import hashlib
import shutil
import sys
import tempfile
from pathlib import Path

if __package__ in {None, ""}:
    repository_root = str(Path(__file__).resolve().parents[2])
    if repository_root not in sys.path:
        sys.path.insert(0, repository_root)

import pydicom
from pydicom.encaps import encapsulate_extended

from bench.process_evidence import run_bounded_command


EXPLICIT_VR_LITTLE_ENDIAN = "1.2.840.10008.1.2.1"
JPEG_2000 = "1.2.840.10008.1.2.4.91"
HTJ2K = "1.2.840.10008.1.2.4.203"
MAX_CONTROL_BYTES = 4 * 1024 * 1024
MAX_COMMAND_OUTPUT_BYTES = 64 * 1024


def _synthetic_frames(dataset) -> list[bytes]:
    rows = int(dataset.Rows)
    columns = int(dataset.Columns)
    matrix_rows = int(dataset.TotalPixelMatrixRows)
    matrix_columns = int(dataset.TotalPixelMatrixColumns)
    pixels = bytearray()
    for y in range(matrix_rows):
        for x in range(matrix_columns):
            pixels.extend(((x * 37 + y * 11) & 0xFF, (x * 17 + y * 29) & 0xFF, (x * 7 + y * 43) & 0xFF))
    frames = []
    for tile_y in range(0, matrix_rows, rows):
        for tile_x in range(0, matrix_columns, columns):
            frame = bytearray(rows * columns * 3)
            for y in range(rows):
                for x in range(columns):
                    source_y = min(tile_y + y, matrix_rows - 1)
                    source_x = min(tile_x + x, matrix_columns - 1)
                    source = (source_y * matrix_columns + source_x) * 3
                    target = (y * columns + x) * 3
                    frame[target : target + 3] = pixels[source : source + 3]
            frames.append(bytes(frame))
    if len(frames) != int(dataset.NumberOfFrames):
        raise RuntimeError("synthetic frame reconstruction disagrees with Number of Frames")
    return frames


def _remove_extended_tables(dataset) -> None:
    for keyword in ("ExtendedOffsetTable", "ExtendedOffsetTableLengths"):
        if keyword in dataset:
            del dataset[keyword]


def _uid(control_id: str, role: str) -> str:
    digest = hashlib.sha256(f"negative-bench-v2:{control_id}:{role}".encode()).digest()[:16]
    return f"2.25.{int.from_bytes(digest, 'big')}"


def _set_control_identities(dataset, control_id: str) -> None:
    dataset.SOPInstanceUID = _uid(control_id, "sop")
    dataset.file_meta.MediaStorageSOPInstanceUID = dataset.SOPInstanceUID
    dataset.SeriesInstanceUID = _uid(control_id, "series")
    dataset.StudyInstanceUID = _uid(control_id, "study")
    dataset.PyramidUID = _uid(control_id, "pyramid")
    specimen = dataset.SpecimenDescriptionSequence[0]
    specimen.SpecimenUID = _uid(control_id, "specimen")
    specimen.SpecimenIdentifier = f"SYNTH-{control_id}"
    dataset.ContainerIdentifier = f"SYNTH-{control_id}"


def _write_ppm(path: Path, rows: int, columns: int, frame: bytes) -> None:
    path.write_bytes(f"P6\n{columns} {rows}\n255\n".encode("ascii") + frame)


def _run_encoder(command: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="wsi-negative-encoder-") as temporary:
        root = Path(temporary)
        stdout_path = root / "stdout.txt"
        stderr_path = root / "stderr.txt"
        result = run_bounded_command(
            command,
            stdout_path=stdout_path,
            stderr_path=stderr_path,
            timeout_secs=30,
            max_output_bytes=MAX_COMMAND_OUTPUT_BYTES,
        )
        detail = (stderr_path.read_bytes() or stdout_path.read_bytes()).decode(
            "utf-8", errors="replace"
        )
    if result["stdout_truncated"] or result["stderr_truncated"]:
        raise RuntimeError(f"encoder output exceeded limit: {command[0]}")
    if result["timed_out"]:
        raise RuntimeError(f"encoder timed out: {command[0]}")
    if result["launch_error"] is not None:
        raise RuntimeError(f"encoder could not be launched: {result['launch_error']}")
    if result["returncode"] != 0:
        raise RuntimeError(f"encoder failed ({result['returncode']}): {detail}")


def _encode_frames(dataset, encoder: str, workspace: Path) -> list[bytes]:
    rows = int(dataset.Rows)
    columns = int(dataset.Columns)
    encoded = []
    for index, frame in enumerate(_synthetic_frames(dataset)):
        ppm = workspace / f"frame-{index}.ppm"
        codestream = workspace / f"frame-{index}.j2c"
        _write_ppm(ppm, rows, columns, frame)
        if encoder == "jpeg2000":
            _run_encoder(
                [
                    "opj_compress",
                    "-i",
                    str(ppm),
                    "-o",
                    str(codestream),
                    "-I",
                    "-r",
                    "4",
                    "-n",
                    "1",
                ]
            )
        else:
            _run_encoder(
                [
                    "ojph_compress",
                    "-i",
                    str(ppm),
                    "-o",
                    str(codestream),
                    "-num_decomps",
                    "0",
                    "-reversible",
                    "false",
                    "-qstep",
                    "0.02",
                    "-colour_trans",
                    "true",
                ]
            )
        encoded.append(codestream.read_bytes())
    return encoded


def _make_native(dataset, anisotropic: bool) -> None:
    frames = _synthetic_frames(dataset)
    dataset.file_meta.TransferSyntaxUID = EXPLICIT_VR_LITTLE_ENDIAN
    dataset.PixelData = b"".join(frames)
    dataset["PixelData"].is_undefined_length = False
    dataset.PhotometricInterpretation = "RGB"
    _remove_extended_tables(dataset)
    if anisotropic:
        spacing = ["0.0005", "0.00025"]
        measures = dataset.SharedFunctionalGroupsSequence[0].PixelMeasuresSequence[0]
        measures.PixelSpacing = spacing
        dataset.ImagedVolumeWidth = int(dataset.TotalPixelMatrixColumns) * float(spacing[1])
        dataset.ImagedVolumeHeight = int(dataset.TotalPixelMatrixRows) * float(spacing[0])


def _make_lossy(dataset, transfer_syntax: str, encoder: str, workspace: Path) -> None:
    frames = _encode_frames(dataset, encoder, workspace)
    pixel_data, offsets, lengths = encapsulate_extended(frames)
    dataset.PixelData = pixel_data
    dataset["PixelData"].is_undefined_length = True
    dataset.ExtendedOffsetTable = offsets
    dataset.ExtendedOffsetTableLengths = lengths
    dataset.file_meta.TransferSyntaxUID = transfer_syntax
    dataset.LossyImageCompression = "01"
    dataset.LossyImageCompressionMethod = (
        "ISO_15444_15" if transfer_syntax == HTJ2K else "ISO_15444_1"
    )
    uncompressed = int(dataset.Rows) * int(dataset.Columns) * int(dataset.SamplesPerPixel) * len(frames)
    dataset.LossyImageCompressionRatio = f"{uncompressed / sum(map(len, frames)):.6g}"


def finalize_controls(source: Path, output: Path) -> None:
    if output.exists():
        raise RuntimeError(f"output already exists: {output}")
    inputs = sorted(source.glob("VC*.dcm"))
    if len(inputs) != 10 or any(path.stat().st_size > MAX_CONTROL_BYTES for path in inputs):
        raise RuntimeError("expected exactly ten bounded synthetic controls")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f".{output.name}-", dir=output.parent) as temporary:
        staging = Path(temporary)
        for path in inputs:
            dataset = pydicom.dcmread(path)
            _set_control_identities(dataset, path.stem)
            with tempfile.TemporaryDirectory(prefix="encode-") as encode_temp:
                if path.stem == "VC01-EVRLE-4F-ANISO":
                    _make_native(dataset, anisotropic=True)
                elif path.stem == "VC10-EVRLE-16F":
                    _make_native(dataset, anisotropic=False)
                elif path.stem == "VC05-J2K-LOSSY-4F":
                    _make_lossy(dataset, JPEG_2000, "jpeg2000", Path(encode_temp))
                elif path.stem == "VC08-HTJ2K-LOSSY-4F":
                    _make_lossy(dataset, HTJ2K, "htj2k", Path(encode_temp))
            destination = staging / path.name
            dataset.save_as(destination, enforce_file_format=True)
        shutil.move(staging, output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    finalize_controls(args.source, args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
