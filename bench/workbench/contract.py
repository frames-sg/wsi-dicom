"""Check compatibility between the selected catalog and emitted validation evidence."""

from pathlib import Path


def validation_contract_errors(validation: dict, catalog: dict, core: bool) -> list[str]:
    expected_profile = "core-2026c" if core else "general"
    errors = []
    if validation.get("profile") != expected_profile:
        errors.append(f"binary did not execute selected profile {expected_profile}")
    checks = validation.get("checks", [])
    known = {name for rule in catalog["rules"] for name in rule["check_names"]}
    for check in checks:
        if check.get("name") not in known:
            errors.append(f"binary emitted an unmapped check: {check.get('name')}")
    # Every instance must have every unconditional intrinsic check. Corpus and
    # decoder rules are conditional and are checked by the evidence release gate.
    required = {
        name for rule in catalog["rules"] if rule["rule_kind"] == "intrinsic"
        for name in rule["check_names"]
    }
    if core:
        for path in validation.get("files", []):
            observed = {
                check.get("name") for check in checks
                if check.get("path") and Path(check["path"]).resolve() == Path(path).resolve()
                and check.get("status") in {"passed", "failed"}
            }
            missing = sorted(required - observed)
            if missing:
                errors.append(f"{path}: missing required checks: {', '.join(missing)}")
    return errors
