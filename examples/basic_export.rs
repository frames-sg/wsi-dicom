use std::path::PathBuf;

use wsi_dicom::Export;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("slide.ndpi"));
    let output = std::env::args_os()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dicom-out"));

    let report = Export::from_slide(source)
        .to_directory(output)
        .with_research_placeholder_metadata()
        .run()?;

    println!(
        "exported {} instance(s), {} frame(s)",
        report.instances.len(),
        report.metrics.routes.total_frames
    );
    Ok(())
}
