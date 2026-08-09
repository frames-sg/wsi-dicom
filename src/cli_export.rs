use std::path::PathBuf;

use wsi_dicom::{Error, Export, ExportReport, MetadataSource};

use crate::cli_args::ExportCliArgs;
use crate::cli_output::{print_cli_output, print_json_line};
use crate::cli_report::{
    format_report_summary, format_sustain_export_iteration_summary, process_memory_pressure,
    process_resident_memory_bytes, process_thermal_state,
};
use crate::sleep_between_iterations;
use crate::time;

pub(crate) fn handle_convert(
    source: PathBuf,
    out: PathBuf,
    metadata: Option<PathBuf>,
    research_placeholder: bool,
    export_args: ExportCliArgs,
    level: Option<u32>,
    json: bool,
) -> Result<(), Error> {
    let metadata = load_metadata_source(metadata, research_placeholder)?;
    let color_management = export_args.color_management.resolve()?;
    let mut export = Export::from_slide(source)
        .to_directory(out)
        .with_metadata(metadata)
        .with_options(export_args.options()?)
        .color_management(color_management);
    if let Some(level) = level {
        export = export.level(level);
    }
    let report = export.run()?;
    print_cli_output(json, &report, format_report_summary)
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
) -> Result<(), Error> {
    if iterations == 0 {
        return Err(Error::Unsupported {
            reason: "sustain-convert requires iterations > 0".into(),
        });
    }
    let metadata = load_metadata_source(metadata, research_placeholder)?;
    let options = export_args.options()?;
    let color_management = export_args.color_management.resolve()?;
    for iteration in 1..=iterations {
        let output_dir = out.join(format!("iteration-{iteration:04}"));
        let started = std::time::Instant::now();
        let mut export = Export::from_slide(source.clone())
            .to_directory(output_dir)
            .with_metadata(metadata.clone())
            .with_options(options.clone())
            .color_management(color_management.clone());
        if let Some(level) = level {
            export = export.level(level);
        }
        let report = export.run()?;
        let elapsed_micros = time::duration_as_reported_micros(started.elapsed());
        let thermal_state = process_thermal_state();
        let memory_pressure = process_memory_pressure();
        let rss_bytes = process_resident_memory_bytes();
        if json {
            print_json_line(&SustainExportIterationJson {
                mode: "convert",
                iteration,
                iterations,
                elapsed_micros,
                rss_bytes,
                thermal_state: thermal_state.as_deref(),
                memory_pressure: memory_pressure.as_deref(),
                report: &report,
            })?;
        } else {
            println!(
                "{}",
                format_sustain_export_iteration_summary(
                    iteration,
                    iterations,
                    &report,
                    elapsed_micros,
                    rss_bytes,
                    thermal_state.as_deref(),
                    memory_pressure.as_deref(),
                )
            );
        }
        sleep_between_iterations(interval_ms, iteration, iterations);
    }
    Ok(())
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

pub(crate) fn load_metadata_source(
    metadata_path: Option<PathBuf>,
    research_placeholder: bool,
) -> Result<MetadataSource, Error> {
    if metadata_path.is_some() && research_placeholder {
        return Err(Error::Metadata {
            reason: "--metadata cannot be combined with --research-placeholder".into(),
        });
    }
    if research_placeholder {
        return Ok(MetadataSource::ResearchPlaceholder);
    }

    let Some(path) = metadata_path else {
        return Err(Error::Metadata {
            reason: "provide --metadata <json> or explicitly pass --research-placeholder".into(),
        });
    };

    MetadataSource::from_json_file(path)
}
