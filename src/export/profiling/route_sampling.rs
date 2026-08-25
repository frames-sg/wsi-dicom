use std::path::Path;

use j2k_jpeg::JpegBackend;
use wsi_rs::Slide;

use super::super::ensure_consistent_pixel_profile;
use super::super::frame_region::FrameRectGrid;
use super::super::j2k_policy::reject_lossy_j2k_lossless_fallback;
use super::super::jpeg_baseline::{
    encode_blank_jpeg_baseline_frame, jpeg_baseline_route_frame_geometry,
    raw_rgb_passthrough_has_no_geometry_fallback, JpegBaselineFrameLocation,
    JpegBaselinePlannedFrame,
};
use super::super::jpeg_baseline_pipeline::{
    jpeg_backend_uses_device, jpeg_baseline_fallback_run, plan_jpeg_baseline_row,
    prepare_jpeg_baseline_fallback_batch, record_jpeg_retile_rejections,
    take_consistent_jpeg_baseline_fallback_frame, EncodedJpegBaselineFrame,
    JpegBaselineCpuEncodeSettings, JpegBaselineFallbackBatchRequest, JpegBaselineRowPlanRequest,
};
use super::super::lossless_j2k_direct_routes::{
    encode_direct_lossless_j2k_routes, try_profile_existing_lossless_j2k_frame,
    ExistingLosslessJ2kFrameContext,
};
use super::super::lossless_j2k_pipeline::{
    encode_lossless_j2k_cpu_fallback_after_routes, record_resolved_lossless_j2k_fallback_frame,
    resolve_lossless_j2k_fallback_frame, LosslessJ2kBatchContext, LosslessJ2kRoutePipeline,
};
use super::super::lossless_j2k_plan::{plan_lossless_j2k_frames, LosslessJ2kPlanRequest};
use super::super::route_plan::{
    incompatible_frame_route, PlannedFrameRoute, RouteExecutionContext,
};
use super::super::tile_grid::TileGrid;
use super::{check_route_level_deadline, RouteLevelDeadline};
use crate::error::Error;
use crate::options::NormalizedExportOptions;
use crate::report::ExportMetrics;
use crate::routing::{j2k_family_passthrough_probe_allowed, j2k_route_tile_size};

#[cfg(all(feature = "metal", target_os = "macos"))]
use super::super::lossless_j2k_pipeline::route_lossless_j2k_metal_input_runs;
#[cfg(all(feature = "metal", target_os = "macos"))]
use super::super::metal_input::MetalInputTileReader;

pub(super) fn profile_lossless_j2k_routes(
    slide: &Slide,
    _source_path: &Path,
    options: NormalizedExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
    deadline: Option<RouteLevelDeadline>,
) -> Result<ExportMetrics, Error> {
    let level_idx = location.level_idx;
    let tile_size = j2k_route_tile_size(
        options.semantics.tile_size,
        options.semantics.transfer_syntax,
        level,
    )?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let grid = TileGrid::square(matrix_columns, matrix_rows, tile_size)?;
    let tiles_down = grid.tiles_down;
    let route_scope_frames = grid.frame_count_u64()?.min(max_frames);
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let route_scope_frames_usize =
        usize::try_from(route_scope_frames).map_err(|_| Error::Unsupported {
            reason: "route profile frame count exceeds platform addressable memory".into(),
        })?;
    let LosslessJ2kRoutePipeline {
        encoder: mut j2k_encoder,
        #[cfg(all(feature = "metal", target_os = "macos"))]
        mut metal_input,
        mut metrics,
        mut pixel_profile,
        mut jpeg_direct_encoder,
    } = LosslessJ2kRoutePipeline::new(_source_path, &options, location, route_scope_frames)?;
    let mut remaining = max_frames;
    let allow_passthrough_probe =
        j2k_family_passthrough_probe_allowed(_source_path, options.semantics.transfer_syntax);
    let route_context = RouteExecutionContext::new(
        options.semantics.transfer_syntax,
        options.execution.encode_backend,
    );

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        check_route_level_deadline(deadline, level_idx)?;
        let row_tile_count = grid.row_tile_count(row)?.min(remaining);
        let planned = plan_lossless_j2k_frames(
            slide,
            LosslessJ2kPlanRequest {
                location,
                start_row: row,
                row_count: 1,
                start_col: 0,
                tile_count: row_tile_count,
                grid: FrameRectGrid {
                    matrix_columns,
                    matrix_rows,
                    frame_columns: tile_size,
                    frame_rows: tile_size,
                },
                transfer_syntax: options.semantics.transfer_syntax,
                allow_passthrough_probe,
            },
        )?;
        let batch_context = LosslessJ2kBatchContext {
            slide,
            level,
            planned: &planned,
            options: &options,
            location,
            tile_size,
        };
        let mut direct_routes =
            encode_direct_lossless_j2k_routes(batch_context, &mut jpeg_direct_encoder)?;
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let mut routed_tiles = route_lossless_j2k_metal_input_runs(
            batch_context,
            &mut metal_input,
            &mut j2k_encoder,
            row,
            &direct_routes,
            route_scope_frames_usize,
            &mut metrics,
        )?;
        let mut cpu_batch_results = encode_lossless_j2k_cpu_fallback_after_routes(
            batch_context,
            &j2k_encoder,
            &direct_routes,
            |idx| {
                #[cfg(all(feature = "metal", target_os = "macos"))]
                {
                    routed_tiles[idx].is_some()
                }
                #[cfg(not(all(feature = "metal", target_os = "macos")))]
                {
                    let _ = idx;
                    false
                }
            },
        )?;
        for (idx, planned_frame) in planned.iter().enumerate() {
            let decision = planned_frame.route_decision(route_context);
            if try_profile_existing_lossless_j2k_frame(ExistingLosslessJ2kFrameContext {
                idx,
                planned_frame,
                direct_routes: &mut direct_routes,
                options: &options,
                metrics: &mut metrics,
                pixel_profile: &mut pixel_profile,
            })? {
                remaining = remaining.saturating_sub(1);
                continue;
            }
            if !decision.allows_j2k_encode_fallback() {
                metrics.record_j2k_passthrough_only_fallback_classification();
                remaining = remaining.saturating_sub(1);
                continue;
            }
            reject_lossy_j2k_lossless_fallback(
                planned_frame,
                options.semantics.transfer_syntax,
                row,
            )?;

            let resolved = resolve_lossless_j2k_fallback_frame(
                batch_context,
                &mut j2k_encoder,
                planned_frame,
                &mut cpu_batch_results[idx],
                #[cfg(all(feature = "metal", target_os = "macos"))]
                routed_tiles[idx].take(),
            )?;
            let encoded = record_resolved_lossless_j2k_fallback_frame(
                &mut metrics,
                &mut pixel_profile,
                resolved,
                options.semantics.transfer_syntax,
                "pixel profile changed across profiled frames",
                |err| err,
            )?;
            let _ = encoded.codestream_bytes()?;
            remaining = remaining.saturating_sub(1);
        }
    }

    Ok(metrics)
}

pub(super) fn profile_jpeg_baseline_routes(
    slide: &Slide,
    normalized_options: NormalizedExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
) -> Result<ExportMetrics, Error> {
    let geometry = jpeg_baseline_route_frame_geometry(
        slide,
        level,
        location,
        normalized_options.semantics.tile_size,
    )?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new(
        normalized_options.execution.encode_backend,
        normalized_options.execution.source_device_decode,
    );
    let mut metrics = ExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;
    let mut blank_jpeg_cache = None;
    let route_context = RouteExecutionContext::new(
        normalized_options.semantics.transfer_syntax,
        normalized_options.execution.encode_backend,
    );

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        let row_tile_count = tiles_across.min(remaining);
        let row_plan = plan_jpeg_baseline_row(
            slide,
            JpegBaselineRowPlanRequest {
                location,
                row,
                tile_count: row_tile_count,
                grid: FrameRectGrid {
                    matrix_columns,
                    matrix_rows,
                    frame_columns,
                    frame_rows,
                },
                allow_raw_rgb_passthrough,
            },
        )?;
        record_jpeg_retile_rejections(&mut metrics, &row_plan.retile_rejections);
        let planned = row_plan.frames;

        let mut index = 0usize;
        while index < planned.len() {
            match planned[index].route_decision(route_context).route {
                PlannedFrameRoute::JpegPassthrough => {
                    let JpegBaselinePlannedFrame::Passthrough { profile, .. } = &planned[index]
                    else {
                        return Err(incompatible_frame_route(
                            "JPEG",
                            PlannedFrameRoute::JpegPassthrough,
                        ));
                    };
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG passthrough pixel profile changed across profiled frames",
                    )?;
                    metrics.record_passthrough_frame();
                    metrics.record_pixel_profile(*profile);
                    remaining = remaining.saturating_sub(1);
                    index += 1;
                }
                PlannedFrameRoute::JpegRetile => {
                    let JpegBaselinePlannedFrame::Retile {
                        profile,
                        retile_duration,
                        ..
                    } = &planned[index]
                    else {
                        return Err(incompatible_frame_route(
                            "JPEG",
                            PlannedFrameRoute::JpegRetile,
                        ));
                    };
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG retile pixel profile changed across profiled frames",
                    )?;
                    metrics.record_jpeg_retile_baseline_frame(*retile_duration);
                    metrics.record_pixel_profile(*profile);
                    remaining = remaining.saturating_sub(1);
                    index += 1;
                }
                PlannedFrameRoute::Blank => {
                    let JpegBaselinePlannedFrame::Blank {
                        profile,
                        uncompressed_bytes,
                    } = &planned[index]
                    else {
                        return Err(incompatible_frame_route("JPEG", PlannedFrameRoute::Blank));
                    };
                    let (_, encode_duration) = encode_blank_jpeg_baseline_frame(
                        frame_columns,
                        frame_rows,
                        normalized_options.semantics.jpeg_quality,
                        *uncompressed_bytes,
                        &mut blank_jpeg_cache,
                    )?;
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "blank JPEG Baseline pixel profile changed across profiled frames",
                    )?;
                    metrics.record_cpu_input();
                    metrics.record_pixel_profile(*profile);
                    metrics.record_transcode_route(false, false);
                    metrics.record_jpeg_decode_fallback();
                    metrics.record_jpeg_cpu_encode(encode_duration);
                    remaining = remaining.saturating_sub(1);
                    index += 1;
                }
                PlannedFrameRoute::JpegCpuEncode | PlannedFrameRoute::JpegDeviceEncodeCandidate => {
                    if !matches!(planned[index], JpegBaselinePlannedFrame::Fallback { .. }) {
                        return Err(incompatible_frame_route(
                            "JPEG",
                            planned[index].route_decision(route_context).route,
                        ));
                    }
                    let (next_index, fallback_frames) = jpeg_baseline_fallback_run(&planned, index);
                    index = next_index;

                    let mut fallback_batch = prepare_jpeg_baseline_fallback_batch(
                        JpegBaselineFallbackBatchRequest {
                            slide,
                            #[cfg(all(feature = "metal", target_os = "macos"))]
                            level,
                            location,
                            #[cfg(all(feature = "metal", target_os = "macos"))]
                            row,
                            frames: &fallback_frames,
                            encode_backend: normalized_options.execution.encode_backend,
                            settings: JpegBaselineCpuEncodeSettings {
                                frame_columns,
                                frame_rows,
                                jpeg_quality: normalized_options.semantics.jpeg_quality,
                                max_prepared_frame_bytes: normalized_options
                                    .resources
                                    .max_prepared_frame_bytes,
                            },
                        },
                        #[cfg(all(feature = "metal", target_os = "macos"))]
                        &mut metal_input,
                        &mut metrics,
                    )?;

                    for (idx, metal_encoded) in
                        fallback_batch.metal_run.frames.iter_mut().enumerate()
                    {
                        let EncodedJpegBaselineFrame {
                            encoded,
                            profile,
                            input_decode_duration,
                            compose_duration,
                            encode_duration,
                        } = take_consistent_jpeg_baseline_fallback_frame(
                            metal_encoded,
                            &mut fallback_batch.cpu_batch_results[idx],
                            normalized_options.execution.encode_backend,
                            &mut pixel_profile,
                            "JPEG Baseline pixel profile changed across profiled frames",
                        )?;
                        let encoded_on_device = jpeg_backend_uses_device(encoded.backend);
                        if encoded_on_device {
                            metrics.record_gpu_input();
                        } else {
                            metrics.record_cpu_input();
                        }
                        metrics.record_pixel_profile(profile);
                        metrics.record_transcode_route(encoded_on_device, encoded_on_device);
                        metrics.record_jpeg_decode_fallback();
                        metrics.record_input_decode_duration(input_decode_duration);
                        metrics.record_compose_duration(compose_duration);
                        match encoded.backend {
                            JpegBackend::Cpu | JpegBackend::Auto => {
                                metrics.record_jpeg_cpu_encode(encode_duration);
                            }
                            JpegBackend::Metal | JpegBackend::Cuda => {}
                        }
                        remaining = remaining.saturating_sub(1);
                    }
                }
                route => return Err(incompatible_frame_route("JPEG", route)),
            }
        }
    }

    Ok(metrics)
}

pub(super) fn coverage_jpeg_baseline_routes(
    slide: &Slide,
    normalized_options: NormalizedExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
    deadline: Option<RouteLevelDeadline>,
) -> Result<ExportMetrics, Error> {
    let level_idx = location.level_idx;
    let geometry = jpeg_baseline_route_frame_geometry(
        slide,
        level,
        location,
        normalized_options.semantics.tile_size,
    )?;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let mut metrics = ExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;
    let route_context = RouteExecutionContext::new(
        normalized_options.semantics.transfer_syntax,
        normalized_options.execution.encode_backend,
    );

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        check_route_level_deadline(deadline, level_idx)?;
        let row_tile_count = tiles_across.min(remaining);
        let row_plan = plan_jpeg_baseline_row(
            slide,
            JpegBaselineRowPlanRequest {
                location,
                row,
                tile_count: row_tile_count,
                grid: FrameRectGrid {
                    matrix_columns: level.dimensions.0,
                    matrix_rows: level.dimensions.1,
                    frame_columns,
                    frame_rows,
                },
                allow_raw_rgb_passthrough,
            },
        )?;
        record_jpeg_retile_rejections(&mut metrics, &row_plan.retile_rejections);
        for planned_frame in &row_plan.frames {
            let decision = planned_frame.route_decision(route_context);
            match (decision.route, planned_frame) {
                (
                    PlannedFrameRoute::JpegPassthrough,
                    JpegBaselinePlannedFrame::Passthrough { profile, .. },
                ) => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG passthrough pixel profile changed across coverage frames",
                    )?;
                    metrics.record_passthrough_frame();
                    metrics.record_pixel_profile(*profile);
                }
                (
                    PlannedFrameRoute::JpegRetile,
                    JpegBaselinePlannedFrame::Retile {
                        profile,
                        retile_duration,
                        ..
                    },
                ) => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG retile pixel profile changed across coverage frames",
                    )?;
                    metrics.record_jpeg_retile_baseline_frame(*retile_duration);
                    metrics.record_pixel_profile(*profile);
                }
                (PlannedFrameRoute::Blank, JpegBaselinePlannedFrame::Blank { profile, .. }) => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "blank JPEG Baseline pixel profile changed across coverage frames",
                    )?;
                    metrics.record_cpu_input();
                    metrics.record_pixel_profile(*profile);
                    metrics.record_transcode_route(false, false);
                    metrics.record_jpeg_decode_fallback();
                }
                (
                    PlannedFrameRoute::JpegCpuEncode | PlannedFrameRoute::JpegDeviceEncodeCandidate,
                    JpegBaselinePlannedFrame::Fallback { .. },
                ) => metrics.record_jpeg_cpu_fallback_route_classification(),
                (route, _) => return Err(incompatible_frame_route("JPEG", route)),
            }
            remaining = remaining.saturating_sub(1);
        }
    }

    Ok(metrics)
}
