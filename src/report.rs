//! Report types returned by export and route profiling APIs.

use serde::Serialize;
use std::path::PathBuf;

mod metrics;

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
    /// Optional verified DICOM annotation sidecars created for this WSI export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<crate::AnnotationExportReport>,
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
    /// ICC profile came from a matched portable calibration registry.
    CalibrationRegistry,
    /// ICC profile was supplied directly by the caller.
    ExplicitProfile,
    /// ICC profile was not applicable because the output is MONOCHROME2.
    #[default]
    NotApplicableMonochrome,
}

/// Decision applied between a configured and source ICC profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum IccConflictDecision {
    /// No differing configured/source pair was present.
    #[default]
    NoConflict,
    /// Configured and source profile digests were identical.
    DigestsMatch,
    /// A differing configured profile was selected explicitly.
    PreferredConfigured,
    /// A differing source profile was selected explicitly.
    PreferredSource,
    /// ICC profile handling was not applicable to MONOCHROME2 output.
    NotApplicableMonochrome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IccProfileReport {
    pub(crate) source: IccProfileSource,
    pub(crate) sha256: Option<String>,
    pub(crate) calibration_id: Option<String>,
    pub(crate) conflict_decision: IccConflictDecision,
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
    /// SHA-256 digest of the exact ICC bytes embedded in this instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icc_profile_sha256: Option<String>,
    /// Matched calibration or explicit-profile ID, when configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icc_calibration_id: Option<String>,
    /// Resolution applied between configured and source profiles.
    pub icc_conflict_decision: IccConflictDecision,
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
