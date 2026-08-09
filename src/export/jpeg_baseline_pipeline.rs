use super::*;

pub(super) fn jpeg_backend_uses_device(backend: JpegBackend) -> bool {
    matches!(backend, JpegBackend::Metal | JpegBackend::Cuda)
}

pub(super) struct EncodedJpegBaselineFrame {
    pub(super) encoded: EncodedJpeg,
    pub(super) profile: PixelProfile,
    pub(super) input_decode_duration: Duration,
    pub(super) compose_duration: Duration,
    pub(super) encode_duration: Duration,
}

#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub(super) fn empty_jpeg_baseline_metal_run_for_non_metal(
    tile_count: usize,
) -> JpegBaselineMetalEncodedRun {
    JpegBaselineMetalEncodedRun {
        frames: (0..tile_count).map(|_| None).collect(),
        input_decode_duration: Duration::ZERO,
        encode_duration: Duration::ZERO,
        input_decode_batches: 0,
        encode_batches: 0,
    }
}

pub(super) fn jpeg_baseline_fallback_run(
    planned: &[JpegBaselinePlannedFrame],
    start: usize,
) -> (usize, Vec<JpegBaselineFallbackFrame>) {
    let mut index = start;
    let mut fallback_frames = Vec::new();
    while let Some(JpegBaselinePlannedFrame::Fallback { frame, .. }) = planned.get(index) {
        fallback_frames.push(*frame);
        index += 1;
    }
    (index, fallback_frames)
}

pub(super) struct JpegBaselineRowPlan {
    pub(super) frames: Vec<JpegBaselinePlannedFrame>,
    pub(super) retile_rejections: Vec<JpegRetileRejectionReason>,
}

#[derive(Clone, Copy)]
pub(super) struct JpegBaselineCpuEncodeSettings {
    pub(super) frame_columns: u32,
    pub(super) frame_rows: u32,
    pub(super) jpeg_quality: u8,
    pub(super) max_prepared_frame_bytes: u64,
}

pub(super) struct JpegBaselineFallbackBatch {
    pub(super) metal_run: JpegBaselineMetalEncodedRun,
    pub(super) cpu_batch_results: Vec<Option<EncodedJpegBaselineFrame>>,
}

#[derive(Clone, Copy)]
pub(super) struct JpegBaselineFallbackBatchRequest<'a> {
    pub(super) slide: &'a Slide,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(super) level: &'a wsi_rs::Level,
    pub(super) location: JpegBaselineFrameLocation,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(super) row: u64,
    pub(super) frames: &'a [JpegBaselineFallbackFrame],
    pub(super) encode_backend: EncodeBackendPreference,
    pub(super) settings: JpegBaselineCpuEncodeSettings,
}

#[derive(Clone, Copy)]
pub(super) struct JpegBaselineRowPlanRequest {
    pub(super) location: JpegBaselineFrameLocation,
    pub(super) row: u64,
    pub(super) tile_count: u64,
    pub(super) grid: FrameRectGrid,
    pub(super) allow_raw_rgb_passthrough: bool,
    pub(super) jpeg_quality: u8,
}

#[derive(Clone, Copy)]
pub(super) struct CpuRegionReadRequest {
    pub(super) location: JpegBaselineFrameLocation,
    pub(super) frame: OutputFrameRect,
    pub(super) output_width: u32,
    pub(super) output_height: u32,
    pub(super) max_prepared_frame_bytes: u64,
}

struct PreparedJpegBaselineFrame {
    bytes: Vec<u8>,
    profile: PixelProfile,
    subsampling: JpegSubsampling,
    input_decode_duration: Duration,
    compose_duration: Duration,
}

pub(super) fn prepare_jpeg_baseline_fallback_batch(
    request: JpegBaselineFallbackBatchRequest<'_>,
    #[cfg(all(feature = "metal", target_os = "macos"))] metal_input: &mut MetalInputTileReader,
    metrics: &mut ExportMetrics,
) -> Result<JpegBaselineFallbackBatch, Error> {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    let metal_run = try_encode_jpeg_baseline_metal_input_tile_run(request, metal_input)?;
    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    let metal_run = empty_jpeg_baseline_metal_run_for_non_metal(request.frames.len());

    metrics.record_gpu_input_decode_duration(metal_run.input_decode_duration);
    metrics.record_jpeg_metal_batch_encode(
        metal_run
            .frames
            .iter()
            .filter(|frame| frame.is_some())
            .count() as u64,
        metal_run.encode_duration,
    );
    metrics.record_gpu_batches(metal_run.input_decode_batches, 0, metal_run.encode_batches);

    let cpu_batch_results = encode_jpeg_baseline_cpu_metal_misses(
        request.slide,
        request.location,
        request.frames,
        &metal_run,
        request.encode_backend,
        request.settings,
    )?;

    Ok(JpegBaselineFallbackBatch {
        metal_run,
        cpu_batch_results,
    })
}

pub(super) fn take_jpeg_baseline_fallback_frame(
    metal_encoded: &mut Option<(EncodedJpeg, PixelProfile)>,
    cpu_batch_result: &mut Option<EncodedJpegBaselineFrame>,
    encode_backend: EncodeBackendPreference,
) -> Result<EncodedJpegBaselineFrame, Error> {
    if let Some((encoded, profile)) = metal_encoded.take() {
        return Ok(EncodedJpegBaselineFrame {
            encoded,
            profile,
            input_decode_duration: Duration::ZERO,
            compose_duration: Duration::ZERO,
            encode_duration: Duration::ZERO,
        });
    }
    if encode_backend == EncodeBackendPreference::RequireDevice {
        return Err(Error::Unsupported {
            reason: "requested JPEG Baseline device encode backend is unavailable or unsupported"
                .into(),
        });
    }
    cpu_batch_result.take().ok_or_else(|| Error::Encode {
        message: "CPU JPEG batch result missing for non-Metal frame".into(),
    })
}

pub(super) fn take_consistent_jpeg_baseline_fallback_frame(
    metal_encoded: &mut Option<(EncodedJpeg, PixelProfile)>,
    cpu_batch_result: &mut Option<EncodedJpegBaselineFrame>,
    encode_backend: EncodeBackendPreference,
    pixel_profile: &mut Option<PixelProfile>,
    mismatch_reason: &'static str,
) -> Result<EncodedJpegBaselineFrame, Error> {
    let frame = take_jpeg_baseline_fallback_frame(metal_encoded, cpu_batch_result, encode_backend)?;
    ensure_consistent_pixel_profile(pixel_profile, frame.profile, mismatch_reason)?;
    Ok(frame)
}

pub(super) fn plan_jpeg_baseline_row(
    slide: &Slide,
    request: JpegBaselineRowPlanRequest,
    blank_jpeg_cache: &mut Option<(Vec<u8>, Duration)>,
) -> Result<JpegBaselineRowPlan, Error> {
    let row_frame_capacity =
        usize::try_from(request.tile_count).map_err(|_| Error::Unsupported {
            reason: "JPEG Baseline row frame count exceeds platform addressable memory".into(),
        })?;
    let mut planned = Vec::new();
    planned
        .try_reserve_exact(row_frame_capacity)
        .map_err(|_| Error::Unsupported {
            reason: "JPEG Baseline row frame count exceeds platform addressable memory".into(),
        })?;
    let mut retile_rejections = Vec::new();

    for col in 0..request.tile_count {
        let mut raw_jpeg_retile_candidate = false;
        let raw = slide.read_raw_compressed_tile(
            &request
                .location
                .tile_request(col as i64, request.row as i64),
        );
        let source_lossy_compression = match raw.as_ref() {
            Ok(raw) if raw.compression() == Compression::Jpeg => {
                Some(crate::lossy::LossyCompressionByteCounts::from_raw_tile(
                    crate::lossy::JPEG_BASELINE_METHOD,
                    raw,
                )?)
            }
            Ok(_) | Err(_) => None,
        };

        let empty_raw_tile = matches!(&raw, Err(err) if raw_compressed_error_is_empty_tile(err));
        match raw {
            Ok(raw)
                if raw_jpeg_matches_frame_geometry(
                    &raw,
                    request.grid.frame_columns,
                    request.grid.frame_rows,
                ) =>
            {
                let profile = pixel_profile_from_raw_jpeg_tile(&raw)?;
                if raw_jpeg_profile_can_passthrough(profile, request.allow_raw_rgb_passthrough) {
                    planned.push(JpegBaselinePlannedFrame::Passthrough {
                        uncompressed_bytes: uncompressed_frame_bytes(&raw)?,
                        data: raw.into_data(),
                        profile,
                    });
                    continue;
                }
            }
            Ok(raw) if raw.compression() == Compression::Jpeg => {
                raw_jpeg_retile_candidate = true;
            }
            Ok(_) | Err(_) => {}
        }

        if empty_raw_tile {
            planned.push(blank_jpeg_baseline_frame(
                request.grid.frame_columns,
                request.grid.frame_rows,
                request.jpeg_quality,
                blank_jpeg_cache,
            )?);
            continue;
        }

        if raw_jpeg_retile_candidate {
            match read_raw_jpeg_retile_display_tile(
                slide,
                request.location,
                col,
                request.row,
                request.grid.frame_columns,
                request.grid.frame_rows,
            )? {
                RawJpegRetileProbe::Accepted(retiled) => {
                    let profile = pixel_profile_from_raw_jpeg_tile(&retiled.raw)?;
                    if raw_jpeg_profile_can_passthrough(profile, request.allow_raw_rgb_passthrough)
                    {
                        planned.push(JpegBaselinePlannedFrame::Retile {
                            uncompressed_bytes: uncompressed_frame_bytes(&retiled.raw)?,
                            data: retiled.raw.into_data(),
                            profile,
                            retile_duration: retiled.duration,
                        });
                        continue;
                    }
                    retile_rejections.push(JpegRetileRejectionReason::ProfileUnsupported);
                }
                RawJpegRetileProbe::Rejected(reason) => {
                    retile_rejections.push(reason);
                }
            }
        }

        planned.push(JpegBaselinePlannedFrame::Fallback {
            frame: jpeg_baseline_fallback_frame(col, request.row, request.grid)?,
            source_lossy_compression,
        });
    }

    Ok(JpegBaselineRowPlan {
        frames: planned,
        retile_rejections,
    })
}

pub(super) fn record_jpeg_retile_rejections(
    metrics: &mut ExportMetrics,
    rejections: &[JpegRetileRejectionReason],
) {
    for &reason in rejections {
        metrics.record_jpeg_retile_rejected_frame(reason);
    }
}

pub(super) fn jpeg_baseline_fallback_frame(
    col: u64,
    row: u64,
    grid: FrameRectGrid,
) -> Result<JpegBaselineFallbackFrame, Error> {
    OutputFrameRect::clamped(
        col,
        row,
        grid,
        FrameRectOverflowReasons {
            x: "JPEG Baseline tile x offset overflow",
            y: "JPEG Baseline tile y offset overflow",
        },
    )
}

pub(super) fn encode_jpeg_baseline_cpu_metal_misses(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    fallback_frames: &[JpegBaselineFallbackFrame],
    metal_run: &JpegBaselineMetalEncodedRun,
    encode_backend: EncodeBackendPreference,
    settings: JpegBaselineCpuEncodeSettings,
) -> Result<Vec<Option<EncodedJpegBaselineFrame>>, Error> {
    let mut cpu_batch_results: Vec<Option<EncodedJpegBaselineFrame>> =
        (0..fallback_frames.len()).map(|_| None).collect();
    if encode_backend == EncodeBackendPreference::RequireDevice {
        return Ok(cpu_batch_results);
    }

    let cpu_indices = missing_metal_frame_indices(&metal_run.frames);
    let cpu_frames = cpu_indices
        .iter()
        .map(|&idx| fallback_frames[idx])
        .collect::<Vec<_>>();
    let cpu_encoded =
        encode_jpeg_baseline_cpu_input_tile_batch(slide, location, &cpu_frames, settings)?;
    for (idx, encoded) in cpu_indices.into_iter().zip(cpu_encoded) {
        cpu_batch_results[idx] = Some(encoded);
    }
    Ok(cpu_batch_results)
}

pub(super) fn encode_jpeg_baseline_cpu_input_tile_batch(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    frames: &[JpegBaselineFallbackFrame],
    settings: JpegBaselineCpuEncodeSettings,
) -> Result<Vec<EncodedJpegBaselineFrame>, Error> {
    frames
        .par_iter()
        .map(|frame| encode_jpeg_baseline_cpu_input_tile(slide, location, *frame, settings))
        .collect()
}

pub(super) fn encode_jpeg_baseline_cpu_input_tile(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    frame: JpegBaselineFallbackFrame,
    settings: JpegBaselineCpuEncodeSettings,
) -> Result<EncodedJpegBaselineFrame, Error> {
    let prepared = prepare_jpeg_baseline_cpu_input_tile(slide, location, frame, settings)?;
    let samples = match prepared.profile.components {
        1 => JpegSamples::Gray8 {
            data: &prepared.bytes,
            width: settings.frame_columns,
            height: settings.frame_rows,
        },
        3 => JpegSamples::Rgb8 {
            data: &prepared.bytes,
            width: settings.frame_columns,
            height: settings.frame_rows,
        },
        components => {
            return Err(Error::UnsupportedPixelData {
                reason: format!("JPEG Baseline supports 1 or 3 components, got {components}"),
            });
        }
    };
    let encode_started = Instant::now();
    let encoded = encode_jpeg_baseline_cpu_fragment(
        samples,
        settings.jpeg_quality,
        prepared.subsampling,
        jpeg_baseline_cpu_restart_interval(
            settings.frame_columns,
            settings.frame_rows,
            prepared.subsampling,
        ),
    )?;
    let encode_duration = encode_started.elapsed();

    Ok(EncodedJpegBaselineFrame {
        encoded,
        profile: prepared.profile,
        input_decode_duration: prepared.input_decode_duration,
        compose_duration: prepared.compose_duration,
        encode_duration,
    })
}

fn prepare_jpeg_baseline_cpu_input_tile(
    slide: &Slide,
    location: JpegBaselineFrameLocation,
    frame: JpegBaselineFallbackFrame,
    settings: JpegBaselineCpuEncodeSettings,
) -> Result<PreparedJpegBaselineFrame, Error> {
    let prepared = read_and_prepare_region(
        slide,
        CpuRegionReadRequest {
            location,
            frame,
            output_width: settings.frame_columns,
            output_height: settings.frame_rows,
            max_prepared_frame_bytes: settings.max_prepared_frame_bytes,
        },
    )?;
    let (profile, subsampling) = jpeg_baseline_output_profile(prepared.profile)?;
    Ok(PreparedJpegBaselineFrame {
        bytes: prepared.bytes,
        profile,
        subsampling,
        input_decode_duration: prepared.input_decode_duration,
        compose_duration: prepared.compose_duration,
    })
}

pub(super) fn read_and_prepare_region(
    slide: &Slide,
    request: CpuRegionReadRequest,
) -> Result<PreparedCpuRegion, Error> {
    let input_decode_started = Instant::now();
    let region = slide
        .read_region(
            &RegionRequest::new(
                SceneId::new(request.location.scene_idx),
                SeriesId::new(request.location.series_idx),
                LevelIdx::new(request.location.level_idx),
                (request.frame.x as i64, request.frame.y as i64),
                (request.frame.width, request.frame.height),
            )
            .with_plane(PlaneSelection::new(
                request.location.z,
                request.location.c,
                request.location.t,
            )),
        )
        .map_err(|source| Error::SlideRead {
            message: source.to_string(),
        })?;
    let input_decode_duration = input_decode_started.elapsed();

    let compose_started = Instant::now();
    let max_prepared_frame_bytes =
        usize::try_from(request.max_prepared_frame_bytes).map_err(|_| {
            Error::UnsupportedPixelData {
                reason: "max_prepared_frame_bytes exceeds platform addressable memory".into(),
            }
        })?;
    let prepared = prepare_tile_samples_with_limit(
        &region,
        request.output_width,
        request.output_height,
        max_prepared_frame_bytes,
    )?;
    let compose_duration = compose_started.elapsed();
    Ok(PreparedCpuRegion {
        bytes: prepared.bytes,
        profile: prepared.profile,
        input_decode_duration,
        compose_duration,
    })
}

pub(super) fn jpeg_baseline_output_profile(
    source: PixelProfile,
) -> Result<(PixelProfile, JpegSubsampling), Error> {
    if source.bits_allocated != 8 {
        return Err(Error::UnsupportedPixelData {
            reason: format!(
                "JPEG Baseline fallback requires 8-bit samples, got {}",
                source.bits_allocated
            ),
        });
    }
    match source.components {
        1 => Ok((source, JpegSubsampling::Gray)),
        3 => Ok((
            PixelProfile {
                components: 3,
                bits_allocated: 8,
                photometric_interpretation: "YBR_FULL_422",
            },
            JpegSubsampling::Ybr422,
        )),
        components => Err(Error::UnsupportedPixelData {
            reason: format!("JPEG Baseline supports 1 or 3 components, got {components}"),
        }),
    }
}
