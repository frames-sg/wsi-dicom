use std::path::PathBuf;

use wsi_dicom::{
    profile_dicom_route_coverage, ExportOptions, RouteCoverageRequest, RouteProgressSink,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("slide.ndpi"));

    let mut request = RouteCoverageRequest::new(source, ExportOptions::default());
    request.max_frames_per_level = 64;
    request.progress = Some(RouteProgressSink::Stderr);
    let report = profile_dicom_route_coverage(request)?;

    println!(
        "sampled {}/{} frame(s)",
        report.metrics.routes.total_frames, report.available_frames
    );
    Ok(())
}
