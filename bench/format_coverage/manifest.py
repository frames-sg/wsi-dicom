"""Versioned format-coverage manifest and catalog provenance validation."""

from __future__ import annotations

import json
import math
from pathlib import Path, PurePosixPath, PureWindowsPath

from bench.file_digest import sha256_file
from bench.path_identifiers import require_portable_identifier


LEGACY_SCHEMA_VERSION = "wsi-dicom-format-coverage-v1"
SCHEMA_VERSION = "wsi-dicom-format-coverage-v2"
SUPPORTED_SCHEMA_VERSIONS = {LEGACY_SCHEMA_VERSION, SCHEMA_VERSION}
DEFAULT_MANIFEST = Path(__file__).resolve().parents[1] / "format-coverage-v2.json"
DEFAULT_WORKBENCH = Path(__file__).resolve().parents[1] / "wsi_dicom_bench.py"
DEFAULT_CATALOG = (
    Path(__file__).resolve().parents[2]
    / "rules"
    / "wsi-dicom-bench-rules-2026c-v2.json"
)
ALLOWED_TRANSFER_SYNTAXES = {
    "jpeg-baseline8-bit",
    "jpeg2000-lossless",
    "htj2k-lossless",
    "htj2k-lossless-rpcl",
}
ALLOWED_BACKENDS = {"cpu", "require-device"}
ALLOWED_ROUTE_CLASSIFICATIONS = {
    "cpu_encode",
    "metal_encode",
    "passthrough",
    "unsupported",
}


class FormatCoverageError(RuntimeError):
    """The format-coverage run could not produce trustworthy evidence."""


def load_manifest(path: Path) -> dict:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise FormatCoverageError(f"failed to load manifest {path}: {exc}") from exc
    if not isinstance(manifest, dict):
        raise FormatCoverageError("format-coverage manifest must be an object")
    schema_version = manifest.get("schema_version")
    if schema_version not in SUPPORTED_SCHEMA_VERSIONS:
        raise FormatCoverageError(
            "manifest schema_version must be one of "
            f"{sorted(SUPPORTED_SCHEMA_VERSIONS)!r}"
        )
    if schema_version == SCHEMA_VERSION:
        validate_rule_catalog_record(manifest.get("rule_catalog"))
    cases = manifest.get("cases")
    if not isinstance(cases, list) or not cases:
        raise FormatCoverageError("manifest cases must be a non-empty array")
    seen_ids: set[str] = set()
    for index, case in enumerate(cases):
        if not isinstance(case, dict):
            raise FormatCoverageError(f"manifest case {index} must be an object")
        case_id = validate_case_identifier(case.get("id"), f"manifest case {index} id")
        if case_id in seen_ids:
            raise FormatCoverageError(f"duplicate case id: {case_id}")
        seen_ids.add(case_id)
        validate_relative_path(case.get("source"), f"case {case_id} source")
        archive_entry = case.get("archive_entry")
        if archive_entry is not None:
            validate_archive_member(archive_entry)
        checksum = case.get("source_sha256")
        if not isinstance(checksum, str) or len(checksum) != 64:
            raise FormatCoverageError(f"case {case_id} requires a SHA-256 digest")
        try:
            int(checksum, 16)
        except ValueError as exc:
            raise FormatCoverageError(
                f"case {case_id} source_sha256 is not hexadecimal"
            ) from exc
        level = case.get("level")
        if not isinstance(level, int) or isinstance(level, bool) or level < 0:
            raise FormatCoverageError(f"case {case_id} level must be non-negative")
        if case.get("transfer_syntax") not in ALLOWED_TRANSFER_SYNTAXES:
            raise FormatCoverageError(f"case {case_id} has unsupported transfer_syntax")
        spacing = case.get("source_pixel_spacing_mm")
        if spacing is not None and (
            not isinstance(spacing, list)
            or len(spacing) != 2
            or any(
                not isinstance(value, (int, float))
                or isinstance(value, bool)
                or not math.isfinite(value)
                or value <= 0
                for value in spacing
            )
        ):
            raise FormatCoverageError(
                f"case {case_id} source_pixel_spacing_mm must be two finite positive values in row/column order"
            )
        expected = case.get("expected")
        if not isinstance(expected, dict) or expected.get("outcome") not in {
            "success",
            "rejected",
        }:
            raise FormatCoverageError(
                f"case {case_id} expected outcome must be success or rejected"
            )
        message_contains = expected.get("message_contains")
        if expected["outcome"] == "rejected" and (
            not isinstance(message_contains, str) or not message_contains
        ):
            raise FormatCoverageError(
                f"case {case_id} expected rejection requires message_contains"
            )
        expected_route = case.get("expected_route", {})
        if not isinstance(expected_route, dict) or any(
            backend not in ALLOWED_BACKENDS
            or route not in ALLOWED_ROUTE_CLASSIFICATIONS
            for backend, route in expected_route.items()
        ):
            raise FormatCoverageError(
                f"case {case_id} expected_route contains an unsupported backend or route"
            )
        evaluation_mode = case.get("evaluation_mode", "convert")
        if evaluation_mode not in {"convert", "route-profile"}:
            raise FormatCoverageError(
                f"case {case_id} evaluation_mode must be convert or route-profile"
            )
    return manifest


def validate_rule_catalog_record(value: object) -> dict:
    if not isinstance(value, dict):
        raise FormatCoverageError("v2 manifest requires a rule_catalog object")
    if set(value) != {"version", "path", "sha256"}:
        raise FormatCoverageError(
            "rule_catalog must contain exactly version, path, and sha256"
        )
    if not isinstance(value["version"], str) or not value["version"].strip():
        raise FormatCoverageError("rule_catalog version must be a non-empty string")
    validate_relative_path(value["path"], "rule_catalog path")
    digest = value["sha256"]
    if not isinstance(digest, str) or len(digest) != 64:
        raise FormatCoverageError("rule_catalog sha256 must be a SHA-256 digest")
    try:
        int(digest, 16)
    except ValueError as exc:
        raise FormatCoverageError("rule_catalog sha256 is not hexadecimal") from exc
    return value


def validate_rule_catalog_provenance(manifest: dict, catalog_path: Path) -> dict:
    expected = validate_rule_catalog_record(manifest.get("rule_catalog"))
    try:
        catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise FormatCoverageError(f"failed to load rule catalog {catalog_path}: {exc}") from exc
    if not isinstance(catalog, dict):
        raise FormatCoverageError(f"rule catalog must be an object: {catalog_path}")
    if catalog.get("catalog_version") != expected["version"]:
        raise FormatCoverageError(
            "configured rule catalog version does not match manifest provenance"
        )
    actual_sha256 = sha256_file(catalog_path)
    if actual_sha256 != expected["sha256"]:
        raise FormatCoverageError(
            "configured rule catalog SHA-256 does not match manifest provenance"
        )
    return expected


def validate_relative_path(value: object, label: str) -> PurePosixPath:
    if not isinstance(value, str) or not value:
        raise FormatCoverageError(f"{label} must be a non-empty relative path")
    path = PurePosixPath(value)
    if (
        "\\" in value
        or PureWindowsPath(value).drive
        or path.is_absolute()
        or value != path.as_posix()
        or ".." in path.parts
        or "." in path.parts
    ):
        raise FormatCoverageError(f"{label} must be a safe relative path: {value}")
    return path


def validate_case_identifier(value: object, label: str = "case id") -> str:
    try:
        return require_portable_identifier(value, label)
    except ValueError as exc:
        raise FormatCoverageError(str(exc)) from exc


def validate_archive_member(value: object) -> PurePosixPath:
    try:
        return validate_relative_path(value, "archive member")
    except FormatCoverageError as exc:
        raise FormatCoverageError(f"unsafe archive member: {value}") from exc
