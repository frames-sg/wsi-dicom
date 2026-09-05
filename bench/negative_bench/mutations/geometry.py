"""Dimension, tile, spacing, and pyramid mutations."""

from __future__ import annotations

import copy

from ..model import MutationContext, MutationResult
from .dicom import (
    frames,
    read_case_source,
    replace_frames,
    save,
    selected_index,
    set_case_sop,
)


def swap_row_column_dimension_indices(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.DimensionIndexSequence[0], context.dataset.DimensionIndexSequence[1] = (
        context.dataset.DimensionIndexSequence[1],
        context.dataset.DimensionIndexSequence[0],
    )


def incorrect_frame_dimension_index(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    index = selected_index(
        context.parameters, "frame_index", context.seed, "dimension-frame"
    )
    context.dataset.PerFrameFunctionalGroupsSequence[index].FrameContentSequence[
        0
    ].DimensionIndexValues = context.parameters["value"]


def duplicate_tile_position(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    index = selected_index(
        context.parameters, "frame_index", context.seed, "duplicate-tile-frame"
    )
    source_index = context.parameters["source_frame_index"]
    source_frame = context.dataset.PerFrameFunctionalGroupsSequence[source_index]
    target = context.dataset.PerFrameFunctionalGroupsSequence[index]
    target.PlanePositionSlideSequence = copy.deepcopy(
        source_frame.PlanePositionSlideSequence
    )
    target.FrameContentSequence = copy.deepcopy(source_frame.FrameContentSequence)


def missing_tile(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    index = selected_index(
        context.parameters, "frame_index", context.seed, "missing-tile-frame"
    )
    frame_values = frames(context.dataset)
    del frame_values[index]
    del context.dataset.PerFrameFunctionalGroupsSequence[index]
    context.dataset.NumberOfFrames = str(len(frame_values))
    replace_frames(context.dataset, frame_values)


def out_of_range_tile_position(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    index = selected_index(
        context.parameters, "frame_index", context.seed, "out-of-range-frame"
    )
    frame = context.dataset.PerFrameFunctionalGroupsSequence[index]
    frame.PlanePositionSlideSequence[0].RowPositionInTotalImagePixelMatrix = (
        context.parameters["row_position"]
    )
    values = list(frame.FrameContentSequence[0].DimensionIndexValues)
    values[0] = context.parameters["row_dimension_index"]
    frame.FrameContentSequence[0].DimensionIndexValues = values


def total_matrix_width(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.TotalPixelMatrixColumns = context.parameters["value"]


def swap_anisotropic_pixel_spacing(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.SharedFunctionalGroupsSequence[0].PixelMeasuresSequence[
        0
    ].PixelSpacing = context.parameters["pixel_spacing"]


def inconsistent_pyramid_physical_extent(
    context: MutationContext,
) -> MutationResult:
    first = context.dataset
    second = read_case_source(context.source)
    set_case_sop(first, context.seed, "first")
    set_case_sop(second, context.seed, "second")
    spacing = context.parameters["second_pixel_spacing"]
    measures = second.SharedFunctionalGroupsSequence[0].PixelMeasuresSequence[0]
    measures.PixelSpacing = spacing
    second.ImagedVolumeWidth = int(second.TotalPixelMatrixColumns) * float(spacing[1])
    second.ImagedVolumeHeight = int(second.TotalPixelMatrixRows) * float(spacing[0])
    first_path = context.input_dir / "level-0.dcm"
    second_path = context.input_dir / "level-1.dcm"
    save(first, first_path)
    save(second, second_path)
    context.changes.append(
        {
            "kind": "attribute_change",
            "attribute": "PixelSpacing",
            "path": "level-1/SharedFunctionalGroupsSequence[0]/PixelMeasuresSequence[0]",
            "after": spacing,
        }
    )
    return MutationResult([first_path, second_path], context.changes)


def remove_total_matrix_origin(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.TotalPixelMatrixOriginSequence


def remove_origin_x_offset(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.TotalPixelMatrixOriginSequence[0].XOffsetInSlideCoordinateSystem


def remove_image_orientation(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.ImageOrientationSlide


def nonorthogonal_image_orientation(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.ImageOrientationSlide = context.parameters.get(
        "value", ["1", "0", "0", "1", "0", "0"]
    )


def conflicting_slide_origin_set(context: MutationContext) -> MutationResult:
    first = context.dataset
    second = copy.deepcopy(context.dataset)
    set_case_sop(first, context.seed, "first")
    set_case_sop(second, context.seed, "second")
    second.TotalPixelMatrixOriginSequence[0].XOffsetInSlideCoordinateSystem = (
        context.parameters.get("value", "1")
    )
    first_path = context.input_dir / "first.dcm"
    second_path = context.input_dir / "second.dcm"
    save(first, first_path)
    save(second, second_path)
    return MutationResult([first_path, second_path], context.changes)


HANDLERS = {
    "swap_row_column_dimension_indices": swap_row_column_dimension_indices,
    "incorrect_frame_dimension_index": incorrect_frame_dimension_index,
    "duplicate_tile_position": duplicate_tile_position,
    "missing_tile": missing_tile,
    "out_of_range_tile_position": out_of_range_tile_position,
    "total_matrix_too_small": total_matrix_width,
    "total_matrix_too_large": total_matrix_width,
    "swap_anisotropic_pixel_spacing": swap_anisotropic_pixel_spacing,
    "inconsistent_pyramid_physical_extent": inconsistent_pyramid_physical_extent,
    "remove_total_matrix_origin": remove_total_matrix_origin,
    "remove_origin_x_offset": remove_origin_x_offset,
    "remove_image_orientation": remove_image_orientation,
    "nonorthogonal_image_orientation": nonorthogonal_image_orientation,
    "conflicting_slide_origin_set": conflicting_slide_origin_set,
}
