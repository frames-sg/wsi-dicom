use wsi_dicom::{
    ExportMetrics, ExportReport, RouteCorpusCoverageReport, RouteCoverageReport, RouteProfileReport,
};

fn format_requested_frames_per_level(max_frames_per_level: u64) -> String {
    if max_frames_per_level == u64::MAX {
        "all".into()
    } else {
        max_frames_per_level.to_string()
    }
}

pub(crate) fn format_report_summary(report: &ExportReport) -> String {
    format_report_summary_with_memory(report, process_resident_memory_bytes())
}

pub(crate) fn format_report_summary_with_memory(
    report: &ExportReport,
    rss_bytes: Option<u64>,
) -> String {
    let metrics = report.metrics;
    format!(
        "wrote {} DICOM instance(s) to {}; frames total={} {} {} write_ms={:.3} rss_mb={}",
        report.instances.len(),
        report.output_dir.display(),
        metrics.routes.total_frames,
        format_route_metric_fields(metrics),
        format_processing_timing_fields(metrics),
        micros_to_ms(metrics.timings.write_micros),
        format_rss_mb(rss_bytes),
    )
}

fn micros_to_ms(micros: u128) -> f64 {
    micros as f64 / 1_000.0
}

fn format_gpu_encode_metrics(metrics: ExportMetrics) -> String {
    let base = format!(
        "gpu_encode_configured_inflight_tiles={} gpu_encode_effective_inflight_tiles={} gpu_encode_max_observed_inflight_tiles={} gpu_encode_configured_memory_mib={} gpu_encode_effective_memory_mib={} gpu_encode_wall_ms={:.3} gpu_encode_effective_parallelism={:.3}",
        metrics.gpu_encode.gpu_encode_configured_inflight_tiles,
        metrics.gpu_encode.gpu_encode_effective_inflight_tiles,
        metrics.gpu_encode.gpu_encode_max_observed_inflight_tiles,
        metrics.gpu_encode.gpu_encode_configured_memory_mib,
        metrics.gpu_encode.gpu_encode_effective_memory_mib,
        micros_to_ms(metrics.gpu_encode.gpu_encode_wall_micros),
        metrics.gpu_encode_effective_parallelism(),
    );
    let stages = format_gpu_encode_stage_metrics(metrics);
    if stages.is_empty() {
        base
    } else {
        format!("{base} {stages}")
    }
}

fn format_gpu_encode_stage_metrics(metrics: ExportMetrics) -> String {
    let has_stage_metrics = metrics.gpu_encode.gpu_encode_plan_micros > 0
        || metrics.gpu_encode.gpu_encode_prepare_submit_micros > 0
        || metrics.gpu_encode.gpu_encode_ht_table_build_micros > 0
        || metrics.gpu_encode.gpu_encode_ht_buffer_allocation_micros > 0
        || metrics.gpu_encode.gpu_encode_ht_command_encode_micros > 0
        || metrics.gpu_encode.gpu_encode_codestream_wait_micros > 0
        || metrics.gpu_encode.gpu_encode_chunk_count > 0
        || metrics.gpu_encode.gpu_encode_tile_count > 0
        || metrics.gpu_encode.gpu_encode_code_block_count > 0;
    if !has_stage_metrics {
        return String::new();
    }
    format!(
        "gpu_encode_plan_ms={:.3} gpu_encode_prepare_submit_ms={:.3} gpu_encode_ht_table_build_ms={:.3} gpu_encode_ht_buffer_allocation_ms={:.3} gpu_encode_ht_command_encode_ms={:.3} gpu_encode_codestream_wait_ms={:.3} gpu_encode_chunk_count={} gpu_encode_tile_count={} gpu_encode_code_block_count={}",
        micros_to_ms(metrics.gpu_encode.gpu_encode_plan_micros),
        micros_to_ms(metrics.gpu_encode.gpu_encode_prepare_submit_micros),
        micros_to_ms(metrics.gpu_encode.gpu_encode_ht_table_build_micros),
        micros_to_ms(metrics.gpu_encode.gpu_encode_ht_buffer_allocation_micros),
        micros_to_ms(metrics.gpu_encode.gpu_encode_ht_command_encode_micros),
        micros_to_ms(metrics.gpu_encode.gpu_encode_codestream_wait_micros),
        metrics.gpu_encode.gpu_encode_chunk_count,
        metrics.gpu_encode.gpu_encode_tile_count,
        metrics.gpu_encode.gpu_encode_code_block_count,
    )
}

fn format_route_metric_fields(metrics: ExportMetrics) -> String {
    let route_passthrough = metrics.route_passthrough_frames();
    let route_unclassified = metrics.route_unclassified_frames();
    format!(
        "route_passthrough={} route_passthrough_pct={:.1} route_gpu_transcode={} route_gpu_transcode_pct={:.1} route_resident_gpu_transcode={} route_partial_gpu_transcode={} route_cpu_fallback={} route_cpu_fallback_pct={:.1} route_unclassified={} cpu_input={} gpu_input_decode={} gpu_encode={} gpu_validation={} gray_frames={} rgb_like_frames={} other_component_frames={} unknown_pixel_profile_frames={} bits8_frames={} bits16_frames={} other_bit_depth_frames={} gpu_input_batches={} gpu_compose_batches={} gpu_encode_batches={} {} gpu_dispatch_ms={:.3} gpu_encode_hardware_ms={:.3} gpu_encode_dispatch_overhead_ms={:.3} auto_probe_frames={} auto_probe_selected_gpu_input={} auto_probe_gpu_batches={} auto_probe_cpu_ms={:.3} auto_probe_gpu_ms={:.3} jpeg_passthrough={} j2k_passthrough={} j2k_direct_htj2k={} jpeg_direct_htj2k_53={} jpeg_direct_htj2k_97={} jpeg_direct_htj2k_rejected={} jpeg_retile={} jpeg_retile_rejected={} jpeg_retile_source_unsupported={} jpeg_retile_geometry_mismatch={} jpeg_retile_profile_unsupported={} jpeg_retile_mcu_invalid={} jpeg_retile_ms={:.3} jpeg_retile_to_htj2k_53={} jpeg_decode_fallback={} jpeg_cpu_encode={} jpeg_metal_encode={}",
        route_passthrough,
        frame_percent(route_passthrough, metrics.routes.total_frames),
        metrics.routes.gpu_transcode_frames,
        frame_percent(metrics.routes.gpu_transcode_frames, metrics.routes.total_frames),
        metrics.routes.resident_gpu_transcode_frames,
        metrics.routes.partial_gpu_transcode_frames,
        metrics.routes.cpu_fallback_frames,
        frame_percent(metrics.routes.cpu_fallback_frames, metrics.routes.total_frames),
        route_unclassified,
        metrics.routes.cpu_input_frames,
        metrics.routes.gpu_input_decode_frames,
        metrics.routes.gpu_encode_frames,
        metrics.routes.gpu_validation_frames,
        metrics.routes.gray_frames,
        metrics.routes.rgb_like_frames,
        metrics.routes.other_component_frames,
        metrics.routes.unknown_pixel_profile_frames,
        metrics.routes.bits8_frames,
        metrics.routes.bits16_frames,
        metrics.routes.other_bit_depth_frames,
        metrics.routes.gpu_input_decode_batches,
        metrics.routes.gpu_compose_batches,
        metrics.routes.gpu_encode_batches,
        format_gpu_encode_metrics(metrics),
        micros_to_ms(metrics.timings.gpu_dispatch_micros),
        micros_to_ms(metrics.gpu_encode.gpu_encode_hardware_micros),
        micros_to_ms(metrics.gpu_encode.gpu_encode_dispatch_overhead_micros),
        metrics.routes.auto_route_probe_frames,
        metrics.routes.auto_route_probe_selected_gpu_input_frames,
        metrics.routes.auto_route_probe_gpu_batches,
        micros_to_ms(metrics.routes.auto_route_probe_cpu_micros),
        micros_to_ms(metrics.routes.auto_route_probe_gpu_micros),
        metrics.routes.jpeg_passthrough_frames,
        metrics.routes.j2k_passthrough_frames,
        metrics.routes.j2k_direct_htj2k_frames,
        metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames,
        metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_97_frames,
        metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_rejected_frames,
        metrics.routes.jpeg_retile_frames,
        metrics.routes.jpeg_retile_rejected_frames,
        metrics.routes.jpeg_retile_source_unsupported_frames,
        metrics.routes.jpeg_retile_geometry_mismatch_frames,
        metrics.routes.jpeg_retile_profile_unsupported_frames,
        metrics.routes.jpeg_retile_mcu_invalid_frames,
        micros_to_ms(metrics.routes.jpeg_retile_us),
        metrics.routes.jpeg_retile_to_htj2k_53_frames,
        metrics.routes.jpeg_decode_fallback_frames,
        metrics.routes.jpeg_cpu_encode_frames,
        metrics.routes.jpeg_metal_encode_frames,
    )
}

fn format_processing_timing_fields(metrics: ExportMetrics) -> String {
    format!(
        "input_decode_ms={:.3} compose_ms={:.3} encode_ms={:.3} validation_ms={:.3}",
        micros_to_ms(metrics.timings.input_decode_micros),
        micros_to_ms(metrics.timings.compose_micros),
        micros_to_ms(metrics.timings.encode_micros),
        micros_to_ms(metrics.timings.validation_micros),
    )
}

pub(crate) fn format_profile_summary(report: &RouteProfileReport) -> String {
    format_profile_summary_with_memory(report, process_resident_memory_bytes())
}

pub(crate) fn format_profile_summary_with_memory(
    report: &RouteProfileReport,
    rss_bytes: Option<u64>,
) -> String {
    let metrics = report.metrics;
    format!(
        "profiled {} level={} transfer_syntax={} requested_frames={} available_frames={} sampled_frames_pct={:.4} frames total={} {} final_byte_ms={:.3} {} elapsed_ms={:.3} rss_mb={}",
        report.source_path.display(),
        report.level,
        report.transfer_syntax_uid,
        report.requested_frames,
        report.available_frames,
        frame_percent(metrics.routes.total_frames, report.available_frames),
        metrics.routes.total_frames,
        format_route_metric_fields(metrics),
        micros_to_ms(metrics.timings.write_micros),
        format_processing_timing_fields(metrics),
        micros_to_ms(report.elapsed_micros),
        format_rss_mb(rss_bytes),
    )
}

pub(crate) fn format_coverage_summary(report: &RouteCoverageReport) -> String {
    format_coverage_summary_with_memory(report, process_resident_memory_bytes())
}

pub(crate) fn format_coverage_summary_with_memory(
    report: &RouteCoverageReport,
    rss_bytes: Option<u64>,
) -> String {
    let metrics = report.metrics;
    format!(
        "covered {} levels={} transfer_syntax={} requested_frames_per_level={} available_frames={} sampled_frames_pct={:.4} complete_frame_coverage={} frames total={} {} final_byte_ms={:.3} {} elapsed_ms={:.3} rss_mb={}",
        report.source_path.display(),
        report.levels.len(),
        report.transfer_syntax_uid,
        format_requested_frames_per_level(report.requested_frames_per_level),
        report.available_frames,
        frame_percent(metrics.routes.total_frames, report.available_frames),
        report.complete_frame_coverage,
        metrics.routes.total_frames,
        format_route_metric_fields(metrics),
        micros_to_ms(metrics.timings.write_micros),
        format_processing_timing_fields(metrics),
        micros_to_ms(report.elapsed_micros),
        format_rss_mb(rss_bytes),
    )
}

pub(crate) fn format_corpus_coverage_summary(report: &RouteCorpusCoverageReport) -> String {
    format_corpus_coverage_summary_with_memory(report, process_resident_memory_bytes())
}

pub(crate) fn format_corpus_coverage_summary_with_memory(
    report: &RouteCorpusCoverageReport,
    rss_bytes: Option<u64>,
) -> String {
    let metrics = report.metrics;
    let common_transfer_syntax = report.transfer_syntax_uid.unwrap_or("mixed");
    let transfer_syntaxes = if report.transfer_syntax_uids.is_empty() {
        "none".into()
    } else {
        report.transfer_syntax_uids.join(",")
    };
    format!(
        "covered_corpus {} sources_considered={} sources_profiled={} failures={} common_transfer_syntax={} transfer_syntaxes={} requested_frames_per_level={} available_frames={} sampled_frames_pct={:.4} complete_frame_coverage={} frames total={} {} final_byte_ms={:.3} {} elapsed_ms={:.3} rss_mb={}",
        report.source_root.display(),
        report.sources_considered,
        report.reports.len(),
        report.failures.len(),
        common_transfer_syntax,
        transfer_syntaxes,
        format_requested_frames_per_level(report.requested_frames_per_level),
        report.available_frames,
        frame_percent(metrics.routes.total_frames, report.available_frames),
        report.complete_frame_coverage,
        metrics.routes.total_frames,
        format_route_metric_fields(metrics),
        micros_to_ms(metrics.timings.write_micros),
        format_processing_timing_fields(metrics),
        micros_to_ms(report.elapsed_micros),
        format_rss_mb(rss_bytes),
    )
}

pub(crate) fn format_sustain_export_iteration_summary(
    iteration: u32,
    iterations: u32,
    report: &ExportReport,
    elapsed_micros: u128,
    rss_bytes: Option<u64>,
    thermal_state: Option<&str>,
    memory_pressure: Option<&str>,
) -> String {
    let metrics = report.metrics;
    let elapsed_seconds = elapsed_micros as f64 / 1_000_000.0;
    let frames_per_sec = if elapsed_seconds > 0.0 {
        metrics.routes.total_frames as f64 / elapsed_seconds
    } else {
        0.0
    };
    let thermal_state = thermal_state
        .map(escape_summary_value)
        .unwrap_or_else(|| "unknown".into());
    let memory_pressure = memory_pressure
        .map(escape_summary_value)
        .unwrap_or_else(|| "unknown".into());
    format!(
        "sustain_iteration={}/{} mode=convert output={} instances={} frames={} frames_per_sec={:.2} {} final_byte_ms={:.3} {} elapsed_ms={:.3} rss_mb={} thermal=\"{}\" memory_pressure=\"{}\"",
        iteration,
        iterations,
        report.output_dir.display(),
        report.instances.len(),
        metrics.routes.total_frames,
        frames_per_sec,
        format_route_metric_fields(metrics),
        micros_to_ms(metrics.timings.write_micros),
        format_processing_timing_fields(metrics),
        micros_to_ms(elapsed_micros),
        format_rss_mb(rss_bytes),
        thermal_state,
        memory_pressure,
    )
}

pub(crate) fn format_sustain_iteration_summary(
    iteration: u32,
    iterations: u32,
    report: &RouteCoverageReport,
    rss_bytes: Option<u64>,
    thermal_state: Option<&str>,
    memory_pressure: Option<&str>,
) -> String {
    let metrics = report.metrics;
    let elapsed_seconds = report.elapsed_micros as f64 / 1_000_000.0;
    let frames_per_sec = if elapsed_seconds > 0.0 {
        metrics.routes.total_frames as f64 / elapsed_seconds
    } else {
        0.0
    };
    let thermal_state = thermal_state
        .map(escape_summary_value)
        .unwrap_or_else(|| "unknown".into());
    let memory_pressure = memory_pressure
        .map(escape_summary_value)
        .unwrap_or_else(|| "unknown".into());
    format!(
        "sustain_iteration={}/{} source={} transfer_syntax={} frames={} available_frames={} sampled_frames_pct={:.4} complete_frame_coverage={} frames_per_sec={:.2} {} final_byte_ms={:.3} {} elapsed_ms={:.3} rss_mb={} thermal=\"{}\" memory_pressure=\"{}\"",
        iteration,
        iterations,
        report.source_path.display(),
        report.transfer_syntax_uid,
        metrics.routes.total_frames,
        report.available_frames,
        frame_percent(metrics.routes.total_frames, report.available_frames),
        report.complete_frame_coverage,
        frames_per_sec,
        format_route_metric_fields(metrics),
        micros_to_ms(metrics.timings.write_micros),
        format_processing_timing_fields(metrics),
        micros_to_ms(report.elapsed_micros),
        format_rss_mb(rss_bytes),
        thermal_state,
        memory_pressure,
    )
}

fn frame_percent(frames: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        frames as f64 * 100.0 / total as f64
    }
}

fn escape_summary_value(value: &str) -> String {
    value.replace('"', "'")
}

fn format_rss_mb(rss_bytes: Option<u64>) -> String {
    rss_bytes
        .map(|bytes| format!("{:.1}", bytes as f64 / (1024.0 * 1024.0)))
        .unwrap_or_else(|| "unknown".into())
}

pub(crate) fn process_thermal_state() -> Option<String> {
    let output = std::process::Command::new("pmset")
        .args(["-g", "therm"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let summary = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    (!summary.is_empty()).then_some(summary)
}

pub(crate) fn process_memory_pressure() -> Option<String> {
    let output = std::process::Command::new("memory_pressure")
        .arg("-Q")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with("System-wide memory free percentage:"))
        .map(str::to_string)
}

pub(crate) fn process_resident_memory_bytes() -> Option<u64> {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let kib = text.trim().parse::<u64>().ok()?;
    kib.checked_mul(1024)
}
