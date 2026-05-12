//! Report types returned by export and route profiling APIs.

use std::path::PathBuf;
use std::time::Duration;

use serde::{ser::SerializeStruct, Serialize};

use crate::encode;
use crate::tile::PixelProfile;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomExportReport {
    pub output_dir: PathBuf,
    pub instances: Vec<DicomInstanceReport>,
    pub metrics: DicomExportMetrics,
}

/// Finished compressed frame bytes ready for DICOM encapsulated Pixel Data insertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomEncodedFrame {
    pub transfer_syntax_uid: &'static str,
    pub bytes: Vec<u8>,
    pub used_device_encode: bool,
    pub used_device_validation: bool,
    pub encode_micros: u128,
    pub validation_micros: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomRouteProfileReport {
    pub source_path: PathBuf,
    pub transfer_syntax_uid: &'static str,
    pub level: u32,
    pub requested_frames: u64,
    pub available_frames: u64,
    pub metrics: DicomExportMetrics,
    pub elapsed_micros: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomRouteCoverageReport {
    pub source_path: PathBuf,
    pub transfer_syntax_uid: &'static str,
    pub requested_frames_per_level: u64,
    pub available_frames: u64,
    pub complete_frame_coverage: bool,
    pub levels: Vec<DicomRouteProfileReport>,
    pub metrics: DicomExportMetrics,
    pub elapsed_micros: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomRouteCorpusCoverageFailure {
    pub source_path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomRouteCorpusCoverageReport {
    pub source_root: PathBuf,
    pub transfer_syntax_uid: &'static str,
    pub requested_frames_per_level: u64,
    pub max_levels: Option<u32>,
    pub sources_considered: usize,
    pub available_frames: u64,
    pub complete_frame_coverage: bool,
    pub reports: Vec<DicomRouteCoverageReport>,
    pub failures: Vec<DicomRouteCorpusCoverageFailure>,
    pub metrics: DicomExportMetrics,
    pub elapsed_micros: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DicomInstanceReport {
    pub path: PathBuf,
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub transfer_syntax_uid: &'static str,
    pub level: u32,
    pub z: u32,
    pub c: u32,
    pub t: u32,
    pub frame_count: u32,
    pub metrics: DicomExportMetrics,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DicomExportMetrics {
    pub total_frames: u64,
    pub cpu_input_frames: u64,
    pub gpu_input_decode_frames: u64,
    pub gpu_encode_frames: u64,
    pub gpu_validation_frames: u64,
    pub gray_frames: u64,
    pub rgb_like_frames: u64,
    pub other_component_frames: u64,
    pub unknown_pixel_profile_frames: u64,
    pub bits8_frames: u64,
    pub bits16_frames: u64,
    pub other_bit_depth_frames: u64,
    pub gpu_transcode_frames: u64,
    pub resident_gpu_transcode_frames: u64,
    pub partial_gpu_transcode_frames: u64,
    pub gpu_input_decode_batches: u64,
    pub gpu_compose_batches: u64,
    pub gpu_encode_batches: u64,
    pub auto_route_probe_frames: u64,
    pub auto_route_probe_gpu_batches: u64,
    pub auto_route_probe_cpu_micros: u128,
    pub auto_route_probe_gpu_micros: u128,
    pub auto_route_probe_selected_gpu_input_frames: u64,
    pub cpu_fallback_frames: u64,
    pub jpeg_passthrough_frames: u64,
    pub j2k_passthrough_frames: u64,
    pub jpeg_cpu_encode_frames: u64,
    pub jpeg_metal_encode_frames: u64,
    pub jpeg_decode_fallback_frames: u64,
    pub input_decode_micros: u128,
    pub compose_micros: u128,
    pub encode_micros: u128,
    pub validation_micros: u128,
    pub gpu_dispatch_micros: u128,
    pub gpu_encode_configured_inflight_tiles: u64,
    pub gpu_encode_effective_inflight_tiles: u64,
    pub gpu_encode_max_observed_inflight_tiles: u64,
    pub gpu_encode_configured_memory_mib: u64,
    pub gpu_encode_effective_memory_mib: u64,
    pub gpu_encode_wall_micros: u128,
    pub gpu_encode_hardware_micros: u128,
    pub gpu_encode_dispatch_overhead_micros: u128,
    pub write_micros: u128,
}

impl Serialize for DicomExportMetrics {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("DicomExportMetrics", 43)?;
        state.serialize_field("total_frames", &self.total_frames)?;
        state.serialize_field("cpu_input_frames", &self.cpu_input_frames)?;
        state.serialize_field("gpu_input_decode_frames", &self.gpu_input_decode_frames)?;
        state.serialize_field("gpu_encode_frames", &self.gpu_encode_frames)?;
        state.serialize_field("gpu_validation_frames", &self.gpu_validation_frames)?;
        state.serialize_field("gray_frames", &self.gray_frames)?;
        state.serialize_field("rgb_like_frames", &self.rgb_like_frames)?;
        state.serialize_field("other_component_frames", &self.other_component_frames)?;
        state.serialize_field(
            "unknown_pixel_profile_frames",
            &self.unknown_pixel_profile_frames,
        )?;
        state.serialize_field("bits8_frames", &self.bits8_frames)?;
        state.serialize_field("bits16_frames", &self.bits16_frames)?;
        state.serialize_field("other_bit_depth_frames", &self.other_bit_depth_frames)?;
        state.serialize_field("gpu_transcode_frames", &self.gpu_transcode_frames)?;
        state.serialize_field(
            "resident_gpu_transcode_frames",
            &self.resident_gpu_transcode_frames,
        )?;
        state.serialize_field(
            "partial_gpu_transcode_frames",
            &self.partial_gpu_transcode_frames,
        )?;
        state.serialize_field("gpu_input_decode_batches", &self.gpu_input_decode_batches)?;
        state.serialize_field("gpu_compose_batches", &self.gpu_compose_batches)?;
        state.serialize_field("gpu_encode_batches", &self.gpu_encode_batches)?;
        state.serialize_field("auto_route_probe_frames", &self.auto_route_probe_frames)?;
        state.serialize_field(
            "auto_route_probe_gpu_batches",
            &self.auto_route_probe_gpu_batches,
        )?;
        state.serialize_field(
            "auto_route_probe_cpu_micros",
            &self.auto_route_probe_cpu_micros,
        )?;
        state.serialize_field(
            "auto_route_probe_gpu_micros",
            &self.auto_route_probe_gpu_micros,
        )?;
        state.serialize_field(
            "auto_route_probe_selected_gpu_input_frames",
            &self.auto_route_probe_selected_gpu_input_frames,
        )?;
        state.serialize_field("cpu_fallback_frames", &self.cpu_fallback_frames)?;
        state.serialize_field("jpeg_passthrough_frames", &self.jpeg_passthrough_frames)?;
        state.serialize_field("j2k_passthrough_frames", &self.j2k_passthrough_frames)?;
        state.serialize_field("jpeg_cpu_encode_frames", &self.jpeg_cpu_encode_frames)?;
        state.serialize_field("jpeg_metal_encode_frames", &self.jpeg_metal_encode_frames)?;
        state.serialize_field(
            "jpeg_decode_fallback_frames",
            &self.jpeg_decode_fallback_frames,
        )?;
        state.serialize_field("input_decode_micros", &self.input_decode_micros)?;
        state.serialize_field("compose_micros", &self.compose_micros)?;
        state.serialize_field("encode_micros", &self.encode_micros)?;
        state.serialize_field("validation_micros", &self.validation_micros)?;
        state.serialize_field("gpu_dispatch_micros", &self.gpu_dispatch_micros)?;
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
            &self.gpu_encode_effective_parallelism(),
        )?;
        state.serialize_field(
            "gpu_encode_hardware_micros",
            &self.gpu_encode_hardware_micros,
        )?;
        state.serialize_field(
            "gpu_encode_dispatch_overhead_micros",
            &self.gpu_encode_dispatch_overhead_micros,
        )?;
        state.serialize_field("write_micros", &self.write_micros)?;
        state.end()
    }
}

impl DicomExportMetrics {
    pub fn route_passthrough_frames(&self) -> u64 {
        self.jpeg_passthrough_frames
            .saturating_add(self.j2k_passthrough_frames)
    }

    pub fn route_unclassified_frames(&self) -> u64 {
        self.total_frames
            .saturating_sub(self.route_passthrough_frames())
            .saturating_sub(self.gpu_transcode_frames)
            .saturating_sub(self.cpu_fallback_frames)
    }

    pub(crate) fn add_assign(&mut self, other: Self) {
        self.total_frames = self.total_frames.saturating_add(other.total_frames);
        self.cpu_input_frames = self.cpu_input_frames.saturating_add(other.cpu_input_frames);
        self.gpu_input_decode_frames = self
            .gpu_input_decode_frames
            .saturating_add(other.gpu_input_decode_frames);
        self.gpu_encode_frames = self
            .gpu_encode_frames
            .saturating_add(other.gpu_encode_frames);
        self.gpu_validation_frames = self
            .gpu_validation_frames
            .saturating_add(other.gpu_validation_frames);
        self.gray_frames = self.gray_frames.saturating_add(other.gray_frames);
        self.rgb_like_frames = self.rgb_like_frames.saturating_add(other.rgb_like_frames);
        self.other_component_frames = self
            .other_component_frames
            .saturating_add(other.other_component_frames);
        self.unknown_pixel_profile_frames = self
            .unknown_pixel_profile_frames
            .saturating_add(other.unknown_pixel_profile_frames);
        self.bits8_frames = self.bits8_frames.saturating_add(other.bits8_frames);
        self.bits16_frames = self.bits16_frames.saturating_add(other.bits16_frames);
        self.other_bit_depth_frames = self
            .other_bit_depth_frames
            .saturating_add(other.other_bit_depth_frames);
        self.gpu_transcode_frames = self
            .gpu_transcode_frames
            .saturating_add(other.gpu_transcode_frames);
        self.resident_gpu_transcode_frames = self
            .resident_gpu_transcode_frames
            .saturating_add(other.resident_gpu_transcode_frames);
        self.partial_gpu_transcode_frames = self
            .partial_gpu_transcode_frames
            .saturating_add(other.partial_gpu_transcode_frames);
        self.gpu_input_decode_batches = self
            .gpu_input_decode_batches
            .saturating_add(other.gpu_input_decode_batches);
        self.gpu_compose_batches = self
            .gpu_compose_batches
            .saturating_add(other.gpu_compose_batches);
        self.gpu_encode_batches = self
            .gpu_encode_batches
            .saturating_add(other.gpu_encode_batches);
        self.auto_route_probe_frames = self
            .auto_route_probe_frames
            .saturating_add(other.auto_route_probe_frames);
        self.auto_route_probe_gpu_batches = self
            .auto_route_probe_gpu_batches
            .saturating_add(other.auto_route_probe_gpu_batches);
        self.auto_route_probe_cpu_micros = self
            .auto_route_probe_cpu_micros
            .saturating_add(other.auto_route_probe_cpu_micros);
        self.auto_route_probe_gpu_micros = self
            .auto_route_probe_gpu_micros
            .saturating_add(other.auto_route_probe_gpu_micros);
        self.auto_route_probe_selected_gpu_input_frames = self
            .auto_route_probe_selected_gpu_input_frames
            .saturating_add(other.auto_route_probe_selected_gpu_input_frames);
        self.cpu_fallback_frames = self
            .cpu_fallback_frames
            .saturating_add(other.cpu_fallback_frames);
        self.jpeg_passthrough_frames = self
            .jpeg_passthrough_frames
            .saturating_add(other.jpeg_passthrough_frames);
        self.j2k_passthrough_frames = self
            .j2k_passthrough_frames
            .saturating_add(other.j2k_passthrough_frames);
        self.jpeg_cpu_encode_frames = self
            .jpeg_cpu_encode_frames
            .saturating_add(other.jpeg_cpu_encode_frames);
        self.jpeg_metal_encode_frames = self
            .jpeg_metal_encode_frames
            .saturating_add(other.jpeg_metal_encode_frames);
        self.jpeg_decode_fallback_frames = self
            .jpeg_decode_fallback_frames
            .saturating_add(other.jpeg_decode_fallback_frames);
        self.input_decode_micros = self
            .input_decode_micros
            .saturating_add(other.input_decode_micros);
        self.compose_micros = self.compose_micros.saturating_add(other.compose_micros);
        self.encode_micros = self.encode_micros.saturating_add(other.encode_micros);
        self.validation_micros = self
            .validation_micros
            .saturating_add(other.validation_micros);
        self.gpu_dispatch_micros = self
            .gpu_dispatch_micros
            .saturating_add(other.gpu_dispatch_micros);
        self.gpu_encode_configured_inflight_tiles = self
            .gpu_encode_configured_inflight_tiles
            .max(other.gpu_encode_configured_inflight_tiles);
        self.gpu_encode_effective_inflight_tiles = self
            .gpu_encode_effective_inflight_tiles
            .max(other.gpu_encode_effective_inflight_tiles);
        self.gpu_encode_max_observed_inflight_tiles = self
            .gpu_encode_max_observed_inflight_tiles
            .max(other.gpu_encode_max_observed_inflight_tiles);
        self.gpu_encode_configured_memory_mib = self
            .gpu_encode_configured_memory_mib
            .max(other.gpu_encode_configured_memory_mib);
        self.gpu_encode_effective_memory_mib = self
            .gpu_encode_effective_memory_mib
            .max(other.gpu_encode_effective_memory_mib);
        self.gpu_encode_wall_micros = self
            .gpu_encode_wall_micros
            .saturating_add(other.gpu_encode_wall_micros);
        self.gpu_encode_hardware_micros = self
            .gpu_encode_hardware_micros
            .saturating_add(other.gpu_encode_hardware_micros);
        self.gpu_encode_dispatch_overhead_micros = self
            .gpu_encode_dispatch_overhead_micros
            .saturating_add(other.gpu_encode_dispatch_overhead_micros);
        self.write_micros = self.write_micros.saturating_add(other.write_micros);
    }

    pub(crate) fn record_cpu_input(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.cpu_input_frames = self.cpu_input_frames.saturating_add(1);
    }

    pub(crate) fn record_gpu_input(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.gpu_input_decode_frames = self.gpu_input_decode_frames.saturating_add(1);
    }

    pub(crate) fn record_passthrough_frame(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.jpeg_passthrough_frames = self.jpeg_passthrough_frames.saturating_add(1);
    }

    pub(crate) fn record_j2k_passthrough_frame(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.j2k_passthrough_frames = self.j2k_passthrough_frames.saturating_add(1);
    }

    pub(crate) fn record_pixel_profile(&mut self, profile: PixelProfile) {
        match profile.components {
            1 => self.gray_frames = self.gray_frames.saturating_add(1),
            3 => self.rgb_like_frames = self.rgb_like_frames.saturating_add(1),
            _ => {
                self.other_component_frames = self.other_component_frames.saturating_add(1);
            }
        }
        match profile.bits_allocated {
            8 => self.bits8_frames = self.bits8_frames.saturating_add(1),
            16 => self.bits16_frames = self.bits16_frames.saturating_add(1),
            _ => self.other_bit_depth_frames = self.other_bit_depth_frames.saturating_add(1),
        }
    }

    pub(crate) fn record_unknown_pixel_profile(&mut self) {
        self.unknown_pixel_profile_frames = self.unknown_pixel_profile_frames.saturating_add(1);
    }

    pub(crate) fn record_transcode_route(&mut self, used_gpu_input: bool, used_gpu_encode: bool) {
        if used_gpu_input || used_gpu_encode {
            self.gpu_transcode_frames = self.gpu_transcode_frames.saturating_add(1);
            if used_gpu_input && used_gpu_encode {
                self.resident_gpu_transcode_frames =
                    self.resident_gpu_transcode_frames.saturating_add(1);
            } else {
                self.partial_gpu_transcode_frames =
                    self.partial_gpu_transcode_frames.saturating_add(1);
            }
        } else {
            self.cpu_fallback_frames = self.cpu_fallback_frames.saturating_add(1);
        }
    }

    pub(crate) fn record_gpu_batches(
        &mut self,
        input_decode_batches: u64,
        compose_batches: u64,
        encode_batches: u64,
    ) {
        self.gpu_input_decode_batches = self
            .gpu_input_decode_batches
            .saturating_add(input_decode_batches);
        self.gpu_compose_batches = self.gpu_compose_batches.saturating_add(compose_batches);
        self.gpu_encode_batches = self.gpu_encode_batches.saturating_add(encode_batches);
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn record_gpu_encode_batch_stats(
        &mut self,
        stats: encode::DicomJ2kGpuEncodeBatchStats,
    ) {
        self.gpu_encode_configured_inflight_tiles = self
            .gpu_encode_configured_inflight_tiles
            .max(stats.configured_inflight_tiles.unwrap_or(0) as u64);
        self.gpu_encode_effective_inflight_tiles = self
            .gpu_encode_effective_inflight_tiles
            .max(stats.effective_inflight_tiles as u64);
        self.gpu_encode_max_observed_inflight_tiles = self
            .gpu_encode_max_observed_inflight_tiles
            .max(stats.max_observed_inflight_tiles as u64);
        self.gpu_encode_configured_memory_mib = self
            .gpu_encode_configured_memory_mib
            .max(stats.configured_memory_mib.unwrap_or(0));
        self.gpu_encode_effective_memory_mib = self
            .gpu_encode_effective_memory_mib
            .max(stats.effective_memory_mib);
        self.gpu_encode_wall_micros = self
            .gpu_encode_wall_micros
            .saturating_add(duration_as_reported_micros(stats.encode_wall_duration));
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
        self.auto_route_probe_frames = self.auto_route_probe_frames.saturating_add(frames);
        self.auto_route_probe_gpu_batches = self
            .auto_route_probe_gpu_batches
            .saturating_add(gpu_batches);
        self.auto_route_probe_cpu_micros = self
            .auto_route_probe_cpu_micros
            .saturating_add(duration_as_reported_micros(cpu_duration));
        self.auto_route_probe_gpu_micros = self
            .auto_route_probe_gpu_micros
            .saturating_add(duration_as_reported_micros(gpu_duration));
        if selected_gpu_input {
            self.auto_route_probe_selected_gpu_input_frames = self
                .auto_route_probe_selected_gpu_input_frames
                .saturating_add(frames);
        }
    }

    pub(crate) fn record_jpeg_decode_fallback(&mut self) {
        self.jpeg_decode_fallback_frames = self.jpeg_decode_fallback_frames.saturating_add(1);
    }

    pub(crate) fn record_jpeg_cpu_fallback_route_classification(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.cpu_fallback_frames = self.cpu_fallback_frames.saturating_add(1);
        self.jpeg_decode_fallback_frames = self.jpeg_decode_fallback_frames.saturating_add(1);
        self.record_unknown_pixel_profile();
    }

    pub(crate) fn record_j2k_passthrough_only_fallback_classification(&mut self) {
        self.total_frames = self.total_frames.saturating_add(1);
        self.cpu_fallback_frames = self.cpu_fallback_frames.saturating_add(1);
        self.record_unknown_pixel_profile();
    }

    pub(crate) fn record_jpeg_cpu_encode(&mut self, duration: Duration) {
        self.jpeg_cpu_encode_frames = self.jpeg_cpu_encode_frames.saturating_add(1);
        self.record_encode_duration(duration);
    }

    pub(crate) fn record_jpeg_metal_batch_encode(&mut self, frames: u64, duration: Duration) {
        self.jpeg_metal_encode_frames = self.jpeg_metal_encode_frames.saturating_add(frames);
        self.record_encode_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    pub(crate) fn record_encoded_frame(&mut self, encoded: &encode::EncodedDicomJ2kFrame) {
        if encoded.used_device_encode {
            self.gpu_encode_frames = self.gpu_encode_frames.saturating_add(1);
            self.record_gpu_dispatch_duration(encoded.encode_duration);
            self.record_gpu_encode_hardware_duration(
                encoded.device_gpu_duration,
                encoded.encode_duration,
            );
        }
        if encoded.used_device_validation {
            self.gpu_validation_frames = self.gpu_validation_frames.saturating_add(1);
            self.record_gpu_dispatch_duration(encoded.validation_duration);
        }
        self.record_encode_duration(encoded.encode_duration);
        self.record_validation_duration(encoded.validation_duration);
    }

    pub(crate) fn record_input_decode_duration(&mut self, duration: Duration) {
        self.input_decode_micros = self
            .input_decode_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    pub(crate) fn record_gpu_input_decode_duration(&mut self, duration: Duration) {
        self.record_input_decode_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    pub(crate) fn record_compose_duration(&mut self, duration: Duration) {
        self.compose_micros = self
            .compose_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    pub(crate) fn record_gpu_compose_duration(&mut self, duration: Duration) {
        self.record_compose_duration(duration);
        self.record_gpu_dispatch_duration(duration);
    }

    pub(crate) fn record_encode_duration(&mut self, duration: Duration) {
        self.encode_micros = self
            .encode_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    pub(crate) fn record_validation_duration(&mut self, duration: Duration) {
        self.validation_micros = self
            .validation_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    pub(crate) fn record_gpu_dispatch_duration(&mut self, duration: Duration) {
        self.gpu_dispatch_micros = self
            .gpu_dispatch_micros
            .saturating_add(duration_as_reported_micros(duration));
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
        self.gpu_encode_hardware_micros = self
            .gpu_encode_hardware_micros
            .saturating_add(hardware_micros);
        let overhead = dispatch_duration.saturating_sub(gpu_duration);
        self.gpu_encode_dispatch_overhead_micros = self
            .gpu_encode_dispatch_overhead_micros
            .saturating_add(duration_as_reported_micros(overhead));
    }

    pub(crate) fn record_write_duration(&mut self, duration: Duration) {
        self.write_micros = self
            .write_micros
            .saturating_add(duration_as_reported_micros(duration));
    }

    pub fn gpu_encode_effective_parallelism(&self) -> f64 {
        if self.gpu_encode_wall_micros == 0 {
            0.0
        } else {
            self.gpu_encode_hardware_micros as f64 / self.gpu_encode_wall_micros as f64
        }
    }
}

pub(crate) fn duration_as_reported_micros(duration: Duration) -> u128 {
    match duration.as_micros() {
        0 if duration > Duration::ZERO => 1,
        micros => micros,
    }
}
