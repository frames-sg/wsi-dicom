"""Build fresh, PHI-free DICOM 2026c core-profile controls."""

from __future__ import annotations

import argparse
import copy
import hashlib
import shutil
import sys
import tempfile
from pathlib import Path

if __package__ in {None, ""}:
    repository_root = str(Path(__file__).resolve().parents[2])
    if repository_root not in sys.path:
        sys.path.insert(0, repository_root)

import pydicom
from pydicom.sequence import Sequence



SOURCE_TO_CORE_ID = {
    "VC01-EVRLE-4F-ANISO": "CP01-EVRLE-4F-ANISO",
    "VC02-JPEG-16F": "CP02-JPEG-16F",
    "VC03-JPEG-1F": "CP03-JPEG-1F",
    "VC04-J2K-LOSSLESS-16F": "CP04-J2K-LOSSLESS-16F",
    "VC05-J2K-LOSSY-4F": "CP05-J2K-LOSSY-4F",
    "VC06-HTJ2K-LOSSLESS-16F": "CP06-HTJ2K-LOSSLESS-16F",
    "VC07-HTJ2K-RPCL-4F": "CP07-HTJ2K-RPCL-4F",
    "VC08-HTJ2K-LOSSY-4F": "CP08-HTJ2K-LOSSY-4F",
    "VC09-J2K-LOSSLESS-4F": "CP09-J2K-LOSSLESS-4F",
    "VC10-EVRLE-16F": "CP10-EVRLE-16F",
}


def _uid(control_id: str, role: str) -> str:
    digest = hashlib.sha256(
        f"wsi-dicom-core-profile-2026c-v2:{control_id}:{role}".encode()
    ).digest()[:16]
    return f"2.25.{int.from_bytes(digest, 'big')}"


def _set_common_identity(dataset, control_id: str) -> None:
    dataset.PatientName = "RESEARCH^PLACEHOLDER"
    dataset.PatientID = "RESEARCH"
    dataset.StudyInstanceUID = _uid(control_id, "study")
    dataset.SeriesInstanceUID = _uid(control_id, "series")
    dataset.FrameOfReferenceUID = _uid(control_id, "frame-of-reference")
    dataset.PyramidUID = _uid(control_id, "pyramid")
    dataset.ContainerIdentifier = f"SYNTH-{control_id}"
    dataset.IssuerOfTheContainerIdentifierSequence = Sequence([])
    specimen = dataset.SpecimenDescriptionSequence[0]
    specimen.SpecimenIdentifier = f"SYNTH-{control_id}"
    specimen.SpecimenUID = _uid(control_id, "specimen")
    specimen.IssuerOfTheSpecimenIdentifierSequence = Sequence([])
    specimen.SpecimenPreparationSequence = Sequence([])


def _set_instance_identity(dataset, control_id: str, role: str = "instance") -> None:
    dataset.SOPInstanceUID = _uid(control_id, role)
    dataset.file_meta.MediaStorageSOPInstanceUID = dataset.SOPInstanceUID
    dimension_uid = _uid(control_id, f"{role}-dimension")
    dataset.DimensionOrganizationSequence[0].DimensionOrganizationUID = dimension_uid
    for item in dataset.DimensionIndexSequence:
        item.DimensionOrganizationUID = dimension_uid


def _fresh_control(dataset, control_id: str) -> None:
    _set_common_identity(dataset, control_id)
    _set_instance_identity(dataset, control_id)


def _monochrome_control(source) -> object:
    dataset = copy.deepcopy(source)
    rgb = bytes(dataset.PixelData)
    if len(rgb) % 3:
        raise RuntimeError("native RGB control has an invalid pixel payload")
    dataset.PixelData = bytes(
        (int(rgb[index]) + int(rgb[index + 1]) + int(rgb[index + 2])) // 3
        for index in range(0, len(rgb), 3)
    )
    dataset["PixelData"].is_undefined_length = False
    dataset.SamplesPerPixel = 1
    dataset.PhotometricInterpretation = "MONOCHROME2"
    if "PlanarConfiguration" in dataset:
        del dataset.PlanarConfiguration
    dataset.PresentationLUTShape = "IDENTITY"
    dataset.RescaleIntercept = "0"
    dataset.RescaleSlope = "1"
    optical_path = dataset.OpticalPathSequence[0]
    if "ICCProfile" in optical_path:
        del optical_path.ICCProfile
    _fresh_control(dataset, "CP11-MONO-EVRLE-4F")
    return dataset


def _pyramid_controls(source) -> tuple[object, object]:
    control_id = "CP12-PYRAMID-2L"
    level_zero = copy.deepcopy(source)
    _set_common_identity(level_zero, control_id)
    _set_instance_identity(level_zero, control_id, "level-0")
    level_zero.InstanceNumber = "1"

    level_one = copy.deepcopy(level_zero)
    _set_instance_identity(level_one, control_id, "level-1")
    level_one.ImageType = ["DERIVED", "PRIMARY", "VOLUME", "RESAMPLED"]
    source_reference = pydicom.Dataset()
    source_reference.ReferencedSOPClassUID = level_zero.SOPClassUID
    source_reference.ReferencedSOPInstanceUID = level_zero.SOPInstanceUID
    level_one.SourceImageSequence = Sequence([source_reference])
    derivation = pydicom.Dataset()
    derivation_code = pydicom.Dataset()
    derivation_code.CodeValue = "113085"
    derivation_code.CodingSchemeDesignator = "DCM"
    derivation_code.CodeMeaning = "Spatial resampling"
    derivation.DerivationCodeSequence = Sequence([derivation_code])
    derived_source = copy.deepcopy(source_reference)
    purpose = pydicom.Dataset()
    purpose.CodeValue = "121322"
    purpose.CodingSchemeDesignator = "DCM"
    purpose.CodeMeaning = "Source image for image processing operation"
    derived_source.PurposeOfReferenceCodeSequence = Sequence([purpose])
    derived_source.SpatialLocationsPreserved = "YES"
    derivation.SourceImageSequence = Sequence([derived_source])
    level_one.SharedFunctionalGroupsSequence[0].DerivationImageSequence = Sequence([derivation])
    referenced_series = pydicom.Dataset()
    referenced_series.SeriesInstanceUID = level_zero.SeriesInstanceUID
    referenced_series.ReferencedInstanceSequence = Sequence(
        [copy.deepcopy(source_reference)]
    )
    level_one.ReferencedSeriesSequence = Sequence([referenced_series])
    level_one.SharedFunctionalGroupsSequence[0].WholeSlideMicroscopyImageFrameTypeSequence[
        0
    ].FrameType = ["DERIVED", "PRIMARY", "VOLUME", "RESAMPLED"]
    level_one.InstanceNumber = "2"
    level_one.NumberOfFrames = "1"
    level_one.TotalPixelMatrixRows = 2
    level_one.TotalPixelMatrixColumns = 2
    level_one.PerFrameFunctionalGroupsSequence = Sequence(
        [copy.deepcopy(level_one.PerFrameFunctionalGroupsSequence[0])]
    )
    frame = level_one.PerFrameFunctionalGroupsSequence[0]
    frame.FrameContentSequence[0].DimensionIndexValues = [1, 1]
    frame.PlanePositionSlideSequence[0].RowPositionInTotalImagePixelMatrix = 1
    frame.PlanePositionSlideSequence[0].ColumnPositionInTotalImagePixelMatrix = 1
    measures = level_one.SharedFunctionalGroupsSequence[0].PixelMeasuresSequence[0]
    row_spacing, column_spacing = (float(value) for value in measures.PixelSpacing)
    measures.PixelSpacing = [str(row_spacing * 2), str(column_spacing * 2)]
    level_one.ImagedVolumeWidth = float(level_one.TotalPixelMatrixColumns) * float(
        measures.PixelSpacing[1]
    )
    level_one.ImagedVolumeHeight = float(level_one.TotalPixelMatrixRows) * float(
        measures.PixelSpacing[0]
    )
    source_frames = level_zero.pixel_array
    if source_frames.shape != (4, 2, 2, 3):
        raise RuntimeError("pyramid control requires the authored 4x4 raster of four 2x2 RGB tiles")
    # Nearest-neighbor reduction by two: one sample from each tile, in raster order.
    level_one.PixelData = source_frames[:, 0, 0, :].tobytes()
    level_one["PixelData"].is_undefined_length = False
    return level_zero, level_one


def build_controls(source: Path, output: Path) -> None:
    if output.exists():
        raise RuntimeError(f"output already exists: {output}")
    inputs = {path.stem: path for path in source.glob("VC*.dcm")}
    if set(inputs) != set(SOURCE_TO_CORE_ID):
        raise RuntimeError("source control cohort does not match the sealed ten-control matrix")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f".{output.name}-", dir=output.parent) as temporary:
        staging = Path(temporary)
        generated = {}
        for source_id, control_id in SOURCE_TO_CORE_ID.items():
            dataset = pydicom.dcmread(inputs[source_id])
            _fresh_control(dataset, control_id)
            destination = staging / f"{control_id}.dcm"
            dataset.save_as(destination, enforce_file_format=True)
            generated[control_id] = destination

        native = pydicom.dcmread(inputs["VC01-EVRLE-4F-ANISO"])
        mono = _monochrome_control(native)
        mono.save_as(staging / "CP11-MONO-EVRLE-4F.dcm", enforce_file_format=True)

        pyramid_dir = staging / "CP12-PYRAMID-2L"
        pyramid_dir.mkdir()
        level_zero, level_one = _pyramid_controls(native)
        level_zero.save_as(pyramid_dir / "level-0.dcm", enforce_file_format=True)
        level_one.save_as(pyramid_dir / "level-1.dcm", enforce_file_format=True)
        implicit = copy.deepcopy(native)
        _fresh_control(implicit, "CP13-IMPLICIT-TILED-FULL")
        del implicit.PerFrameFunctionalGroupsSequence
        del implicit.DimensionIndexSequence
        del implicit.SharedFunctionalGroupsSequence[0].OpticalPathIdentificationSequence
        implicit.save_as(staging / "CP13-IMPLICIT-TILED-FULL.dcm", enforce_file_format=True)
        mono16 = copy.deepcopy(mono)
        _fresh_control(mono16, "CP14-MONO16-EVRLE")
        mono16.PixelData = b"".join((value * 257).to_bytes(2, "little") for value in mono.PixelData)
        mono16.BitsAllocated = mono16.BitsStored = 16
        mono16.HighBit = 15
        mono16["PixelData"].VR = "OW"
        mono16.save_as(staging / "CP14-MONO16-EVRLE.dcm", enforce_file_format=True)
        shutil.move(staging, output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    build_controls(args.source, args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
