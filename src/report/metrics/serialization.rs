use serde::{ser::SerializeStruct, Serialize};

use super::super::*;

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
