use super::super::*;

macro_rules! saturating_add_fields {
    ($target:expr, $source:expr, [$($field:ident),+ $(,)?]) => {
        $(
            $target.$field = $target.$field.saturating_add($source.$field);
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
}
