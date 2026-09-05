"""Unified WSI-DICOM Bench findings and DICOM fact extraction."""

from __future__ import annotations

import hashlib
import math
from pathlib import Path

from bench.file_digest import sha256_file

from .catalog import DOMAINS, catalog_index


STATUS_PRIORITY = {"execution_error": 4, "failed": 3, "passed": 2, "skipped": 1}
INTRINSIC_KINDS = {"intrinsic", "intrinsic_set", "independent_decoder"}
EXTERNAL_KINDS = {"independent_validator"}


def map_findings(validation: dict, catalog: dict) -> list[dict]:
    """Attach catalog citations, severity, and ownership to validation checks."""
    index = catalog_index(catalog)
    findings = []
    for check in validation.get("checks", []):
        check_name = str(check.get("name", ""))
        rule = index.get(check_name)
        execution = check.get("execution")
        status = check.get("status", "failed")
        if status == "failed" and isinstance(execution, dict) and execution.get("failure"):
            status = "execution_error"
        finding = {
            "check_name": check_name,
            "path": check.get("path"),
            "status": status,
            "execution": execution,
            "command": list(check.get("command") or []),
            "message": str(check.get("message", "")),
            "stdout": str(check.get("stdout", "")),
            "stderr": str(check.get("stderr", "")),
        }
        if rule is None:
            finding.update(
                {
                    "catalog_status": "unmapped",
                    "rule_id": None,
                    "rule_kind": None,
                    "primary_domain": "conformance",
                    "clinical_severity": None,
                    "operational_severity": None,
                    "likely_fix_owners": [],
                    "citations": [],
                }
            )
        else:
            finding.update(
                {
                    "catalog_status": "mapped",
                    "rule_id": rule["rule_id"],
                    "rule_kind": rule["rule_kind"],
                    "primary_domain": rule["primary_domain"],
                    "clinical_severity": rule["clinical_severity"],
                    "clinical_severity_rationale": rule["clinical_severity_rationale"],
                    "operational_severity": rule["operational_severity"],
                    "operational_severity_rationale": rule[
                        "operational_severity_rationale"
                    ],
                    "likely_fix_owners": list(rule["likely_fix_owners"]),
                    "citations": list(rule["citations"]),
                }
            )
        findings.append(finding)
    return findings


def summarize_domains(findings: list[dict]) -> dict[str, dict]:
    """Summarize rule outcomes in the five workbench domains."""
    summaries = {}
    for domain in sorted(DOMAINS):
        selected = [finding for finding in findings if finding["primary_domain"] == domain]
        statuses = [str(finding["status"]) for finding in selected]
        unmapped = sum(finding["catalog_status"] == "unmapped" for finding in selected)
        if not selected:
            status = "not_evaluated"
        elif "execution_error" in statuses:
            status = "execution_error"
        elif unmapped or "failed" in statuses:
            status = "failed"
        elif "passed" in statuses:
            status = "passed"
        else:
            status = "skipped"
        summaries[domain] = {
            "status": status,
            "finding_count": len(selected),
            "passed": statuses.count("passed"),
            "failed": statuses.count("failed"),
            "execution_errors": statuses.count("execution_error"),
            "skipped": statuses.count("skipped"),
            "unmapped": unmapped,
        }
    return summaries


def validator_comparison(findings: list[dict]) -> dict:
    """Compare intrinsic/decoder findings with each independent validator."""
    intrinsic_findings = [
        finding for finding in findings if finding.get("rule_kind") in INTRINSIC_KINDS
    ]
    external_findings = [
        finding for finding in findings if finding.get("rule_kind") in EXTERNAL_KINDS
    ]
    intrinsic = aggregate_status(intrinsic_findings)
    external: dict[str, str] = {}
    for check_name in sorted({finding["check_name"] for finding in external_findings}):
        external[check_name] = aggregate_status(
            [finding for finding in external_findings if finding["check_name"] == check_name]
        )
    evaluated_external = [status for status in external.values() if status in {"passed", "failed"}]
    disagreement = (
        intrinsic in {"passed", "failed"}
        and bool(evaluated_external)
        and any(status != intrinsic for status in evaluated_external)
    )
    return {
        "intrinsic": intrinsic,
        "external": external,
        "disagreement": disagreement,
        "interpretation": (
            "adjudication_required"
            if disagreement
            else "no_aggregate_disagreement_observed"
        ),
    }


def aggregate_status(findings: list[dict]) -> str:
    statuses = [str(finding.get("status", "failed")) for finding in findings]
    if not statuses:
        return "not_evaluated"
    if any(finding.get("catalog_status") == "unmapped" for finding in findings):
        return "failed"
    return max(statuses, key=lambda status: STATUS_PRIORITY.get(status, 4))


def inspect_dicom_set(paths: list[Path]) -> dict:
    """Extract bounded geometry, color, compression, and identity facts."""
    if not paths:
        raise ValueError("at least one DICOM path is required")
    try:
        import pydicom
    except ImportError as exc:
        raise RuntimeError("pydicom is required for workbench inspection") from exc

    instances = []
    slide_scopes = set()
    for path in sorted(paths):
        dataset = pydicom.dcmread(path, stop_before_pixels=True)
        container_identifier = text_value(dataset, "ContainerIdentifier")
        frame_of_reference_uid = text_value(dataset, "FrameOfReferenceUID")
        slide_scopes.add((container_identifier, frame_of_reference_uid))
        instances.append(instance_facts(path, dataset))
    ordered_scopes = sorted(
        slide_scopes,
        key=lambda scope: (str(scope[0] or ""), str(scope[1] or "")),
    )
    if len(ordered_scopes) == 1:
        container_identifier, frame_of_reference_uid = ordered_scopes[0]
        slide_id = (
            container_identifier
            or frame_of_reference_uid
            or paths[0].parent.name
            or paths[0].stem
        )
        identity_status = "consistent"
    else:
        slide_id = "multiple-slide-identities"
        identity_status = "conflicting"
    return {
        "slide_id": slide_id,
        "slide_identity_status": identity_status,
        "slide_scopes": [
            {
                "container_identifier": container,
                "frame_of_reference_uid": frame,
            }
            for container, frame in ordered_scopes
        ],
        "instance_count": len(instances),
        "instances": instances,
    }


def instance_facts(path: Path, dataset) -> dict:
    rows = int_value(dataset, "Rows")
    columns = int_value(dataset, "Columns")
    total_rows = int_value(dataset, "TotalPixelMatrixRows")
    total_columns = int_value(dataset, "TotalPixelMatrixColumns")
    frame_count = int_value(dataset, "NumberOfFrames")
    expected_full_frame_count = None
    if rows and columns and total_rows and total_columns:
        expected_full_frame_count = math.ceil(total_rows / rows) * math.ceil(
            total_columns / columns
        )
    return {
        "path": str(path),
        "sha256": sha256_file(path),
        "transfer_syntax_uid": str(getattr(dataset.file_meta, "TransferSyntaxUID", "")),
        "geometry": {
            "rows": rows,
            "columns": columns,
            "number_of_frames": frame_count,
            "total_pixel_matrix_rows": total_rows,
            "total_pixel_matrix_columns": total_columns,
            "expected_full_frame_count": expected_full_frame_count,
            "pixel_spacing_mm": shared_pixel_spacing(dataset),
        },
        "color": {
            "photometric_interpretation": text_value(
                dataset, "PhotometricInterpretation"
            ),
            "icc_profile_sha256": optical_path_icc_digests(dataset),
        },
        "identity": {
            "patient_id": text_value(dataset, "PatientID"),
            "study_instance_uid": text_value(dataset, "StudyInstanceUID"),
            "series_instance_uid": text_value(dataset, "SeriesInstanceUID"),
            "sop_instance_uid": text_value(dataset, "SOPInstanceUID"),
            "frame_of_reference_uid": text_value(dataset, "FrameOfReferenceUID"),
            "container_identifier": text_value(dataset, "ContainerIdentifier"),
            "specimens": specimen_facts(dataset),
        },
        "compression": {
            "lossy_image_compression": text_value(dataset, "LossyImageCompression"),
            "lossy_methods": list_value(dataset, "LossyImageCompressionMethod"),
            "lossy_ratios": list_value(dataset, "LossyImageCompressionRatio"),
        },
    }


def shared_pixel_spacing(dataset) -> list[float] | None:
    shared = getattr(dataset, "SharedFunctionalGroupsSequence", None)
    if not shared:
        return None
    pixel_measures = getattr(shared[0], "PixelMeasuresSequence", None)
    if not pixel_measures:
        return None
    spacing = getattr(pixel_measures[0], "PixelSpacing", None)
    if spacing is None:
        return None
    try:
        values = [float(value) for value in spacing]
    except (TypeError, ValueError):
        return None
    return values if len(values) == 2 else None


def optical_path_icc_digests(dataset) -> list[str]:
    digests = []
    for optical_path in getattr(dataset, "OpticalPathSequence", []) or []:
        profile = getattr(optical_path, "ICCProfile", None)
        if profile is not None:
            digests.append(hashlib.sha256(bytes(profile)).hexdigest())
    return digests


def specimen_facts(dataset) -> list[dict]:
    specimens = []
    for specimen in getattr(dataset, "SpecimenDescriptionSequence", []) or []:
        issuer_sequence = getattr(specimen, "IssuerOfTheSpecimenIdentifierSequence", []) or []
        issuer = issuer_sequence[0] if issuer_sequence else None
        specimens.append(
            {
                "identifier": text_value(specimen, "SpecimenIdentifier"),
                "uid": text_value(specimen, "SpecimenUID"),
                "issuer_local_namespace_entity_id": (
                    text_value(issuer, "LocalNamespaceEntityID") if issuer else None
                ),
                "issuer_universal_entity_id": (
                    text_value(issuer, "UniversalEntityID") if issuer else None
                ),
                "issuer_universal_entity_id_type": (
                    text_value(issuer, "UniversalEntityIDType") if issuer else None
                ),
            }
        )
    return specimens


def text_value(dataset, keyword: str) -> str | None:
    value = getattr(dataset, keyword, None)
    if value is None:
        return None
    text = str(value).strip().rstrip("\0")
    return text or None


def int_value(dataset, keyword: str) -> int | None:
    value = getattr(dataset, keyword, None)
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def list_value(dataset, keyword: str) -> list:
    value = getattr(dataset, keyword, None)
    if value is None:
        return []
    if isinstance(value, (str, bytes)):
        return [str(value)]
    try:
        return [float(item) if keyword.endswith("Ratio") else str(item) for item in value]
    except TypeError:
        return [str(value)]
