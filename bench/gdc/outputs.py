"""Produced-file accounting and DICOM metadata inspection."""

from __future__ import annotations

from pathlib import Path

def count_output_files(output_dir: Path) -> tuple[int, int]:
    if not output_dir.exists():
        return 0, 0
    produced_files = 0
    output_bytes = 0
    for path in output_dir.rglob("*"):
        if path.is_file():
            produced_files += 1
            output_bytes += path.stat().st_size
    return produced_files, output_bytes

def dicom_metadata_from_dataset(path: Path, dataset) -> dict:
    return {
        "file": path.name,
        "transfer_syntax_uid": str(getattr(dataset.file_meta, "TransferSyntaxUID", "")),
        "rows": int(getattr(dataset, "Rows", 0) or 0),
        "columns": int(getattr(dataset, "Columns", 0) or 0),
        "number_of_frames": int(getattr(dataset, "NumberOfFrames", 0) or 0),
        "total_pixel_matrix_columns": int(
            getattr(dataset, "TotalPixelMatrixColumns", 0) or 0
        ),
        "total_pixel_matrix_rows": int(getattr(dataset, "TotalPixelMatrixRows", 0) or 0),
    }

def collect_dicom_outputs(output_dir: Path) -> tuple[list[dict], str | None]:
    if not output_dir.exists():
        return [], None
    try:
        import pydicom
    except ImportError as exc:
        return [], f"pydicom unavailable: {exc}"

    outputs = []
    for path in sorted(output_dir.rglob("*.dcm")):
        try:
            dataset = pydicom.dcmread(str(path), stop_before_pixels=True)
        except Exception as exc:  # noqa: BLE001 - evidence should report failures.
            return outputs, f"failed to read DICOM metadata from {path}: {exc}"
        outputs.append(dicom_metadata_from_dataset(path, dataset))
    return outputs, None
