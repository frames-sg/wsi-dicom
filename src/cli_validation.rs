use std::path::PathBuf;

use serde::Serialize;
use wsi_dicom::{
    doctor_dicom_environment, run_dicom_self_test, validate_dicom_path, DoctorOptions,
    DoctorReport, Error, SelfTestOptions, SelfTestReport, ValidationOptions, ValidationProfile,
    ValidationReport,
};

use crate::cli_args::SelfTestArgs;
use crate::cli_output::print_cli_output;

const DOCTOR_REPORT_SCHEMA: &str = "wsi-dicom-doctor-report-v1";
const VALIDATION_REPORT_SCHEMA: &str = "wsi-dicom-validation-report-v1";
const GENERAL_RULE_SET: &str = "wsi-dicom-general-v1";
const CORE_2026C_RULE_SET: &str = "wsi-dicom-core-profile-2026c-v2";

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
        htj2k_decoder,
        max_pixel_frames,
        command_timeout_secs,
        profile,
        max_input_bytes,
        json,
    } = arguments;
    let mut options = ValidationOptions::default();
    options.profile = profile;
    options.max_input_bytes = max_input_bytes;
    options.strict = strict;
    options.dcmvalidate_iod = dcmvalidate_iod;
    options.htj2k_decoder = htj2k_decoder;
    options.max_pixel_frames = max_pixel_frames;
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
    htj2k_decoder: Option<String>,
    json: bool,
) -> Result<(), Error> {
    let mut options = DoctorOptions::default();
    options.strict = strict;
    options.dcmvalidate_iod = dcmvalidate_iod;
    options.htj2k_decoder = htj2k_decoder;
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
    validation.htj2k_decoder = arguments.htj2k_decoder;
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
    format!(
        "validated {} DICOM file(s) from {}; checks passed={} failed={} skipped={}",
        report.files.len(),
        report.input.display(),
        report.passed_checks(),
        report.failed_checks(),
        report.skipped_checks()
    )
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
