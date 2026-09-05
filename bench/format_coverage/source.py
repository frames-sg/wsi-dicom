"""Checksummed source materialization for format-coverage cases."""

from __future__ import annotations

import shutil
import stat
import zipfile
from dataclasses import dataclass
from pathlib import Path

from bench.file_digest import sha256_file

from .manifest import FormatCoverageError, validate_archive_member, validate_relative_path


@dataclass(frozen=True)
class MaterializedSource:
    entry_path: Path
    container_path: Path
    container_sha256: str
    extracted_files: tuple[Path, ...]


def materialize_source(
    case: dict, corpus_root: Path, extraction_root: Path
) -> MaterializedSource:
    relative_source = validate_relative_path(case["source"], f"case {case['id']} source")
    container_path = corpus_root.joinpath(*relative_source.parts)
    if not container_path.is_file():
        raise FormatCoverageError(f"source does not exist: {container_path}")
    actual_sha256 = sha256_file(container_path)
    if actual_sha256 != case["source_sha256"]:
        raise FormatCoverageError(
            f"source SHA-256 mismatch for {case['id']}: expected {case['source_sha256']}, got {actual_sha256}"
        )
    archive_entry = case.get("archive_entry")
    if archive_entry is None:
        return MaterializedSource(
            entry_path=container_path,
            container_path=container_path,
            container_sha256=actual_sha256,
            extracted_files=(),
        )
    entry_member = validate_archive_member(archive_entry)
    extracted_files = extract_zip_safely(container_path, extraction_root)
    entry_path = extraction_root.joinpath(*entry_member.parts)
    if not entry_path.is_file():
        raise FormatCoverageError(
            f"archive entry for {case['id']} does not exist after extraction: {archive_entry}"
        )
    return MaterializedSource(
        entry_path=entry_path,
        container_path=container_path,
        container_sha256=actual_sha256,
        extracted_files=tuple(extracted_files),
    )


def extract_zip_safely(archive: Path, destination: Path) -> list[Path]:
    destination.mkdir(parents=True, exist_ok=False)
    extracted: list[Path] = []
    try:
        with zipfile.ZipFile(archive) as bundle:
            members = bundle.infolist()
            if len(members) > 100_000:
                raise FormatCoverageError(
                    f"archive contains too many members ({len(members)}): {archive}"
                )
            total_size = sum(member.file_size for member in members)
            if total_size > 20 * 1024**3:
                raise FormatCoverageError(
                    f"archive expands beyond the 20 GiB safety limit: {archive}"
                )
            for member in members:
                relative = validate_archive_member(member.filename)
                unix_mode = member.external_attr >> 16
                if stat.S_ISLNK(unix_mode):
                    raise FormatCoverageError(
                        f"unsafe archive member is a symlink: {member.filename}"
                    )
                output = destination.joinpath(*relative.parts)
                if member.is_dir():
                    output.mkdir(parents=True, exist_ok=True)
                    continue
                output.parent.mkdir(parents=True, exist_ok=True)
                with bundle.open(member) as source, output.open("xb") as target:
                    shutil.copyfileobj(source, target, length=1024 * 1024)
                extracted.append(output)
    except (OSError, zipfile.BadZipFile) as exc:
        raise FormatCoverageError(f"failed to extract {archive}: {exc}") from exc
    return extracted
