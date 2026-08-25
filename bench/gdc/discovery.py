"""Manifest parsing and local GDC slide discovery."""

from __future__ import annotations

from dataclasses import asdict
from pathlib import Path
from typing import Sequence

from .models import ManifestEntry, Slide, SUPPORTED_SUFFIXES, safe_slug


def parse_manifest(path: Path) -> list[ManifestEntry]:
    if not path.exists():
        return []
    lines = [line for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not lines:
        return []
    header = lines[0].split("\t")
    entries: list[ManifestEntry] = []
    for line in lines[1:]:
        fields = dict(zip(header, line.split("\t")))
        size_text = fields.get("size")
        try:
            size = int(size_text) if size_text else None
        except ValueError:
            size = None
        entries.append(
            ManifestEntry(
                file_id=fields.get("id", ""),
                filename=fields.get("filename", ""),
                md5=fields.get("md5", ""),
                size=size,
                state=fields.get("state", ""),
            )
        )
    return entries


def manifest_entry_for_slide(
    slide_path: Path,
    download_dir: Path,
    entries: Sequence[ManifestEntry],
) -> ManifestEntry | None:
    relative_path = slide_path.relative_to(download_dir).as_posix()
    for entry in entries:
        if entry.filename == relative_path:
            return entry
    for entry in entries:
        if entry.file_id and slide_path.parent.name == entry.file_id:
            return entry
    for entry in entries:
        if entry.filename and Path(entry.filename).name.lower() == slide_path.name.lower():
            return entry
    return None


def read_slide_metadata(path: Path) -> dict:
    try:
        import openslide
    except Exception as exc:  # noqa: BLE001 - metadata is optional evidence.
        return {"error": f"openslide unavailable: {type(exc).__name__}: {exc}"}

    try:
        slide = openslide.OpenSlide(str(path))
        try:
            return {
                "vendor": slide.properties.get("openslide.vendor"),
                "dimensions": list(slide.dimensions),
                "level_count": slide.level_count,
                "level_dimensions": [list(dim) for dim in slide.level_dimensions],
                "level_downsamples": list(slide.level_downsamples),
            }
        finally:
            slide.close()
    except Exception as exc:  # noqa: BLE001 - keep the slide in the benchmark.
        return {"error": f"{type(exc).__name__}: {exc}"}


def discover_gdc_slides(
    downloads_root: Path,
    *,
    gdc_glob: str = "gdc_download*",
    probe_metadata: bool = False,
) -> list[Slide]:
    downloads_root = downloads_root.expanduser().resolve()
    slides: list[Slide] = []
    for download_dir in sorted(path for path in downloads_root.glob(gdc_glob) if path.is_dir()):
        entries = parse_manifest(download_dir / "MANIFEST.txt")
        for path in sorted(download_dir.rglob("*")):
            if not path.is_file() or path.suffix.lower() not in SUPPORTED_SUFFIXES:
                continue
            entry = manifest_entry_for_slide(path, download_dir, entries)
            manifest_name = Path(entry.filename).name if entry and entry.filename else None
            display_name = manifest_name or path.name
            file_id = entry.file_id if entry else None
            stem = Path(display_name).stem
            id_part = file_id[:8] if file_id else download_dir.name.replace("gdc_download_", "")
            slide_id = safe_slug(f"{stem}-{id_part}")
            metadata = read_slide_metadata(path) if probe_metadata else None
            slides.append(
                Slide(
                    slide_id=slide_id,
                    display_name=display_name,
                    path=path,
                    download_dir=download_dir,
                    relative_path=path.relative_to(download_dir).as_posix(),
                    gdc_file_id=file_id,
                    manifest_filename=entry.filename if entry else None,
                    manifest_md5=entry.md5 if entry else None,
                    manifest_size=entry.size if entry else None,
                    manifest_state=entry.state if entry else None,
                    bytes_on_disk=path.stat().st_size,
                    metadata=metadata,
                )
            )
    return sorted(slides, key=lambda slide: (slide.display_name.lower(), slide.slide_id))


def slide_to_json(slide: Slide) -> dict:
    record = asdict(slide)
    record["path"] = str(slide.path)
    record["download_dir"] = str(slide.download_dir)
    return record


def select_slides(
    slides: Sequence[Slide],
    *,
    only_filters: Sequence[str],
    max_slides: int | None,
) -> list[Slide]:
    selected = list(slides)
    for only in only_filters:
        needle = only.lower()
        selected = [
            slide
            for slide in selected
            if needle in slide.slide_id.lower()
            or needle in slide.display_name.lower()
            or needle in slide.relative_path.lower()
            or (slide.gdc_file_id and needle in slide.gdc_file_id.lower())
        ]
    if max_slides is not None:
        selected = selected[:max_slides]
    return selected
