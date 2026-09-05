"""Versioned rule-catalog loading and validation."""

from __future__ import annotations

import json
from pathlib import Path


REQUIRED_RULE_FIELDS = {
    "rule_id",
    "check_names",
    "rule_kind",
    "primary_domain",
    "summary",
    "applicability",
    "clinical_severity",
    "clinical_severity_rationale",
    "operational_severity",
    "operational_severity_rationale",
    "likely_fix_owners",
    "citations",
}
DOMAINS = {"conformance", "pixel", "geometry", "color", "identity"}
CLINICAL_SEVERITIES = {"none", "minor", "major", "critical"}
OPERATIONAL_SEVERITIES = {"minor", "major", "critical"}


class CatalogError(ValueError):
    """The rule catalog is missing required or unique data."""


def load_catalog(path: Path) -> dict:
    """Load and validate one WSI-DICOM Bench rule catalog."""
    try:
        catalog = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise CatalogError(f"failed to load rule catalog {path}: {exc}") from exc
    validate_catalog(catalog)
    return catalog


def validate_catalog(catalog: object) -> None:
    if not isinstance(catalog, dict):
        raise CatalogError("rule catalog must be a JSON object")
    for field in ("catalog_schema_version", "catalog_version", "dicom_edition", "rules"):
        if field not in catalog:
            raise CatalogError(f"rule catalog is missing {field}")
    if catalog["catalog_schema_version"] != "wsi-dicom-bench-rule-catalog-v1":
        raise CatalogError("unsupported rule catalog schema")
    if not isinstance(catalog["rules"], list) or not catalog["rules"]:
        raise CatalogError("rule catalog must contain at least one rule")

    rule_ids: set[str] = set()
    check_names: set[str] = set()
    for index, rule in enumerate(catalog["rules"]):
        if not isinstance(rule, dict):
            raise CatalogError(f"rule {index} must be a JSON object")
        missing = REQUIRED_RULE_FIELDS - set(rule)
        if missing:
            raise CatalogError(f"rule {index} is missing {sorted(missing)}")
        rule_id = required_string(rule, "rule_id", index)
        if rule_id in rule_ids:
            raise CatalogError(f"duplicate rule_id {rule_id}")
        rule_ids.add(rule_id)
        if rule["primary_domain"] not in DOMAINS:
            raise CatalogError(f"rule {rule_id} has an invalid primary_domain")
        if rule["clinical_severity"] not in CLINICAL_SEVERITIES:
            raise CatalogError(f"rule {rule_id} has an invalid clinical_severity")
        if rule["operational_severity"] not in OPERATIONAL_SEVERITIES:
            raise CatalogError(f"rule {rule_id} has an invalid operational_severity")
        require_nonempty_string_list(rule, "check_names", rule_id)
        require_nonempty_string_list(rule, "likely_fix_owners", rule_id)
        for check_name in rule["check_names"]:
            if check_name in check_names:
                raise CatalogError(f"check name {check_name} maps to multiple rules")
            check_names.add(check_name)
        citations = rule["citations"]
        if not isinstance(citations, list) or not citations:
            raise CatalogError(f"rule {rule_id} must contain citations")
        for citation in citations:
            if not isinstance(citation, dict):
                raise CatalogError(f"rule {rule_id} contains a non-object citation")
            for field in ("edition", "part", "section", "requirement", "url"):
                if not isinstance(citation.get(field), str) or not citation[field].strip():
                    raise CatalogError(f"rule {rule_id} citation is missing {field}")
            if citation["edition"] != catalog["dicom_edition"]:
                raise CatalogError(f"rule {rule_id} citation edition differs from catalog")
            if not citation["url"].startswith("https://dicom.nema.org/"):
                raise CatalogError(f"rule {rule_id} citation is not an official DICOM URL")


def catalog_index(catalog: dict) -> dict[str, dict]:
    """Index catalog entries by validation check name."""
    validate_catalog(catalog)
    return {
        check_name: rule
        for rule in catalog["rules"]
        for check_name in rule["check_names"]
    }


def required_string(rule: dict, field: str, index: int) -> str:
    value = rule.get(field)
    if not isinstance(value, str) or not value.strip():
        raise CatalogError(f"rule {index} has an invalid {field}")
    return value


def require_nonempty_string_list(rule: dict, field: str, rule_id: str) -> None:
    value = rule.get(field)
    if not isinstance(value, list) or not value:
        raise CatalogError(f"rule {rule_id} has an invalid {field}")
    if any(not isinstance(item, str) or not item.strip() for item in value):
        raise CatalogError(f"rule {rule_id} has a non-string {field} entry")
