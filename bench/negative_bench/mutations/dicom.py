"""Checked DICOM byte and frame operations used by authored mutations."""

from __future__ import annotations

import hashlib
import struct
from pathlib import Path

import pydicom
from pydicom.encaps import encapsulate_extended, generate_frames

from ..model import GenerationError


def deterministic_uid(seed: str, suffix: str) -> str:
    digest = hashlib.sha256(f"{seed}:{suffix}".encode("utf-8")).digest()[:16]
    return f"2.25.{int.from_bytes(digest, 'big')}"


def seeded_choice(seed: str, label: str, candidates: list[int]) -> int:
    if not candidates:
        raise GenerationError(f"{label} candidate list is empty")
    digest = hashlib.sha256(f"{seed}:{label}".encode("utf-8")).digest()
    return candidates[int.from_bytes(digest[:8], "big") % len(candidates)]


def selected_index(parameters: dict, key: str, seed: str, label: str) -> int:
    candidates = parameters.get(f"{key}_candidates")
    if candidates is not None:
        return seeded_choice(seed, label, candidates)
    return int(parameters[key])


def read_case_source(path: Path):
    return pydicom.dcmread(path)


def set_case_sop(dataset, seed: str, suffix: str = "instance") -> str:
    uid = deterministic_uid(seed, suffix)
    dataset.SOPInstanceUID = uid
    dataset.file_meta.MediaStorageSOPInstanceUID = uid
    return uid


def save(dataset, path: Path) -> None:
    dataset.save_as(path, enforce_file_format=True)


def encoded_ui(uid: str) -> bytes:
    encoded = uid.encode("ascii")
    return encoded if len(encoded) % 2 == 0 else encoded + b"\0"


def alternate_uid_with_encoded_length(seed: str, length: int) -> str:
    for attempt in range(1024):
        candidate = deterministic_uid(seed, f"file-meta-{attempt}")
        if len(encoded_ui(candidate)) == length:
            return candidate
    raise GenerationError("could not derive a same-length file-meta SOP Instance UID")


def patch_file_meta_sop_instance_uid(
    path: Path, expected_uid: str, replacement_uid: str
) -> int:
    marker = b"\x02\x00\x03\x00UI"
    contents = bytearray(path.read_bytes())
    offsets = []
    start = 0
    while True:
        offset = contents.find(marker, start)
        if offset < 0:
            break
        offsets.append(offset)
        start = offset + len(marker)
    if len(offsets) != 1:
        raise GenerationError(
            f"expected one Media Storage SOP Instance UID element, found {len(offsets)}"
        )
    offset = offsets[0]
    value_length = struct.unpack_from("<H", contents, offset + 6)[0]
    value_offset = offset + 8
    before = bytes(contents[value_offset : value_offset + value_length])
    expected = encoded_ui(expected_uid)
    replacement = encoded_ui(replacement_uid)
    if before != expected or len(replacement) != value_length:
        raise GenerationError("file-meta SOP Instance UID bytes differ from patch precondition")
    contents[value_offset : value_offset + value_length] = replacement
    path.write_bytes(contents)
    return value_offset


def frames(dataset) -> list[bytes]:
    extended = None
    if "ExtendedOffsetTable" in dataset and "ExtendedOffsetTableLengths" in dataset:
        extended = (dataset.ExtendedOffsetTable, dataset.ExtendedOffsetTableLengths)
    return list(
        generate_frames(
            dataset.PixelData,
            number_of_frames=int(dataset.NumberOfFrames),
            extended_offsets=extended,
        )
    )


def replace_frames(dataset, frame_values: list[bytes]) -> None:
    pixel_data, offsets, lengths = encapsulate_extended(frame_values)
    dataset.PixelData = pixel_data
    dataset["PixelData"].is_undefined_length = True
    dataset.ExtendedOffsetTable = offsets
    dataset.ExtendedOffsetTableLengths = lengths


def remove_extended_tables(dataset) -> None:
    for keyword in ["ExtendedOffsetTable", "ExtendedOffsetTableLengths"]:
        if keyword in dataset:
            del dataset[keyword]


def unpack_u64(raw: bytes) -> list[int]:
    if len(raw) % 8:
        raise GenerationError("64-bit offset table byte length is not divisible by 8")
    return list(struct.unpack(f"<{len(raw) // 8}Q", raw))


def pack_u64(values: list[int]) -> bytes:
    return struct.pack(f"<{len(values)}Q", *values)
