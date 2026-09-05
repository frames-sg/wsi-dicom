"""Materialize one source and record its provenance evidence."""

from __future__ import annotations

from pathlib import Path

from bench.file_digest import sha256_file
from bench.json_document import write_json

from .source import MaterializedSource, materialize_source


def collect_source_evidence(
    case: dict, corpus_root: Path, case_root: Path
) -> tuple[MaterializedSource, dict]:
    extraction_root = case_root / "extracted-source"
    materialized = materialize_source(case, corpus_root, extraction_root)
    source_record = {
        "format": case["format"],
        "container_path": str(materialized.container_path),
        "container_bytes": materialized.container_path.stat().st_size,
        "container_sha256": materialized.container_sha256,
        "entry_path": str(materialized.entry_path),
        "entry_bytes": materialized.entry_path.stat().st_size,
        "entry_sha256": sha256_file(materialized.entry_path),
        "supplied_source_pixel_spacing_mm": case.get("source_pixel_spacing_mm"),
        "source_pixel_spacing_provenance": case.get(
            "source_pixel_spacing_provenance"
        ),
        "extracted_files": [
            {
                "path": str(path.relative_to(extraction_root)),
                "bytes": path.stat().st_size,
                "sha256": sha256_file(path),
            }
            for path in materialized.extracted_files
        ],
    }
    write_json(case_root / "source.json", source_record)
    return materialized, source_record
