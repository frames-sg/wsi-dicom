"""Canonical identifiers for negative-bench package paths."""

from __future__ import annotations

from bench.path_identifiers import require_portable_identifier


def manifest_identifier_items(manifest: object) -> tuple[list[dict], list[dict]]:
    """Validate manifest collection shapes, path-safe IDs, and uniqueness."""
    if not isinstance(manifest, dict):
        raise ValueError("challenge manifest must be an object")
    rule_catalog = manifest.get("rule_catalog")
    if rule_catalog is not None:
        if not isinstance(rule_catalog, dict):
            raise ValueError("challenge manifest rule_catalog must be an object")
        catalog_path = rule_catalog.get("path")
        if not isinstance(catalog_path, str) or not catalog_path:
            raise ValueError("challenge manifest rule_catalog.path must be a non-empty string")
    core_profile = manifest.get("core_profile")
    if core_profile is not None:
        if not isinstance(core_profile, dict):
            raise ValueError("challenge manifest core_profile must be an object")
        profile_path = core_profile.get("path")
        profile_id = core_profile.get("profile_id")
        if not isinstance(profile_path, str) or not profile_path:
            raise ValueError("challenge manifest core_profile.path must be a non-empty string")
        if not isinstance(profile_id, str) or not profile_id:
            raise ValueError("challenge manifest core_profile.profile_id must be a non-empty string")
    collections = (
        ("valid_controls", "control_id", "control"),
        ("evaluation_cases", "case_id", "case"),
    )
    validated: list[list[dict]] = []
    for collection_name, id_key, label in collections:
        items = manifest.get(collection_name)
        if not isinstance(items, list):
            raise ValueError(f"challenge manifest {collection_name} must be an array")
        seen: set[str] = set()
        for index, item in enumerate(items):
            if not isinstance(item, dict):
                raise ValueError(f"challenge manifest {label} {index} must be an object")
            identifier = require_portable_identifier(
                item.get(id_key), f"challenge manifest {label} {index} {id_key}"
            )
            if identifier in seen:
                raise ValueError(f"challenge manifest {label} IDs are not unique")
            seen.add(identifier)
        validated.append(items)
    return validated[0], validated[1]
