use std::path::{Path, PathBuf};

use statumen::CpuTile;

use crate::instance_context::DicomInstanceContext;
use crate::options::TransferSyntax;
use crate::report::DicomExportMetrics;
use crate::tile::prepare_tile_samples;
use crate::writer::pixel_data_offsets_from_lengths;
use crate::WsiDicomError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedTileBenchSummary {
    pub bytes_len: usize,
    pub components: u8,
    pub bits_allocated: u16,
    pub photometric_interpretation: &'static str,
}

pub fn prepare_tile_samples_summary(
    tile: &CpuTile,
    output_width: u32,
    output_height: u32,
) -> Result<PreparedTileBenchSummary, WsiDicomError> {
    let prepared = prepare_tile_samples(tile, output_width, output_height)?;
    Ok(PreparedTileBenchSummary {
        bytes_len: prepared.bytes.len(),
        components: prepared.profile.components,
        bits_allocated: prepared.profile.bits_allocated,
        photometric_interpretation: prepared.profile.photometric_interpretation,
    })
}

pub fn pixel_data_offsets_for_bench(lengths: &[u64]) -> Result<Vec<u64>, WsiDicomError> {
    pixel_data_offsets_from_lengths(lengths)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceContextBenchSummary {
    pub path: PathBuf,
    pub uid_bytes: usize,
    pub series_number: u32,
    pub frame_count: u32,
}

#[allow(clippy::too_many_arguments)]
pub fn instance_context_summary(
    source_path: &Path,
    output_dir: &Path,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
) -> InstanceContextBenchSummary {
    let context = DicomInstanceContext::new(
        source_path,
        output_dir,
        (0.0005, 0.0005),
        scene_idx,
        series_idx,
        level_idx,
        z,
        c,
        t,
    );
    let report = context.report(
        TransferSyntax::Htj2kLosslessRpcl.uid(),
        1024,
        DicomExportMetrics::default(),
    );
    InstanceContextBenchSummary {
        path: report.path,
        uid_bytes: report.sop_instance_uid.len() + report.series_instance_uid.len(),
        series_number: context.series_number,
        frame_count: report.frame_count,
    }
}
