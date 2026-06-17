use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn export_jpeg_passthrough_instance(
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
    let tile_size = request.options.tile_size;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let location = JpegBaselineFrameLocation {
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
    };
    let geometry = jpeg_baseline_route_frame_geometry(slide, level, location, tile_size)?;
    let (tiles_across, tiles_down) = (geometry.tiles_across, geometry.tiles_down);
    let (frame_columns, frame_rows) = (geometry.frame_columns, geometry.frame_rows);
    let frame_count = checked_frame_count_u32(tiles_across, tiles_down)?;
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
    let icc_profile = resolve_icc_profile(slide, request, scene_idx, series_idx, level_idx, level)?;

    if let Some(direct_frames) =
        try_plan_direct_jpeg_passthrough_frames(slide, location, level, geometry)?
    {
        let mut pixel_profile = None;
        let mut metrics = ExportMetrics::default();
        let mut compressed_bytes = 0u64;
        let mut uncompressed_bytes = 0u64;
        let mut lengths = Vec::with_capacity(direct_frames.len());
        for frame in &direct_frames {
            ensure_consistent_pixel_profile(
                &mut pixel_profile,
                frame.profile,
                "JPEG passthrough pixel profile changed across frames",
            )?;
            compressed_bytes = compressed_bytes.saturating_add(frame.compressed_bytes);
            uncompressed_bytes = uncompressed_bytes.saturating_add(frame.uncompressed_bytes);
            lengths.push(frame.compressed_bytes);
            metrics.record_passthrough_frame();
            metrics.record_pixel_profile(frame.profile);
        }

        let profile = pixel_profile.ok_or_else(|| Error::Unsupported {
            reason: "slide level produced no frames".into(),
        })?;
        let offsets = pixel_data_offsets_from_lengths(&lengths)?;
        let object = context.build_dicom_object(InstanceDicomObjectParams {
            metadata,
            study_uid,
            instance_number,
            frame_grid: FrameGrid {
                frame_columns,
                frame_rows,
                matrix_columns,
                matrix_rows,
            },
            frame_count,
            profile,
            pixel_data_offsets: PixelDataOffsetTables {
                offsets,
                lengths: lengths.clone(),
            },
            icc_profile: icc_profile.bytes.as_deref(),
            lossy_compression: Some(LossyCompressionMetadata {
                method: "ISO_10918_1",
                ratio: (compressed_bytes > 0)
                    .then_some(uncompressed_bytes as f64 / compressed_bytes as f64),
            }),
        })?;
        let mut direct_writer = DirectJpegPassthroughFrameWriter::new(
            slide,
            location,
            geometry,
            direct_frames.len(),
            DIRECT_JPEG_PASSTHROUGH_WRITE_CHUNK_FRAMES,
        );
        let write_started = Instant::now();
        write_dicom_object_with_direct_pixel_data(
            &context.path,
            object,
            context.file_meta(request.options.transfer_syntax.uid()),
            request.options.overwrite,
            &lengths,
            |idx, output| direct_writer.write_frame(idx, output),
        )?;
        metrics.record_write_duration(write_started.elapsed());

        return Ok(context.report(
            request.options.transfer_syntax.uid(),
            frame_count,
            icc_profile.source,
            metrics,
        ));
    }

    let spool_path = unique_spool_path(&context.path);
    let mut pixel_spool = PixelDataSpool::create(spool_path, frame_count as usize)?;
    let mut pixel_profile = None;
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let mut metal_input = MetalInputTileReader::new(
        request.options.encode_backend,
        request.options.source_device_decode,
    );
    let mut metrics = ExportMetrics::default();
    let mut compressed_bytes = 0u64;
    let mut uncompressed_bytes = 0u64;
    let allow_raw_rgb_passthrough = raw_rgb_passthrough_has_no_geometry_fallback(level, geometry);
    let mut blank_jpeg_cache = None;

    for row in 0..tiles_down {
        let row_plan = plan_jpeg_baseline_row(
            slide,
            location,
            row,
            tiles_across,
            matrix_columns,
            matrix_rows,
            frame_columns,
            frame_rows,
            allow_raw_rgb_passthrough,
            request.options.jpeg_quality,
            &mut blank_jpeg_cache,
            "JPEG Baseline row frame count exceeds platform addressable memory",
            "JPEG Baseline tile x offset overflow",
            "JPEG Baseline tile y offset overflow",
        )?;
        record_jpeg_retile_rejections(&mut metrics, &row_plan.retile_rejections);
        let planned = row_plan.frames;

        let mut index = 0usize;
        while index < planned.len() {
            match &planned[index] {
                JpegBaselinePlannedFrame::Passthrough {
                    data,
                    profile,
                    uncompressed_bytes: frame_uncompressed_bytes,
                } => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG passthrough pixel profile changed across frames",
                    )?;
                    compressed_bytes = compressed_bytes
                        .saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                    uncompressed_bytes =
                        uncompressed_bytes.saturating_add(*frame_uncompressed_bytes);
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(data)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_passthrough_frame();
                    metrics.record_pixel_profile(*profile);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Retile {
                    data,
                    profile,
                    uncompressed_bytes: frame_uncompressed_bytes,
                    retile_duration,
                } => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "JPEG retile pixel profile changed across frames",
                    )?;
                    compressed_bytes = compressed_bytes
                        .saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                    uncompressed_bytes =
                        uncompressed_bytes.saturating_add(*frame_uncompressed_bytes);
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(data)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_jpeg_retile_baseline_frame(*retile_duration);
                    metrics.record_pixel_profile(*profile);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Blank {
                    data,
                    profile,
                    uncompressed_bytes: frame_uncompressed_bytes,
                    encode_duration,
                } => {
                    ensure_consistent_pixel_profile(
                        &mut pixel_profile,
                        *profile,
                        "blank JPEG Baseline pixel profile changed across frames",
                    )?;
                    compressed_bytes = compressed_bytes
                        .saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                    uncompressed_bytes =
                        uncompressed_bytes.saturating_add(*frame_uncompressed_bytes);
                    let byte_started = Instant::now();
                    pixel_spool.push_frame(data)?;
                    metrics.record_write_duration(byte_started.elapsed());
                    metrics.record_cpu_input();
                    metrics.record_pixel_profile(*profile);
                    metrics.record_transcode_route(false, false);
                    metrics.record_jpeg_decode_fallback();
                    metrics.record_jpeg_cpu_encode(*encode_duration);
                    index += 1;
                }
                JpegBaselinePlannedFrame::Fallback(_) => {
                    let (next_index, fallback_frames) = jpeg_baseline_fallback_run(&planned, index);
                    index = next_index;

                    #[cfg(all(feature = "metal", target_os = "macos"))]
                    let mut metal_run = try_encode_jpeg_baseline_metal_input_tile_run(
                        slide,
                        &mut metal_input,
                        level,
                        location,
                        row,
                        &fallback_frames,
                        frame_columns,
                        frame_rows,
                        request.options.jpeg_quality,
                        request.options.max_prepared_frame_bytes,
                    )?;
                    #[cfg(not(all(feature = "metal", target_os = "macos")))]
                    let mut metal_run =
                        empty_jpeg_baseline_metal_run_for_non_metal(fallback_frames.len());

                    metrics.record_gpu_input_decode_duration(metal_run.input_decode_duration);
                    metrics.record_jpeg_metal_batch_encode(
                        metal_run
                            .frames
                            .iter()
                            .filter(|frame| frame.is_some())
                            .count() as u64,
                        metal_run.encode_duration,
                    );
                    metrics.record_gpu_batches(
                        metal_run.input_decode_batches,
                        0,
                        metal_run.encode_batches,
                    );

                    let mut cpu_batch_results = encode_jpeg_baseline_cpu_metal_misses(
                        slide,
                        location,
                        &fallback_frames,
                        &metal_run,
                        request.options.encode_backend,
                        JpegBaselineCpuEncodeSettings {
                            frame_columns,
                            frame_rows,
                            jpeg_quality: request.options.jpeg_quality,
                            max_prepared_frame_bytes: request.options.max_prepared_frame_bytes,
                        },
                    )?;

                    for (idx, (_frame, metal_encoded)) in fallback_frames
                        .iter()
                        .copied()
                        .zip(metal_run.frames.iter_mut())
                        .enumerate()
                    {
                        let (
                            encoded,
                            profile,
                            input_decode_duration,
                            compose_duration,
                            encode_duration,
                        ) = if let Some((encoded, profile)) = metal_encoded.take() {
                            (
                                encoded,
                                profile,
                                Duration::ZERO,
                                Duration::ZERO,
                                Duration::ZERO,
                            )
                        } else {
                            if request.options.encode_backend
                                == EncodeBackendPreference::RequireDevice
                            {
                                return Err(Error::Unsupported {
                                        reason:
                                            "requested JPEG Baseline device encode backend is unavailable or unsupported"
                                                .into(),
                                    });
                            }
                            cpu_batch_results[idx].take().ok_or_else(|| Error::Encode {
                                message: "CPU JPEG batch result missing for non-Metal frame".into(),
                            })?
                        };
                        ensure_consistent_pixel_profile(
                            &mut pixel_profile,
                            profile,
                            "JPEG Baseline pixel profile changed across frames",
                        )?;
                        compressed_bytes = compressed_bytes
                            .saturating_add(u64::try_from(encoded.data.len()).unwrap_or(u64::MAX));
                        uncompressed_bytes = uncompressed_bytes.saturating_add(
                            jpeg_baseline_fallback_uncompressed_bytes(
                                frame_columns,
                                frame_rows,
                                profile,
                            )?,
                        );
                        let byte_started = Instant::now();
                        pixel_spool.push_frame(&encoded.data)?;
                        metrics.record_write_duration(byte_started.elapsed());
                        if encoded.backend == JpegBackend::Metal {
                            metrics.record_gpu_input();
                        } else {
                            metrics.record_cpu_input();
                        }
                        metrics.record_pixel_profile(profile);
                        metrics.record_transcode_route(
                            encoded.backend == JpegBackend::Metal,
                            encoded.backend == JpegBackend::Metal,
                        );
                        metrics.record_jpeg_decode_fallback();
                        metrics.record_input_decode_duration(input_decode_duration);
                        metrics.record_compose_duration(compose_duration);
                        match encoded.backend {
                            JpegBackend::Cpu | JpegBackend::Auto => {
                                metrics.record_jpeg_cpu_encode(encode_duration);
                            }
                            JpegBackend::Metal => {}
                        }
                    }
                }
            }
        }
    }

    let profile = pixel_profile.ok_or_else(|| Error::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let object = context.build_dicom_object(InstanceDicomObjectParams {
        metadata,
        study_uid,
        instance_number,
        frame_grid: FrameGrid {
            frame_columns,
            frame_rows,
            matrix_columns,
            matrix_rows,
        },
        frame_count,
        profile,
        pixel_data_offsets: PixelDataOffsetTables {
            offsets: pixel_spool.offsets(),
            lengths: pixel_spool.lengths(),
        },
        icc_profile: icc_profile.bytes.as_deref(),
        lossy_compression: Some(LossyCompressionMetadata {
            method: "ISO_10918_1",
            ratio: (compressed_bytes > 0)
                .then_some(uncompressed_bytes as f64 / compressed_bytes as f64),
        }),
    })?;
    let write_started = Instant::now();
    write_dicom_object_with_spooled_pixel_data(
        &context.path,
        object,
        context.file_meta(request.options.transfer_syntax.uid()),
        request.options.overwrite,
        &mut pixel_spool,
    )?;
    metrics.record_write_duration(write_started.elapsed());

    Ok(context.report(
        request.options.transfer_syntax.uid(),
        frame_count,
        icc_profile.source,
        metrics,
    ))
}
