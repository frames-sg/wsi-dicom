"""Specimen, SOP identity, and source-reference mutations."""

from __future__ import annotations

import copy

import pydicom
from pydicom.dataset import Dataset
from pydicom.sequence import Sequence

from ..model import GenerationError, MutationContext, MutationResult
from .dicom import (
    alternate_uid_with_encoded_length,
    deterministic_uid,
    encoded_ui,
    patch_file_meta_sop_instance_uid,
    read_case_source,
    save,
    set_case_sop,
)


def remove_specimen_description(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.SpecimenDescriptionSequence


def remove_specimen_identifier(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.SpecimenDescriptionSequence[
        context.parameters["specimen_index"]
    ].SpecimenIdentifier


def invalid_specimen_uid(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.SpecimenDescriptionSequence[0].SpecimenUID = context.parameters[
        "value"
    ]


def conflicting_specimen_uid_scope(context: MutationContext) -> MutationResult:
    first = context.dataset
    second = read_case_source(context.source)
    set_case_sop(first, context.seed, "first")
    set_case_sop(second, context.seed, "second")
    second.SpecimenDescriptionSequence[0].SpecimenIdentifier = context.parameters[
        "second_specimen_identifier"
    ]
    first_path = context.input_dir / "first.dcm"
    second_path = context.input_dir / "second.dcm"
    save(first, first_path)
    save(second, second_path)
    return MutationResult([first_path, second_path], context.changes)


def file_meta_dataset_sop_mismatch(context: MutationContext) -> MutationResult:
    dataset_uid = set_case_sop(context.dataset, context.seed, "dataset-only")
    encoded_length = len(encoded_ui(dataset_uid))
    file_meta_uid = alternate_uid_with_encoded_length(context.seed, encoded_length)
    context.changes[0]["value"] = dataset_uid
    save(context.dataset, context.default_output)
    byte_offset = patch_file_meta_sop_instance_uid(
        context.default_output, dataset_uid, file_meta_uid
    )
    observed = pydicom.dcmread(context.default_output, stop_before_pixels=True)
    if str(observed.file_meta.MediaStorageSOPInstanceUID) == str(
        observed.SOPInstanceUID
    ):
        raise GenerationError("file-meta/dataset SOP mismatch did not survive writing")
    context.changes.append(
        {
            "kind": "raw_byte_patch",
            "attribute": "MediaStorageSOPInstanceUID",
            "tag": "(0002,0003)",
            "byte_offset": byte_offset,
            "before": dataset_uid,
            "after": file_meta_uid,
        }
    )
    return MutationResult([context.default_output], context.changes)


def duplicate_sop_uid_divergent_series(context: MutationContext) -> MutationResult:
    first = context.dataset
    second = read_case_source(context.source)
    second.SeriesInstanceUID = deterministic_uid(context.seed, "second-series")
    first_path = context.input_dir / "first.dcm"
    second_path = context.input_dir / "second.dcm"
    save(first, first_path)
    save(second, second_path)
    return MutationResult([first_path, second_path], context.changes)


def incorrect_source_image_reference_class(context: MutationContext) -> MutationResult:
    owner = context.dataset
    target = read_case_source(context.source)
    set_case_sop(owner, context.seed, "owner")
    set_case_sop(target, context.seed, "target")
    reference = pydicom.Dataset()
    reference.ReferencedSOPClassUID = context.parameters["referenced_sop_class_uid"]
    reference.ReferencedSOPInstanceUID = target.SOPInstanceUID
    owner.SourceImageSequence = [reference]
    owner_path = context.input_dir / "derived.dcm"
    target_path = context.input_dir / "source.dcm"
    save(owner, owner_path)
    save(target, target_path)
    context.changes.append(
        {
            "kind": "attribute_change",
            "attribute": "ReferencedSOPClassUID",
            "path": "derived/SourceImageSequence[0]",
            "after": str(reference.ReferencedSOPClassUID),
            "referenced_sop_instance_uid": str(target.SOPInstanceUID),
        }
    )
    return MutationResult([owner_path, target_path], context.changes)


def _save_identity_pair(
    context: MutationContext, second_change
) -> MutationResult:
    first = context.dataset
    second = copy.deepcopy(context.dataset)
    set_case_sop(first, context.seed, "first")
    set_case_sop(second, context.seed, "second")
    second_change(second)
    first_path = context.input_dir / "first.dcm"
    second_path = context.input_dir / "second.dcm"
    save(first, first_path)
    save(second, second_path)
    return MutationResult([first_path, second_path], context.changes)


def conflicting_patient_study_set(context: MutationContext) -> MutationResult:
    return _save_identity_pair(
        context,
        lambda second: setattr(
            second, "PatientID", context.parameters.get("value", "CONFLICTING-PATIENT")
        ),
    )


def conflicting_study_series_set(context: MutationContext) -> MutationResult:
    return _save_identity_pair(
        context,
        lambda second: setattr(
            second,
            "StudyInstanceUID",
            deterministic_uid(context.seed, "conflicting-study"),
        ),
    )


def conflicting_container_frame_set(context: MutationContext) -> MutationResult:
    return _save_identity_pair(
        context,
        lambda second: setattr(
            second,
            "ContainerIdentifier",
            context.parameters.get("value", "CONFLICTING-CONTAINER"),
        ),
    )


def conflicting_container_issuer_set(context: MutationContext) -> MutationResult:
    def change(second) -> None:
        issuer = Dataset()
        issuer.LocalNamespaceEntityID = context.parameters.get(
            "value", "CONFLICTING-ISSUER"
        )
        second.IssuerOfTheContainerIdentifierSequence = Sequence([issuer])

    return _save_identity_pair(context, change)


def remove_container_identifier(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.ContainerIdentifier


def remove_container_issuer_sequence(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.IssuerOfTheContainerIdentifierSequence


def remove_specimen_preparation_sequence(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.SpecimenDescriptionSequence[0].SpecimenPreparationSequence


def invalid_specimen_preparation_item(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    context.dataset.SpecimenDescriptionSequence[0].SpecimenPreparationSequence = Sequence(
        [Dataset()]
    )


def remove_patient_birth_date(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.PatientBirthDate


def remove_study_date(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.StudyDate


def remove_series_number(context: MutationContext) -> None:
    set_case_sop(context.dataset, context.seed)
    del context.dataset.SeriesNumber


HANDLERS = {
    "remove_specimen_description": remove_specimen_description,
    "remove_specimen_identifier": remove_specimen_identifier,
    "invalid_specimen_uid": invalid_specimen_uid,
    "conflicting_specimen_uid_scope": conflicting_specimen_uid_scope,
    "file_meta_dataset_sop_mismatch": file_meta_dataset_sop_mismatch,
    "duplicate_sop_uid_divergent_series": duplicate_sop_uid_divergent_series,
    "incorrect_source_image_reference_class": incorrect_source_image_reference_class,
    "conflicting_patient_study_set": conflicting_patient_study_set,
    "conflicting_study_series_set": conflicting_study_series_set,
    "conflicting_container_frame_set": conflicting_container_frame_set,
    "conflicting_container_issuer_set": conflicting_container_issuer_set,
    "remove_container_identifier": remove_container_identifier,
    "remove_container_issuer_sequence": remove_container_issuer_sequence,
    "remove_specimen_preparation_sequence": remove_specimen_preparation_sequence,
    "invalid_specimen_preparation_item": invalid_specimen_preparation_item,
    "remove_patient_birth_date": remove_patient_birth_date,
    "remove_study_date": remove_study_date,
    "remove_series_number": remove_series_number,
}
