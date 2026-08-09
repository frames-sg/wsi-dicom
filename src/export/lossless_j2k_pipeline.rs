use super::*;

pub(super) struct ResolvedLosslessJ2kFallbackFrame {
    encoded: Result<EncodedDicomJ2kFrame, Error>,
    profile: PixelProfile,
    used_gpu_input: bool,
    input_decode_duration: Duration,
    compose_duration: Duration,
}

#[derive(Clone, Copy)]
pub(super) struct LosslessJ2kBatchContext<'a> {
    pub(super) slide: &'a Slide,
    pub(super) level: &'a wsi_rs::Level,
    pub(super) planned: &'a [LosslessJ2kPlannedFrame],
    pub(super) options: &'a ExportOptions,
    pub(super) location: InstanceCoordinate,
    pub(super) tile_size: u32,
}

pub(super) struct LosslessJ2kRoutePipeline {
    pub(super) encoder: DicomJ2kEncoder,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(super) metal_input: MetalInputTileReader,
    pub(super) metrics: ExportMetrics,
    pub(super) pixel_profile: Option<PixelProfile>,
    pub(super) jpeg_direct_encoder: Option<jpeg_direct_htj2k::BatchEncoder>,
}

impl LosslessJ2kRoutePipeline {
    pub(super) fn new(
        _source_path: &Path,
        options: &ExportOptions,
        _location: InstanceCoordinate,
        route_scope_frames: u64,
    ) -> Result<Self, Error> {
        let effective_backend = effective_lossless_j2k_encode_backend(options, route_scope_frames);
        let encoder = DicomJ2kEncoder::new(
            effective_backend,
            j2k_encode_transfer_syntax(options.transfer_syntax),
            options.codec_validation,
        )
        .with_j2k_decomposition_levels(options.j2k_decomposition_levels)
        .with_gpu_encode_tuning(
            options.gpu_encode_inflight_tiles,
            hybrid_lane::effective_lossless_gpu_encode_memory_mib(options, route_scope_frames),
        );
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let mut encoder = encoder;
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let metal_input_backend =
            lossless_j2k_metal_input_preference(effective_backend, options.source_device_decode);
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let metal_input = MetalInputTileReader::new_for_lossless_j2k(
            metal_input_backend,
            lossless_j2k_auto_allows_metal_input(
                metal_input_backend,
                options.transfer_syntax,
                route_scope_frames,
                options.source_device_decode,
            ),
            auto_metal_input_route_cache_key(
                _source_path,
                options.clone(),
                _location,
                route_scope_frames,
            ),
            options.source_device_decode,
        )
        .with_row_batch_tuning(
            options.gpu_row_batch_rows,
            hybrid_lane::effective_lossless_gpu_row_batch_target_tiles(options, route_scope_frames),
        )
        .with_pipeline_depth(effective_gpu_pipeline_depth(options));
        #[cfg(all(feature = "metal", target_os = "macos"))]
        if lossless_j2k_auto_should_start_cpu_only(
            effective_backend,
            options.transfer_syntax,
            route_scope_frames,
            options.source_device_decode,
        ) || metal_input.auto_route_decision() == AutoLosslessJ2kRouteDecision::CpuOnly
        {
            encoder.force_cpu_only_for_auto();
        }
        let metrics = ExportMetrics::default();
        #[cfg(all(feature = "metal", target_os = "macos"))]
        let mut metrics = metrics;
        #[cfg(all(feature = "metal", target_os = "macos"))]
        if metal_input.enabled() {
            metrics.record_gpu_pipeline_depth(effective_gpu_pipeline_depth(options));
        }
        let jpeg_direct_encoder =
            jpeg_direct_htj2k_supported_for_backend(options.transfer_syntax, effective_backend)
                .then(|| {
                    jpeg_direct_htj2k::BatchEncoder::new(
                        options.transfer_syntax,
                        options.jpeg_direct_htj2k_profile,
                        effective_backend,
                    )
                })
                .transpose()?;

        Ok(Self {
            encoder,
            #[cfg(all(feature = "metal", target_os = "macos"))]
            metal_input,
            metrics,
            pixel_profile: None,
            jpeg_direct_encoder,
        })
    }
}

pub(super) fn encode_lossless_j2k_cpu_fallback_batch(
    context: LosslessJ2kBatchContext<'_>,
    j2k_encoder: &DicomJ2kEncoder,
    mut skip_index: impl FnMut(usize) -> bool,
) -> Result<Vec<Option<LosslessJ2kCpuBatchOutcome>>, Error> {
    let LosslessJ2kBatchContext {
        slide,
        level,
        planned,
        options,
        location,
        tile_size,
    } = context;
    let mut cpu_batch_results: Vec<Option<LosslessJ2kCpuBatchOutcome>> =
        (0..planned.len()).map(|_| None).collect();
    if let Some((
        transfer_syntax,
        codec_validation,
        j2k_decomposition_levels,
        reversible_transform,
    )) = (options.transfer_syntax != TransferSyntax::Jpeg2000)
        .then(|| j2k_encoder.cpu_batch_settings())
        .flatten()
    {
        let cpu_indices =
            lossless_j2k_cpu_fallback_indices(planned, options.transfer_syntax, tile_size, |idx| {
                skip_index(idx)
            });
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
                    max_prepared_frame_bytes: options.max_prepared_frame_bytes,
                },
                location,
                planned,
                &cpu_indices,
                tile_size,
            )?,
        )?;
    }
    Ok(cpu_batch_results)
}

pub(super) fn encode_lossless_j2k_cpu_fallback_after_routes(
    context: LosslessJ2kBatchContext<'_>,
    j2k_encoder: &DicomJ2kEncoder,
    direct_routes: &LosslessJ2kDirectRouteBatch,
    mut routed_result_is_ready: impl FnMut(usize) -> bool,
) -> Result<Vec<Option<LosslessJ2kCpuBatchOutcome>>, Error> {
    encode_lossless_j2k_cpu_fallback_batch(context, j2k_encoder, |idx| {
        routed_result_is_ready(idx) || lossless_j2k_direct_route_succeeded(direct_routes, idx)
    })
}

pub(super) fn resolve_lossless_j2k_fallback_frame(
    context: LosslessJ2kBatchContext<'_>,
    j2k_encoder: &mut DicomJ2kEncoder,
    planned_frame: &LosslessJ2kPlannedFrame,
    cpu_batch_result: &mut Option<LosslessJ2kCpuBatchOutcome>,
    #[cfg(all(feature = "metal", target_os = "macos"))] routed_encoded: Option<
        RoutedLosslessJ2kTile,
    >,
) -> Result<ResolvedLosslessJ2kFallbackFrame, Error> {
    let LosslessJ2kBatchContext {
        slide,
        options,
        location,
        tile_size,
        ..
    } = context;
    let transfer_syntax = options.transfer_syntax;
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if let Some(routed) = routed_encoded {
        return Ok(ResolvedLosslessJ2kFallbackFrame {
            encoded: routed.encoded,
            profile: j2k_fallback_profile(planned_frame, routed.profile, transfer_syntax),
            used_gpu_input: routed.used_gpu_input,
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
        });
    }

    let (encoded, profile, input_decode_duration, compose_duration) =
        if let Some(outcome) = cpu_batch_result.take() {
            (
                outcome.encoded,
                outcome.profile,
                outcome.input_decode_duration,
                outcome.compose_duration,
            )
        } else {
            j2k_encoder.set_reversible_transform(j2k_fallback_reversible_transform(
                planned_frame,
                transfer_syntax,
            ));
            encode_cpu_input_tile(
                slide,
                j2k_encoder,
                location,
                planned_frame.rect(),
                tile_size,
            )?
        };

    Ok(ResolvedLosslessJ2kFallbackFrame {
        encoded,
        profile: j2k_fallback_profile(planned_frame, profile, transfer_syntax),
        used_gpu_input: false,
        input_decode_duration,
        compose_duration,
    })
}

pub(super) fn record_resolved_lossless_j2k_fallback_frame(
    metrics: &mut ExportMetrics,
    pixel_profile: &mut Option<PixelProfile>,
    resolved: ResolvedLosslessJ2kFallbackFrame,
    mismatch_reason: &'static str,
    map_encode_error: impl FnOnce(Error) -> Error,
) -> Result<EncodedDicomJ2kFrame, Error> {
    if resolved.used_gpu_input {
        metrics.record_gpu_input();
    } else {
        metrics.record_cpu_input();
        metrics.record_input_decode_duration(resolved.input_decode_duration);
        metrics.record_compose_duration(resolved.compose_duration);
    }
    metrics.record_pixel_profile(resolved.profile);
    ensure_consistent_pixel_profile(pixel_profile, resolved.profile, mismatch_reason)?;

    let encoded = resolved.encoded.map_err(map_encode_error)?;
    metrics.record_encoded_frame(&encoded);
    metrics.record_transcode_route(resolved.used_gpu_input, encoded.used_device_encode);
    Ok(encoded)
}

#[cfg(all(feature = "metal", target_os = "macos"))]
pub(super) fn route_lossless_j2k_metal_input_runs(
    context: LosslessJ2kBatchContext<'_>,
    metal_input: &mut MetalInputTileReader,
    j2k_encoder: &mut DicomJ2kEncoder,
    row: u64,
    direct_routes: &LosslessJ2kDirectRouteBatch,
    auto_probe_frame_count: usize,
    metrics: &mut ExportMetrics,
) -> Result<Vec<Option<RoutedLosslessJ2kTile>>, Error> {
    let LosslessJ2kBatchContext {
        slide,
        level,
        planned,
        options,
        location,
        tile_size,
    } = context;
    let transfer_syntax = options.transfer_syntax;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let mut routed_tiles: Vec<Option<RoutedLosslessJ2kTile>> =
        (0..planned.len()).map(|_| None).collect();
    let mut run_start = 0usize;
    while run_start < planned.len() {
        if planned[run_start].passthrough.is_some()
            || lossless_j2k_direct_route_succeeded(direct_routes, run_start)
        {
            run_start += 1;
            continue;
        }
        let mut run_end = run_start + 1;
        while run_end < planned.len()
            && planned[run_end].passthrough.is_none()
            && !lossless_j2k_direct_route_succeeded(direct_routes, run_end)
        {
            run_end += 1;
        }
        if transfer_syntax.is_jpeg2000_passthrough_only() {
            run_start = run_end;
            continue;
        }
        if metal_input.auto_input_probe_pending() {
            let probe_end = (run_start + LOSSLESS_J2K_AUTO_ROUTE_PROBE_MAX_FRAMES).min(run_end);
            let probe_run = probe_auto_metal_input_tile_run(
                slide,
                metal_input,
                j2k_encoder,
                AutoMetalInputProbeRequest {
                    level,
                    location,
                    row,
                    planned: &planned[run_start..probe_end],
                    route_scope_frames: auto_probe_frame_count,
                    matrix_columns,
                    matrix_rows,
                    tile_size,
                },
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
                .zip(probe_run.tiles)
            {
                *slot = encoded;
            }
            run_start = probe_end;
            continue;
        }
        if metal_input.enabled() {
            let metal_run = try_encode_metal_input_tile_run(
                slide,
                metal_input,
                j2k_encoder,
                MetalInputTileRunRequest {
                    level,
                    location,
                    row,
                    start_col: planned[run_start].col,
                    tile_count: run_end - run_start,
                    matrix_columns,
                    matrix_rows,
                    tile_size,
                },
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
                .zip(metal_run.tiles)
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
    Ok(routed_tiles)
}
