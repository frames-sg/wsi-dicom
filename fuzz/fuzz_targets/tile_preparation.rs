#![no_main]

use libfuzzer_sys::fuzz_target;
use statumen::{ColorSpace, CpuTile, CpuTileData, CpuTileLayout};
use wsi_dicom::bench_support::prepare_tile_samples_summary;

fuzz_target!(|data: &[u8]| {
    if data.len() < 5 {
        return;
    }
    let width = u32::from(data[0] % 32) + 1;
    let height = u32::from(data[1] % 32) + 1;
    let channels: u16 = match data[2] % 3 {
        0 => 1,
        1 => 3,
        _ => 4,
    };
    let color_space = match channels {
        1 => ColorSpace::Grayscale,
        3 => ColorSpace::Rgb,
        _ => ColorSpace::Rgba,
    };
    let output_width = width + u32::from(data[3] % 4);
    let output_height = height + u32::from(data[4] % 4);
    let Some(samples) = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|pixels| pixels.checked_mul(usize::from(channels)))
    else {
        return;
    };
    if samples > 4096 {
        return;
    }

    let mut bytes = vec![0u8; samples];
    let payload = &data[5..];
    for (idx, byte) in bytes.iter_mut().enumerate() {
        *byte = payload
            .get(idx % payload.len().max(1))
            .copied()
            .unwrap_or(0);
    }
    let Ok(tile) = CpuTile::new(
        width,
        height,
        channels,
        color_space,
        CpuTileLayout::Interleaved,
        CpuTileData::u8(bytes),
    ) else {
        return;
    };
    let _ = prepare_tile_samples_summary(&tile, output_width, output_height);
});
