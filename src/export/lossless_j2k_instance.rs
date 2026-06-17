use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn export_instance(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    instance_number: u32,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    level: &statumen::Level,
) -> Result<InstanceReport, Error> {
    prepare_lossless_j2k_instance(
        slide,
        request,
        metadata,
        study_uid,
        instance_number,
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
        level,
    )?
    .finish()
}

pub(super) struct PendingLosslessJ2kInstance {
    context: DicomInstanceContext,
    metadata: DicomMetadata,
    study_uid: String,
    instance_number: u32,
    tile_size: u32,
    matrix_columns: u64,
    matrix_rows: u64,
    frame_count: u32,
    profile: PixelProfile,
    pixel_data: BufferedPixelDataSink,
    icc_profile: Option<Vec<u8>>,
    icc_profile_source: IccProfileSource,
    j2k_lossy_compression: Option<LossyCompressionMetadata>,
    metrics: ExportMetrics,
    transfer_syntax: TransferSyntax,
    overwrite: bool,
}

impl PendingLosslessJ2kInstance {
    pub(super) fn finish(mut self) -> Result<InstanceReport, Error> {
        let object = self.context.build_dicom_object(InstanceDicomObjectParams {
            metadata: &self.metadata,
            study_uid: &self.study_uid,
            instance_number: self.instance_number,
            frame_grid: FrameGrid {
                frame_columns: self.tile_size,
                frame_rows: self.tile_size,
                matrix_columns: self.matrix_columns,
                matrix_rows: self.matrix_rows,
            },
            frame_count: self.frame_count,
            profile: self.profile,
            pixel_data_offsets: PixelDataOffsetTables {
                offsets: vec![0; self.frame_count as usize],
                lengths: vec![0; self.frame_count as usize],
            },
            icc_profile: self.icc_profile.as_deref(),
            lossy_compression: self.j2k_lossy_compression,
        })?;
        let write_started = Instant::now();
        let streamed = write_dicom_object_with_streamed_pixel_data(
            &self.context.path,
            object,
            self.context.file_meta(self.transfer_syntax.uid()),
            self.overwrite,
            self.frame_count as usize,
            |writer| self.pixel_data.stream_frames_to(writer),
        )?;
        self.metrics
            .record_streaming_write_duration(streamed.streaming_write_duration);
        self.metrics
            .record_pixel_data_patch_duration(streamed.pixel_data_patch_duration);
        self.metrics.record_write_duration(write_started.elapsed());

        Ok(self.context.report(
            self.transfer_syntax.uid(),
            self.frame_count,
            self.icc_profile_source,
            self.metrics,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_lossless_j2k_instance(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    instance_number: u32,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    level: &statumen::Level,
) -> Result<PendingLosslessJ2kInstance, Error> {
    let tile_size = j2k_route_tile_size(&request.options, level)?;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let grid = TileGrid::square(matrix_columns, matrix_rows, tile_size)?;
    let tiles_across = grid.tiles_across;
    let tiles_down = grid.tiles_down;
    let frame_count = grid.frame_count_u32()?;
    let context = DicomInstanceContext::new(
        &request.source_path,
        &request.output_dir,
        require_pixel_spacing_mm(level_pixel_spacing_mm(slide, level))?,
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
    );
    let location = JpegBaselineFrameLocation {
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
    };
    let icc_profile = resolve_icc_profile(slide, request, scene_idx, series_idx, level_idx, level)?;

    let spool_path = unique_spool_path(&context.path);
    let mut pixel_data = BufferedPixelDataSink::create(
        spool_path,
        frame_count as usize,
        lossless_j2k_use_direct_pixel_data(frame_count, tile_size, rayon::current_num_threads()),
    )?;
    let mut pixel_profile = None;
    let effective_backend =
        effective_lossless_j2k_encode_backend(&request.options, u64::from(frame_count));
    let mut j2k_encoder = DicomJ2kEncoder::new(
        effective_backend,
        j2k_encode_transfer_syntax(request.options.transfer_syntax),
        request.options.codec_validation,
    )
    .with_j2k_decomposition_levels(request.options.j2k_decomposition_levels)
    .with_gpu_encode_tuning(
        request.options.gpu_encode_inflight_tiles,
        hybrid_lane::effective_lossless_gpu_encode_memory_mib(
            &request.options,
            u64::from(frame_count),
        ),
    );
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let metal_input_backend = lossless_j2k_metal_input_preference(
        effective_backend,
        request.options.source_device_decode,
    );
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new_for_lossless_j2k(
        metal_input_backend,
        lossless_j2k_auto_allows_metal_input(
            metal_input_backend,
            request.options.transfer_syntax,
            u64::from(frame_count),
            request.options.source_device_decode,
        ),
        auto_metal_input_route_cache_key(
            &request.source_path,
            request.options.clone(),
            location,
            u64::from(frame_count),
        ),
        request.options.source_device_decode,
    )
    .with_row_batch_tuning(
        request.options.gpu_row_batch_rows,
        hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(
            &request.options,
            u64::from(frame_count),
        ),
    )
    .with_pipeline_depth(effective_gpu_pipeline_depth(&request.options));
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if lossless_j2k_auto_should_start_cpu_only(
        effective_backend,
        request.options.transfer_syntax,
        u64::from(frame_count),
        request.options.source_device_decode,
    ) || metal_input.auto_route_decision() == AutoLosslessJ2kRouteDecision::CpuOnly
    {
        j2k_encoder.force_cpu_only_for_auto();
    }
    let mut metrics = ExportMetrics::default();
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if metal_input.enabled() {
        metrics.record_gpu_pipeline_depth(effective_gpu_pipeline_depth(&request.options));
    }
    let mut j2k_passthrough_lossy = false;
    let allow_passthrough_probe =
        j2k_family_passthrough_probe_allowed(&request.source_path, request.options.transfer_syntax);
    let mut jpeg_direct_encoder =
        jpeg_direct_htj2k_supported_for_backend(request.options.transfer_syntax, effective_backend)
            .then(|| {
                jpeg_direct_htj2k::BatchEncoder::new(
                    request.options.transfer_syntax,
                    request.options.jpeg_direct_htj2k_profile,
                    effective_backend,
                )
            })
            .transpose()?;

    let mut row = 0;
    while row < tiles_down {
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let planned_row_count = 1;
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        let planned_row_count = lossless_j2k_cpu_row_batch_count(tiles_across, tiles_down - row);
        let planned = plan_lossless_j2k_rows(
            slide,
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
            row,
            planned_row_count,
            0,
            tiles_across,
            matrix_columns,
            matrix_rows,
            tile_size,
            request.options.transfer_syntax,
            allow_passthrough_probe,
        )?;
        let mut direct_jpeg_results =
            if let Some(jpeg_direct_encoder) = jpeg_direct_encoder.as_mut() {
                jpeg_direct_htj2k::encode_planned_batch_with_encoder(&planned, jpeg_direct_encoder)?
            } else {
                (0..planned.len()).map(|_| None).collect()
            };
        let mut direct_j2k_results = j2k_direct_htj2k::encode_planned_batch(
            &planned,
            request.options.transfer_syntax,
            request.options.codec_validation,
        )?;
        let mut generated_jpeg_direct_results: Vec<Option<GeneratedJpegDirectHtj2kOutcome>> =
            (0..planned.len()).map(|_| None).collect();
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let generated_jpeg_direct_allowed = jpeg_direct_encoder.is_some()
            && generated_jpeg_direct_htj2k_allowed_for_route(
                request.options.transfer_syntax,
                &metal_input,
            );
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        let generated_jpeg_direct_allowed = jpeg_direct_encoder.is_some();
        if generated_jpeg_direct_allowed {
            let generated_indices = generated_jpeg_direct_htj2k_indices(
                &planned,
                request.options.transfer_syntax,
                |idx| direct_jpeg_results[idx].as_ref().is_some_and(Result::is_ok),
            );
            scatter_indexed_results(
                &mut generated_jpeg_direct_results,
                encode_generated_jpeg_direct_htj2k_planned_batch(
                    slide,
                    jpeg_direct_encoder.as_mut().ok_or_else(|| Error::Encode {
                        message: "generated JPEG direct route missing HTJ2K encoder".into(),
                    })?,
                    location,
                    &planned,
                    &generated_indices,
                    tile_size,
                    request.options.jpeg_quality,
                    request.options.max_prepared_frame_bytes,
                )?,
            )?;
        }
        #[cfg(all(feature = "metal", target_os = "macos"))]
        {
            let mut routed_tiles: Vec<Option<RoutedLosslessJ2kTile>> =
                (0..planned.len()).map(|_| None).collect();
            let mut run_start = 0usize;
            while run_start < planned.len() {
                if planned[run_start].passthrough.is_some()
                    || jpeg_direct_htj2k_result_is_ok(
                        &direct_jpeg_results,
                        &generated_jpeg_direct_results,
                        run_start,
                    )
                    || j2k_direct_htj2k_result_is_ok(&direct_j2k_results, run_start)
                {
                    run_start += 1;
                    continue;
                }
                let mut run_end = run_start + 1;
                while run_end < planned.len()
                    && planned[run_end].passthrough.is_none()
                    && !jpeg_direct_htj2k_result_is_ok(
                        &direct_jpeg_results,
                        &generated_jpeg_direct_results,
                        run_end,
                    )
                    && !j2k_direct_htj2k_result_is_ok(&direct_j2k_results, run_end)
                {
                    run_end += 1;
                }
                if request
                    .options
                    .transfer_syntax
                    .is_jpeg2000_passthrough_only()
                {
                    run_start = run_end;
                    continue;
                }
                if metal_input.auto_input_probe_pending() {
                    let probe_end =
                        (run_start + LOSSLESS_J2K_AUTO_ROUTE_PROBE_MAX_FRAMES).min(run_end);
                    let probe_run = probe_auto_metal_input_tile_run(
                        slide,
                        &mut metal_input,
                        &mut j2k_encoder,
                        level,
                        scene_idx,
                        series_idx,
                        level_idx,
                        z,
                        c,
                        t,
                        row,
                        &planned[run_start..probe_end],
                        frame_count as usize,
                        matrix_columns,
                        matrix_rows,
                        tile_size,
                    )?;
                    let selected_gpu_input =
                        probe_run.route == AutoLosslessJ2kRouteDecision::GpuInputDeviceEncode;
                    if selected_gpu_input {
                        metrics.record_gpu_input_decode_duration(probe_run.input_decode_duration);
                        metrics.record_gpu_compose_duration(probe_run.compose_duration);
                    } else {
                        metrics.record_input_decode_duration(probe_run.input_decode_duration);
                        metrics.record_compose_duration(probe_run.compose_duration);
                    }
                    metrics.record_gpu_batches(
                        probe_run.gpu_input_decode_batches,
                        probe_run.gpu_compose_batches,
                        probe_run.gpu_encode_batches,
                    );
                    metrics.record_gpu_encode_batch_stats(probe_run.gpu_encode_stats);
                    metrics.record_auto_route_probe(
                        u64::try_from(probe_end - run_start).map_err(|_| Error::Unsupported {
                            reason: "auto route probe frame count exceeds u64".into(),
                        })?,
                        probe_run.probe_cpu_duration,
                        probe_run.probe_gpu_duration,
                        probe_run.probe_gpu_batches,
                        selected_gpu_input,
                    );
                    for (slot, encoded) in routed_tiles[run_start..probe_end]
                        .iter_mut()
                        .zip(probe_run.tiles.into_iter())
                    {
                        *slot = encoded;
                    }
                    run_start = probe_end;
                    continue;
                }
                if metal_input.enabled() {
                    let metal_run = try_encode_metal_input_tile_run(
                        slide,
                        &mut metal_input,
                        &mut j2k_encoder,
                        level,
                        scene_idx,
                        series_idx,
                        level_idx,
                        z,
                        c,
                        t,
                        row,
                        planned[run_start].col,
                        (run_end - run_start) as u64,
                        matrix_columns,
                        matrix_rows,
                        tile_size,
                    )?;
                    metrics.record_gpu_input_decode_duration(metal_run.input_decode_duration);
                    metrics.record_gpu_compose_duration(metal_run.compose_duration);
                    metrics.record_gpu_batches(
                        metal_run.input_decode_batches,
                        metal_run.compose_batches,
                        metal_run.encode_batches,
                    );
                    metrics.record_gpu_encode_batch_stats(metal_run.gpu_encode_stats);
                    metrics.record_gpu_row_batch_config(
                        metal_run.row_batch_rows,
                        metal_run.row_batch_target_tiles,
                    );
                    for (slot, encoded) in routed_tiles[run_start..run_end]
                        .iter_mut()
                        .zip(metal_run.tiles.into_iter())
                    {
                        *slot = encoded.map(|(encoded, profile)| RoutedLosslessJ2kTile {
                            encoded: Ok(encoded),
                            profile,
                            used_gpu_input: true,
                        });
                    }
                }
                run_start = run_end;
            }

            let mut cpu_batch_results: Vec<Option<LosslessJ2kCpuBatchOutcome>> =
                (0..planned.len()).map(|_| None).collect();
            if let Some((
                transfer_syntax,
                codec_validation,
                j2k_decomposition_levels,
                reversible_transform,
            )) = (request.options.transfer_syntax != TransferSyntax::Jpeg2000)
                .then(|| j2k_encoder.cpu_batch_settings())
                .flatten()
            {
                let cpu_indices = lossless_j2k_cpu_fallback_indices(
                    &planned,
                    request.options.transfer_syntax,
                    tile_size,
                    |idx| {
                        routed_tiles[idx].is_some()
                            || jpeg_direct_htj2k_result_is_ok(
                                &direct_jpeg_results,
                                &generated_jpeg_direct_results,
                                idx,
                            )
                            || j2k_direct_htj2k_result_is_ok(&direct_j2k_results, idx)
                    },
                );
                scatter_indexed_results(
                    &mut cpu_batch_results,
                    encode_cpu_input_lossless_j2k_planned_batch(
                        slide,
                        level,
                        LosslessJ2kCpuBatchSettings {
                            transfer_syntax,
                            codec_validation,
                            j2k_decomposition_levels,
                            reversible_transform,
                            max_prepared_frame_bytes: request.options.max_prepared_frame_bytes,
                        },
                        scene_idx,
                        series_idx,
                        level_idx,
                        z,
                        c,
                        t,
                        &planned,
                        &cpu_indices,
                        tile_size,
                    )?,
                )?;
            }

            for (idx, planned_frame) in planned.into_iter().enumerate() {
                let encode_allowed = j2k_non_passthrough_encode_allowed(
                    &planned_frame,
                    request.options.transfer_syntax,
                    tile_size,
                );
                if try_write_existing_lossless_j2k_frame(
                    idx,
                    &planned_frame,
                    &mut direct_j2k_results,
                    &mut generated_jpeg_direct_results,
                    &mut direct_jpeg_results,
                    &request.options,
                    &mut metrics,
                    &mut pixel_profile,
                    &mut pixel_data,
                    &mut j2k_passthrough_lossy,
                )? {
                    continue;
                }
                if !encode_allowed {
                    return Err(unsupported_j2k_route_error(
                        request.options.transfer_syntax,
                        planned_frame.row,
                        planned_frame.col,
                    ));
                }
                reject_lossy_j2k_lossless_fallback(
                    &planned_frame,
                    request.options.transfer_syntax,
                    planned_frame.row,
                )?;

                let routed_encoded = routed_tiles[idx].take();
                let (encoded, profile, used_gpu_input, input_decode_duration, compose_duration) =
                    match routed_encoded {
                        Some(routed) => (
                            routed.encoded,
                            routed.profile,
                            routed.used_gpu_input,
                            Duration::ZERO,
                            Duration::ZERO,
                        ),
                        None if cpu_batch_results[idx].is_some() => {
                            let outcome =
                                cpu_batch_results[idx].take().ok_or_else(|| Error::Encode {
                                    message:
                                        "CPU JPEG 2000 batch result missing for fallback frame"
                                            .into(),
                                })?;
                            (
                                outcome.encoded,
                                outcome.profile,
                                false,
                                outcome.input_decode_duration,
                                outcome.compose_duration,
                            )
                        }
                        None => {
                            j2k_encoder.set_reversible_transform(
                                j2k_fallback_reversible_transform(
                                    &planned_frame,
                                    request.options.transfer_syntax,
                                ),
                            );
                            let (encoded, profile, input_decode_duration, compose_duration) =
                                encode_cpu_input_tile(
                                    slide,
                                    &mut j2k_encoder,
                                    location,
                                    planned_frame.x,
                                    planned_frame.y,
                                    planned_frame.width,
                                    planned_frame.height,
                                    tile_size,
                                )?;
                            (
                                encoded,
                                profile,
                                false,
                                input_decode_duration,
                                compose_duration,
                            )
                        }
                    };
                let profile =
                    j2k_fallback_profile(&planned_frame, profile, request.options.transfer_syntax);
                if used_gpu_input {
                    metrics.record_gpu_input();
                } else {
                    metrics.record_cpu_input();
                    metrics.record_input_decode_duration(input_decode_duration);
                    metrics.record_compose_duration(compose_duration);
                }
                metrics.record_pixel_profile(profile);

                ensure_consistent_pixel_profile(
                    &mut pixel_profile,
                    profile,
                    "pixel profile changed across frames",
                )?;

                let encoded = encoded.map_err(|err| match err {
                    Error::Encode { message } => Error::FrameEncode {
                        level: level_idx,
                        row: planned_frame.row,
                        col: planned_frame.col,
                        message,
                    },
                    other => other,
                })?;
                metrics.record_encoded_frame(&encoded);
                metrics.record_transcode_route(used_gpu_input, encoded.used_device_encode);
                let byte_started = Instant::now();
                pixel_data.push_owned_frame(encoded.into_codestream()?)?;
                metrics.record_write_duration(byte_started.elapsed());
            }
        }
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        {
            let mut cpu_batch_results: Vec<Option<LosslessJ2kCpuBatchOutcome>> =
                (0..planned.len()).map(|_| None).collect();
            if let Some((
                transfer_syntax,
                codec_validation,
                j2k_decomposition_levels,
                reversible_transform,
            )) = (request.options.transfer_syntax != TransferSyntax::Jpeg2000)
                .then(|| j2k_encoder.cpu_batch_settings())
                .flatten()
            {
                let cpu_indices = lossless_j2k_cpu_fallback_indices(
                    &planned,
                    request.options.transfer_syntax,
                    tile_size,
                    |idx| {
                        jpeg_direct_htj2k_result_is_ok(
                            &direct_jpeg_results,
                            &generated_jpeg_direct_results,
                            idx,
                        ) || j2k_direct_htj2k_result_is_ok(&direct_j2k_results, idx)
                    },
                );
                scatter_indexed_results(
                    &mut cpu_batch_results,
                    encode_cpu_input_lossless_j2k_planned_batch(
                        slide,
                        level,
                        LosslessJ2kCpuBatchSettings {
                            transfer_syntax,
                            codec_validation,
                            j2k_decomposition_levels,
                            reversible_transform,
                            max_prepared_frame_bytes: request.options.max_prepared_frame_bytes,
                        },
                        scene_idx,
                        series_idx,
                        level_idx,
                        z,
                        c,
                        t,
                        &planned,
                        &cpu_indices,
                        tile_size,
                    )?,
                )?;
            }
            for (idx, planned_frame) in planned.into_iter().enumerate() {
                let encode_allowed = j2k_non_passthrough_encode_allowed(
                    &planned_frame,
                    request.options.transfer_syntax,
                    tile_size,
                );
                if try_write_existing_lossless_j2k_frame(
                    idx,
                    &planned_frame,
                    &mut direct_j2k_results,
                    &mut generated_jpeg_direct_results,
                    &mut direct_jpeg_results,
                    &request.options,
                    &mut metrics,
                    &mut pixel_profile,
                    &mut pixel_data,
                    &mut j2k_passthrough_lossy,
                )? {
                    continue;
                }
                if !encode_allowed {
                    return Err(unsupported_j2k_route_error(
                        request.options.transfer_syntax,
                        planned_frame.row,
                        planned_frame.col,
                    ));
                }
                reject_lossy_j2k_lossless_fallback(
                    &planned_frame,
                    request.options.transfer_syntax,
                    planned_frame.row,
                )?;

                let (encoded, profile, input_decode_duration, compose_duration) =
                    if let Some(outcome) = cpu_batch_results[idx].take() {
                        (
                            outcome.encoded,
                            outcome.profile,
                            outcome.input_decode_duration,
                            outcome.compose_duration,
                        )
                    } else {
                        j2k_encoder.set_reversible_transform(j2k_fallback_reversible_transform(
                            &planned_frame,
                            request.options.transfer_syntax,
                        ));
                        encode_cpu_input_tile(
                            slide,
                            &mut j2k_encoder,
                            location,
                            planned_frame.x,
                            planned_frame.y,
                            planned_frame.width,
                            planned_frame.height,
                            tile_size,
                        )?
                    };
                let profile =
                    j2k_fallback_profile(&planned_frame, profile, request.options.transfer_syntax);
                metrics.record_input_decode_duration(input_decode_duration);
                metrics.record_compose_duration(compose_duration);
                metrics.record_cpu_input();
                metrics.record_pixel_profile(profile);

                ensure_consistent_pixel_profile(
                    &mut pixel_profile,
                    profile,
                    "pixel profile changed across frames",
                )?;

                let encoded = encoded.map_err(|err| match err {
                    Error::Encode { message } => Error::FrameEncode {
                        level: level_idx,
                        row: planned_frame.row,
                        col: planned_frame.col,
                        message,
                    },
                    other => other,
                })?;
                metrics.record_encoded_frame(&encoded);
                metrics.record_transcode_route(false, encoded.used_device_encode);
                let byte_started = Instant::now();
                pixel_data.push_owned_frame(encoded.into_codestream()?)?;
                metrics.record_write_duration(byte_started.elapsed());
            }
        }
        row = row
            .checked_add(planned_row_count)
            .ok_or_else(|| Error::Unsupported {
                reason: "lossless J2K row batch overflow".into(),
            })?;
    }

    let profile = pixel_profile.ok_or_else(|| Error::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let j2k_lossy_compression =
        if j2k_passthrough_lossy || request.options.transfer_syntax == TransferSyntax::Htj2k {
            let compressed_bytes = pixel_data.lengths().into_iter().sum::<u64>();
            let bytes_per_sample = u64::from(profile.bits_allocated).div_ceil(8);
            let uncompressed_bytes = u64::from(frame_count)
                .saturating_mul(u64::from(tile_size))
                .saturating_mul(u64::from(tile_size))
                .saturating_mul(u64::from(profile.components))
                .saturating_mul(bytes_per_sample);
            Some(LossyCompressionMetadata {
                method: j2k_lossy_compression_method(request.options.transfer_syntax),
                ratio: (compressed_bytes > 0)
                    .then_some(uncompressed_bytes as f64 / compressed_bytes as f64),
            })
        } else {
            None
        };

    Ok(PendingLosslessJ2kInstance {
        context,
        metadata: metadata.clone(),
        study_uid: study_uid.to_string(),
        instance_number,
        tile_size,
        matrix_columns,
        matrix_rows,
        frame_count,
        profile,
        pixel_data,
        icc_profile: icc_profile.bytes,
        icc_profile_source: icc_profile.source,
        j2k_lossy_compression,
        metrics,
        transfer_syntax: request.options.transfer_syntax,
        overwrite: request.options.overwrite,
    })
}
