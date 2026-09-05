"""Optical-path ICC profile mutations."""

from __future__ import annotations

from ..model import MutationContext
from .dicom import set_case_sop


def remove_required_icc(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.OpticalPathSequence[context.parameters["optical_path_index"]][
        "ICCProfile"
    ]


def truncate_icc_header(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    optical = context.dataset.OpticalPathSequence[0]
    optical.ICCProfile = optical.ICCProfile[: context.parameters["length"]]


def replace_icc_bytes(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    optical = context.dataset.OpticalPathSequence[0]
    profile = bytearray(optical.ICCProfile)
    offset = context.parameters["offset"]
    replacement = bytes.fromhex(context.parameters["after_hex"])
    profile[offset : offset + len(replacement)] = replacement
    optical.ICCProfile = bytes(profile)


def optical_path_count_mismatch(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.NumberOfOpticalPaths = context.parameters.get("value", 2)


def remove_optical_path_sequence(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.OpticalPathSequence


def remove_optical_path_identifier(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.OpticalPathSequence[0].OpticalPathIdentifier


def remove_illumination_type(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.OpticalPathSequence[0].IlluminationTypeCodeSequence


def remove_illumination_description(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    optical_path = context.dataset.OpticalPathSequence[0]
    for keyword in ("IlluminationColorCodeSequence", "IlluminationWaveLength"):
        if keyword in optical_path:
            del optical_path[keyword]


def dangling_optical_path_reference(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    reference = context.dataset.SharedFunctionalGroupsSequence[
        0
    ].OpticalPathIdentificationSequence[0]
    reference.OpticalPathIdentifier = context.parameters.get("value", "missing")


HANDLERS = {
    "remove_required_icc": remove_required_icc,
    "truncate_icc_header": truncate_icc_header,
    "wrong_icc_device_class": replace_icc_bytes,
    "inconsistent_icc_input_color_space": replace_icc_bytes,
    "optical_path_count_mismatch": optical_path_count_mismatch,
    "remove_optical_path_sequence": remove_optical_path_sequence,
    "remove_optical_path_identifier": remove_optical_path_identifier,
    "remove_illumination_type": remove_illumination_type,
    "remove_illumination_description": remove_illumination_description,
    "dangling_optical_path_reference": dangling_optical_path_reference,
}
