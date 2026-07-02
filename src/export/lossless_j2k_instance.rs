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
    level: &wsi_rs::Level,
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
    level: &wsi_rs::Level,
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
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let generated_jpeg_direct_allowed = jpeg_direct_encoder.is_some()
            && generated_jpeg_direct_htj2k_allowed_for_route(
                request.options.transfer_syntax,
                &metal_input,
            );
        #[cfg(not(all(feature = "metal", target_os = "macos")))]
        let generated_jpeg_direct_allowed = jpeg_direct_encoder.is_some();
        let mut direct_routes = encode_direct_lossless_j2k_routes(
            slide,
            &mut jpeg_direct_encoder,
            &planned,
            &request.options,
            location,
            tile_size,
            generated_jpeg_direct_allowed,
        )?;
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let mut routed_tiles = route_lossless_j2k_metal_input_runs(
            slide,
            &mut metal_input,
            &mut j2k_encoder,
            level,
            location,
            row,
            &planned,
            &direct_routes,
            request.options.transfer_syntax,
            frame_count as usize,
            matrix_columns,
            matrix_rows,
            tile_size,
            &mut metrics,
        )?;
        let mut cpu_batch_results = encode_lossless_j2k_cpu_fallback_after_routes(
            slide,
            level,
            &j2k_encoder,
            &planned,
            &request.options,
            location,
            tile_size,
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
        for (idx, planned_frame) in planned.into_iter().enumerate() {
            let encode_allowed = j2k_non_passthrough_encode_allowed(
                &planned_frame,
                request.options.transfer_syntax,
                tile_size,
            );
            if try_write_existing_lossless_j2k_frame(
                idx,
                &planned_frame,
                &mut direct_routes,
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

            let resolved = resolve_lossless_j2k_fallback_frame(
                slide,
                &mut j2k_encoder,
                location,
                &planned_frame,
                &mut cpu_batch_results[idx],
                #[cfg(all(feature = "metal", target_os = "macos"))]
                routed_tiles[idx].take(),
                request.options.transfer_syntax,
                tile_size,
            )?;
            let encoded = record_resolved_lossless_j2k_fallback_frame(
                &mut metrics,
                &mut pixel_profile,
                resolved,
                "pixel profile changed across frames",
                |err| match err {
                    Error::Encode { message } => Error::FrameEncode {
                        level: level_idx,
                        row: planned_frame.row,
                        col: planned_frame.col,
                        message,
                    },
                    other => other,
                },
            )?;
            let byte_started = Instant::now();
            pixel_data.push_owned_frame(encoded.into_codestream()?)?;
            metrics.record_write_duration(byte_started.elapsed());
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
