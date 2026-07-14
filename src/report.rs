//! Report types returned by export and route profiling APIs.

use std::path::PathBuf;
use std::time::Duration;

use serde::{ser::SerializeStruct, Serialize};

use crate::encode;
use crate::tile::PixelProfile;
use crate::time::duration_as_reported_micros;

macro_rules! saturating_add_fields {
    ($target:expr, $source:expr, [$($field:ident),+ $(,)?]) => {
        $(
            $target.$field = $target.$field.saturating_add($source.$field);
        )+
    };
}

macro_rules! saturating_add_values {
    ($target:expr, [$($field:ident => $value:expr),+ $(,)?]) => {
        $(
            $target.$field = $target.$field.saturating_add($value);
        )+
    };
}

macro_rules! max_fields {
    ($target:expr, $source:expr, [$($field:ident),+ $(,)?]) => {
        $(
            $target.$field = $target.$field.max($source.$field);
        )+
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JpegRetileRejectionReason {
    SourceUnsupported,
    GeometryMismatch,
    ProfileUnsupported,
    McuInvalid,
}

/// Top-level report returned by a DICOM export.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ExportReport {
    /// Output directory containing generated DICOM instances.
    pub output_dir: PathBuf,
    /// Per-instance reports, one per generated VL WSI DICOM object.
    pub instances: Vec<InstanceReport>,
    /// Aggregate metrics across all generated instances.
    pub metrics: ExportMetrics,
}

/// Finished compressed frame bytes ready for DICOM encapsulated Pixel Data insertion.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct EncodedFrame {
    /// DICOM transfer syntax UID for the encoded frame.
    pub transfer_syntax_uid: &'static str,
    /// Encoded codestream bytes for one DICOM Pixel Data fragment.
    pub bytes: Vec<u8>,
    /// Whether frame encoding used a device backend.
    pub used_device_encode: bool,
    /// Whether validation decode used a device backend.
    pub used_device_validation: bool,
    /// Encode duration in microseconds.
    pub encode_micros: u128,
    /// Validation decode duration in microseconds.
    pub validation_micros: u128,
}

/// Route profiling report for one source level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RouteProfileReport {
    /// Source slide path.
    pub source_path: PathBuf,
    /// Transfer syntax UID used for route planning.
    pub transfer_syntax_uid: &'static str,
    /// Source pyramid level that was profiled.
    pub level: u32,
    /// Number of frames requested for profiling.
    pub requested_frames: u64,
    /// Number of frames available at this level.
    pub available_frames: u64,
    /// Route metrics collected during profiling.
    pub metrics: ExportMetrics,
    /// Wall-clock elapsed time in microseconds.
    pub elapsed_micros: u128,
}

/// Route coverage report across source levels.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RouteCoverageReport {
    /// Source slide path.
    pub source_path: PathBuf,
    /// Transfer syntax UID used for route planning.
    pub transfer_syntax_uid: &'static str,
    /// Requested frame sample count per level.
    pub requested_frames_per_level: u64,
    /// Total frames available across reported levels.
    pub available_frames: u64,
    /// Whether every available frame in scope was sampled.
    pub complete_frame_coverage: bool,
    /// Per-level route profile reports.
    pub levels: Vec<RouteProfileReport>,
    /// Aggregate metrics across reported levels.
    pub metrics: ExportMetrics,
    /// Wall-clock elapsed time in microseconds.
    pub elapsed_micros: u128,
}

/// Failure encountered while profiling one source in a corpus.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RouteCorpusCoverageFailure {
    /// Source path that failed.
    pub source_path: PathBuf,
    /// Error message for the failed source.
    pub message: String,
}

/// Route coverage report for a source corpus.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RouteCorpusCoverageReport {
    /// Root directory scanned for source slides.
    pub source_root: PathBuf,
    /// Common transfer syntax UID used by all successful reports, when one exists.
    pub transfer_syntax_uid: Option<&'static str>,
    /// Unique transfer syntax UIDs used by successful per-source reports.
    pub transfer_syntax_uids: Vec<&'static str>,
    /// Requested frame sample count per level.
    pub requested_frames_per_level: u64,
    /// Optional cap on levels inspected per source.
    pub max_levels: Option<u32>,
    /// Number of source files considered.
    pub sources_considered: usize,
    /// Total frames available across successful reports.
    pub available_frames: u64,
    /// Whether every available frame in successful reports was sampled.
    pub complete_frame_coverage: bool,
    /// Successful per-source reports.
    pub reports: Vec<RouteCoverageReport>,
    /// Per-source failures that did not stop corpus aggregation.
    pub failures: Vec<RouteCorpusCoverageFailure>,
    /// Aggregate metrics across successful reports.
    pub metrics: ExportMetrics,
    /// Wall-clock elapsed time in microseconds.
    pub elapsed_micros: u128,
}

/// Provenance for the ICC profile written to a DICOM instance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum IccProfileSource {
    /// ICC profile came from source-level metadata.
    Source,
    /// ICC profile came from an embedded JPEG APP2 profile.
    SourceJpeg,
    /// ICC profile was synthesized as sRGB fallback.
    SynthesizedSrgb,
    /// ICC profile was synthesized as Display P3 fallback.
    SynthesizedDisplayP3,
    /// ICC Profile attribute was intentionally omitted because source metadata was missing.
    #[default]
    OmittedMissing,
}

/// Report for one generated DICOM instance.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct InstanceReport {
    /// Generated DICOM file path.
    pub path: PathBuf,
    /// SOP Instance UID written to the file.
    pub sop_instance_uid: String,
    /// Series Instance UID written to the file.
    pub series_instance_uid: String,
    /// DICOM transfer syntax UID written to the file meta.
    pub transfer_syntax_uid: &'static str,
    /// ICC profile provenance for the instance.
    pub icc_profile_source: IccProfileSource,
    /// Source scene index.
    pub scene: usize,
    /// Source series index within the scene.
    pub series: usize,
    /// Source pyramid level exported.
    pub level: u32,
    /// Z stack index.
    pub z: u32,
    /// Channel index.
    pub c: u32,
    /// Timepoint index.
    pub t: u32,
    /// Number of DICOM frames in this instance.
    pub frame_count: u32,
    /// Per-instance metrics.
    pub metrics: ExportMetrics,
}

/// Route, pixel-profile, and non-GPU-specific frame counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RouteCounters {
    /// Total frames handled by the report scope.
    pub total_frames: u64,
    /// Frames whose source pixels were prepared on CPU.
    pub cpu_input_frames: u64,
    /// Frames decoded from source tiles on GPU.
    pub gpu_input_decode_frames: u64,
    /// Frames encoded on GPU.
    pub gpu_encode_frames: u64,
    /// Frames validated with GPU decode.
    pub gpu_validation_frames: u64,
    /// Frames classified as grayscale.
    pub gray_frames: u64,
    /// Frames classified as RGB-like.
    pub rgb_like_frames: u64,
    /// Frames with a component count other than gray or RGB-like.
    pub other_component_frames: u64,
    /// Frames whose pixel profile could not be classified.
    pub unknown_pixel_profile_frames: u64,
    /// Frames with 8-bit samples.
    pub bits8_frames: u64,
    /// Frames with 16-bit samples.
    pub bits16_frames: u64,
    /// Frames with another bit depth.
    pub other_bit_depth_frames: u64,
    /// Frames using a GPU transcode route.
    pub gpu_transcode_frames: u64,
    /// Frames using a resident GPU transcode route.
    pub resident_gpu_transcode_frames: u64,
    /// Frames using a mixed CPU/GPU transcode route.
    pub partial_gpu_transcode_frames: u64,
    /// GPU input decode batch count.
    pub gpu_input_decode_batches: u64,
    /// GPU composition batch count.
    pub gpu_compose_batches: u64,
    /// GPU encode batch count.
    pub gpu_encode_batches: u64,
    /// Frames sampled by automatic route probes.
    pub auto_route_probe_frames: u64,
    /// GPU batches dispatched by automatic route probes.
    pub auto_route_probe_gpu_batches: u64,
    /// CPU-side automatic route probe time in microseconds.
    pub auto_route_probe_cpu_micros: u128,
    /// GPU-side automatic route probe time in microseconds.
    pub auto_route_probe_gpu_micros: u128,
    /// Frames routed to GPU input because an automatic probe selected it.
    pub auto_route_probe_selected_gpu_input_frames: u64,
    /// Frames routed through CPU fallback.
    pub cpu_fallback_frames: u64,
    /// Frames emitted by JPEG Baseline passthrough.
    pub jpeg_passthrough_frames: u64,
    /// Frames emitted by JPEG 2000 passthrough.
    pub j2k_passthrough_frames: u64,
    /// Frames emitted by direct J2K-to-HTJ2K recoding.
    pub j2k_direct_htj2k_frames: u64,
    /// Frames re-tiled in the JPEG compressed domain.
    pub jpeg_retile_frames: u64,
    /// Frames rejected from JPEG compressed-domain retiling.
    pub jpeg_retile_rejected_frames: u64,
    /// JPEG retile rejections because the source could not provide raw compressed tiles.
    pub jpeg_retile_source_unsupported_frames: u64,
    /// JPEG retile rejections because raw tile geometry did not match the requested frame.
    pub jpeg_retile_geometry_mismatch_frames: u64,
    /// JPEG retile rejections because the JPEG pixel profile is unsupported for passthrough.
    pub jpeg_retile_profile_unsupported_frames: u64,
    /// JPEG retile rejections because the source MCU/restart table was invalid.
    pub jpeg_retile_mcu_invalid_frames: u64,
    /// JPEG compressed-domain retiling time in microseconds.
    pub jpeg_retile_us: u128,
    /// JPEG-retiled frames then transcoded through HTJ2K 5/3.
    pub jpeg_retile_to_htj2k_53_frames: u64,
    /// Frames encoded to JPEG on CPU.
    pub jpeg_cpu_encode_frames: u64,
    /// Frames encoded to JPEG on Metal.
    pub jpeg_metal_encode_frames: u64,
    /// Frames that required JPEG decode fallback.
    pub jpeg_decode_fallback_frames: u64,
}

/// Direct JPEG-to-HTJ2K coefficient-transcode counters and timings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct JpegDirectHtj2kMetrics {
    /// Frames emitted by direct JPEG-to-HTJ2K 5/3 transcoding.
    pub jpeg_direct_htj2k_53_frames: u64,
    /// Frames emitted by direct JPEG-to-HTJ2K 9/7 transcoding.
    pub jpeg_direct_htj2k_97_frames: u64,
    /// Frames rejected from direct JPEG-to-HTJ2K routing.
    pub jpeg_direct_htj2k_rejected_frames: u64,
    /// JPEG coefficient extraction time in microseconds.
    pub jpeg_direct_htj2k_extract_micros: u128,
    /// JPEG coefficient repack time in microseconds.
    pub jpeg_direct_htj2k_repack_micros: u128,
    /// JPEG coefficient transform time in microseconds.
    pub jpeg_direct_htj2k_transform_micros: u128,
    /// Accelerator time for direct JPEG-to-HTJ2K work in microseconds.
    pub jpeg_direct_htj2k_accelerator_micros: u128,
    /// CPU fallback time for direct JPEG-to-HTJ2K work in microseconds.
    pub jpeg_direct_htj2k_cpu_fallback_micros: u128,
    /// Direct JPEG-to-HTJ2K DWT decomposition time in microseconds.
    pub jpeg_direct_htj2k_dwt_decompose_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 pack/upload time in microseconds.
    pub jpeg_direct_htj2k_dwt97_pack_upload_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 row-lift time in microseconds.
    pub jpeg_direct_htj2k_dwt97_idct_row_lift_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 column-lift time in microseconds.
    pub jpeg_direct_htj2k_dwt97_column_lift_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 quantization time in microseconds.
    pub jpeg_direct_htj2k_dwt97_quantize_codeblock_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 HT encode time in microseconds.
    pub jpeg_direct_htj2k_dwt97_ht_encode_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 HT kernel time in microseconds.
    pub jpeg_direct_htj2k_dwt97_ht_kernel_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 HT status readback time in microseconds.
    pub jpeg_direct_htj2k_dwt97_ht_status_readback_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 HT compaction time in microseconds.
    pub jpeg_direct_htj2k_dwt97_ht_compact_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 HT output readback time in microseconds.
    pub jpeg_direct_htj2k_dwt97_ht_output_readback_micros: u128,
    /// Direct JPEG-to-HTJ2K 9/7 HT code-block dispatch count.
    pub jpeg_direct_htj2k_dwt97_ht_codeblock_dispatches: u64,
    /// Direct JPEG-to-HTJ2K 9/7 readback time in microseconds.
    pub jpeg_direct_htj2k_dwt97_readback_micros: u128,
    /// Direct JPEG-to-HTJ2K HTJ2K encode time in microseconds.
    pub jpeg_direct_htj2k_htj2k_encode_micros: u128,
    /// Direct JPEG-to-HTJ2K accelerator encode dispatch count.
    pub jpeg_direct_htj2k_encode_accelerator_dispatches: u64,
    /// Direct JPEG-to-HTJ2K HT block-code dispatch count.
    pub jpeg_direct_htj2k_encode_ht_code_block_dispatches: u64,
    /// Direct JPEG-to-HTJ2K packetization dispatch count.
    pub jpeg_direct_htj2k_encode_packetization_dispatches: u64,
    /// Direct JPEG-to-HTJ2K batch count.
    pub jpeg_direct_htj2k_batch_count: u64,
    /// Direct JPEG-to-HTJ2K jobs submitted in batches.
    pub jpeg_direct_htj2k_batch_jobs: u64,
    /// Direct JPEG-to-HTJ2K accelerator attempts.
    pub jpeg_direct_htj2k_accelerator_attempts: u64,
    /// Direct JPEG-to-HTJ2K accelerator jobs.
    pub jpeg_direct_htj2k_accelerator_jobs: u64,
    /// Direct JPEG-to-HTJ2K accelerator dispatches.
    pub jpeg_direct_htj2k_accelerator_dispatches: u64,
    /// Direct JPEG-to-HTJ2K jobs dispatched to accelerator.
    pub jpeg_direct_htj2k_accelerator_dispatched_jobs: u64,
    /// Direct JPEG-to-HTJ2K jobs handled by CPU fallback.
    pub jpeg_direct_htj2k_cpu_fallback_jobs: u64,
}

/// Processing and writer timing metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct WriteTimings {
    /// Input decode time in microseconds.
    pub input_decode_micros: u128,
    /// Frame composition time in microseconds.
    pub compose_micros: u128,
    /// Frame encode time in microseconds.
    pub encode_micros: u128,
    /// Runtime validation decode time in microseconds.
    pub validation_micros: u128,
    /// CPU-observed GPU dispatch time in microseconds.
    pub gpu_dispatch_micros: u128,
    /// Streaming pixel-data write time in microseconds.
    pub streaming_write_micros: u128,
    /// Pixel-data offset patch time in microseconds.
    pub pixel_data_patch_micros: u128,
    /// Writer backpressure time in microseconds.
    pub writer_backpressure_micros: u128,
    /// Total DICOM write time in microseconds.
    pub write_micros: u128,
}

/// GPU JPEG 2000 encode configuration, counters, and stage timings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct GpuEncodeMetrics {
    /// Configured GPU encode in-flight tile count.
    pub gpu_encode_configured_inflight_tiles: u64,
    /// Effective GPU encode in-flight tile count.
    pub gpu_encode_effective_inflight_tiles: u64,
    /// Maximum observed GPU encode in-flight tile count.
    pub gpu_encode_max_observed_inflight_tiles: u64,
    /// Configured GPU encode memory budget in MiB.
    pub gpu_encode_configured_memory_mib: u64,
    /// Effective GPU encode memory budget in MiB.
    pub gpu_encode_effective_memory_mib: u64,
    /// CPU-observed GPU encode wall time in microseconds.
    pub gpu_encode_wall_micros: u128,
    /// GPU-reported encode hardware time in microseconds.
    pub gpu_encode_hardware_micros: u128,
    /// Positive CPU dispatch overhead after subtracting hardware time.
    pub gpu_encode_dispatch_overhead_micros: u128,
    /// GPU encode planning time in microseconds.
    pub gpu_encode_plan_micros: u128,
    /// GPU encode prepare/submit time in microseconds.
    pub gpu_encode_prepare_submit_micros: u128,
    /// GPU HT table build time in microseconds.
    pub gpu_encode_ht_table_build_micros: u128,
    /// GPU HT buffer allocation time in microseconds.
    pub gpu_encode_ht_buffer_allocation_micros: u128,
    /// GPU command encoding time in microseconds.
    pub gpu_encode_ht_command_encode_micros: u128,
    /// GPU codestream wait/readback time in microseconds.
    pub gpu_encode_codestream_wait_micros: u128,
    /// GPU encode chunk count.
    pub gpu_encode_chunk_count: u64,
    /// GPU encode tile count.
    pub gpu_encode_tile_count: u64,
    /// GPU encode code-block count.
    pub gpu_encode_code_block_count: u64,
    /// Effective GPU pipeline depth.
    pub gpu_pipeline_depth: u64,
    /// Maximum rows per GPU row batch.
    pub gpu_row_batch_rows_max: u64,
    /// Target tiles per GPU row batch.
    pub gpu_row_batch_target_tiles: u64,
}

/// Export, routing, encode, validation, and writer counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ExportMetrics {
    /// Route, pixel-profile, and non-GPU-specific frame counters.
    #[serde(flatten)]
    pub routes: RouteCounters,
    /// Direct JPEG-to-HTJ2K coefficient-transcode counters and timings.
    #[serde(flatten)]
    pub jpeg_direct_htj2k: JpegDirectHtj2kMetrics,
    /// GPU JPEG 2000 encode configuration, counters, and stage timings.
    #[serde(flatten)]
    pub gpu_encode: GpuEncodeMetrics,
    /// Processing and writer timing metrics.
    #[serde(flatten)]
    pub timings: WriteTimings,
}

impl Serialize for GpuEncodeMetrics {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state =
            serializer.serialize_struct("GpuEncodeMetrics", Self::SERIALIZED_FIELD_COUNT)?;
        state.serialize_field(
            "gpu_encode_configured_inflight_tiles",
            &self.gpu_encode_configured_inflight_tiles,
        )?;
        state.serialize_field(
            "gpu_encode_effective_inflight_tiles",
            &self.gpu_encode_effective_inflight_tiles,
        )?;
        state.serialize_field(
            "gpu_encode_max_observed_inflight_tiles",
            &self.gpu_encode_max_observed_inflight_tiles,
        )?;
        state.serialize_field(
            "gpu_encode_configured_memory_mib",
            &self.gpu_encode_configured_memory_mib,
        )?;
        state.serialize_field(
            "gpu_encode_effective_memory_mib",
            &self.gpu_encode_effective_memory_mib,
        )?;
        state.serialize_field("gpu_encode_wall_micros", &self.gpu_encode_wall_micros)?;
        state.serialize_field(
            "gpu_encode_effective_parallelism",
            &self.effective_parallelism(),
        )?;
        state.serialize_field(
            "gpu_encode_hardware_micros",
            &self.gpu_encode_hardware_micros,
        )?;
        state.serialize_field(
            "gpu_encode_dispatch_overhead_micros",
            &self.gpu_encode_dispatch_overhead_micros,
        )?;
        state.serialize_field("gpu_encode_plan_micros", &self.gpu_encode_plan_micros)?;
        state.serialize_field(
            "gpu_encode_prepare_submit_micros",
            &self.gpu_encode_prepare_submit_micros,
        )?;
        state.serialize_field(
            "gpu_encode_ht_table_build_micros",
            &self.gpu_encode_ht_table_build_micros,
        )?;
        state.serialize_field(
            "gpu_encode_ht_buffer_allocation_micros",
            &self.gpu_encode_ht_buffer_allocation_micros,
        )?;
        state.serialize_field(
            "gpu_encode_ht_command_encode_micros",
            &self.gpu_encode_ht_command_encode_micros,
        )?;
        state.serialize_field(
            "gpu_encode_codestream_wait_micros",
            &self.gpu_encode_codestream_wait_micros,
        )?;
        state.serialize_field("gpu_encode_chunk_count", &self.gpu_encode_chunk_count)?;
        state.serialize_field("gpu_encode_tile_count", &self.gpu_encode_tile_count)?;
        state.serialize_field(
            "gpu_encode_code_block_count",
            &self.gpu_encode_code_block_count,
        )?;
        state.serialize_field("gpu_pipeline_depth", &self.gpu_pipeline_depth)?;
        state.serialize_field("gpu_row_batch_rows_max", &self.gpu_row_batch_rows_max)?;
        state.serialize_field(
            "gpu_row_batch_target_tiles",
            &self.gpu_row_batch_target_tiles,
        )?;
        state.end()
    }
}

impl GpuEncodeMetrics {
    const SERIALIZED_FIELD_COUNT: usize = 21;

    /// Ratio of summed GPU encode hardware time to observed GPU encode wall time.
    pub fn effective_parallelism(&self) -> f64 {
        if self.gpu_encode_wall_micros == 0 {
            0.0
        } else {
            self.gpu_encode_hardware_micros as f64 / self.gpu_encode_wall_micros as f64
        }
    }
}

impl ExportMetrics {
    /// Number of fields emitted by the public serialized metrics object.
    pub const SERIALIZED_FIELD_COUNT: usize = 99;

    /// Total frames emitted by compressed passthrough routes.
    pub fn route_passthrough_frames(&self) -> u64 {
        self.routes
            .jpeg_passthrough_frames
            .saturating_add(self.routes.j2k_passthrough_frames)
    }

    /// Frames emitted by JPEG compressed-domain retiling without HTJ2K transcode.
    pub fn jpeg_retile_baseline_frames(&self) -> u64 {
        self.routes
            .jpeg_retile_frames
            .saturating_sub(self.routes.jpeg_retile_to_htj2k_53_frames)
    }

    /// Frames not accounted for by a known route counter.
    pub fn route_unclassified_frames(&self) -> u64 {
        self.routes
            .total_frames
            .saturating_sub(self.route_passthrough_frames())
            .saturating_sub(self.routes.j2k_direct_htj2k_frames)
            .saturating_sub(self.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames)
            .saturating_sub(self.jpeg_direct_htj2k.jpeg_direct_htj2k_97_frames)
            .saturating_sub(self.jpeg_retile_baseline_frames())
            .saturating_sub(self.routes.gpu_transcode_frames)
            .saturating_sub(self.routes.cpu_fallback_frames)
    }

    pub(crate) fn add_assign(&mut self, other: Self) {
        saturating_add_fields!(
            self.routes,
            other.routes,
            [
                total_frames,
                cpu_input_frames,
                gpu_input_decode_frames,
                gpu_encode_frames,
                gpu_validation_frames,
                gray_frames,
                rgb_like_frames,
                other_component_frames,
                unknown_pixel_profile_frames,
                bits8_frames,
                bits16_frames,
                other_bit_depth_frames,
                gpu_transcode_frames,
                resident_gpu_transcode_frames,
                partial_gpu_transcode_frames,
                gpu_input_decode_batches,
                gpu_compose_batches,
                gpu_encode_batches,
                auto_route_probe_frames,
                auto_route_probe_gpu_batches,
                auto_route_probe_cpu_micros,
                auto_route_probe_gpu_micros,
                auto_route_probe_selected_gpu_input_frames,
                cpu_fallback_frames,
                jpeg_passthrough_frames,
                j2k_passthrough_frames,
                j2k_direct_htj2k_frames,
                jpeg_retile_frames,
                jpeg_retile_rejected_frames,
                jpeg_retile_source_unsupported_frames,
                jpeg_retile_geometry_mismatch_frames,
                jpeg_retile_profile_unsupported_frames,
                jpeg_retile_mcu_invalid_frames,
                jpeg_retile_us,
                jpeg_retile_to_htj2k_53_frames,
                jpeg_cpu_encode_frames,
                jpeg_metal_encode_frames,
                jpeg_decode_fallback_frames,
            ]
        );
        saturating_add_fields!(
            self.jpeg_direct_htj2k,
            other.jpeg_direct_htj2k,
            [
                jpeg_direct_htj2k_53_frames,
                jpeg_direct_htj2k_97_frames,
                jpeg_direct_htj2k_rejected_frames,
                jpeg_direct_htj2k_extract_micros,
                jpeg_direct_htj2k_repack_micros,
                jpeg_direct_htj2k_transform_micros,
                jpeg_direct_htj2k_accelerator_micros,
                jpeg_direct_htj2k_cpu_fallback_micros,
                jpeg_direct_htj2k_dwt_decompose_micros,
                jpeg_direct_htj2k_dwt97_pack_upload_micros,
                jpeg_direct_htj2k_dwt97_idct_row_lift_micros,
                jpeg_direct_htj2k_dwt97_column_lift_micros,
                jpeg_direct_htj2k_dwt97_quantize_codeblock_micros,
                jpeg_direct_htj2k_dwt97_ht_encode_micros,
                jpeg_direct_htj2k_dwt97_ht_kernel_micros,
                jpeg_direct_htj2k_dwt97_ht_status_readback_micros,
                jpeg_direct_htj2k_dwt97_ht_compact_micros,
                jpeg_direct_htj2k_dwt97_ht_output_readback_micros,
                jpeg_direct_htj2k_dwt97_ht_codeblock_dispatches,
                jpeg_direct_htj2k_dwt97_readback_micros,
                jpeg_direct_htj2k_htj2k_encode_micros,
                jpeg_direct_htj2k_encode_accelerator_dispatches,
                jpeg_direct_htj2k_encode_ht_code_block_dispatches,
                jpeg_direct_htj2k_encode_packetization_dispatches,
                jpeg_direct_htj2k_batch_count,
                jpeg_direct_htj2k_batch_jobs,
                jpeg_direct_htj2k_accelerator_attempts,
                jpeg_direct_htj2k_accelerator_jobs,
                jpeg_direct_htj2k_accelerator_dispatches,
                jpeg_direct_htj2k_accelerator_dispatched_jobs,
                jpeg_direct_htj2k_cpu_fallback_jobs,
            ]
        );
        saturating_add_fields!(
            self.timings,
            other.timings,
            [
                input_decode_micros,
                compose_micros,
                encode_micros,
                validation_micros,
                gpu_dispatch_micros,
                streaming_write_micros,
                pixel_data_patch_micros,
                writer_backpressure_micros,
                write_micros,
            ]
        );
        saturating_add_fields!(
            self.gpu_encode,
            other.gpu_encode,
            [
                gpu_encode_wall_micros,
                gpu_encode_hardware_micros,
                gpu_encode_dispatch_overhead_micros,
                gpu_encode_plan_micros,
                gpu_encode_prepare_submit_micros,
                gpu_encode_ht_table_build_micros,
                gpu_encode_ht_buffer_allocation_micros,
                gpu_encode_ht_command_encode_micros,
                gpu_encode_codestream_wait_micros,
                gpu_encode_chunk_count,
                gpu_encode_tile_count,
                gpu_encode_code_block_count,
            ]
        );
        max_fields!(
            self.gpu_encode,
            other.gpu_encode,
            [
                gpu_encode_configured_inflight_tiles,
                gpu_encode_effective_inflight_tiles,
                gpu_encode_max_observed_inflight_tiles,
                gpu_encode_configured_memory_mib,
                gpu_encode_effective_memory_mib,
                gpu_pipeline_depth,
                gpu_row_batch_rows_max,
                gpu_row_batch_target_tiles,
            ]
        );
    }

    pub(crate) fn record_cpu_input(&mut self) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.routes.cpu_input_frames);
    }

    pub(crate) fn record_gpu_input(&mut self) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.routes.gpu_input_decode_frames);
    }

    pub(crate) fn record_passthrough_frame(&mut self) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.routes.jpeg_passthrough_frames);
    }

    pub(crate) fn record_j2k_passthrough_frame(&mut self) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.routes.j2k_passthrough_frames);
    }

    pub(crate) fn record_j2k_direct_htj2k_frame(&mut self, transcode_micros: u128) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.routes.j2k_direct_htj2k_frames);
        self.timings.encode_micros = self.timings.encode_micros.saturating_add(transcode_micros);
    }

    pub(crate) fn record_jpeg_direct_htj2k_53_frame(&mut self, transcode_micros: u128) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames);
        self.timings.encode_micros = self.timings.encode_micros.saturating_add(transcode_micros);
    }

    pub(crate) fn record_jpeg_direct_htj2k_97_frame(&mut self, transcode_micros: u128) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.jpeg_direct_htj2k.jpeg_direct_htj2k_97_frames);
        self.timings.encode_micros = self.timings.encode_micros.saturating_add(transcode_micros);
    }

    pub(crate) fn record_jpeg_direct_htj2k_rejected_frame(&mut self) {
        increment_u64(&mut self.jpeg_direct_htj2k.jpeg_direct_htj2k_rejected_frames);
    }

    pub(crate) fn record_jpeg_direct_htj2k_timings(
        &mut self,
        timings: j2k_transcode::TranscodeTimingReport,
    ) {
        saturating_add_values!(
            self.jpeg_direct_htj2k,
            [
                jpeg_direct_htj2k_extract_micros => timings.jpeg_dct_extract_us,
                jpeg_direct_htj2k_repack_micros => timings.jpeg_dct_repack_us,
                jpeg_direct_htj2k_transform_micros => timings.dct_to_wavelet_total_us,
                jpeg_direct_htj2k_accelerator_micros => timings.dct_to_wavelet_accelerator_us,
                jpeg_direct_htj2k_cpu_fallback_micros => timings.dct_to_wavelet_cpu_fallback_us,
                jpeg_direct_htj2k_dwt_decompose_micros => timings.dwt_decompose_us,
                jpeg_direct_htj2k_dwt97_pack_upload_micros => timings.dwt97_batch_pack_upload_us,
                jpeg_direct_htj2k_dwt97_idct_row_lift_micros => timings.dwt97_batch_idct_row_lift_us,
                jpeg_direct_htj2k_dwt97_column_lift_micros => timings.dwt97_batch_column_lift_us,
                jpeg_direct_htj2k_dwt97_quantize_codeblock_micros => timings.dwt97_batch_quantize_codeblock_us,
                jpeg_direct_htj2k_dwt97_ht_encode_micros => timings.dwt97_batch_ht_encode_us,
                jpeg_direct_htj2k_dwt97_ht_kernel_micros => timings.dwt97_batch_ht_kernel_us,
                jpeg_direct_htj2k_dwt97_ht_status_readback_micros => timings.dwt97_batch_ht_status_readback_us,
                jpeg_direct_htj2k_dwt97_ht_compact_micros => timings.dwt97_batch_ht_compact_us,
                jpeg_direct_htj2k_dwt97_ht_output_readback_micros => timings.dwt97_batch_ht_output_readback_us,
                jpeg_direct_htj2k_dwt97_ht_codeblock_dispatches => timings.dwt97_batch_ht_codeblock_dispatches as u64,
                jpeg_direct_htj2k_dwt97_readback_micros => timings.dwt97_batch_readback_us,
                jpeg_direct_htj2k_htj2k_encode_micros => timings.htj2k_encode_us,
                jpeg_direct_htj2k_encode_accelerator_dispatches => timings.htj2k_encode_accelerator_dispatches as u64,
                jpeg_direct_htj2k_encode_ht_code_block_dispatches => timings.htj2k_encode_ht_code_block_dispatches as u64,
                jpeg_direct_htj2k_encode_packetization_dispatches => timings.htj2k_encode_packetization_dispatches as u64,
                jpeg_direct_htj2k_batch_count => timings.batch_count as u64,
                jpeg_direct_htj2k_batch_jobs => timings.batch_jobs as u64,
                jpeg_direct_htj2k_accelerator_attempts => timings.accelerator_attempts as u64,
                jpeg_direct_htj2k_accelerator_jobs => timings.accelerator_jobs as u64,
                jpeg_direct_htj2k_accelerator_dispatches => timings.accelerator_dispatches as u64,
                jpeg_direct_htj2k_accelerator_dispatched_jobs => timings.accelerator_dispatched_jobs as u64,
                jpeg_direct_htj2k_cpu_fallback_jobs => timings.cpu_fallback_jobs as u64,
            ]
        );
    }

    pub(crate) fn record_jpeg_retile_baseline_frame(&mut self, duration: Duration) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.routes.jpeg_retile_frames);
        add_duration_micros(&mut self.routes.jpeg_retile_us, duration);
    }

    pub(crate) fn record_jpeg_retile_to_htj2k_53_frame(&mut self, duration: Duration) {
        increment_u64(&mut self.routes.jpeg_retile_frames);
        increment_u64(&mut self.routes.jpeg_retile_to_htj2k_53_frames);
        add_duration_micros(&mut self.routes.jpeg_retile_us, duration);
    }

    pub(crate) fn record_jpeg_retile_rejected_frame(&mut self, reason: JpegRetileRejectionReason) {
        increment_u64(&mut self.routes.jpeg_retile_rejected_frames);
        match reason {
            JpegRetileRejectionReason::SourceUnsupported => {
                increment_u64(&mut self.routes.jpeg_retile_source_unsupported_frames);
            }
            JpegRetileRejectionReason::GeometryMismatch => {
                increment_u64(&mut self.routes.jpeg_retile_geometry_mismatch_frames);
            }
            JpegRetileRejectionReason::ProfileUnsupported => {
                increment_u64(&mut self.routes.jpeg_retile_profile_unsupported_frames);
            }
            JpegRetileRejectionReason::McuInvalid => {
                increment_u64(&mut self.routes.jpeg_retile_mcu_invalid_frames);
            }
        }
    }

    pub(crate) fn record_pixel_profile(&mut self, profile: PixelProfile) {
        match profile.components {
            1 => self.routes.gray_frames = self.routes.gray_frames.saturating_add(1),
            3 => self.routes.rgb_like_frames = self.routes.rgb_like_frames.saturating_add(1),
            _ => {
                self.routes.other_component_frames =
                    self.routes.other_component_frames.saturating_add(1);
            }
        }
        match profile.bits_allocated {
            8 => self.routes.bits8_frames = self.routes.bits8_frames.saturating_add(1),
            16 => self.routes.bits16_frames = self.routes.bits16_frames.saturating_add(1),
            _ => {
                self.routes.other_bit_depth_frames =
                    self.routes.other_bit_depth_frames.saturating_add(1)
            }
        }
    }

    pub(crate) fn record_unknown_pixel_profile(&mut self) {
        increment_u64(&mut self.routes.unknown_pixel_profile_frames);
    }

    pub(crate) fn record_transcode_route(&mut self, used_gpu_input: bool, used_gpu_encode: bool) {
        if used_gpu_input || used_gpu_encode {
            self.routes.gpu_transcode_frames = self.routes.gpu_transcode_frames.saturating_add(1);
            if used_gpu_input && used_gpu_encode {
                self.routes.resident_gpu_transcode_frames =
                    self.routes.resident_gpu_transcode_frames.saturating_add(1);
            } else {
                self.routes.partial_gpu_transcode_frames =
                    self.routes.partial_gpu_transcode_frames.saturating_add(1);
            }
        } else {
            self.routes.cpu_fallback_frames = self.routes.cpu_fallback_frames.saturating_add(1);
        }
    }

    pub(crate) fn record_gpu_batches(
        &mut self,
        input_decode_batches: u64,
        compose_batches: u64,
        encode_batches: u64,
    ) {
        self.routes.gpu_input_decode_batches = self
            .routes
            .gpu_input_decode_batches
            .saturating_add(input_decode_batches);
        self.routes.gpu_compose_batches = self
            .routes
            .gpu_compose_batches
            .saturating_add(compose_batches);
        self.routes.gpu_encode_batches = self
            .routes
            .gpu_encode_batches
            .saturating_add(encode_batches);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn record_gpu_encode_batch_stats(
        &mut self,
        stats: encode::DicomJ2kGpuEncodeBatchStats,
    ) {
        self.gpu_encode.gpu_encode_configured_inflight_tiles = self
            .gpu_encode
            .gpu_encode_configured_inflight_tiles
            .max(stats.configured_inflight_tiles.unwrap_or(0) as u64);
        self.gpu_encode.gpu_encode_effective_inflight_tiles = self
            .gpu_encode
            .gpu_encode_effective_inflight_tiles
            .max(stats.effective_inflight_tiles as u64);
        self.gpu_encode.gpu_encode_max_observed_inflight_tiles = self
            .gpu_encode
            .gpu_encode_max_observed_inflight_tiles
            .max(stats.max_observed_inflight_tiles as u64);
        self.gpu_encode.gpu_encode_configured_memory_mib = self
            .gpu_encode
            .gpu_encode_configured_memory_mib
            .max(stats.configured_memory_mib.unwrap_or(0));
        self.gpu_encode.gpu_encode_effective_memory_mib = self
            .gpu_encode
            .gpu_encode_effective_memory_mib
            .max(stats.effective_memory_mib);
        self.gpu_encode.gpu_encode_wall_micros = self
            .gpu_encode
            .gpu_encode_wall_micros
            .saturating_add(duration_as_reported_micros(stats.encode_wall_duration));
        self.gpu_encode.gpu_encode_plan_micros = self
            .gpu_encode
            .gpu_encode_plan_micros
            .saturating_add(duration_as_reported_micros(stats.stage_stats.plan_duration));
        self.gpu_encode.gpu_encode_prepare_submit_micros = self
            .gpu_encode
            .gpu_encode_prepare_submit_micros
            .saturating_add(duration_as_reported_micros(
                stats.stage_stats.prepare_submit_duration,
            ));
        self.gpu_encode.gpu_encode_ht_table_build_micros = self
            .gpu_encode
            .gpu_encode_ht_table_build_micros
            .saturating_add(duration_as_reported_micros(
                stats.stage_stats.ht_table_build_duration,
            ));
        self.gpu_encode.gpu_encode_ht_buffer_allocation_micros = self
            .gpu_encode
            .gpu_encode_ht_buffer_allocation_micros
            .saturating_add(duration_as_reported_micros(
                stats.stage_stats.ht_buffer_allocation_duration,
            ));
        self.gpu_encode.gpu_encode_ht_command_encode_micros = self
            .gpu_encode
            .gpu_encode_ht_command_encode_micros
            .saturating_add(duration_as_reported_micros(
                stats.stage_stats.ht_command_encode_duration,
            ));
        self.gpu_encode.gpu_encode_codestream_wait_micros = self
            .gpu_encode
            .gpu_encode_codestream_wait_micros
            .saturating_add(duration_as_reported_micros(
                stats.stage_stats.codestream_wait_duration,
            ));
        self.gpu_encode.gpu_encode_chunk_count = self
            .gpu_encode
            .gpu_encode_chunk_count
            .saturating_add(stats.stage_stats.chunk_count as u64);
        self.gpu_encode.gpu_encode_tile_count = self
            .gpu_encode
            .gpu_encode_tile_count
            .saturating_add(stats.stage_stats.tile_count as u64);
        self.gpu_encode.gpu_encode_code_block_count = self
            .gpu_encode
            .gpu_encode_code_block_count
            .saturating_add(stats.stage_stats.code_block_count as u64);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn record_auto_route_probe(
        &mut self,
        frames: u64,
        cpu_duration: Duration,
        gpu_duration: Duration,
        gpu_batches: u64,
        selected_gpu_input: bool,
    ) {
        self.routes.auto_route_probe_frames =
            self.routes.auto_route_probe_frames.saturating_add(frames);
        self.routes.auto_route_probe_gpu_batches = self
            .routes
            .auto_route_probe_gpu_batches
            .saturating_add(gpu_batches);
        self.routes.auto_route_probe_cpu_micros = self
            .routes
            .auto_route_probe_cpu_micros
            .saturating_add(duration_as_reported_micros(cpu_duration));
        self.routes.auto_route_probe_gpu_micros = self
            .routes
            .auto_route_probe_gpu_micros
            .saturating_add(duration_as_reported_micros(gpu_duration));
        if selected_gpu_input {
            self.routes.auto_route_probe_selected_gpu_input_frames = self
                .routes
                .auto_route_probe_selected_gpu_input_frames
                .saturating_add(frames);
        }
    }

    pub(crate) fn record_jpeg_decode_fallback(&mut self) {
        increment_u64(&mut self.routes.jpeg_decode_fallback_frames);
    }

    pub(crate) fn record_jpeg_cpu_fallback_route_classification(&mut self) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.routes.cpu_fallback_frames);
        increment_u64(&mut self.routes.jpeg_decode_fallback_frames);
        self.record_unknown_pixel_profile();
    }

    pub(crate) fn record_j2k_passthrough_only_fallback_classification(&mut self) {
        increment_u64(&mut self.routes.total_frames);
        increment_u64(&mut self.routes.cpu_fallback_frames);
        self.record_unknown_pixel_profile();
    }

    pub(crate) fn record_jpeg_cpu_encode(&mut self, duration: Duration) {
        increment_u64(&mut self.routes.jpeg_cpu_encode_frames);
        self.record_encode_duration(duration);
    }

    pub(crate) fn record_jpeg_metal_batch_encode(&mut self, frames: u64, duration: Duration) {
        self.routes.jpeg_metal_encode_frames =
            self.routes.jpeg_metal_encode_frames.saturating_add(frames);
        self.record_encode_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    pub(crate) fn record_encoded_frame(&mut self, encoded: &encode::EncodedDicomJ2kFrame) {
        if encoded.used_device_encode {
            increment_u64(&mut self.routes.gpu_encode_frames);
            if let Some(duration) = encoded.gpu_encode_wall_duration {
                self.record_gpu_encode_wall_duration(duration);
            }
            self.record_gpu_dispatch_duration(encoded.encode_duration);
            self.record_gpu_encode_hardware_duration(
                encoded.device_gpu_duration,
                encoded.encode_duration,
            );
        }
        if encoded.used_device_validation {
            increment_u64(&mut self.routes.gpu_validation_frames);
            self.record_gpu_dispatch_duration(encoded.validation_duration);
        }
        self.record_encode_duration(encoded.encode_duration);
        self.record_validation_duration(encoded.validation_duration);
    }

    pub(crate) fn record_input_decode_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.timings.input_decode_micros, duration);
    }

    pub(crate) fn record_gpu_input_decode_duration(&mut self, duration: Duration) {
        self.record_input_decode_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    pub(crate) fn record_compose_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.timings.compose_micros, duration);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn record_gpu_compose_duration(&mut self, duration: Duration) {
        self.record_compose_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    pub(crate) fn record_encode_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.timings.encode_micros, duration);
    }

    pub(crate) fn record_validation_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.timings.validation_micros, duration);
    }

    pub(crate) fn record_gpu_dispatch_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.timings.gpu_dispatch_micros, duration);
    }

    pub(crate) fn record_gpu_encode_hardware_duration(
        &mut self,
        gpu_duration: Option<Duration>,
        dispatch_duration: Duration,
    ) {
        let Some(gpu_duration) = gpu_duration else {
            return;
        };
        let hardware_micros = duration_as_reported_micros(gpu_duration);
        self.gpu_encode.gpu_encode_hardware_micros = self
            .gpu_encode
            .gpu_encode_hardware_micros
            .saturating_add(hardware_micros);
        let overhead = dispatch_duration.saturating_sub(gpu_duration);
        self.gpu_encode.gpu_encode_dispatch_overhead_micros = self
            .gpu_encode
            .gpu_encode_dispatch_overhead_micros
            .saturating_add(duration_as_reported_micros(overhead));
    }

    pub(crate) fn record_gpu_encode_wall_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.gpu_encode.gpu_encode_wall_micros, duration);
    }

    pub(crate) fn record_write_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.timings.write_micros, duration);
    }

    pub(crate) fn record_streaming_write_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.timings.streaming_write_micros, duration);
    }

    pub(crate) fn record_pixel_data_patch_duration(&mut self, duration: Duration) {
        add_duration_micros(&mut self.timings.pixel_data_patch_micros, duration);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn record_gpu_pipeline_depth(&mut self, depth: usize) {
        self.gpu_encode.gpu_pipeline_depth = self.gpu_encode.gpu_pipeline_depth.max(depth as u64);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn record_gpu_row_batch_config(&mut self, rows: usize, target_tiles: Option<usize>) {
        self.gpu_encode.gpu_row_batch_rows_max =
            self.gpu_encode.gpu_row_batch_rows_max.max(rows as u64);
        self.gpu_encode.gpu_row_batch_target_tiles = self
            .gpu_encode
            .gpu_row_batch_target_tiles
            .max(target_tiles.unwrap_or(0) as u64);
    }

    /// Ratio of summed GPU encode hardware time to observed GPU encode wall time.
    pub fn gpu_encode_effective_parallelism(&self) -> f64 {
        if self.gpu_encode.gpu_encode_wall_micros == 0 {
            0.0
        } else {
            self.gpu_encode.gpu_encode_hardware_micros as f64
                / self.gpu_encode.gpu_encode_wall_micros as f64
        }
    }
}

fn increment_u64(value: &mut u64) {
    *value = value.saturating_add(1);
}

fn add_duration_micros(value: &mut u128, duration: Duration) {
    *value = value.saturating_add(duration_as_reported_micros(duration));
}

#[cfg(test)]
mod tests {
    use super::{
        ExportMetrics, GpuEncodeMetrics, JpegDirectHtj2kMetrics, JpegRetileRejectionReason,
        RouteCounters, WriteTimings,
    };
    use crate::tile::PixelProfile;
    use j2k_transcode::TranscodeTimingReport;
    use std::time::Duration;

    #[test]
    fn dicom_export_metrics_serializes_stable_public_fields() {
        let metrics = ExportMetrics {
            routes: RouteCounters {
                total_frames: 10,
                cpu_fallback_frames: 4,
                ..RouteCounters::default()
            },
            jpeg_direct_htj2k: JpegDirectHtj2kMetrics {
                jpeg_direct_htj2k_rejected_frames: 3,
                ..JpegDirectHtj2kMetrics::default()
            },
            gpu_encode: GpuEncodeMetrics {
                gpu_encode_wall_micros: 5_000,
                gpu_encode_hardware_micros: 2_500,
                ..GpuEncodeMetrics::default()
            },
            ..ExportMetrics::default()
        };

        let value = serde_json::to_value(metrics).expect("serialize metrics");

        assert_eq!(value["total_frames"], 10);
        assert_eq!(value["jpeg_direct_htj2k_rejected_frames"], 3);
        assert_eq!(value["cpu_fallback_frames"], 4);
        assert_eq!(value["gpu_encode_wall_micros"], 5_000);
        assert_eq!(value["gpu_encode_hardware_micros"], 2_500);
        assert_eq!(value["gpu_encode_effective_parallelism"], 0.5);
        assert!(value.get("jpeg_retile_to_htj2k_53_frames").is_some());
        assert!(value.get("jpeg_retile_source_unsupported_frames").is_some());
        assert!(value.get("jpeg_retile_geometry_mismatch_frames").is_some());
        assert!(value
            .get("jpeg_retile_profile_unsupported_frames")
            .is_some());
        assert!(value.get("jpeg_retile_mcu_invalid_frames").is_some());
        assert!(value.get("writer_backpressure_micros").is_some());
        assert_eq!(
            value
                .as_object()
                .expect("metrics serialize as object")
                .len(),
            ExportMetrics::SERIALIZED_FIELD_COUNT
        );
    }

    #[test]
    fn metrics_aggregation_saturates_counters_and_preserves_configuration_maxima() {
        let mut aggregate = ExportMetrics {
            routes: RouteCounters {
                total_frames: u64::MAX,
                jpeg_retile_frames: 2,
                ..RouteCounters::default()
            },
            jpeg_direct_htj2k: JpegDirectHtj2kMetrics {
                jpeg_direct_htj2k_53_frames: 3,
                ..JpegDirectHtj2kMetrics::default()
            },
            gpu_encode: GpuEncodeMetrics {
                gpu_encode_configured_inflight_tiles: 8,
                gpu_encode_wall_micros: 5,
                ..GpuEncodeMetrics::default()
            },
            ..ExportMetrics::default()
        };
        let mut next = ExportMetrics {
            routes: RouteCounters {
                total_frames: 1,
                jpeg_retile_frames: 7,
                ..RouteCounters::default()
            },
            jpeg_direct_htj2k: JpegDirectHtj2kMetrics {
                jpeg_direct_htj2k_53_frames: 11,
                ..JpegDirectHtj2kMetrics::default()
            },
            gpu_encode: GpuEncodeMetrics {
                gpu_encode_configured_inflight_tiles: 4,
                gpu_encode_effective_memory_mib: 32,
                gpu_encode_wall_micros: 13,
                ..GpuEncodeMetrics::default()
            },
            ..ExportMetrics::default()
        };
        next.timings.write_micros = 17;

        aggregate.add_assign(next);

        assert_eq!(aggregate.routes.total_frames, u64::MAX);
        assert_eq!(aggregate.routes.jpeg_retile_frames, 9);
        assert_eq!(aggregate.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames, 14);
        assert_eq!(aggregate.timings.write_micros, 17);
        assert_eq!(aggregate.gpu_encode.gpu_encode_wall_micros, 18);
        assert_eq!(aggregate.gpu_encode.gpu_encode_configured_inflight_tiles, 8);
        assert_eq!(aggregate.gpu_encode.gpu_encode_effective_memory_mib, 32);
    }

    fn fully_populated_metrics() -> ExportMetrics {
        ExportMetrics {
            routes: RouteCounters {
                total_frames: 1,
                cpu_input_frames: 1,
                gpu_input_decode_frames: 1,
                gpu_encode_frames: 1,
                gpu_validation_frames: 1,
                gray_frames: 1,
                rgb_like_frames: 1,
                other_component_frames: 1,
                unknown_pixel_profile_frames: 1,
                bits8_frames: 1,
                bits16_frames: 1,
                other_bit_depth_frames: 1,
                gpu_transcode_frames: 1,
                resident_gpu_transcode_frames: 1,
                partial_gpu_transcode_frames: 1,
                gpu_input_decode_batches: 1,
                gpu_compose_batches: 1,
                gpu_encode_batches: 1,
                auto_route_probe_frames: 1,
                auto_route_probe_gpu_batches: 1,
                auto_route_probe_cpu_micros: 1,
                auto_route_probe_gpu_micros: 1,
                auto_route_probe_selected_gpu_input_frames: 1,
                cpu_fallback_frames: 1,
                jpeg_passthrough_frames: 1,
                j2k_passthrough_frames: 1,
                j2k_direct_htj2k_frames: 1,
                jpeg_retile_frames: 1,
                jpeg_retile_rejected_frames: 1,
                jpeg_retile_source_unsupported_frames: 1,
                jpeg_retile_geometry_mismatch_frames: 1,
                jpeg_retile_profile_unsupported_frames: 1,
                jpeg_retile_mcu_invalid_frames: 1,
                jpeg_retile_us: 1,
                jpeg_retile_to_htj2k_53_frames: 1,
                jpeg_cpu_encode_frames: 1,
                jpeg_metal_encode_frames: 1,
                jpeg_decode_fallback_frames: 1,
            },
            jpeg_direct_htj2k: JpegDirectHtj2kMetrics {
                jpeg_direct_htj2k_53_frames: 1,
                jpeg_direct_htj2k_97_frames: 1,
                jpeg_direct_htj2k_rejected_frames: 1,
                jpeg_direct_htj2k_extract_micros: 1,
                jpeg_direct_htj2k_repack_micros: 1,
                jpeg_direct_htj2k_transform_micros: 1,
                jpeg_direct_htj2k_accelerator_micros: 1,
                jpeg_direct_htj2k_cpu_fallback_micros: 1,
                jpeg_direct_htj2k_dwt_decompose_micros: 1,
                jpeg_direct_htj2k_dwt97_pack_upload_micros: 1,
                jpeg_direct_htj2k_dwt97_idct_row_lift_micros: 1,
                jpeg_direct_htj2k_dwt97_column_lift_micros: 1,
                jpeg_direct_htj2k_dwt97_quantize_codeblock_micros: 1,
                jpeg_direct_htj2k_dwt97_ht_encode_micros: 1,
                jpeg_direct_htj2k_dwt97_ht_kernel_micros: 1,
                jpeg_direct_htj2k_dwt97_ht_status_readback_micros: 1,
                jpeg_direct_htj2k_dwt97_ht_compact_micros: 1,
                jpeg_direct_htj2k_dwt97_ht_output_readback_micros: 1,
                jpeg_direct_htj2k_dwt97_ht_codeblock_dispatches: 1,
                jpeg_direct_htj2k_dwt97_readback_micros: 1,
                jpeg_direct_htj2k_htj2k_encode_micros: 1,
                jpeg_direct_htj2k_encode_accelerator_dispatches: 1,
                jpeg_direct_htj2k_encode_ht_code_block_dispatches: 1,
                jpeg_direct_htj2k_encode_packetization_dispatches: 1,
                jpeg_direct_htj2k_batch_count: 1,
                jpeg_direct_htj2k_batch_jobs: 1,
                jpeg_direct_htj2k_accelerator_attempts: 1,
                jpeg_direct_htj2k_accelerator_jobs: 1,
                jpeg_direct_htj2k_accelerator_dispatches: 1,
                jpeg_direct_htj2k_accelerator_dispatched_jobs: 1,
                jpeg_direct_htj2k_cpu_fallback_jobs: 1,
            },
            gpu_encode: GpuEncodeMetrics {
                gpu_encode_configured_inflight_tiles: 1,
                gpu_encode_effective_inflight_tiles: 1,
                gpu_encode_max_observed_inflight_tiles: 1,
                gpu_encode_configured_memory_mib: 1,
                gpu_encode_effective_memory_mib: 1,
                gpu_encode_wall_micros: 1,
                gpu_encode_hardware_micros: 1,
                gpu_encode_dispatch_overhead_micros: 1,
                gpu_encode_plan_micros: 1,
                gpu_encode_prepare_submit_micros: 1,
                gpu_encode_ht_table_build_micros: 1,
                gpu_encode_ht_buffer_allocation_micros: 1,
                gpu_encode_ht_command_encode_micros: 1,
                gpu_encode_codestream_wait_micros: 1,
                gpu_encode_chunk_count: 1,
                gpu_encode_tile_count: 1,
                gpu_encode_code_block_count: 1,
                gpu_pipeline_depth: 1,
                gpu_row_batch_rows_max: 1,
                gpu_row_batch_target_tiles: 1,
            },
            timings: WriteTimings {
                input_decode_micros: 1,
                compose_micros: 1,
                encode_micros: 1,
                validation_micros: 1,
                gpu_dispatch_micros: 1,
                streaming_write_micros: 1,
                pixel_data_patch_micros: 1,
                writer_backpressure_micros: 1,
                write_micros: 1,
            },
        }
    }

    #[test]
    fn every_serialized_metric_field_has_aggregation_behavior() {
        let mut aggregate = ExportMetrics::default();

        aggregate.add_assign(fully_populated_metrics());

        let aggregated = serde_json::to_value(aggregate).expect("serialize aggregate metrics");
        for (field, value) in aggregated
            .as_object()
            .expect("metrics serialize as a flat object")
        {
            assert_eq!(
                value.as_f64(),
                Some(1.0),
                "serialized metric `{field}` was not aggregated"
            );
        }
    }

    #[test]
    fn metrics_record_route_pixel_and_timing_counters() {
        let mut metrics = ExportMetrics::default();

        metrics.record_cpu_input();
        metrics.record_gpu_input();
        metrics.record_passthrough_frame();
        metrics.record_j2k_passthrough_frame();
        metrics.record_j2k_direct_htj2k_frame(11);
        metrics.record_jpeg_direct_htj2k_53_frame(13);
        metrics.record_jpeg_direct_htj2k_97_frame(17);
        metrics.record_jpeg_direct_htj2k_rejected_frame();
        metrics.record_jpeg_retile_baseline_frame(Duration::from_micros(19));
        metrics.record_jpeg_retile_to_htj2k_53_frame(Duration::from_micros(23));
        metrics.record_jpeg_retile_rejected_frame(JpegRetileRejectionReason::SourceUnsupported);
        metrics.record_jpeg_retile_rejected_frame(JpegRetileRejectionReason::GeometryMismatch);
        metrics.record_jpeg_retile_rejected_frame(JpegRetileRejectionReason::ProfileUnsupported);
        metrics.record_jpeg_retile_rejected_frame(JpegRetileRejectionReason::McuInvalid);
        metrics.record_pixel_profile(PixelProfile {
            components: 1,
            bits_allocated: 8,
            photometric_interpretation: "MONOCHROME2",
        });
        metrics.record_pixel_profile(PixelProfile {
            components: 3,
            bits_allocated: 16,
            photometric_interpretation: "RGB",
        });
        metrics.record_pixel_profile(PixelProfile {
            components: 4,
            bits_allocated: 12,
            photometric_interpretation: "RGBA",
        });
        metrics.record_unknown_pixel_profile();
        metrics.record_transcode_route(true, true);
        metrics.record_transcode_route(true, false);
        metrics.record_transcode_route(false, false);
        metrics.record_gpu_batches(2, 3, 4);
        metrics.record_jpeg_decode_fallback();
        metrics.record_jpeg_cpu_fallback_route_classification();
        metrics.record_j2k_passthrough_only_fallback_classification();
        metrics.record_jpeg_cpu_encode(Duration::from_micros(29));
        metrics.record_jpeg_metal_batch_encode(5, Duration::from_micros(31));
        metrics.record_input_decode_duration(Duration::from_micros(37));
        metrics.record_gpu_input_decode_duration(Duration::from_micros(41));
        metrics.record_compose_duration(Duration::from_micros(43));
        metrics.record_encode_duration(Duration::from_micros(47));
        metrics.record_validation_duration(Duration::from_micros(53));
        metrics.record_gpu_dispatch_duration(Duration::from_micros(59));
        metrics.record_gpu_encode_hardware_duration(
            Some(Duration::from_micros(61)),
            Duration::from_micros(67),
        );
        metrics.record_gpu_encode_wall_duration(Duration::from_micros(69));
        metrics.record_write_duration(Duration::from_micros(71));
        metrics.record_streaming_write_duration(Duration::from_micros(73));
        metrics.record_pixel_data_patch_duration(Duration::from_micros(79));

        metrics.record_jpeg_direct_htj2k_timings(TranscodeTimingReport {
            source_raw_probe_us: 0,
            read_region_decode_us: 0,
            compose_pad_us: 0,
            generated_jpeg_encode_us: 0,
            jpeg_dct_extract_us: 1,
            jpeg_dct_repack_us: 2,
            dct_to_wavelet_total_us: 3,
            dct_to_wavelet_accelerator_us: 4,
            dct_to_wavelet_cpu_fallback_us: 5,
            dwt_decompose_us: 6,
            dwt97_batch_pack_upload_us: 7,
            dwt97_batch_pack_upload_transfers: 0,
            dwt97_batch_pack_upload_bytes: 0,
            dwt97_batch_resident_dct_handoff_count: 0,
            dwt97_batch_idct_row_lift_us: 8,
            dwt97_batch_column_lift_us: 9,
            dwt97_batch_resident_dwt_handoff_count: 0,
            dwt97_batch_quantize_codeblock_us: 10,
            dwt97_batch_ht_encode_us: 11,
            dwt97_batch_ht_kernel_us: 12,
            dwt97_batch_ht_status_readback_us: 13,
            dwt97_batch_ht_status_readback_transfers: 0,
            dwt97_batch_ht_status_readback_bytes: 0,
            dwt97_batch_ht_compact_us: 14,
            dwt97_batch_ht_output_readback_us: 15,
            dwt97_batch_ht_output_readback_transfers: 0,
            dwt97_batch_ht_output_readback_bytes: 0,
            dwt97_batch_ht_codeblock_dispatches: 16,
            dwt97_batch_readback_us: 17,
            dwt97_batch_readback_transfers: 0,
            dwt97_batch_readback_bytes: 0,
            htj2k_encode_us: 18,
            htj2k_encode_accelerator_dispatches: 19,
            htj2k_encode_ht_code_block_dispatches: 20,
            htj2k_encode_packetization_dispatches: 21,
            dicom_spool_write_us: 0,
            dicom_final_write_us: 0,
            tile_count: 0,
            component_count: 0,
            batch_count: 22,
            batch_jobs: 23,
            accelerator_attempts: 24,
            accelerator_jobs: 25,
            accelerator_dispatches: 26,
            accelerator_dispatched_jobs: 27,
            cpu_fallback_jobs: 28,
        });

        assert_eq!(metrics.routes.total_frames, 10);
        assert_eq!(metrics.route_passthrough_frames(), 2);
        assert_eq!(metrics.routes.j2k_direct_htj2k_frames, 1);
        assert_eq!(metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames, 1);
        assert_eq!(metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_97_frames, 1);
        assert_eq!(
            metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_rejected_frames,
            1
        );
        assert_eq!(metrics.jpeg_retile_baseline_frames(), 1);
        assert_eq!(metrics.routes.jpeg_retile_to_htj2k_53_frames, 1);
        assert_eq!(metrics.routes.jpeg_retile_rejected_frames, 4);
        assert_eq!(metrics.routes.jpeg_retile_source_unsupported_frames, 1);
        assert_eq!(metrics.routes.jpeg_retile_geometry_mismatch_frames, 1);
        assert_eq!(metrics.routes.jpeg_retile_profile_unsupported_frames, 1);
        assert_eq!(metrics.routes.jpeg_retile_mcu_invalid_frames, 1);
        assert_eq!(metrics.routes.gray_frames, 1);
        assert_eq!(metrics.routes.rgb_like_frames, 1);
        assert_eq!(metrics.routes.other_component_frames, 1);
        assert_eq!(metrics.routes.bits8_frames, 1);
        assert_eq!(metrics.routes.bits16_frames, 1);
        assert_eq!(metrics.routes.other_bit_depth_frames, 1);
        assert_eq!(metrics.routes.unknown_pixel_profile_frames, 3);
        assert_eq!(metrics.routes.gpu_transcode_frames, 2);
        assert_eq!(metrics.routes.resident_gpu_transcode_frames, 1);
        assert_eq!(metrics.routes.partial_gpu_transcode_frames, 1);
        assert_eq!(metrics.routes.cpu_fallback_frames, 3);
        assert_eq!(metrics.routes.gpu_input_decode_batches, 2);
        assert_eq!(metrics.routes.gpu_compose_batches, 3);
        assert_eq!(metrics.routes.gpu_encode_batches, 4);
        assert_eq!(metrics.routes.jpeg_decode_fallback_frames, 2);
        assert_eq!(metrics.routes.jpeg_cpu_encode_frames, 1);
        assert_eq!(metrics.routes.jpeg_metal_encode_frames, 5);
        assert_eq!(
            metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_extract_micros,
            1
        );
        assert_eq!(
            metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_encode_packetization_dispatches,
            21
        );
        assert_eq!(
            metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_dwt97_ht_encode_micros,
            11
        );
        assert_eq!(
            metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_dwt97_ht_kernel_micros,
            12
        );
        assert_eq!(
            metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_dwt97_ht_status_readback_micros,
            13
        );
        assert_eq!(
            metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_dwt97_ht_compact_micros,
            14
        );
        assert_eq!(
            metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_dwt97_ht_output_readback_micros,
            15
        );
        assert_eq!(
            metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_dwt97_ht_codeblock_dispatches,
            16
        );
        assert_eq!(
            metrics
                .jpeg_direct_htj2k
                .jpeg_direct_htj2k_cpu_fallback_jobs,
            28
        );
        assert_eq!(metrics.gpu_encode.gpu_encode_wall_micros, 69);
        assert_eq!(metrics.gpu_encode.gpu_encode_hardware_micros, 61);
        assert_eq!(metrics.gpu_encode.gpu_encode_dispatch_overhead_micros, 6);
        assert_eq!(metrics.timings.streaming_write_micros, 73);
        assert_eq!(metrics.timings.pixel_data_patch_micros, 79);
        assert_eq!(metrics.route_unclassified_frames(), 0);
    }
}
