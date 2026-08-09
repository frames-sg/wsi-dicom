use super::*;

pub(super) fn profile_lossless_j2k_routes(
    slide: &Slide,
    _source_path: &Path,
    options: ExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
    deadline: Option<RouteLevelDeadline>,
) -> Result<ExportMetrics, Error> {
    let level_idx = location.level_idx;
    let tile_size = j2k_route_tile_size(&options, level)?;
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
        j2k_family_passthrough_probe_allowed(_source_path, options.transfer_syntax);

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
                transfer_syntax: options.transfer_syntax,
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
            let encode_allowed = j2k_non_passthrough_encode_allowed(
                planned_frame,
                options.transfer_syntax,
                tile_size,
            );
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
            if !encode_allowed {
                metrics.record_j2k_passthrough_only_fallback_classification();
                remaining = remaining.saturating_sub(1);
                continue;
            }
            reject_lossy_j2k_lossless_fallback(planned_frame, options.transfer_syntax, row)?;

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
    options: ExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
) -> Result<ExportMetrics, Error> {
    let geometry = jpeg_baseline_route_frame_geometry(slide, level, location, options.tile_size)?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input =
        MetalInputTileReader::new(options.encode_backend, options.source_device_decode);
    let mut metrics = ExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;
    let mut blank_jpeg_cache = None;

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
                jpeg_quality: options.jpeg_quality,
            },
            &mut blank_jpeg_cache,
        )?;
        record_jpeg_retile_rejections(&mut metrics, &row_plan.retile_rejections);
        let planned = row_plan.frames;

        let mut index = 0usize;
        while index < planned.len() {
            match &planned[index] {
                JpegBaselinePlannedFrame::Passthrough { profile, .. } => {
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
                JpegBaselinePlannedFrame::Retile {
                    profile,
                    retile_duration,
                    ..
                } => {
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
                JpegBaselinePlannedFrame::Blank {
                    profile,
                    encode_duration,
                    ..
                } => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "blank JPEG Baseline pixel profile changed across profiled frames",
                    )?;
                    metrics.record_cpu_input();
                    metrics.record_pixel_profile(*profile);
                    metrics.record_transcode_route(false, false);
                    metrics.record_jpeg_decode_fallback();
                    metrics.record_jpeg_cpu_encode(*encode_duration);
                    remaining = remaining.saturating_sub(1);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Fallback { .. } => {
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
                            encode_backend: options.encode_backend,
                            settings: JpegBaselineCpuEncodeSettings {
                                frame_columns,
                                frame_rows,
                                jpeg_quality: options.jpeg_quality,
                                max_prepared_frame_bytes: options.max_prepared_frame_bytes,
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
                            options.encode_backend,
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
            }
        }
    }

    Ok(metrics)
}

pub(super) fn coverage_jpeg_baseline_routes(
    slide: &Slide,
    options: ExportOptions,
    level: &wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    max_frames: u64,
    deadline: Option<RouteLevelDeadline>,
) -> Result<ExportMetrics, Error> {
    let level_idx = location.level_idx;
    let geometry = jpeg_baseline_route_frame_geometry(slide, level, location, options.tile_size)?;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let mut metrics = ExportMetrics::default();
    let mut pixel_profile = None;
    let mut remaining = max_frames;

    for row in 0..tiles_down {
        if remaining == 0 {
            break;
        }
        check_route_level_deadline(deadline, level_idx)?;
        let row_tile_count = tiles_across.min(remaining);
        for col in 0..row_tile_count {
            let mut raw_jpeg_retile_candidate = false;
            let raw =
                slide.read_raw_compressed_tile(&location.tile_request(col as i64, row as i64));

            match raw {
                Ok(raw) if raw_jpeg_matches_frame_geometry(&raw, frame_columns, frame_rows) => {
                    let profile = pixel_profile_from_raw_jpeg_tile(&raw)?;
                    if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                        ensure_consistent_pixel_profile(
                            &mut pixel_profile,
                            profile,
                            "JPEG passthrough pixel profile changed across coverage frames",
                        )?;
                        metrics.record_passthrough_frame();
                        metrics.record_pixel_profile(profile);
                        remaining = remaining.saturating_sub(1);
                        continue;
                    }
                }
                Ok(raw) if raw.compression() == Compression::Jpeg => {
                    raw_jpeg_retile_candidate = true;
                }
                Ok(_) | Err(_) => {}
            }

            if raw_jpeg_retile_candidate {
                match read_raw_jpeg_retile_display_tile(
                    slide,
                    location,
                    col,
                    row,
                    frame_columns,
                    frame_rows,
                )? {
                    RawJpegRetileProbe::Accepted(retiled) => {
                        let profile = pixel_profile_from_raw_jpeg_tile(&retiled.raw)?;
                        if raw_jpeg_profile_can_passthrough(profile, allow_raw_rgb_passthrough) {
                            ensure_consistent_pixel_profile(
                                &mut pixel_profile,
                                profile,
                                "JPEG retile pixel profile changed across coverage frames",
                            )?;
                            metrics.record_jpeg_retile_baseline_frame(retiled.duration);
                            metrics.record_pixel_profile(profile);
                            remaining = remaining.saturating_sub(1);
                            continue;
                        }
                        metrics.record_jpeg_retile_rejected_frame(
                            JpegRetileRejectionReason::ProfileUnsupported,
                        );
                    }
                    RawJpegRetileProbe::Rejected(reason) => {
                        metrics.record_jpeg_retile_rejected_frame(reason);
                    }
                }
            }

            metrics.record_jpeg_cpu_fallback_route_classification();
            remaining = remaining.saturating_sub(1);
        }
    }

    Ok(metrics)
}
