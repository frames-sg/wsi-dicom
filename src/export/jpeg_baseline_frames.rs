//! Ordered JPEG frame routing, encoding and spool ownership.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use j2k_jpeg::JpegBackend;
use wsi_rs::Slide;

use super::frame_region::FrameRectGrid;
use super::jpeg_baseline::{
    encode_blank_jpeg_baseline_frame, jpeg_baseline_fallback_uncompressed_bytes,
    raw_rgb_passthrough_has_no_geometry_fallback, JpegBaselineFrameGeometry,
    JpegBaselineFrameLocation, JpegBaselinePlannedFrame,
};
use super::jpeg_baseline_pipeline::{
    jpeg_backend_uses_device, jpeg_baseline_fallback_run, plan_jpeg_baseline_row,
    prepare_jpeg_baseline_fallback_batch, record_jpeg_retile_rejections,
    take_consistent_jpeg_baseline_fallback_frame, EncodedJpegBaselineFrame,
    JpegBaselineCpuEncodeSettings, JpegBaselineFallbackBatchRequest, JpegBaselineRowPlanRequest,
};
use super::route_plan::{incompatible_frame_route, PlannedFrameRoute, RouteExecutionContext};
use super::{ensure_consistent_pixel_profile, InstanceExportContext};
use crate::error::Error;
use crate::lossy::{LossyCompressionAccumulator, JPEG_BASELINE_METHOD};
use crate::options::NormalizedExportOptions;
use crate::report::ExportMetrics;
use crate::tile::PixelProfile;
use crate::writer::PixelDataSpool;

#[cfg(all(feature = "metal", target_os = "macos"))]
use super::metal_input::MetalInputTileReader;

pub(super) struct PreparedJpegFrames {
    pub(super) spool: PixelDataSpool,
    pub(super) profile: PixelProfile,
    pub(super) metrics: ExportMetrics,
    pub(super) source_lossy: LossyCompressionAccumulator,
    pub(super) target_lossy: LossyCompressionAccumulator,
}

struct JpegFrameEncoder<'a> {
    slide: &'a Slide,
    options: &'a NormalizedExportOptions,
    level: &'a wsi_rs::Level,
    location: JpegBaselineFrameLocation,
    geometry: JpegBaselineFrameGeometry,
    route_context: RouteExecutionContext,
    pixel_spool: PixelDataSpool,
    pixel_profile: Option<PixelProfile>,
    metrics: ExportMetrics,
    source_lossy_compression: LossyCompressionAccumulator,
    target_lossy_compression: LossyCompressionAccumulator,
    blank_jpeg_cache: Option<(Vec<u8>, Duration)>,
    #[cfg(all(feature = "metal", target_os = "macos"))]
    metal_input: MetalInputTileReader,
}

pub(super) fn prepare_jpeg_frames(
    slide: &Slide,
    export: InstanceExportContext<'_>,
    geometry: JpegBaselineFrameGeometry,
    spool_path: PathBuf,
    frame_count: usize,
) -> Result<PreparedJpegFrames, Error> {
    let pixel_spool = PixelDataSpool::create(spool_path, frame_count)?;
    JpegFrameEncoder {
        slide,
        options: export.options,
        level: export.level,
        location: export.coordinate,
        geometry,
        pixel_spool,
        route_context: RouteExecutionContext::new(
            export.options.semantics.transfer_syntax,
            export.options.execution.encode_backend,
        ),
        pixel_profile: None,
        metrics: ExportMetrics::default(),
        source_lossy_compression: LossyCompressionAccumulator::default(),
        target_lossy_compression: LossyCompressionAccumulator::default(),
        blank_jpeg_cache: None,
        #[cfg(all(feature = "metal", target_os = "macos"))]
        metal_input: MetalInputTileReader::new(
            export.options.execution.encode_backend,
            export.options.execution.source_device_decode,
        ),
    }
    .encode()
}

impl JpegFrameEncoder<'_> {
    fn encode(mut self) -> Result<PreparedJpegFrames, Error> {
        let allow_raw_rgb_passthrough =
            raw_rgb_passthrough_has_no_geometry_fallback(self.level, self.geometry);
        for row in 0..self.geometry.tiles_down {
            let row_plan = plan_jpeg_baseline_row(
                self.slide,
                JpegBaselineRowPlanRequest {
                    location: self.location,
                    row,
                    tile_count: self.geometry.tiles_across,
                    grid: FrameRectGrid {
                        matrix_columns: self.level.dimensions.0,
                        matrix_rows: self.level.dimensions.1,
                        frame_columns: self.geometry.frame_columns,
                        frame_rows: self.geometry.frame_rows,
                    },
                    allow_raw_rgb_passthrough,
                },
            )?;
            record_jpeg_retile_rejections(&mut self.metrics, &row_plan.retile_rejections);
            self.encode_row(&row_plan.frames, row)?;
        }
        let profile = self.pixel_profile.ok_or_else(|| Error::Unsupported {
            reason: "slide level produced no frames".into(),
        })?;
        Ok(PreparedJpegFrames {
            spool: self.pixel_spool,
            profile,
            metrics: self.metrics,
            source_lossy: self.source_lossy_compression,
            target_lossy: self.target_lossy_compression,
        })
    }

    fn encode_row(&mut self, planned: &[JpegBaselinePlannedFrame], row: u64) -> Result<(), Error> {
        let mut index = 0;
        while index < planned.len() {
            match planned[index].route_decision(self.route_context).route {
                route @ (PlannedFrameRoute::JpegPassthrough | PlannedFrameRoute::JpegRetile) => {
                    self.write_existing(&planned[index], route)?
                }
                PlannedFrameRoute::Blank => self.blank(&planned[index])?,
                PlannedFrameRoute::JpegCpuEncode | PlannedFrameRoute::JpegDeviceEncodeCandidate => {
                    index = self.fallback(planned, index, row)?;
                    continue;
                }
                route => return Err(incompatible_frame_route("JPEG", route)),
            }
            index += 1;
        }
        Ok(())
    }

    fn write_existing(
        &mut self,
        frame: &JpegBaselinePlannedFrame,
        route: PlannedFrameRoute,
    ) -> Result<(), Error> {
        let (data, profile, uncompressed_bytes, retile_duration, changed_profile) =
            match (route, frame) {
                (
                    PlannedFrameRoute::JpegPassthrough,
                    JpegBaselinePlannedFrame::Passthrough {
                        data,
                        profile,
                        uncompressed_bytes,
                    },
                ) => (
                    data,
                    profile,
                    uncompressed_bytes,
                    None,
                    "JPEG passthrough pixel profile changed across frames",
                ),
                (
                    PlannedFrameRoute::JpegRetile,
                    JpegBaselinePlannedFrame::Retile {
                        data,
                        profile,
                        uncompressed_bytes,
                        retile_duration,
                    },
                ) => (
                    data,
                    profile,
                    uncompressed_bytes,
                    Some(*retile_duration),
                    "JPEG retile pixel profile changed across frames",
                ),
                _ => return Err(incompatible_frame_route("JPEG", route)),
            };
        ensure_consistent_pixel_profile(&mut self.pixel_profile, *profile, changed_profile)?;
        self.source_lossy_compression.observe_encoded_frame(
            JPEG_BASELINE_METHOD,
            *uncompressed_bytes,
            data,
        )?;
        let started = Instant::now();
        self.pixel_spool.push_frame(data)?;
        self.metrics.record_write_duration(started.elapsed());
        if let Some(duration) = retile_duration {
            self.metrics.record_jpeg_retile_baseline_frame(duration);
        } else {
            self.metrics.record_passthrough_frame();
        }
        self.metrics.record_pixel_profile(*profile);
        Ok(())
    }

    fn blank(&mut self, frame: &JpegBaselinePlannedFrame) -> Result<(), Error> {
        let JpegBaselinePlannedFrame::Blank {
            profile,
            uncompressed_bytes: frame_uncompressed_bytes,
        } = frame
        else {
            return Err(incompatible_frame_route("JPEG", PlannedFrameRoute::Blank));
        };
        let (data, encode_duration) = encode_blank_jpeg_baseline_frame(
            self.geometry.frame_columns,
            self.geometry.frame_rows,
            self.options.semantics.jpeg_quality,
            *frame_uncompressed_bytes,
            &mut self.blank_jpeg_cache,
        )?;
        ensure_consistent_pixel_profile(
            &mut self.pixel_profile,
            *profile,
            "blank JPEG Baseline pixel profile changed across frames",
        )?;
        self.target_lossy_compression.observe_encoded_frame(
            JPEG_BASELINE_METHOD,
            *frame_uncompressed_bytes,
            &data,
        )?;
        let byte_started = Instant::now();
        self.pixel_spool.push_frame(&data)?;
        self.metrics.record_write_duration(byte_started.elapsed());
        self.metrics.record_cpu_input();
        self.metrics.record_pixel_profile(*profile);
        self.metrics.record_transcode_route(false, false);
        self.metrics.record_jpeg_decode_fallback();
        self.metrics.record_jpeg_cpu_encode(encode_duration);

        Ok(())
    }

    fn fallback(
        &mut self,
        planned: &[JpegBaselinePlannedFrame],
        index: usize,
        _row: u64,
    ) -> Result<usize, Error> {
        if !matches!(planned[index], JpegBaselinePlannedFrame::Fallback { .. }) {
            return Err(incompatible_frame_route(
                "JPEG",
                planned[index].route_decision(self.route_context).route,
            ));
        }
        let start_index = index;
        let (next_index, fallback_frames) = jpeg_baseline_fallback_run(planned, index);
        for source in planned[start_index..next_index].iter().filter_map(|frame| {
            let JpegBaselinePlannedFrame::Fallback {
                source_lossy_compression: source_history,
                ..
            } = frame
            else {
                return None;
            };
            source_history.as_ref()
        }) {
            self.source_lossy_compression.observe(source)?;
        }

        let mut fallback_batch = prepare_jpeg_baseline_fallback_batch(
            JpegBaselineFallbackBatchRequest {
                slide: self.slide,
                #[cfg(all(feature = "metal", target_os = "macos"))]
                level: self.level,
                location: self.location,
                #[cfg(all(feature = "metal", target_os = "macos"))]
                row: _row,
                frames: &fallback_frames,
                encode_backend: self.options.execution.encode_backend,
                settings: JpegBaselineCpuEncodeSettings {
                    frame_columns: self.geometry.frame_columns,
                    frame_rows: self.geometry.frame_rows,
                    jpeg_quality: self.options.semantics.jpeg_quality,
                    max_prepared_frame_bytes: self.options.resources.max_prepared_frame_bytes,
                },
            },
            #[cfg(all(feature = "metal", target_os = "macos"))]
            &mut self.metal_input,
            &mut self.metrics,
        )?;

        for (idx, metal_encoded) in fallback_batch.metal_run.frames.iter_mut().enumerate() {
            let frame = take_consistent_jpeg_baseline_fallback_frame(
                metal_encoded,
                &mut fallback_batch.cpu_batch_results[idx],
                self.options.execution.encode_backend,
                &mut self.pixel_profile,
                "JPEG Baseline pixel profile changed across frames",
            )?;
            self.accept_fallback_frame(frame)?;
        }
        Ok(next_index)
    }
    fn accept_fallback_frame(&mut self, frame: EncodedJpegBaselineFrame) -> Result<(), Error> {
        let EncodedJpegBaselineFrame {
            encoded,
            profile,
            input_decode_duration,
            compose_duration,
            encode_duration,
        } = frame;
        self.target_lossy_compression.observe_encoded_frame(
            JPEG_BASELINE_METHOD,
            jpeg_baseline_fallback_uncompressed_bytes(
                self.geometry.frame_columns,
                self.geometry.frame_rows,
                profile,
            )?,
            &encoded.data,
        )?;
        let byte_started = Instant::now();
        self.pixel_spool.push_frame(&encoded.data)?;
        self.metrics.record_write_duration(byte_started.elapsed());
        let encoded_on_device = jpeg_backend_uses_device(encoded.backend);
        if encoded_on_device {
            self.metrics.record_gpu_input();
        } else {
            self.metrics.record_cpu_input();
        }
        self.metrics.record_pixel_profile(profile);
        self.metrics
            .record_transcode_route(encoded_on_device, encoded_on_device);
        self.metrics.record_jpeg_decode_fallback();
        self.metrics
            .record_input_decode_duration(input_decode_duration);
        self.metrics.record_compose_duration(compose_duration);
        match encoded.backend {
            JpegBackend::Cpu | JpegBackend::Auto => {
                self.metrics.record_jpeg_cpu_encode(encode_duration);
            }
            JpegBackend::Metal | JpegBackend::Cuda => {}
        }
        Ok(())
    }
}
