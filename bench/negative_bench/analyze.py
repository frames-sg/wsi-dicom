"""Analyze frozen WSI-DICOM Negative Bench workbench evidence."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

if __package__ in {None, ""}:
    sys.dont_write_bytecode = True
    source = Path(__file__).resolve()
    for import_root in (source.parents[2], source.parents[1] / "generator"):
        value = str(import_root)
        if import_root.is_dir() and value not in sys.path:
            sys.path.insert(0, value)

from bench.negative_bench.identifiers import manifest_identifier_items

try:
    from bench.file_digest import sha256_file
    from bench.negative_bench.analysis_artifacts import (
        EXTERNAL_NAMES,
        _results_markdown,
        _write_artifacts,
    )
except ImportError:  # Packaged standalone analysis directory.
    from analysis_artifacts import EXTERNAL_NAMES, _results_markdown, _write_artifacts
    from file_digest import sha256_file


INTRINSIC_KINDS = {"intrinsic", "intrinsic_set", "independent_decoder"}
DEFAULT_ADJUDICATIONS = Path(__file__).with_name("adjudications-v1.json")


class AnalysisError(RuntimeError):
    """Frozen challenge evidence is missing or internally inconsistent."""


def exact_binomial(successes: int, total: int) -> tuple[float, float]:
    if total == 0:
        return (math.nan, math.nan)
    alpha_tail = 0.025

    def upper_tail(probability: float) -> float:
        return sum(
            math.comb(total, value)
            * probability**value
            * (1 - probability) ** (total - value)
            for value in range(successes, total + 1)
        )

    def lower_tail(probability: float) -> float:
        return sum(
            math.comb(total, value)
            * probability**value
            * (1 - probability) ** (total - value)
            for value in range(0, successes + 1)
        )

    lower = 0.0
    if successes:
        low, high = 0.0, successes / total
        for _ in range(80):
            middle = (low + high) / 2
            if upper_tail(middle) < alpha_tail:
                low = middle
            else:
                high = middle
        lower = (low + high) / 2

    upper = 1.0
    if successes < total:
        low, high = successes / total, 1.0
        for _ in range(80):
            middle = (low + high) / 2
            if lower_tail(middle) > alpha_tail:
                low = middle
            else:
                high = middle
        upper = (low + high) / 2
    return (lower, upper)


def load_adjudications(path: Path) -> tuple[dict[str, dict[str, str]], dict]:
    document = _read_json(path)
    if document.get("schema_version") != "wsi-dicom-negative-bench-adjudications-v1":
        raise AnalysisError("unsupported adjudication schema")
    cases = document.get("cases")
    if not isinstance(cases, dict):
        raise AnalysisError("adjudication cases must be an object")
    for case_id, entry in cases.items():
        if (
            not isinstance(case_id, str)
            or not case_id
            or not isinstance(entry, dict)
            or set(entry) != {"disposition", "rationale"}
            or any(not isinstance(value, str) or not value.strip() for value in entry.values())
        ):
            raise AnalysisError(f"invalid adjudication entry for {case_id!r}")
    return cases, {
        "schema_version": document["schema_version"],
        "sha256": sha256_file(path),
    }


def analyze_challenge(
    package: Path, adjudications_path: Path = DEFAULT_ADJUDICATIONS
) -> dict:
    package = package.resolve()
    if (package / "SHA256SUMS").exists():
        raise AnalysisError("cannot write analysis into a sealed package; reproduce into a new output directory")
    rows, domain_rows, summary, adjudications = _compute_analysis(package, adjudications_path)
    _write_artifacts(package, rows, domain_rows, summary, adjudications)
    return summary


def summarize_challenge(package: Path, adjudications_path: Path = DEFAULT_ADJUDICATIONS) -> dict:
    """Recompute metrics without writing into retained or sealed evidence."""
    return _compute_analysis(package.resolve(), adjudications_path)[2]


def _compute_analysis(package: Path, adjudications_path: Path) -> tuple:
    adjudications, adjudication_provenance = load_adjudications(adjudications_path)
    manifest = _read_json(package / "manifest.json")
    try:
        manifest_identifier_items(manifest)
    except ValueError as exc:
        raise AnalysisError(str(exc)) from exc
    run_summary = _read_json(package / "observed-results" / "run-summary.json")
    rows = _build_case_rows(package, manifest, run_summary)
    evaluation = [row for row in rows if row["cohort"] == "evaluation-v1"]
    control_rows = [row for row in rows if row["cohort"] == "valid-controls-v1"]
    bench_detected = sum(row["bench_detected"] for row in evaluation)
    intrinsic_detected = sum(row["intrinsic_detected"] for row in evaluation)
    controls_accepted = sum(row["bench_status"] == "passed" for row in control_rows)
    rule_localized = sum(row["expected_rule_localized"] for row in evaluation if row["bench_detected"])
    domain_localized = sum(
        row["expected_domain_localized"] for row in evaluation if row["bench_detected"]
    )

    domain_rows = _build_domain_rows(package, evaluation, control_rows)
    validator_counts, unique_counts, agreement = _validator_metrics(evaluation)
    sensitivity_ci = exact_binomial(bench_detected, len(evaluation))
    specificity_ci = exact_binomial(controls_accepted, len(control_rows))
    summary = {
        "schema_version": "wsi-dicom-negative-bench-analysis-v1",
        "challenge_id": manifest["challenge_id"],
        "evaluation_cases": len(evaluation),
        "valid_controls": len(control_rows),
        "bench_detected": bench_detected,
        "defect_detection_sensitivity": bench_detected / len(evaluation),
        "defect_detection_sensitivity_ci95": list(sensitivity_ci),
        "intrinsic_detected": intrinsic_detected,
        "intrinsic_detection_sensitivity": intrinsic_detected / len(evaluation),
        "valid_controls_accepted": controls_accepted,
        "valid_control_specificity": controls_accepted / len(control_rows),
        "valid_control_specificity_ci95": list(specificity_ci),
        "rule_localized_detected_cases": rule_localized,
        "rule_localization_accuracy": rule_localized / len(evaluation),
        "rule_localization_accuracy_among_detected": (
            rule_localized / bench_detected if bench_detected else None
        ),
        "domain_localized_detected_cases": domain_localized,
        "domain_localization_accuracy": domain_localized / len(evaluation),
        "domain_localization_accuracy_among_detected": (
            domain_localized / bench_detected if bench_detected else None
        ),
        "validator_detection_counts": validator_counts,
        "validator_unique_detection_counts": unique_counts,
        "intrinsic_external_agreement": agreement,
        "unmapped_findings": sum(row["unmapped_findings"] for row in rows),
        "execution_failures": sum(row["execution_failure"] for row in rows),
        "total_runtime_seconds": sum(row["runtime_seconds"] for row in rows),
        "cases_not_detected": sorted(row["id"] for row in evaluation if not row["bench_detected"]),
        "cases_not_rule_localized": sorted(
            row["id"] for row in evaluation if not row["expected_rule_localized"]
        ),
        "not_executed_validators": sorted(
            name
            for name in EXTERNAL_NAMES
            if not any(row[name] in {"passed", "failed"} for row in rows)
        ),
        "adjudications": adjudication_provenance,
    }
    return rows, domain_rows, summary, adjudications


def _build_case_rows(package: Path, manifest: dict, run_summary: dict) -> list[dict]:
    execution_by_id = {item["id"]: item for item in run_summary["executions"]}
    cases = {item["case_id"]: item for item in manifest["evaluation_cases"]}
    controls = {item["control_id"]: item for item in manifest["valid_controls"]}
    rows = []
    for identifier in [*sorted(controls), *sorted(cases)]:
        cohort = "valid-controls-v1" if identifier in controls else "evaluation-v1"
        expected = cases.get(identifier)
        report_path = (
            package
            / "observed-results"
            / identifier
            / "workbench"
            / "workbench-report.json"
        )
        report = _read_json(report_path)
        findings = report.get("findings", [])
        failed = [finding for finding in findings if finding.get("status") == "failed"]
        failed_rules = sorted({finding.get("rule_id") for finding in failed if finding.get("rule_id")})
        failed_domains = sorted({finding.get("primary_domain") for finding in failed})
        intrinsic_failed = [
            finding for finding in failed if finding.get("rule_kind") in INTRINSIC_KINDS
        ]
        external = report.get("validator_comparison", {}).get("external", {})
        expected_rules = expected["expected_failing_rules"] if expected else []
        expected_unaffected = expected["expected_unaffected_rules"] if expected else []
        expected_domain = expected["domain"] if expected else None
        execution = execution_by_id[identifier]
        execution_failure = (
            report.get("status") == "execution_error"
            or any(finding.get("status") == "execution_error" for finding in findings)
            or bool(execution.get("timed_out") or execution.get("launch_error"))
            or execution.get("returncode", 0) not in {0, 1}
        )
        bench_detected = report.get("status") == "failed" and not execution_failure
        intrinsic_detected = bool(intrinsic_failed) and not execution_failure
        expected_rule_localized = bool(expected_rules) and not execution_failure and set(expected_rules).issubset(failed_rules)
        expected_domain_localized = (
            not execution_failure and any(finding.get("rule_id") in expected_rules and finding.get("primary_domain") == expected_domain for finding in intrinsic_failed) if expected_domain else False
        )
        unexpected_intrinsic = sorted(
            set(expected_unaffected) & {item.get("rule_id") for item in intrinsic_failed}
        )
        rows.append(
            {
                "id": identifier,
                "cohort": cohort,
                "domain": expected_domain or "control",
                "expected_failing_rules": ";".join(expected_rules),
                "bench_status": report.get("status", "execution_failure"),
                "bench_detected": bench_detected,
                "intrinsic_detected": intrinsic_detected,
                "expected_rule_localized": expected_rule_localized,
                "expected_domain_localized": expected_domain_localized,
                "failed_rules": ";".join(failed_rules),
                "failed_domains": ";".join(failed_domains),
                "unexpected_intrinsic_rules": ";".join(unexpected_intrinsic),
                "dciodvfy": external.get("dciodvfy", "unavailable"),
                "dcentvfy": external.get("dcentvfy", "unavailable"),
                "validate_iods": external.get("validate_iods", "unavailable"),
                "dcmvalidate": external.get("dcmvalidate", "unavailable"),
                "unmapped_findings": sum(
                    finding.get("catalog_status") == "unmapped" for finding in findings
                ),
                "runtime_seconds": execution_by_id[identifier]["runtime_seconds"],
                "execution_failure": execution_failure,
            }
        )

    return rows


def _build_domain_rows(
    package: Path, evaluation: list[dict], control_rows: list[dict]
) -> list[dict]:
    domain_rows = []
    for domain in sorted({row["domain"] for row in evaluation}):
        selected = [row for row in evaluation if row["domain"] == domain]
        detected = sum(row["bench_detected"] for row in selected)
        localized = sum(row["expected_domain_localized"] for row in selected)
        control_specific = sum(
            _domain_passes(package, row["id"], domain) for row in control_rows
        )
        sensitivity_ci = exact_binomial(detected, len(selected))
        specificity_ci = exact_binomial(control_specific, len(control_rows))
        domain_rows.append(
            {
                "domain": domain,
                "evaluation_cases": len(selected),
                "detected": detected,
                "sensitivity": detected / len(selected),
                "sensitivity_ci95_low": sensitivity_ci[0],
                "sensitivity_ci95_high": sensitivity_ci[1],
                "domain_localized": localized,
                "domain_localization_accuracy": localized / len(selected),
                "controls": len(control_rows),
                "controls_passing_domain": control_specific,
                "specificity": control_specific / len(control_rows),
                "specificity_ci95_low": specificity_ci[0],
                "specificity_ci95_high": specificity_ci[1],
            }
        )

    return domain_rows


def _validator_metrics(
    evaluation: list[dict],
) -> tuple[dict[str, int], dict[str, int], dict[str, dict]]:
    detection_sets = {
        "wsi_dicom_intrinsic": {row["id"] for row in evaluation if row["intrinsic_detected"]}
    }
    for name in EXTERNAL_NAMES:
        detection_sets[name] = {
            row["id"] for row in evaluation if row[name] == "failed"
        }
    validator_counts = {name: len(values) for name, values in detection_sets.items()}
    external_union = set().union(*(detection_sets[name] for name in EXTERNAL_NAMES))
    unique_counts = {
        "wsi_dicom_intrinsic": len(detection_sets["wsi_dicom_intrinsic"] - external_union)
    }
    for name in EXTERNAL_NAMES:
        others = set().union(
            *(values for other, values in detection_sets.items() if other != name)
        )
        unique_counts[name] = len(detection_sets[name] - others)

    agreement = {}
    intrinsic_set = detection_sets["wsi_dicom_intrinsic"]
    for name in EXTERNAL_NAMES:
        executed = {row["id"] for row in evaluation if row[name] in {"passed", "failed"}}
        agreeing = {
            identifier
            for identifier in executed
            if (identifier in intrinsic_set) == (identifier in detection_sets[name])
        }
        agreement[name] = {
            "executed_cases": len(executed),
            "agreeing_cases": len(agreeing),
            "agreement": len(agreeing) / len(executed) if executed else None,
        }

    return validator_counts, unique_counts, agreement


def _domain_passes(package: Path, identifier: str, domain: str) -> bool:
    report = _read_json(
        package / "observed-results" / identifier / "workbench" / "workbench-report.json"
    )
    return report["domains"][domain]["status"] == "passed"


def _read_json(path: Path) -> dict:
    if not path.is_file():
        raise AnalysisError(f"required JSON does not exist: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--adjudications", type=Path, default=DEFAULT_ADJUDICATIONS)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        summary = analyze_challenge(args.package, args.adjudications)
    except (AnalysisError, OSError, ValueError) as exc:
        print(f"analysis failed: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
