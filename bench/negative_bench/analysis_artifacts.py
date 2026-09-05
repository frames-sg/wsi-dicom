"""Render negative-bench analysis artifacts from computed metrics."""

from __future__ import annotations

import csv
import json
import os
from pathlib import Path


EXTERNAL_NAMES = ["dciodvfy", "dcentvfy", "validate_iods", "dcmvalidate"]


def _write_artifacts(
    package: Path,
    rows: list[dict],
    domain_rows: list[dict],
    summary: dict,
    observed_adjudications: dict[str, dict[str, str]],
) -> None:
    analysis = package / "analysis"
    analysis.mkdir(exist_ok=True)
    _write_json(analysis / "case-validator-matrix.json", rows)
    _write_csv(analysis / "case-validator-matrix.csv", rows)
    _write_json(analysis / "domain-metrics.json", domain_rows)
    _write_csv(analysis / "domain-metrics.csv", domain_rows)
    _write_json(analysis / "summary.json", summary)

    disagreements = []
    for row in rows:
        statuses = [
            row["intrinsic_detected"],
            *[row[name] == "failed" for name in EXTERNAL_NAMES if row[name] in {"passed", "failed"}],
        ]
        if len(set(statuses)) > 1 or row["unexpected_intrinsic_rules"] or (
            row["cohort"] == "evaluation-v1" and not row["expected_rule_localized"]
        ):
            disagreements.append(row)
    _write_csv(analysis / "disagreements.csv", disagreements)
    _write_json(analysis / "disagreements.json", disagreements)

    adjudication = []
    for row in disagreements:
        observed = observed_adjudications.get(
            row["id"],
            {
                "disposition": "validator_disagreement",
                "rationale": "Raw validator output retained; expected result was not changed.",
            },
        )
        adjudication.append(
            {
                "case_id": row["id"],
                "expected_rule_localized": row["expected_rule_localized"],
                "unexpected_intrinsic_rules": row["unexpected_intrinsic_rules"],
                "intrinsic": "failed" if row["intrinsic_detected"] else "passed",
                "dciodvfy": row["dciodvfy"],
                "dcentvfy": row["dcentvfy"],
                "validate_iods": row["validate_iods"],
                "dcmvalidate": row["dcmvalidate"],
                "adjudication_status": "adjudicated_observed_v1",
                "disposition": observed["disposition"],
                "rationale": observed["rationale"],
            }
        )
    _write_csv(package / "adjudication.csv", adjudication)

    text = _results_markdown(summary, domain_rows)
    (analysis / "USCAP-results.md").write_text(text, encoding="utf-8")
    _write_validator_svg(analysis / "validator-detection.svg", summary)


def _results_markdown(summary: dict, domain_rows: list[dict]) -> str:
    sensitivity = 100 * summary["defect_detection_sensitivity"]
    sensitivity_ci = [100 * value for value in summary["defect_detection_sensitivity_ci95"]]
    specificity = 100 * summary["valid_control_specificity"]
    specificity_ci = [100 * value for value in summary["valid_control_specificity_ci95"]]
    localization = 100 * summary["rule_localization_accuracy"]
    validator_counts = summary["validator_detection_counts"]
    agreement = summary["intrinsic_external_agreement"]
    unique = summary["validator_unique_detection_counts"]
    domain_sensitivity = ", ".join(
        f"{row['domain']} ({row['detected']}/{row['evaluation_cases']})"
        for row in domain_rows
    )
    domain_localization = ", ".join(
        f"{row['domain']} ({row['domain_localized']}/{row['evaluation_cases']})"
        for row in domain_rows
    )
    total = summary["evaluation_cases"]
    return f"""# WSI-DICOM Negative Bench v1 results

WSI-DICOM Bench detected {summary['bench_detected']} of {summary['evaluation_cases']} locked single-defect cases (sensitivity {sensitivity:.1f}%, exact 95% CI {sensitivity_ci[0]:.1f}–{sensitivity_ci[1]:.1f}%) and accepted {summary['valid_controls_accepted']} of {summary['valid_controls']} unmodified controls (valid-control specificity {specificity:.1f}%, exact 95% CI {specificity_ci[0]:.1f}–{specificity_ci[1]:.1f}%). The prespecified rule and domain were localized in {summary['rule_localized_detected_cases']} of {summary['evaluation_cases']} cases ({localization:.1f}%). Intrinsic rules alone detected {summary['intrinsic_detected']} of {summary['evaluation_cases']} cases.

Detected cases by domain were {domain_sensitivity}. Domain-localized cases were {domain_localization}. Undetected cases were {', '.join(summary['cases_not_detected']) or 'none'}. Cases without the prespecified rule localization were {', '.join(summary['cases_not_rule_localized']) or 'none'}.

Independent validators detected {validator_counts['dciodvfy']}/{total} cases with dciodvfy, {validator_counts['dcentvfy']}/{total} with dcentvfy, and {validator_counts['validate_iods']}/{total} with validate_iods. Binary agreement with intrinsic detection was {agreement['dciodvfy']['agreeing_cases']}/{agreement['dciodvfy']['executed_cases']}, {agreement['dcentvfy']['agreeing_cases']}/{agreement['dcentvfy']['executed_cases']}, and {agreement['validate_iods']['agreeing_cases']}/{agreement['validate_iods']['executed_cases']}, respectively. Intrinsic rules uniquely detected {unique['wsi_dicom_intrinsic']} cases; dciodvfy uniquely detected {unique['dciodvfy']}. `dcmvalidate` was not reproducibly configured and is reported as unavailable, not passed.

There were {summary['execution_failures']} execution failures and {summary['unmapped_findings']} unmapped findings. Total sequential workbench runtime was {summary['total_runtime_seconds']:.2f} s. These are engineered challenge cases, not a prevalence sample. Real-slide holdout material is excluded pending provenance, licensing, PHI, and redistribution review.
"""


def _write_validator_svg(path: Path, summary: dict) -> None:
    labels = ["WSI-DICOM intrinsic", "dciodvfy", "dcentvfy", "validate_iods"]
    keys = ["wsi_dicom_intrinsic", "dciodvfy", "dcentvfy", "validate_iods"]
    counts = summary["validator_detection_counts"]
    total = summary["evaluation_cases"]
    scale = 360 / max(total, 1)
    bars = []
    for index, (label, key) in enumerate(zip(labels, keys, strict=True)):
        y = 55 + index * 54
        width = scale * counts[key]
        bars.append(
            f'<text x="12" y="{y + 17}" class="label">{label}</text>'
            f'<rect x="165" y="{y}" width="{width}" height="28" rx="3" />'
            f'<text x="{175 + width}" y="{y + 19}" class="value">{counts[key]}/{total}</text>'
        )
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="620" height="300" viewBox="0 0 620 300">
<style>text{{font-family:Helvetica,Arial,sans-serif;fill:#18212b}}.title{{font-size:20px;font-weight:700}}.label{{font-size:14px}}.value{{font-size:14px;font-weight:700}}rect{{fill:#2c6e9b}}</style>
<rect width="620" height="300" fill="white"/><text x="12" y="28" class="title">Locked evaluation-case detections</text>
{''.join(bars)}<text x="165" y="285" class="label">Number detected (n={total} engineered defects)</text></svg>\n"""
    path.write_text(svg, encoding="utf-8")


def _write_json(path: Path, value: object) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def _write_csv(path: Path, rows: list[dict]) -> None:
    if not rows:
        path.write_text("", encoding="utf-8")
        return
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    os.replace(temporary, path)
