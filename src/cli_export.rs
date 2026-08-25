use std::path::PathBuf;

use wsi_dicom::{run_export_workflow, ExportReport, ExportWorkflowRequest, MetadataInput};

use crate::cli_args::{AnnotationCliArgs, ExportCliArgs};
use crate::cli_output::{print_cli_output, print_json_line};
use crate::cli_report::{format_report_summary, format_sustain_export_iteration_summary};
use crate::cli_sustain::{run_sustained, SustainConfig};

pub(crate) struct ConvertRequest {
    pub(crate) source: PathBuf,
    pub(crate) out: PathBuf,
    pub(crate) metadata: Option<PathBuf>,
    pub(crate) research_placeholder: bool,
    pub(crate) export_args: ExportCliArgs,
    pub(crate) annotation_args: AnnotationCliArgs,
    pub(crate) level: Option<u32>,
    pub(crate) json: bool,
}

pub(crate) fn handle_convert(request: ConvertRequest) -> Result<(), Box<dyn std::error::Error>> {
    let ConvertRequest {
        source,
        out,
        metadata,
        research_placeholder,
        export_args,
        annotation_args,
        level,
        json,
    } = request;
    let metadata = MetadataInput::from_parts(metadata, research_placeholder)?;
    let color_management = export_args.color_management.resolve()?;
    let mut workflow = ExportWorkflowRequest::new(
        source,
        out,
        export_args.options()?,
        color_management,
        metadata,
    );
    workflow.level_filter = level;
    workflow.annotations = annotation_args.options()?;
    let report = run_export_workflow(workflow)?;
    print_cli_output(json, &report.export, format_report_summary)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_sustain_convert(
    source: PathBuf,
    out: PathBuf,
    metadata: Option<PathBuf>,
    research_placeholder: bool,
    export_args: ExportCliArgs,
    level: Option<u32>,
    iterations: u32,
    interval_ms: u64,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = SustainConfig::new(iterations, interval_ms)?;
    let metadata = MetadataInput::from_parts(metadata, research_placeholder)?;
    let options = export_args.options()?;
    let color_management = export_args.color_management.resolve()?;
    run_sustained(
        config,
        |iteration| {
            let output_dir = out.join(format!("iteration-{iteration:04}"));
            let mut workflow = ExportWorkflowRequest::new(
                source.clone(),
                output_dir,
                options.clone(),
                color_management.clone(),
                metadata.clone(),
            );
            workflow.level_filter = level;
            run_export_workflow(workflow)
                .map(|report| report.export)
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
        },
        |iteration, report| {
            if json {
                print_json_line(&SustainExportIterationJson {
                    mode: "convert",
                    iteration: iteration.iteration,
                    iterations: iteration.iterations,
                    elapsed_micros: iteration.elapsed_micros,
                    rss_bytes: iteration.rss_bytes,
                    thermal_state: iteration.thermal_state.as_deref(),
                    memory_pressure: iteration.memory_pressure.as_deref(),
                    report,
                })
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)?;
            } else {
                println!(
                    "{}",
                    format_sustain_export_iteration_summary(
                        iteration.iteration,
                        iteration.iterations,
                        report,
                        iteration.elapsed_micros,
                        iteration.rss_bytes,
                        iteration.thermal_state.as_deref(),
                        iteration.memory_pressure.as_deref(),
                    )
                );
            }
            Ok(())
        },
    )
}

#[allow(clippy::too_many_arguments)]
#[derive(serde::Serialize)]
struct SustainExportIterationJson<'a> {
    mode: &'static str,
    iteration: u32,
    iterations: u32,
    elapsed_micros: u128,
    rss_bytes: Option<u64>,
    thermal_state: Option<&'a str>,
    memory_pressure: Option<&'a str>,
    report: &'a ExportReport,
}
