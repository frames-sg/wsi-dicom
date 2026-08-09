use std::time::Duration;

use super::super::*;
use crate::encode;
use crate::tile::PixelProfile;
use crate::time::duration_as_reported_micros;

macro_rules! saturating_add_values {
    ($target:expr, [$($field:ident => $value:expr),+ $(,)?]) => {
        $(
            $target.$field = $target.$field.saturating_add($value);
        )+
    };
}

impl ExportMetrics {
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
