"""Compression history and image-module conformance mutations."""

from __future__ import annotations

import pydicom

from ..model import MutationContext, MutationResult
from .dicom import set_case_sop, save


def remove_lossy_declaration(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.LossyImageCompression


def lossy_syntax_declared_lossless(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.LossyImageCompression = context.parameters["value"]


def missing_lossy_method(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.LossyImageCompressionMethod


def lossy_history_vm_mismatch(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.LossyImageCompressionMethod = context.parameters["methods"]
    context.dataset.LossyImageCompressionRatio = context.parameters["ratios"]


def samples_photometric_contradiction(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.SamplesPerPixel = context.parameters["value"]


def remove_acquisition_datetime(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.AcquisitionDateTime


def remove_acquisition_context_sequence(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.AcquisitionContextSequence


def remove_manufacturer(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.Manufacturer


def remove_device_serial_number(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.DeviceSerialNumber


def invalid_image_type_flavor(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    values = list(context.dataset.ImageType)
    values[2] = context.parameters.get("value", "LABEL")
    context.dataset.ImageType = values


def remove_focus_method(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.FocusMethod


def volume_specimen_label_yes(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.SpecimenLabelInImage = "YES"


def remove_monochrome_presentation(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    for keyword in ("PresentationLUTShape", "RescaleIntercept", "RescaleSlope"):
        if keyword in context.dataset:
            del context.dataset[keyword]


def incorrect_monochrome_rescale_slope(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.RescaleSlope = context.parameters.get("value", "2")


def remove_wsi_frame_type(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.SharedFunctionalGroupsSequence[0].WholeSlideMicroscopyImageFrameTypeSequence


def remove_derivation_image(context: MutationContext) -> MutationResult:
    source = context.dataset
    derived = pydicom.dcmread(context.source.with_name("level-1.dcm"))
    set_case_sop(derived, context.seed)
    del derived.SharedFunctionalGroupsSequence[0].DerivationImageSequence
    paths = [context.input_dir / "level-0.dcm", context.input_dir / "level-1.dcm"]
    save(source, paths[0])
    save(derived, paths[1])
    return MutationResult(paths, [{"kind": "harness_identity", "attribute": "SOPInstanceUID", "value": str(derived.SOPInstanceUID)}, {"kind": "defect", "attribute": "SharedFunctionalGroupsSequence.DerivationImageSequence", "operation": "remove"}])


def remove_position_reference(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.PositionReferenceIndicator


def signed_wsi_pixels(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.PixelRepresentation = 1


def unequal_wsi_bit_depth(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.BitsStored = 7
    context.dataset.HighBit = 6


def planar_wsi_pixels(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.PlanarConfiguration = 1


def remove_content_date(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.ContentDate


def permute_tiled_full_frames(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    frames = context.dataset.PerFrameFunctionalGroupsSequence
    frames[0], frames[1] = frames[1], frames[0]


HANDLERS = {
    "remove_wsi_frame_type": remove_wsi_frame_type,
    "remove_derivation_image": remove_derivation_image,
    "remove_position_reference": remove_position_reference,
    "signed_wsi_pixels": signed_wsi_pixels,
    "unequal_wsi_bit_depth": unequal_wsi_bit_depth,
    "planar_wsi_pixels": planar_wsi_pixels,
    "remove_content_date": remove_content_date,
    "permute_tiled_full_frames": permute_tiled_full_frames,

    "remove_lossy_declaration": remove_lossy_declaration,
    "lossy_syntax_declared_lossless": lossy_syntax_declared_lossless,
    "missing_lossy_method": missing_lossy_method,
    "lossy_history_vm_mismatch": lossy_history_vm_mismatch,
    "samples_photometric_contradiction": samples_photometric_contradiction,
    "remove_acquisition_datetime": remove_acquisition_datetime,
    "remove_acquisition_context_sequence": remove_acquisition_context_sequence,
    "remove_manufacturer": remove_manufacturer,
    "remove_device_serial_number": remove_device_serial_number,
    "invalid_image_type_flavor": invalid_image_type_flavor,
    "remove_focus_method": remove_focus_method,
    "volume_specimen_label_yes": volume_specimen_label_yes,
    "remove_monochrome_presentation": remove_monochrome_presentation,
    "incorrect_monochrome_rescale_slope": incorrect_monochrome_rescale_slope,
}
