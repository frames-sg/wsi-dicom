use wsi_rs::{Compression, PlaneSelection, Slide, TileLayout, TileRequest};

use crate::options::IccProfilePolicy;
use crate::report::IccProfileSource;
use crate::request::ExportRequest;
use crate::writer::{synthetic_display_p3_icc_profile, synthetic_srgb_icc_profile};
use crate::Error;

#[derive(Debug, Clone)]
pub(super) struct ResolvedIccProfile {
    pub(super) bytes: Option<Vec<u8>>,
    pub(super) source: IccProfileSource,
}

const JPEG_ICC_SAMPLE_TILE_LIMIT: usize = 16;

pub(super) fn resolve_icc_profile(
    slide: &Slide,
    request: &ExportRequest,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    level: &wsi_rs::Level,
) -> Result<ResolvedIccProfile, Error> {
    if let Some(profile) = slide
        .dataset()
        .icc_profiles
        .get(&(scene_idx, series_idx))
        .filter(|profile| !profile.is_empty())
    {
        return Ok(ResolvedIccProfile {
            bytes: Some(profile.clone()),
            source: IccProfileSource::Source,
        });
    }

    if let Some(profile) = sampled_jpeg_icc_profile(slide, scene_idx, series_idx, level_idx, level)?
    {
        return Ok(ResolvedIccProfile {
            bytes: Some(profile),
            source: IccProfileSource::SourceJpeg,
        });
    }

    match request.options.icc_profile_policy {
        IccProfilePolicy::Strict => Err(Error::Metadata {
            reason: format!(
                "ICC profile is missing for scene {scene_idx} series {series_idx}; use fallback-srgb, fallback-display-p3, or omit-if-missing if this source is intentionally unprofiled"
            ),
        }),
        IccProfilePolicy::FallbackSrgb => Ok(ResolvedIccProfile {
            bytes: Some(synthetic_srgb_icc_profile()?),
            source: IccProfileSource::SynthesizedSrgb,
        }),
        IccProfilePolicy::FallbackDisplayP3 => Ok(ResolvedIccProfile {
            bytes: Some(synthetic_display_p3_icc_profile()?),
            source: IccProfileSource::SynthesizedDisplayP3,
        }),
        IccProfilePolicy::OmitIfMissing => Ok(ResolvedIccProfile {
            bytes: None,
            source: IccProfileSource::OmittedMissing,
        }),
    }
}

fn sampled_jpeg_icc_profile(
    slide: &Slide,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    level: &wsi_rs::Level,
) -> Result<Option<Vec<u8>>, Error> {
    let mut profile = None;
    for request in icc_probe_tile_requests(scene_idx, series_idx, level_idx, level) {
        let Ok(raw) = slide.read_raw_compressed_tile(&request) else {
            continue;
        };
        if raw.compression != Compression::Jpeg {
            continue;
        }
        let Some(raw_profile) = jpeg_icc_profile(&raw.data)? else {
            continue;
        };
        if let Some(existing) = &profile {
            if existing != &raw_profile {
                return Err(Error::Metadata {
                    reason: format!(
                        "embedded JPEG ICC profile changed across sampled tiles for scene {scene_idx} series {series_idx} level {level_idx}"
                    ),
                });
            }
        } else {
            profile = Some(raw_profile);
        }
    }
    Ok(profile)
}

fn icc_probe_tile_requests(
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    level: &wsi_rs::Level,
) -> Vec<TileRequest> {
    let mut coords = Vec::new();
    match &level.tile_layout {
        TileLayout::Regular {
            tiles_across,
            tiles_down,
            ..
        } => {
            push_unique_coord(&mut coords, 0, 0);
            push_unique_coord(
                &mut coords,
                tiles_across.saturating_sub(1) as i64,
                tiles_down.saturating_sub(1) as i64,
            );
            push_unique_coord(
                &mut coords,
                (*tiles_across / 2) as i64,
                (*tiles_down / 2) as i64,
            );
            fill_row_major_coords(&mut coords, *tiles_across, *tiles_down);
        }
        TileLayout::WholeLevel {
            width,
            height,
            virtual_tile_width,
            virtual_tile_height,
        } => {
            let tiles_across = width.div_ceil(u64::from(*virtual_tile_width));
            let tiles_down = height.div_ceil(u64::from(*virtual_tile_height));
            push_unique_coord(&mut coords, 0, 0);
            push_unique_coord(
                &mut coords,
                tiles_across.saturating_sub(1) as i64,
                tiles_down.saturating_sub(1) as i64,
            );
            push_unique_coord(
                &mut coords,
                (tiles_across / 2) as i64,
                (tiles_down / 2) as i64,
            );
            fill_row_major_coords(&mut coords, tiles_across, tiles_down);
        }
        TileLayout::Irregular { tiles, .. } => {
            for coord in tiles.keys().take(JPEG_ICC_SAMPLE_TILE_LIMIT) {
                push_unique_coord(&mut coords, coord.0, coord.1);
            }
        }
    }

    coords
        .into_iter()
        .take(JPEG_ICC_SAMPLE_TILE_LIMIT)
        .map(|(col, row)| TileRequest {
            scene: scene_idx,
            series: series_idx,
            level: level_idx,
            plane: PlaneSelection::default(),
            col,
            row,
        })
        .collect()
}

fn fill_row_major_coords(coords: &mut Vec<(i64, i64)>, tiles_across: u64, tiles_down: u64) {
    for row in 0..tiles_down {
        for col in 0..tiles_across {
            push_unique_coord(coords, col as i64, row as i64);
            if coords.len() >= JPEG_ICC_SAMPLE_TILE_LIMIT {
                return;
            }
        }
    }
}

fn push_unique_coord(coords: &mut Vec<(i64, i64)>, col: i64, row: i64) {
    let coord = (col, row);
    if !coords.contains(&coord) {
        coords.push(coord);
    }
}

fn jpeg_icc_profile(data: &[u8]) -> Result<Option<Vec<u8>>, Error> {
    if data.len() < 4 || data[..2] != [0xFF, 0xD8] {
        return Ok(None);
    }
    let mut chunks: Vec<(u8, u8, &[u8])> = Vec::new();
    let mut cursor = 2usize;
    while cursor + 4 <= data.len() {
        if data[cursor] != 0xFF {
            break;
        }
        while cursor < data.len() && data[cursor] == 0xFF {
            cursor += 1;
        }
        if cursor >= data.len() {
            break;
        }
        let marker = data[cursor];
        cursor += 1;
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if cursor + 2 > data.len() {
            break;
        }
        let segment_len = usize::from(u16::from_be_bytes([data[cursor], data[cursor + 1]]));
        if segment_len < 2 || cursor + segment_len > data.len() {
            break;
        }
        let payload = &data[cursor + 2..cursor + segment_len];
        if marker == 0xE2 && payload.starts_with(b"ICC_PROFILE\0") {
            if payload.len() < 14 {
                return Err(invalid_jpeg_icc("APP2 ICC_PROFILE segment is too short"));
            }
            chunks.push((payload[12], payload[13], &payload[14..]));
        }
        cursor += segment_len;
    }

    if chunks.is_empty() {
        return Ok(None);
    }
    assemble_jpeg_icc_chunks(chunks).map(Some)
}

fn assemble_jpeg_icc_chunks(chunks: Vec<(u8, u8, &[u8])>) -> Result<Vec<u8>, Error> {
    let chunk_count = chunks[0].1;
    if chunk_count == 0 {
        return Err(invalid_jpeg_icc("APP2 ICC_PROFILE chunk count is zero"));
    }
    let mut ordered = vec![None; usize::from(chunk_count)];
    for (sequence, count, bytes) in chunks {
        if count != chunk_count {
            return Err(invalid_jpeg_icc(
                "APP2 ICC_PROFILE chunks disagree on chunk count",
            ));
        }
        if sequence == 0 || sequence > chunk_count {
            return Err(invalid_jpeg_icc(
                "APP2 ICC_PROFILE chunk sequence is out of range",
            ));
        }
        let slot = &mut ordered[usize::from(sequence - 1)];
        if slot.is_some() {
            return Err(invalid_jpeg_icc(
                "APP2 ICC_PROFILE contains duplicate chunk sequence",
            ));
        }
        *slot = Some(bytes);
    }
    let mut profile = Vec::new();
    for chunk in ordered {
        let Some(chunk) = chunk else {
            return Err(invalid_jpeg_icc(
                "APP2 ICC_PROFILE chunk sequence is incomplete",
            ));
        };
        profile.extend_from_slice(chunk);
    }
    Ok(profile)
}

fn invalid_jpeg_icc(reason: &str) -> Error {
    Error::Metadata {
        reason: format!("invalid embedded JPEG ICC profile: {reason}"),
    }
}
