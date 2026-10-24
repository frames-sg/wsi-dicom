use std::path::PathBuf;

use serde::Serialize;
use wsi_dicom::{
    doctor_dicom_environment, requirement_catalog, run_dicom_self_test, validate_dicom_path,
    DoctorOptions, DoctorReport, Error, RequirementDisposition, RequirementEntry, SelfTestOptions,
    SelfTestReport, ValidationOptions, ValidationProfile, ValidationReport,
};

use crate::cli_args::SelfTestArgs;
use crate::cli_output::print_cli_output;

const DOCTOR_REPORT_SCHEMA: &str = "wsi-dicom-doctor-report-v1";
const VALIDATION_REPORT_SCHEMA: &str = "wsi-dicom-validation-report-v1";
const REQUIREMENTS_SCHEMA: &str = "wsi-dicom-requirements-v1";
const GENERAL_RULE_SET: &str = "wsi-dicom-general-v3";
const CORE_2026C_RULE_SET: &str = "wsi-dicom-core-profile-2026c-v4";

#[derive(Serialize)]
struct DoctorCliReport<'a> {
    schema_version: &'static str,
    #[serde(flatten)]
    report: &'a DoctorReport,
}

#[derive(Serialize)]
struct ValidationCliReport<'a> {
    schema_version: &'static str,
    rule_set_id: &'static str,
    #[serde(flatten)]
    report: &'a ValidationReport,
}

pub(crate) fn handle_validate(arguments: crate::cli_args::ValidateArgs) -> Result<(), Error> {
    let crate::cli_args::ValidateArgs {
        path,
        strict,
        dcmvalidate_iod,
        command_timeout_secs,
        profile,
        max_input_bytes,
        conformance,
        json,
    } = arguments;
    let mut options = ValidationOptions::default();
    options.profile = profile;
    options.conformance = conformance.level();
    options.max_input_bytes = max_input_bytes;
    options.strict = strict;
    options.dcmvalidate_iod = dcmvalidate_iod;
    options.command_timeout_secs = command_timeout_secs;
    let report = validate_dicom_path(&path, &options)?;
    let has_failures = report.has_failures();
    let failed_checks = report.failed_checks();
    let cli_report = ValidationCliReport {
        schema_version: VALIDATION_REPORT_SCHEMA,
        rule_set_id: rule_set_id(options.profile),
        report: &report,
    };
    print_cli_output(json, &cli_report, |cli_report| {
        format_validation_summary(cli_report.report)
    })?;
    if has_failures {
        Err(Error::Validation {
            reason: format!("{failed_checks} validation check(s) failed"),
        })
    } else {
        Ok(())
    }
}

pub(crate) fn handle_doctor(
    strict: bool,
    dcmvalidate_iod: Option<PathBuf>,
    json: bool,
) -> Result<(), Error> {
    let mut options = DoctorOptions::default();
    options.strict = strict;
    options.dcmvalidate_iod = dcmvalidate_iod;
    let report = doctor_dicom_environment(&options);
    let has_failures = report.has_failures();
    let failed_tools = report.failed_tools();
    let cli_report = DoctorCliReport {
        schema_version: DOCTOR_REPORT_SCHEMA,
        report: &report,
    };
    print_cli_output(json, &cli_report, |cli_report| {
        format_doctor_summary(cli_report.report)
    })?;
    if has_failures {
        Err(Error::Validation {
            reason: format!("{failed_tools} doctor check(s) failed"),
        })
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
struct RequirementsCliReport {
    schema_version: &'static str,
    requirements: Vec<RequirementEntry>,
}

pub(crate) fn handle_requirements(json: bool) -> Result<(), Error> {
    let report = RequirementsCliReport {
        schema_version: REQUIREMENTS_SCHEMA,
        requirements: requirement_catalog(),
    };
    print_cli_output(json, &report, |report| {
        format_requirements_summary(&report.requirements)
    })
}

/// Counts per Part and disposition, then each statement no check enforces yet.
fn format_requirements_summary(requirements: &[RequirementEntry]) -> String {
    let kind = |disposition: &RequirementDisposition| match disposition {
        RequirementDisposition::Tables => "generated tables",
        RequirementDisposition::Checked(_) => "checked",
        RequirementDisposition::Unchecked(_) => "not yet checked",
        RequirementDisposition::OutsideScope(_) => "outside the VL WSI scope",
        RequirementDisposition::Behavior(_) => "application behavior",
        RequirementDisposition::Undecidable(_) => "undecidable from the objects",
        _ => "other",
    };
    let mut counts = std::collections::BTreeMap::<(&str, &str), usize>::new();
    for entry in requirements {
        *counts
            .entry((entry.part, kind(&entry.disposition)))
            .or_insert(0) += 1;
    }
    let mut summary = format!(
        "{} normative statements in the VL WSI scope",
        requirements.len()
    );
    for ((part, kind), count) in counts {
        summary.push_str(&format!("\n  {part} {kind}: {count}"));
    }
    for entry in requirements {
        if let RequirementDisposition::Unchecked(family) = entry.disposition {
            summary.push_str(&format!(
                "\nnot yet checked ({family}): {} {}: {}",
                entry.part, entry.section, entry.text
            ));
        }
    }
    summary
}

fn rule_set_id(profile: ValidationProfile) -> &'static str {
    if profile == ValidationProfile::Core2026c {
        CORE_2026C_RULE_SET
    } else {
        GENERAL_RULE_SET
    }
}

pub(crate) fn handle_self_test(arguments: SelfTestArgs) -> Result<(), Error> {
    let mut validation = ValidationOptions::default();
    validation.strict = arguments.strict;
    validation.dcmvalidate_iod = arguments.dcmvalidate_iod;
    validation.command_timeout_secs = arguments.command_timeout_secs;
    let mut options = SelfTestOptions::default();
    options.output_dir = arguments.out;
    options.keep_output = arguments.keep_output;
    options.validation = validation;
    let report = run_dicom_self_test(options)?;
    let has_failures = report.validation_report.has_failures();
    let failed_checks = report.validation_report.failed_checks();
    print_cli_output(arguments.json, &report, format_self_test_summary)?;
    if has_failures {
        Err(Error::Validation {
            reason: format!("{failed_checks} self-test validation check(s) failed"),
        })
    } else {
        Ok(())
    }
}

fn format_validation_summary(report: &ValidationReport) -> String {
    let mut summary = format!(
        "validated {} DICOM file(s) from {}; checks passed={} failed={} skipped={}",
        report.files.len(),
        report.input.display(),
        report.passed_checks(),
        report.failed_checks(),
        report.skipped_checks()
    );
    let optional = &report.optional;
    append_counts(
        &mut summary,
        "optional VL WSI items present",
        optional.iter().flat_map(|object| &object.present),
    );
    append_counts(
        &mut summary,
        "optional VL WSI items absent",
        optional.iter().flat_map(|object| &object.absent),
    );
    append_counts(
        &mut summary,
        "suggested codes and templates not followed",
        optional.iter().flat_map(|object| &object.advisories),
    );
    summary
}

/// Appends each distinct entry with the number of files that list it.
fn append_counts<'a>(summary: &mut String, title: &str, entries: impl Iterator<Item = &'a String>) {
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for entry in entries {
        *counts.entry(entry).or_insert(0) += 1;
    }
    if counts.is_empty() {
        return;
    }
    summary.push_str(&format!(
        "\n{title} ({} distinct, not failures; --required skips this):",
        counts.len()
    ));
    for (entry, files) in counts {
        summary.push_str(&format!("\n  {entry} [{files} file(s)]"));
    }
}

fn format_doctor_summary(report: &DoctorReport) -> String {
    format!(
        "checked DICOM tooling; available={} failed={} missing={} skipped={}",
        report.available_tools(),
        report.failed_tools(),
        report.missing_tools(),
        report.skipped_tools()
    )
}

fn format_self_test_summary(report: &SelfTestReport) -> String {
    format!(
        "self-test wrote {} DICOM file(s) to {}; validation passed={} failed={} skipped={}; kept_output={}",
        report.export_report.instances.len(),
        report.output_dir.display(),
        report.validation_report.passed_checks(),
        report.validation_report.failed_checks(),
        report.validation_report.skipped_checks(),
        report.kept_output
    )
}
