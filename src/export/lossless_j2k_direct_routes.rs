use super::*;

pub(super) struct LosslessJ2kDirectRouteBatch {
    direct_jpeg_results: Vec<Option<Result<jpeg_direct_htj2k::BatchOutcome, Error>>>,
    direct_j2k_results: Vec<Option<Result<j2k_direct_htj2k::BatchOutcome, Error>>>,
}

pub(super) fn encode_direct_lossless_j2k_routes(
    context: LosslessJ2kBatchContext<'_>,
    jpeg_direct_encoder: &mut Option<jpeg_direct_htj2k::BatchEncoder>,
) -> Result<LosslessJ2kDirectRouteBatch, Error> {
    let LosslessJ2kBatchContext {
        planned, options, ..
    } = context;
    let direct_jpeg_results = if let Some(jpeg_direct_encoder) = jpeg_direct_encoder.as_mut() {
        jpeg_direct_htj2k::encode_planned_batch_with_encoder(planned, jpeg_direct_encoder)?
    } else {
        (0..planned.len()).map(|_| None).collect()
    };
    let direct_j2k_results = j2k_direct_htj2k::encode_planned_batch(
        planned,
        options.transfer_syntax,
        options.codec_validation,
    )?;
    Ok(LosslessJ2kDirectRouteBatch {
        direct_jpeg_results,
        direct_j2k_results,
    })
}

pub(super) fn lossless_j2k_direct_route_succeeded(
    direct_routes: &LosslessJ2kDirectRouteBatch,
    idx: usize,
) -> bool {
    direct_routes.direct_jpeg_results[idx]
        .as_ref()
        .is_some_and(Result::is_ok)
        || j2k_direct_htj2k_result_is_ok(&direct_routes.direct_j2k_results, idx)
}

pub(super) struct ExistingLosslessJ2kFrameContext<'a> {
    pub(super) idx: usize,
    pub(super) planned_frame: &'a LosslessJ2kPlannedFrame,
    pub(super) direct_routes: &'a mut LosslessJ2kDirectRouteBatch,
    pub(super) options: &'a ExportOptions,
    pub(super) metrics: &'a mut ExportMetrics,
    pub(super) pixel_profile: &'a mut Option<PixelProfile>,
}

pub(super) fn try_write_existing_lossless_j2k_frame(
    context: ExistingLosslessJ2kFrameContext<'_>,
    pixel_data: &mut impl PixelDataSink,
) -> Result<bool, Error> {
    try_record_existing_lossless_j2k_frame(
        context,
        "pixel profile changed across frames",
        |metrics, codestream| {
            write_existing_lossless_j2k_codestream(pixel_data, metrics, codestream)
        },
    )
}

pub(super) fn try_profile_existing_lossless_j2k_frame(
    context: ExistingLosslessJ2kFrameContext<'_>,
) -> Result<bool, Error> {
    try_record_existing_lossless_j2k_frame(
        context,
        "pixel profile changed across profiled frames",
        |_, _| Ok(()),
    )
}

pub(super) fn try_record_existing_lossless_j2k_frame(
    context: ExistingLosslessJ2kFrameContext<'_>,
    mismatch_reason: &'static str,
    mut codestream_sink: impl FnMut(&mut ExportMetrics, &[u8]) -> Result<(), Error>,
) -> Result<bool, Error> {
    let ExistingLosslessJ2kFrameContext {
        idx,
        planned_frame,
        direct_routes,
        options,
        metrics,
        pixel_profile,
    } = context;
    if let Some(passthrough) = planned_frame.passthrough.as_ref() {
        let profile = passthrough.profile;
        ensure_consistent_pixel_profile(pixel_profile, profile, mismatch_reason)?;
        codestream_sink(metrics, &passthrough.codestream)?;
        metrics.record_j2k_passthrough_frame();
        metrics.record_pixel_profile(profile);
        return Ok(true);
    }

    if let Some(Ok(direct)) = direct_routes.direct_j2k_results[idx].take() {
        j2k_direct_htj2k::record_success(metrics, pixel_profile, &direct, mismatch_reason)?;
        codestream_sink(metrics, &direct.codestream)?;
        return Ok(true);
    }

    if let Some(direct_result) = direct_routes.direct_jpeg_results[idx].take() {
        match direct_result {
            Ok(direct) => {
                jpeg_direct_htj2k::record_route_success(
                    metrics,
                    pixel_profile,
                    &direct,
                    options.jpeg_direct_htj2k_profile,
                    planned_frame.source_jpeg_retiled,
                    planned_frame.source_jpeg_retile_duration,
                    mismatch_reason,
                )?;
                codestream_sink(metrics, &direct.codestream)?;
                return Ok(true);
            }
            Err(_) => metrics.record_jpeg_direct_htj2k_rejected_frame(),
        }
    } else if planned_frame.source_jpeg_direct_rejected {
        metrics.record_jpeg_direct_htj2k_rejected_frame();
    }

    if let Some(reason) = planned_frame.source_jpeg_retile_rejection {
        metrics.record_jpeg_retile_rejected_frame(reason);
    }

    Ok(false)
}

pub(super) fn write_existing_lossless_j2k_codestream(
    pixel_data: &mut impl PixelDataSink,
    metrics: &mut ExportMetrics,
    codestream: &[u8],
) -> Result<(), Error> {
    let byte_started = Instant::now();
    pixel_data.push_frame(codestream)?;
    metrics.record_write_duration(byte_started.elapsed());
    Ok(())
}

pub(super) fn j2k_direct_htj2k_result_is_ok(
    direct_results: &[Option<Result<j2k_direct_htj2k::BatchOutcome, Error>>],
    idx: usize,
) -> bool {
    direct_results[idx].as_ref().is_some_and(Result::is_ok)
}

pub(super) fn encode_cpu_input_tile(
    slide: &Slide,
    j2k_encoder: &mut DicomJ2kEncoder,
    location: JpegBaselineFrameLocation,
    frame: OutputFrameRect,
    tile_size: u32,
) -> Result<
    (
        Result<EncodedDicomJ2kFrame, Error>,
        PixelProfile,
        Duration,
        Duration,
    ),
    Error,
> {
    let prepared =
        prepare_cpu_input_lossless_j2k_tile(slide, location, frame, tile_size, u64::MAX)?;
    let samples = lossless_j2k_samples_from_prepared_region(&prepared, tile_size)?;
    Ok((
        j2k_encoder.encode(samples),
        prepared.profile,
        prepared.input_decode_duration,
        prepared.compose_duration,
    ))
}
