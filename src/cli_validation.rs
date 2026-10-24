use std::path::PathBuf;

use serde::Serialize;
use wsi_dicom::{
    check_dicomweb_server, doctor_dicom_environment, requirement_catalog, run_dicom_self_test,
    validate_dicom_json_path, validate_dicom_path, DicomJsonOptions, DicomWebOptions,
    DicomWebReport, DicomWebStatus, DoctorOptions, DoctorReport, Error, RequirementDisposition,
    RequirementEntry, SelfTestOptions, SelfTestReport, ValidationOptions, ValidationProfile,
    ValidationReport, ValidationStatus, DICOM_EDITION,
};

use crate::cli_args::{DicomwebArgs, SelfTestArgs, ValidateJsonArgs};
use crate::cli_output::print_cli_output;

const DOCTOR_REPORT_SCHEMA: &str = "wsi-dicom-doctor-report-v1";
const VALIDATION_REPORT_SCHEMA: &str = "wsi-dicom-validation-report-v1";
const REQUIREMENTS_SCHEMA: &str = "wsi-dicom-requirements-v1";
const JSON_VALIDATION_REPORT_SCHEMA: &str = "wsi-dicom-json-validation-report-v1";
const DICOMWEB_REPORT_SCHEMA: &str = "wsi-dicom-dicomweb-report-v1";
const GENERAL_RULE_SET: &str = "wsi-dicom-general-v4";
const CORE_RULE_SET: &str = "wsi-dicom-core-profile-v5";

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
    dicom_edition: &'static str,
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
        dicom_edition: DICOM_EDITION,
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

pub(crate) fn handle_validate_json(arguments: ValidateJsonArgs) -> Result<(), Error> {
    let mut options = DicomJsonOptions::default();
    options.validation.profile = arguments.profile;
    options.validation.conformance = arguments.conformance.level();
    options.validation.max_input_bytes = arguments.max_input_bytes;
    options.transfer_syntax = arguments.transfer_syntax;
    options.bulkdata_base_url = arguments.bulkdata_base_url;
    if arguments.fetch_bulkdata {
        options.fetch_bulkdata = Some(arguments.http.options()?);
    }
    let report = validate_dicom_json_path(&arguments.path, &options)?;
    let cli_report = ValidationCliReport {
        schema_version: JSON_VALIDATION_REPORT_SCHEMA,
        rule_set_id: rule_set_id(options.validation.profile),
        dicom_edition: DICOM_EDITION,
        report: &report,
    };
    print_cli_output(arguments.json, &cli_report, |cli_report| {
        format_json_validation_summary(cli_report.report)
    })?;
    if report.has_failures() {
        Err(Error::Validation {
            reason: format!("{} validation check(s) failed", report.failed_checks()),
        })
    } else {
        Ok(())
    }
}

/// Counts, then each check that did not pass, with its message.
fn format_json_validation_summary(report: &ValidationReport) -> String {
    let mut summary = format!(
        "validated DICOM JSON {}; checks passed={} failed={} skipped={}",
        report.input.display(),
        report.passed_checks(),
        report.failed_checks(),
        report.skipped_checks()
    );
    append_unpassed(&mut summary, &report.checks);
    summary
}

fn append_unpassed(summary: &mut String, checks: &[wsi_dicom::ValidationCheck]) {
    for check in checks {
        let status = match check.status {
            ValidationStatus::Failed => "failed",
            ValidationStatus::Skipped => "skipped",
            _ => continue,
        };
        let path = check
            .path
            .as_ref()
            .map(|path| format!(" {}", path.display()))
            .unwrap_or_default();
        summary.push_str(&format!(
            "\n  {status} {}{path}: {}",
            check.name, check.message
        ));
    }
}

#[derive(Serialize)]
struct DicomWebCliReport<'a> {
    schema_version: &'static str,
    dicom_edition: &'static str,
    #[serde(flatten)]
    report: &'a DicomWebReport,
}

pub(crate) fn handle_dicomweb(arguments: DicomwebArgs) -> Result<(), Error> {
    let mut options = DicomWebOptions::default();
    options.base_url = arguments.base_url;
    options.study = arguments.study;
    options.series = arguments.series;
    options.instance = arguments.instance;
    options.frames = arguments.frames;
    options.skip = arguments.skip;
    options.store_file = arguments.allow_store;
    options.max_requests = usize::try_from(arguments.max_requests).unwrap_or(usize::MAX);
    options.http = arguments.http.options()?;
    options.validation.profile = arguments.profile;
    options.validation.conformance = arguments.conformance.level();
    let report = check_dicomweb_server(&options)?;
    let cli_report = DicomWebCliReport {
        schema_version: DICOMWEB_REPORT_SCHEMA,
        dicom_edition: DICOM_EDITION,
        report: &report,
    };
    print_cli_output(arguments.json, &cli_report, |cli_report| {
        format_dicomweb_summary(cli_report.report)
    })?;
    if report.has_failures() {
        Err(Error::Validation {
            reason: format!(
                "{} DICOMweb check(s) and {} check(s) of the retrieved objects failed",
                report.failed_checks(),
                report.objects.failed_checks()
            ),
        })
    } else {
        Ok(())
    }
}

/// Each DICOMweb check with its status, then the object checks that did not
/// pass.
fn format_dicomweb_summary(report: &DicomWebReport) -> String {
    let mut summary = format!(
        "checked DICOMweb origin server {} ({} requests); checks passed={} failed={} unsupported={} not-run={}; retrieved-object checks passed={} failed={} skipped={}",
        report.base_url,
        report.exchanges.len(),
        report.count(DicomWebStatus::Passed),
        report.count(DicomWebStatus::Failed),
        report.count(DicomWebStatus::Unsupported),
        report.count(DicomWebStatus::NotRun),
        report.objects.passed_checks(),
        report.objects.failed_checks(),
        report.objects.skipped_checks()
    );
    for check in &report.checks {
        let status = match check.status {
            DicomWebStatus::Passed => "passed",
            DicomWebStatus::Failed => "failed",
            DicomWebStatus::Unsupported => "unsupported",
            DicomWebStatus::NotRun => "not-run",
            _ => "other",
        };
        summary.push_str(&format!(
            "\n  {status} {} [{}]: {}",
            check.name, check.section, check.message
        ));
    }
    append_unpassed(&mut summary, &report.objects.checks);
    summary
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
    dicom_edition: &'static str,
    requirements: Vec<RequirementEntry>,
}

pub(crate) fn handle_requirements(json: bool) -> Result<(), Error> {
    let report = RequirementsCliReport {
        schema_version: REQUIREMENTS_SCHEMA,
        dicom_edition: DICOM_EDITION,
        requirements: requirement_catalog(),
    };
    print_cli_output(json, &report, |report| {
        format_requirements_summary(&report.requirements)
    })
}

/// Counts per IOD and disposition, then per Part and disposition, then each
/// statement no check enforces yet in some IOD.
fn format_requirements_summary(requirements: &[RequirementEntry]) -> String {
    let kind = |disposition: &RequirementDisposition| match disposition {
        RequirementDisposition::Tables => "generated tables",
        RequirementDisposition::Checked(_) => "checked",
        RequirementDisposition::Unchecked(_) => "not yet checked",
        RequirementDisposition::OutsideScope(_) => "outside the IOD's scope",
        RequirementDisposition::Behavior(_) => "application behavior",
        RequirementDisposition::Undecidable(_) => "undecidable from the objects",
        _ => "other",
    };
    let in_iod = |entry: &RequirementEntry, iod: &str| {
        entry
            .iod_dispositions
            .get(iod)
            .copied()
            .unwrap_or(entry.disposition)
    };
    let mut iods = Vec::<&str>::new();
    for iod in requirements
        .iter()
        .flat_map(|entry| entry.iods.iter().copied())
    {
        if !iods.contains(&iod) {
            iods.push(iod);
        }
    }
    let mut summary = format!(
        "{} normative statements of DICOM {DICOM_EDITION} in the scope of {} pathology-slide IODs",
        requirements.len(),
        iods.len()
    );
    for &iod in &iods {
        let mut counts = std::collections::BTreeMap::<&str, usize>::new();
        let mut total = 0;
        for entry in requirements
            .iter()
            .filter(|entry| entry.iods.contains(&iod))
        {
            *counts.entry(kind(&in_iod(entry, iod))).or_insert(0) += 1;
            total += 1;
        }
        let counts = counts
            .into_iter()
            .map(|(kind, count)| format!("{kind} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        summary.push_str(&format!("\n  {iod}: {total} statements: {counts}"));
    }
    let mut counts = std::collections::BTreeMap::<(&str, &str), usize>::new();
    for entry in requirements {
        *counts
            .entry((entry.part, kind(&entry.disposition)))
            .or_insert(0) += 1;
    }
    summary.push_str("\nper Part, as in VL WSI objects or in every IOD of the statement:");
    for ((part, kind), count) in counts {
        summary.push_str(&format!("\n  {part} {kind}: {count}"));
    }
    for entry in requirements {
        let unchecked = entry
            .iods
            .iter()
            .copied()
            .filter_map(|iod| match in_iod(entry, iod) {
                RequirementDisposition::Unchecked(family) => Some(format!("{iod} {family}")),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !unchecked.is_empty() {
            summary.push_str(&format!(
                "\nnot yet checked ({}): {} {}: {}",
                unchecked.join(", "),
                entry.part,
                entry.section,
                entry.text
            ));
        }
    }
    summary
}

fn rule_set_id(profile: ValidationProfile) -> &'static str {
    if profile == ValidationProfile::Core {
        CORE_RULE_SET
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
