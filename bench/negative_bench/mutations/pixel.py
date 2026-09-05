"""Pixel Data, encapsulation, and codestream mutations."""

from __future__ import annotations

import hashlib
import struct

from pydicom.encaps import encapsulate

from ..model import GenerationError, MutationContext
from .dicom import (
    frames,
    pack_u64,
    remove_extended_tables,
    replace_frames,
    seeded_choice,
    selected_index,
    set_case_sop,
    unpack_u64,
)


def number_of_frames_plus_one(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.NumberOfFrames = str(context.parameters["value"])


def encapsulated_pixel_data_without_fragments(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.PixelData = encapsulate([], has_bot=False)
    context.dataset["PixelData"].is_undefined_length = True
    remove_extended_tables(context.dataset)


def basic_offset_table_nonzero_origin(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    pixel = bytearray(encapsulate(frames(context.dataset), has_bot=True))
    struct.pack_into("<I", pixel, 8, context.parameters["first_offset"])
    context.dataset.PixelData = bytes(pixel)
    context.dataset["PixelData"].is_undefined_length = True
    remove_extended_tables(context.dataset)


def extended_offset_not_fragment_boundary(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    offsets = unpack_u64(context.dataset.ExtendedOffsetTable)
    index = selected_index(
        context.parameters, "entry_index", context.seed, "extended-offset-entry"
    )
    offsets[index] += context.parameters["delta"]
    context.dataset.ExtendedOffsetTable = pack_u64(offsets)


def extended_length_includes_item_header(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    lengths = unpack_u64(context.dataset.ExtendedOffsetTableLengths)
    index = selected_index(
        context.parameters, "entry_index", context.seed, "extended-length-entry"
    )
    lengths[index] += context.parameters["delta"]
    context.dataset.ExtendedOffsetTableLengths = pack_u64(lengths)


def truncate_first_codestream(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    frame_values = frames(context.dataset)
    parameters = context.parameters
    if "frame_index_candidates" in parameters:
        index = seeded_choice(
            context.seed, "codestream-frame", parameters["frame_index_candidates"]
        )
    else:
        index = parameters["frame_index"]
    if index < 0 or index >= len(frame_values):
        raise GenerationError("selected codestream frame is out of range")
    retain = int(parameters.get("retain_prefix_bytes", 4))
    if retain < 1 or len(frame_values[index]) <= retain:
        raise GenerationError("frame is too short for codestream truncation mutation")
    before = frame_values[index]
    frame_values[index] = before[:retain]
    context.changes.append(
        {
            "kind": "codestream_truncation",
            "frame_index": index,
            "retained_prefix_bytes": retain,
            "before_size_bytes": len(before),
            "after_size_bytes": len(frame_values[index]),
            "before_sha256": hashlib.sha256(before).hexdigest(),
            "after_sha256": hashlib.sha256(frame_values[index]).hexdigest(),
        }
    )
    replace_frames(context.dataset, frame_values)


def native_transfer_syntax_with_encapsulation(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.file_meta.TransferSyntaxUID = context.parameters["transfer_syntax_uid"]
    if context.dataset["PixelData"].is_undefined_length:
        return
    frame_bytes = (
        int(context.dataset.Rows)
        * int(context.dataset.Columns)
        * int(context.dataset.SamplesPerPixel)
        * int(context.dataset.BitsAllocated)
        // 8
    )
    frame_count = int(context.dataset.NumberOfFrames)
    raw = bytes(context.dataset.PixelData)
    expected = frame_bytes * frame_count
    if len(raw) < expected:
        raise GenerationError("native Pixel Data is shorter than its declared frames")
    frame_values = [
        raw[index * frame_bytes : (index + 1) * frame_bytes]
        for index in range(frame_count)
    ]
    context.dataset.PixelData = encapsulate(frame_values, has_bot=True)
    context.dataset["PixelData"].is_undefined_length = True
    remove_extended_tables(context.dataset)


HANDLERS = {
    "number_of_frames_plus_one": number_of_frames_plus_one,
    "encapsulated_pixel_data_without_fragments": encapsulated_pixel_data_without_fragments,
    "basic_offset_table_nonzero_origin": basic_offset_table_nonzero_origin,
    "extended_offset_not_fragment_boundary": extended_offset_not_fragment_boundary,
    "extended_length_includes_item_header": extended_length_includes_item_header,
    "truncate_first_codestream": truncate_first_codestream,
    "native_transfer_syntax_with_encapsulation": native_transfer_syntax_with_encapsulation,
}
