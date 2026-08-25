"""Typed benchmark inputs and stable command-line choices."""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass
from pathlib import Path


SUPPORTED_SUFFIXES = {
    ".bif",
    ".czi",
    ".mrxs",
    ".ndpi",
    ".scn",
    ".svs",
    ".tif",
    ".tiff",
    ".vms",
    ".vmu",
}

PROFILE_CHOICES = ("htj2k-lossless-rpcl", "jpeg-baseline")
SCOPE_CHOICES = ("base", "pyramid")
TOOL_CHOICES = ("wsi-dicom-cpu", "wsi-dicom-device", "wsidicomizer")


@dataclass(frozen=True)
class ManifestEntry:
    file_id: str
    filename: str
    md5: str
    size: int | None
    state: str


@dataclass(frozen=True)
class Slide:
    slide_id: str
    display_name: str
    path: Path
    download_dir: Path
    relative_path: str
    gdc_file_id: str | None
    manifest_filename: str | None
    manifest_md5: str | None
    manifest_size: int | None
    manifest_state: str | None
    bytes_on_disk: int
    metadata: dict | None = None


def safe_slug(value: str) -> str:
    slug = "".join(
        character.lower() if character.isalnum() or character in ".-_" else "-"
        for character in value
    )
    return re.sub("-+", "-", slug).strip("-") or "slide"


def split_command(command: str) -> list[str]:
    parts = shlex.split(command)
    if not parts:
        raise ValueError("command cannot be empty")
    return parts
