"""Validate links and executed evidence for declared WSI rule-family challenge rows.

This is not an exhaustive inventory of normative DICOM attributes or conditions.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path


class CoreProfileError(ValueError):
    """A core profile or its coverage evidence is incomplete or inconsistent."""


def _load_object(path: Path, label: str) -> dict:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise CoreProfileError(f"cannot read {label} {path}: {exc}") from exc
    if not isinstance(document, dict):
        raise CoreProfileError(f"{label} must be a JSON object")
    return document


def load_profile(path: Path) -> dict:
    profile = _load_object(path, "core profile")
    if profile.get("schema_version") != "wsi-dicom-core-profile-v1":
        raise CoreProfileError("unsupported core-profile schema")
    for field in (
        "profile_id",
        "profile_version",
        "dicom_edition",
        "sop_class_uid",
        "scope",
        "required_external_validators",
        "excluded_capabilities",
        "requirements",
    ):
        if field not in profile:
            raise CoreProfileError(f"core profile is missing {field}")
    if not isinstance(profile["profile_version"], str) or not profile[
        "profile_version"
    ]:
        raise CoreProfileError("core profile profile_version must be a nonempty string")
    if not isinstance(profile["requirements"], list) or not profile["requirements"]:
        raise CoreProfileError("core profile requirements must be a nonempty array")
    identifiers = []
    for index, requirement in enumerate(profile["requirements"]):
        if not isinstance(requirement, dict):
            raise CoreProfileError(f"requirement {index} must be an object")
        required = {
            "requirement_id",
            "part",
            "section",
            "requirement_type",
            "applicability",
            "statement",
            "enforcement",
            "positive_control_ids",
            "negative_case_ids",
        }
        missing = sorted(required - set(requirement))
        if missing:
            raise CoreProfileError(
                f"requirement {index} is missing {', '.join(missing)}"
            )
        identifier = requirement["requirement_id"]
        if not isinstance(identifier, str) or not identifier:
            raise CoreProfileError(f"requirement {index} has an invalid identifier")
        identifiers.append(identifier)
    if len(set(identifiers)) != len(identifiers):
        raise CoreProfileError("core profile contains a duplicate requirement ID")
    validators = profile["required_external_validators"]
    if not isinstance(validators, list) or not all(
        isinstance(value, str) and value for value in validators
    ):
        raise CoreProfileError("required_external_validators must contain names")
    return profile


def validate_profile_coverage(profile: dict, catalog: dict, manifest: dict) -> dict:
    if manifest.get("dicom_edition") != profile.get("dicom_edition"):
        raise CoreProfileError("manifest and core profile use different DICOM editions")
    if catalog.get("dicom_edition") != profile.get("dicom_edition"):
        raise CoreProfileError("catalog and core profile use different DICOM editions")
    manifest_profile = manifest.get("core_profile")
    if not isinstance(manifest_profile, dict) or manifest_profile.get(
        "profile_id"
    ) != profile.get("profile_id"):
        raise CoreProfileError("manifest does not declare this core profile")
    catalog_rules = {
        rule.get("rule_id"): rule
        for rule in catalog.get("rules", [])
        if isinstance(rule, dict) and isinstance(rule.get("rule_id"), str)
    }
    intrinsic_rules = {
        identifier
        for identifier, rule in catalog_rules.items()
        if rule.get("rule_kind") != "independent_validator"
    }
    catalog_checks = {
        check_name
        for rule in catalog_rules.values()
        for check_name in rule.get("check_names", [])
        if isinstance(check_name, str)
    }
    missing_required_validators = sorted(
        set(profile.get("required_external_validators", [])) - catalog_checks
    )
    if missing_required_validators:
        raise CoreProfileError(
            "required external validators are absent from the catalog: "
            + ", ".join(missing_required_validators)
        )
    controls = {
        item.get("control_id")
        for item in manifest.get("valid_controls", [])
        if isinstance(item, dict)
    }
    cases = {
        item.get("case_id"): item
        for item in manifest.get("evaluation_cases", [])
        if isinstance(item, dict) and isinstance(item.get("case_id"), str)
    }
    uncovered = []
    for requirement in profile.get("requirements", []):
        identifier = requirement.get("requirement_id", "<unknown>")
        if requirement.get("applicability") != "required":
            uncovered.append(f"{identifier}: applicability is not required")
            continue
        enforcement = requirement.get("enforcement")
        if not isinstance(enforcement, dict):
            uncovered.append(f"{identifier}: enforcement is missing")
            continue
        rule_ids = enforcement.get("rule_ids", [])
        check_names = enforcement.get("external_check_names", [])
        if not rule_ids and not check_names:
            uncovered.append(f"{identifier}: no rule or external validator maps the requirement")
        unknown_rules = sorted(set(rule_ids) - set(catalog_rules))
        if unknown_rules:
            uncovered.append(
                f"{identifier}: unknown catalog rules {', '.join(unknown_rules)}"
            )
        unknown_checks = sorted(
            set(check_names) - set(profile.get("required_external_validators", []))
        )
        if unknown_checks:
            uncovered.append(
                f"{identifier}: external checks are not required validators: {', '.join(unknown_checks)}"
            )
        positive = requirement.get("positive_control_ids")
        negative = requirement.get("negative_case_ids")
        if not isinstance(positive, list) or not positive:
            uncovered.append(f"{identifier}: no positive control coverage")
        elif not set(positive).issubset(controls):
            uncovered.append(f"{identifier}: positive controls are absent from the manifest")
        if not isinstance(negative, list) or not negative:
            uncovered.append(f"{identifier}: no negative case coverage")
        elif not set(negative).issubset(cases):
            uncovered.append(f"{identifier}: negative cases are absent from the manifest")
        elif rule_ids and not any(
            set(cases[case_id].get("expected_failing_rules", [])) & set(rule_ids)
            for case_id in negative
        ):
            uncovered.append(
                f"{identifier}: negative cases do not exercise a mapped intrinsic rule"
            )

    exercised = {
        rule_id
        for case in cases.values()
        for rule_id in case.get("expected_failing_rules", [])
        if isinstance(rule_id, str)
    }
    rules_without_cases = sorted(intrinsic_rules - exercised)
    if uncovered or rules_without_cases:
        details = [*uncovered]
        if rules_without_cases:
            details.append(
                "intrinsic rules without negative cases: " + ", ".join(rules_without_cases)
            )
        raise CoreProfileError("core-profile coverage is incomplete: " + "; ".join(details))
    return {
        "profile_id": profile["profile_id"],
        "coverage_scope": "declared_challenge_rows_only",
        "normative_completeness_evaluated": False,
        "requirement_count": len(profile["requirements"]),
        "intrinsic_rule_count": len(intrinsic_rules),
        "uncovered_requirements": [],
        "intrinsic_rules_without_negative_cases": [],
    }


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_evidence_coverage(
    profile: dict, catalog: dict, manifest: dict, evidence_root: Path
) -> dict:
    """Verify the post-execution core-profile release gates."""
    validate_profile_coverage(profile, catalog, manifest)
    catalog_rules = {
        rule["rule_id"]: rule
        for rule in catalog["rules"]
        if rule.get("rule_kind") != "independent_validator"
    }
    profile_digest = _sha256(
        evidence_root
        / "expected-results"
        / Path(manifest["core_profile"]["path"]).name
    )

    reports = {}
    for collection, key in (
        (manifest["valid_controls"], "control_id"),
        (manifest["evaluation_cases"], "case_id"),
    ):
        for item in collection:
            identifier = item[key]
            report_path = (
                evidence_root
                / "observed-results"
                / identifier
                / "workbench"
                / "workbench-report.json"
            )
            try:
                report = json.loads(report_path.read_text(encoding="utf-8"))
            except (OSError, UnicodeError, json.JSONDecodeError) as exc:
                raise CoreProfileError(
                    f"cannot read workbench evidence for {identifier}: {exc}"
                ) from exc
            if report.get("profile", {}).get("id") != profile["profile_id"]:
                raise CoreProfileError(f"{identifier} does not record the core profile")
            if report.get("profile", {}).get("sha256") != profile_digest:
                raise CoreProfileError(f"{identifier} records the wrong core-profile digest")
            _validate_report_evidence(identifier, report, catalog, manifest, evidence_root)
            reports[identifier] = report

    for requirement in profile["requirements"]:
        for rule_id in requirement["enforcement"].get("rule_ids", []):
            if rule_id not in catalog_rules:
                continue
            if not any(
                any(
                    finding.get("rule_id") == rule_id
                    and finding.get("status") == "passed"
                    for finding in reports[control_id].get("findings", [])
                )
                for control_id in requirement["positive_control_ids"]
            ):
                raise CoreProfileError(
                    f"requirement {requirement['requirement_id']} lacks passing "
                    f"positive evidence for {rule_id}"
                )

    required_validators = profile["required_external_validators"]
    positive_rules: set[str] = set()
    for control in manifest["valid_controls"]:
        identifier = control["control_id"]
        report = reports[identifier]
        if report.get("status") != "passed":
            raise CoreProfileError(f"control {identifier} did not pass the workbench")
        findings = report.get("findings", [])
        for validator in required_validators:
            statuses = [
                finding.get("status")
                for finding in findings
                if finding.get("check_name") == validator
            ]
            if not statuses or any(status != "passed" for status in statuses):
                raise CoreProfileError(
                    f"control {identifier} did not pass required validator {validator}"
                )
        positive_rules.update(
            finding["rule_id"]
            for finding in findings
            if finding.get("status") == "passed"
            and finding.get("rule_id") in catalog_rules
        )

    missing_positive_rules = sorted(set(catalog_rules) - positive_rules)
    if missing_positive_rules:
        raise CoreProfileError(
            "intrinsic rules without passing control evidence: "
            + ", ".join(missing_positive_rules)
        )

    negative_rules: set[str] = set()
    for case in manifest["evaluation_cases"]:
        identifier = case["case_id"]
        report = reports[identifier]
        findings = report.get("findings", [])
        failed_rules = {
            finding.get("rule_id")
            for finding in findings
            if finding.get("status") == "failed"
        }
        expected = set(case.get("expected_failing_rules", []))
        if report.get("status") != "failed" or not expected.issubset(failed_rules):
            raise CoreProfileError(
                f"case {identifier} was not localized to every expected rule"
            )
        failed_domains = {
            finding.get("primary_domain")
            for finding in findings
            if finding.get("status") == "failed"
            and finding.get("rule_id") in expected
        }
        if case.get("domain") not in failed_domains:
            raise CoreProfileError(f"case {identifier} was not localized to its domain")
        intrinsic_failed = {
            finding.get("rule_id") for finding in findings
            if finding.get("status") == "failed"
            and (finding.get("rule_id") in catalog_rules
                 or finding.get("rule_kind") in {"intrinsic", "intrinsic_set", "independent_decoder"})
        }
        unexpected = intrinsic_failed - expected - set(case.get("adjudicated_cascade_rules", []))
        if unexpected:
            raise CoreProfileError(f"case {identifier} has unadjudicated intrinsic failures: {sorted(unexpected)}")
        negative_rules.update(expected)

    missing_negative_rules = sorted(set(catalog_rules) - negative_rules)
    if missing_negative_rules:
        raise CoreProfileError(
            "intrinsic rules without negative evidence: "
            + ", ".join(missing_negative_rules)
        )
    return {
        "schema_version": "wsi-dicom-core-profile-evidence-gate-v1",
        "profile_id": profile["profile_id"],
        "profile_sha256": profile_digest,
        "requirements": len(profile["requirements"]),
        "intrinsic_rules": len(catalog_rules),
        "controls_passing": len(manifest["valid_controls"]),
        "negative_cases_detected_and_localized": len(manifest["evaluation_cases"]),
        "required_external_validators": required_validators,
        "uncovered_requirements": [],
        "intrinsic_rules_without_positive_evidence": [],
        "intrinsic_rules_without_negative_evidence": [],
    }


def _validate_report_evidence(identifier: str, report: dict, catalog: dict, manifest: dict, root: Path) -> None:
    """Bind a completed verdict to its catalog, actual inputs, and required checks."""
    findings = report.get("findings", [])
    if report.get("status") not in {"passed", "failed"} or report.get("contract_errors"):
        raise CoreProfileError(f"{identifier} has incomplete execution or incompatible checks")
    if any(f.get("status") == "execution_error" or f.get("catalog_status") == "unmapped"
           or (f.get("execution") or {}).get("failure") for f in findings if f.get("status") != "skipped"):
        raise CoreProfileError(f"{identifier} contains an execution error or unmapped finding")
    catalog_path = root / "expected-results" / Path(manifest["rule_catalog"]["path"]).name
    if report.get("catalog", {}).get("sha256") != _sha256(catalog_path):
        raise CoreProfileError(f"{identifier} records the wrong catalog digest")
    for validator in manifest.get("required_external_validators", []) or [
        name for rule in catalog["rules"] if rule.get("rule_kind") == "independent_validator"
        for name in rule.get("check_names", []) if name != "dcmvalidate"
    ]:
        statuses = [f.get("status") for f in findings if f.get("check_name") == validator]
        if not statuses or any(status not in {"passed", "failed"} for status in statuses):
            raise CoreProfileError(f"{identifier} lacks completed required validator {validator}")
    for execution in report.get("execution", {}).values():
        if execution.get("returncode") not in {0, 1} or execution.get("timed_out") or execution.get("launch_error") or execution.get("stdout_truncated") or execution.get("stderr_truncated"):
            raise CoreProfileError(f"{identifier} has incomplete process evidence")
    control = any(c["control_id"] == identifier for c in manifest["valid_controls"])
    input_root = root / (f"controls/{identifier}" if control else f"cases/{identifier}/input")
    paths = {p.resolve() for p in input_root.rglob("*.dcm")} if input_root.is_dir() else {input_root.with_name(input_root.name + ".dcm").resolve()}
    instances = report.get("slide", {}).get("instances", [])
    observed = set()
    required = {name for rule in catalog["rules"] if rule.get("rule_kind") == "intrinsic" for name in rule["check_names"]}
    for instance in instances:
        path = (root / instance["path"]).resolve()
        if path not in paths or path in observed or not path.is_relative_to(root.resolve()) or not path.is_file():
            raise CoreProfileError(f"{identifier} records an unexpected or duplicate input")
        observed.add(path)
        if instance.get("sha256") != _sha256(path):
            raise CoreProfileError(f"{identifier} input digest differs from observed bytes")
        emitted = {f.get("check_name") for f in findings if f.get("path") and (root / f["path"]).resolve() == path and f.get("status") in {"passed", "failed"}}
        if required - emitted:
            raise CoreProfileError(f"{identifier} is missing required per-instance checks: {sorted(required - emitted)}")
    if observed != paths:
        raise CoreProfileError(f"{identifier} report input set differs from the challenge")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        profile = load_profile(args.profile)
        catalog = _load_object(args.catalog, "rule catalog")
        manifest = _load_object(args.manifest, "challenge manifest")
        summary = (
            validate_evidence_coverage(
                profile, catalog, manifest, args.evidence.resolve()
            )
            if args.evidence
            else validate_profile_coverage(profile, catalog, manifest)
        )
    except CoreProfileError as exc:
        print(f"core-profile validation failed: {exc}", file=sys.stderr)
        return 2
    rendered = json.dumps(summary, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
