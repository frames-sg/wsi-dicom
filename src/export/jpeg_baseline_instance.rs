use std::time::Instant;

use j2k_jpeg::JpegBackend;
use wsi_rs::Slide;

use super::frame_region::FrameRectGrid;
use super::icc_profile::resolve_icc_profile;
use super::jpeg_baseline::{
    encode_blank_jpeg_baseline_frame, jpeg_baseline_fallback_uncompressed_bytes,
    jpeg_baseline_route_frame_geometry, raw_rgb_passthrough_has_no_geometry_fallback,
    JpegBaselinePlannedFrame,
};
use super::jpeg_baseline_pipeline::{
    jpeg_backend_uses_device, jpeg_baseline_fallback_run, plan_jpeg_baseline_row,
    prepare_jpeg_baseline_fallback_batch, record_jpeg_retile_rejections,
    take_consistent_jpeg_baseline_fallback_frame, EncodedJpegBaselineFrame,
    JpegBaselineCpuEncodeSettings, JpegBaselineFallbackBatchRequest, JpegBaselineRowPlanRequest,
};
use super::jpeg_passthrough::{
    try_plan_direct_jpeg_passthrough_frames, DirectJpegPassthroughFrameWriter,
};
use super::route_plan::{incompatible_frame_route, PlannedFrameRoute, RouteExecutionContext};
use super::tile_grid::checked_frame_count_u32;
use super::{
    ensure_consistent_pixel_profile, level_pixel_spacing_mm, require_pixel_spacing_mm,
    InstanceExportContext, DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES,
};
use crate::error::Error;
use crate::instance_context::{DicomInstanceContext, InstanceDicomObjectParams};
use crate::report::{ExportMetrics, InstanceReport};
use crate::request::ExportRequest;
use crate::writer::{
    unique_spool_path, write_dicom_object_with_streamed_pixel_data, FrameGrid,
    LossyCompressionHistory, PixelDataSpool, StreamedDicomWritePlan,
};

#[cfg(all(feature = "metal", target_os = "macos"))]
use super::metal_input::MetalInputTileReader;
use crate::lossy::{LossyCompressionAccumulator, JPEG_BASELINE_METHOD};

pub(super) fn export_jpeg_passthrough_instance(
    slide: &Slide,
    request: &ExportRequest,
    export: InstanceExportContext<'_>,
) -> Result<InstanceReport, Error> {
    let InstanceExportContext {
        options,
        metadata,
        identity,
        instance_number,
        coordinate,
        level,
    } = export;
    let tile_size = options.semantics.tile_size;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let location = coordinate;
    let geometry = jpeg_baseline_route_frame_geometry(slide, level, location, tile_size)?;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let frame_count = checked_frame_count_u32(tiles_across, tiles_down)?;
    let context = DicomInstanceContext::new(
        identity,
        &request.output_dir,
        require_pixel_spacing_mm(level_pixel_spacing_mm(slide, level))?,
        coordinate,
    )?;
    let declared_lossy_compression =
        super::lossy_provenance::declared_source_lossy_history(&request.source_path)?;
    if let Some(direct_plan) =
        try_plan_direct_jpeg_passthrough_frames(slide, location, level, geometry)?
    {
        let icc_profile = resolve_icc_profile(
            slide,
            request,
            metadata,
            coordinate,
            level,
            direct_plan.profile,
        )?;
        let mut metrics = ExportMetrics::default();
        for _ in 0..direct_plan.frame_count {
            metrics.record_passthrough_frame();
            metrics.record_pixel_profile(direct_plan.profile);
        }

        let frame_grid = FrameGrid {
            frame_columns,
            frame_rows,
            matrix_columns,
            matrix_rows,
        };
        let lossy_compression = if declared_lossy_compression.is_empty() {
            LossyCompressionHistory::from_byte_counts(
                JPEG_BASELINE_METHOD,
                direct_plan.uncompressed_bytes,
                direct_plan.compressed_bytes,
            )?
        } else {
            declared_lossy_compression.clone()
        };
        let object = context.build_dicom_object(InstanceDicomObjectParams {
            metadata,
            study_uid: identity.study_uid(),
            specimen_uid: identity.specimen_uid(),
            instance_number,
            frame_grid,
            frame_count,
            profile: direct_plan.profile,
            icc_profile: icc_profile.bytes.as_deref(),
            lossy_compression,
        })?;
        let mut direct_writer = DirectJpegPassthroughFrameWriter::new(
            slide,
            location,
            geometry,
            direct_plan.frame_count,
            DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES,
        );
        let per_frame_plan = context.per_frame_plan(frame_count, frame_grid)?;
        let write_started = Instant::now();
        write_dicom_object_with_streamed_pixel_data(
            &context.path,
            StreamedDicomWritePlan {
                object,
                meta: context.file_meta(options.semantics.transfer_syntax.uid()),
                overwrite: options.semantics.overwrite,
                per_frame_plan,
                max_instance_metadata_bytes: options.resources.max_instance_metadata_bytes,
                frame_count: direct_plan.frame_count,
            },
            |writer| {
                for idx in 0..direct_plan.frame_count {
                    let compressed_bytes =
                        direct_writer.frame_len(idx).map_err(|source| Error::Io {
                            path: context.path.clone(),
                            source,
                        })?;
                    writer.push_frame_with(compressed_bytes, |output| {
                        direct_writer.write_frame(idx, output)
                    })?;
                }
                Ok(())
            },
        )?;
        metrics.record_write_duration(write_started.elapsed());

        return Ok(context.report(
            options.semantics.transfer_syntax.uid(),
            frame_count,
            icc_profile.report,
            metrics,
        ));
    }

    let spool_path = unique_spool_path(&context.path);
    let mut pixel_spool = PixelDataSpool::create(spool_path, frame_count as usize)?;
    let mut pixel_profile = None;
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new(
        options.execution.encode_backend,
        options.execution.source_device_decode,
    );
    let mut metrics = ExportMetrics::default();
    let mut source_lossy_compression = LossyCompressionAccumulator::default();
    let mut target_lossy_compression = LossyCompressionAccumulator::default();
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let mut blank_jpeg_cache = None;
    let route_context = RouteExecutionContext::new(
        options.semantics.transfer_syntax,
        options.execution.encode_backend,
    );

    for row in 0..tiles_down {
        let row_plan = plan_jpeg_baseline_row(
            slide,
            JpegBaselineRowPlanRequest {
                location,
                row,
                tile_count: tiles_across,
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
                    let JpegBaselinePlannedFrame::Passthrough {
                        data,
                        profile,
                        uncompressed_bytes: frame_uncompressed_bytes,
                    } = &planned[index]
                    else {
                        return Err(incompatible_frame_route(
                            "JPEG",
                            PlannedFrameRoute::JpegPassthrough,
                        ));
                    };
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG passthrough pixel profile changed across frames",
                    )?;
                    source_lossy_compression.observe_encoded_frame(
                        JPEG_BASELINE_METHOD,
                        *frame_uncompressed_bytes,
                        data,
                    )?;
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(data)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_passthrough_frame();
                    metrics.record_pixel_profile(*profile);
                    index += 1;
                }
                PlannedFrameRoute::JpegRetile => {
                    let JpegBaselinePlannedFrame::Retile {
                        data,
                        profile,
                        uncompressed_bytes: frame_uncompressed_bytes,
                        retile_duration,
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
                        "JPEG retile pixel profile changed across frames",
                    )?;
                    source_lossy_compression.observe_encoded_frame(
                        JPEG_BASELINE_METHOD,
                        *frame_uncompressed_bytes,
                        data,
                    )?;
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(data)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_jpeg_retile_baseline_frame(*retile_duration);
                    metrics.record_pixel_profile(*profile);
                    index += 1;
                }
                PlannedFrameRoute::Blank => {
                    let JpegBaselinePlannedFrame::Blank {
                        profile,
                        uncompressed_bytes: frame_uncompressed_bytes,
                    } = &planned[index]
                    else {
                        return Err(incompatible_frame_route("JPEG", PlannedFrameRoute::Blank));
                    };
                    let (data, encode_duration) = encode_blank_jpeg_baseline_frame(
                        frame_columns,
                        frame_rows,
                        options.semantics.jpeg_quality,
                        *frame_uncompressed_bytes,
                        &mut blank_jpeg_cache,
                    )?;
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "blank JPEG Baseline pixel profile changed across frames",
                    )?;
                    target_lossy_compression.observe_encoded_frame(
                        JPEG_BASELINE_METHOD,
                        *frame_uncompressed_bytes,
                        &data,
                    )?;
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(&data)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_cpu_input();
                    metrics.record_pixel_profile(*profile);
                    metrics.record_transcode_route(false, false);
                    metrics.record_jpeg_decode_fallback();
                    metrics.record_jpeg_cpu_encode(encode_duration);
                    index += 1;
                }
                PlannedFrameRoute::JpegCpuEncode | PlannedFrameRoute::JpegDeviceEncodeCandidate => {
                    if !matches!(planned[index], JpegBaselinePlannedFrame::Fallback { .. }) {
                        return Err(incompatible_frame_route(
                            "JPEG",
                            planned[index].route_decision(route_context).route,
                        ));
                    }
                    let start_index = index;
                    let (next_index, fallback_frames) = jpeg_baseline_fallback_run(&planned, index);
                    index = next_index;
                    for source in planned[start_index..next_index].iter().filter_map(|frame| {
                        let JpegBaselinePlannedFrame::Fallback {
                            source_lossy_compression,
                            ..
                        } = frame
                        else {
                            return None;
                        };
                        source_lossy_compression.as_ref()
                    }) {
                        source_lossy_compression.observe(source)?;
                    }

                    let mut fallback_batch = prepare_jpeg_baseline_fallback_batch(
                        JpegBaselineFallbackBatchRequest {
                            slide,
                            #[cfg(all(feature = "metal", target_os = "macos"))]
                            level,
                            location,
                            #[cfg(all(feature = "metal", target_os = "macos"))]
                            row,
                            frames: &fallback_frames,
                            encode_backend: options.execution.encode_backend,
                            settings: JpegBaselineCpuEncodeSettings {
                                frame_columns,
                                frame_rows,
                                jpeg_quality: options.semantics.jpeg_quality,
                                max_prepared_frame_bytes: options
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
                            options.execution.encode_backend,
                            &mut pixel_profile,
                            "JPEG Baseline pixel profile changed across frames",
                        )?;
                        target_lossy_compression.observe_encoded_frame(
                            JPEG_BASELINE_METHOD,
                            jpeg_baseline_fallback_uncompressed_bytes(
                                frame_columns,
                                frame_rows,
                                profile,
                            )?,
                            &encoded.data,
                        )?;
                        let byte_started = Instant::now();
                        pixel_spool.push_frame(&encoded.data)?;
                        metrics.record_write_duration(byte_started.elapsed());
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
                    }
                }
                route => return Err(incompatible_frame_route("JPEG", route)),
            }
        }
    }

    let profile = pixel_profile.ok_or_else(|| Error::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let icc_profile = resolve_icc_profile(slide, request, metadata, coordinate, level, profile)?;
    let frame_grid = FrameGrid {
        frame_columns,
        frame_rows,
        matrix_columns,
        matrix_rows,
    };
    let observed_lossy_compression = source_lossy_compression.into_history()?;
    let mut lossy_compression = if declared_lossy_compression.is_empty() {
        observed_lossy_compression
    } else {
        declared_lossy_compression
    };
    lossy_compression.append(target_lossy_compression.into_history()?);
    let object = context.build_dicom_object(InstanceDicomObjectParams {
        metadata,
        study_uid: identity.study_uid(),
        specimen_uid: identity.specimen_uid(),
        instance_number,
        frame_grid,
        frame_count,
        profile,
        icc_profile: icc_profile.bytes.as_deref(),
        lossy_compression,
    })?;
    let per_frame_plan = context.per_frame_plan(frame_count, frame_grid)?;
    let write_started = Instant::now();
    write_dicom_object_with_streamed_pixel_data(
        &context.path,
        StreamedDicomWritePlan {
            object,
            meta: context.file_meta(options.semantics.transfer_syntax.uid()),
            overwrite: options.semantics.overwrite,
            per_frame_plan,
            max_instance_metadata_bytes: options.resources.max_instance_metadata_bytes,
            frame_count: frame_count as usize,
        },
        |writer| pixel_spool.stream_frames_to(writer),
    )?;
    metrics.record_write_duration(write_started.elapsed());

    Ok(context.report(
        options.semantics.transfer_syntax.uid(),
        frame_count,
        icc_profile.report,
        metrics,
    ))
}
