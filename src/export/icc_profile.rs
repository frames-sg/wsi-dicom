use wsi_rs::{
    Compression, IccProfileKey, LevelIdx, PlaneSelection, RegionRequest, SceneId, SeriesId, Slide,
    TileLayout, TileRequest,
};

use crate::calibration::{sha256_hex, ColorManagement, IccConflictPolicy, IccProfile};
use crate::coordinate::InstanceCoordinate;
use crate::icc::{
    synthetic_display_p3_icc_profile, synthetic_srgb_icc_profile, validate_dicom_icc_profile,
};
use crate::metadata::DicomMetadata;
use crate::report::{IccConflictDecision, IccProfileReport, IccProfileSource};
use crate::request::ExportRequest;
use crate::tile::{prepare_tile_samples_with_limit, PixelProfile};
use crate::Error;

use super::jobs::DicomExportInstanceJob;

#[derive(Debug, Clone)]
pub(super) struct ResolvedIccProfile {
    pub(super) bytes: Option<Vec<u8>>,
    pub(super) report: IccProfileReport,
}

const JPEG_ICC_SAMPLE_TILE_LIMIT: usize = 16;

pub(super) fn resolve_icc_profile(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    coordinate: InstanceCoordinate,
    level: &wsi_rs::Level,
    pixel_profile: PixelProfile,
) -> Result<ResolvedIccProfile, Error> {
    let scene_idx = coordinate.scene_idx;
    let series_idx = coordinate.series_idx;
    let level_idx = coordinate.level_idx;
    if pixel_profile.photometric_interpretation == "MONOCHROME2" {
        return Ok(ResolvedIccProfile {
            bytes: None,
            report: IccProfileReport {
                source: IccProfileSource::NotApplicableMonochrome,
                sha256: None,
                calibration_id: None,
                conflict_decision: IccConflictDecision::NotApplicableMonochrome,
            },
        });
    }

    let source = if let Some(profile) = slide
        .dataset()
        .icc_profiles
        .get(&IccProfileKey::new(scene_idx.into(), series_idx.into()))
        .filter(|profile| !profile.is_empty())
    {
        validate_dicom_icc_profile(profile)?;
        Some(SourceIccProfile {
            bytes: profile.clone(),
            source: IccProfileSource::Source,
        })
    } else if let Some(profile) =
        sampled_jpeg_icc_profile(slide, scene_idx, series_idx, level_idx, level)?
    {
        validate_dicom_icc_profile(&profile)?;
        Some(SourceIccProfile {
            bytes: profile,
            source: IccProfileSource::SourceJpeg,
        })
    } else {
        None
    };

    match &request.color_management {
        ColorManagement::RequireSource => source
            .map(resolve_source_profile)
            .ok_or_else(|| Error::Metadata {
            reason: format!(
                "ICC profile is missing for color scene {scene_idx} series {series_idx}; choose SourceOrSrgb, SourceOrDisplayP3, a calibration registry, or an explicit profile only when that assumption is governed"
            ),
        }),
        ColorManagement::SourceOrSrgb => match source {
            Some(source) => Ok(resolve_source_profile(source)),
            None => resolve_unconfigured_profile(
                synthetic_srgb_icc_profile()?,
                IccProfileSource::SynthesizedSrgb,
            ),
        },
        ColorManagement::SourceOrDisplayP3 => match source {
            Some(source) => Ok(resolve_source_profile(source)),
            None => resolve_unconfigured_profile(
                synthetic_display_p3_icc_profile()?,
                IccProfileSource::SynthesizedDisplayP3,
            ),
        },
        ColorManagement::Calibration { registry, conflict } => {
            let profile = registry.match_metadata(metadata)?;
            resolve_configured_profile(
                source,
                profile,
                IccProfileSource::CalibrationRegistry,
                *conflict,
                scene_idx,
                series_idx,
            )
        }
        ColorManagement::ExplicitProfile { profile, conflict } => resolve_configured_profile(
            source,
            profile,
            IccProfileSource::ExplicitProfile,
            *conflict,
            scene_idx,
            series_idx,
        ),
    }
}

pub(super) fn preflight_icc_profiles(
    slide: &Slide,
    request: &ExportRequest,
    metadata: &DicomMetadata,
    jobs: &[DicomExportInstanceJob<'_>],
) -> Result<Vec<Option<String>>, Error> {
    let mut digests = Vec::new();
    digests
        .try_reserve(jobs.len())
        .map_err(|_| Error::Unsupported {
            reason: "ICC profile preflight exceeds available memory".into(),
        })?;
    for job in jobs {
        let profile = preflight_pixel_profile(slide, job)?;
        let resolved =
            resolve_icc_profile(slide, request, metadata, job.coordinate, job.level, profile)?;
        digests.push(resolved.report.sha256);
    }
    Ok(digests)
}

fn preflight_pixel_profile(
    slide: &Slide,
    job: &DicomExportInstanceJob<'_>,
) -> Result<PixelProfile, Error> {
    let coordinate = job.coordinate;
    if let Ok(raw) = slide.read_raw_compressed_tile(&coordinate.tile_request(0, 0)) {
        if raw.compression() == Compression::Jpeg {
            if let Ok(profile) = super::pixel_profile_from_raw_jpeg_tile(&raw) {
                return Ok(profile);
            }
        }
    }
    let tile = slide
        .read_region(
            &RegionRequest::new(
                SceneId::new(coordinate.scene_idx),
                SeriesId::new(coordinate.series_idx),
                LevelIdx::new(coordinate.level_idx),
                (0, 0),
                (1, 1),
            )
            .with_plane(PlaneSelection::new(
                coordinate.z,
                coordinate.c,
                coordinate.t,
            )),
        )
        .map_err(|source| Error::SlideRead {
            message: format!("ICC preflight sample failed: {source}"),
        })?;
    Ok(prepare_tile_samples_with_limit(&tile, 1, 1, 16)?.profile)
}

#[derive(Debug)]
struct SourceIccProfile {
    bytes: Vec<u8>,
    source: IccProfileSource,
}

fn resolve_source_profile(source: SourceIccProfile) -> ResolvedIccProfile {
    let sha256 = sha256_hex(&source.bytes);
    ResolvedIccProfile {
        bytes: Some(source.bytes),
        report: IccProfileReport {
            source: source.source,
            sha256: Some(sha256),
            calibration_id: None,
            conflict_decision: IccConflictDecision::NoConflict,
        },
    }
}

fn resolve_unconfigured_profile(
    bytes: Vec<u8>,
    source: IccProfileSource,
) -> Result<ResolvedIccProfile, Error> {
    validate_dicom_icc_profile(&bytes)?;
    let sha256 = sha256_hex(&bytes);
    Ok(ResolvedIccProfile {
        bytes: Some(bytes),
        report: IccProfileReport {
            source,
            sha256: Some(sha256),
            calibration_id: None,
            conflict_decision: IccConflictDecision::NoConflict,
        },
    })
}

fn resolve_configured_profile(
    source: Option<SourceIccProfile>,
    configured: &IccProfile,
    configured_source: IccProfileSource,
    conflict: IccConflictPolicy,
    scene_idx: usize,
    series_idx: usize,
) -> Result<ResolvedIccProfile, Error> {
    let calibration_id = Some(configured.id().to_string());
    let Some(source) = source else {
        return Ok(ResolvedIccProfile {
            bytes: Some(configured.bytes().to_vec()),
            report: IccProfileReport {
                source: configured_source,
                sha256: Some(configured.sha256().to_string()),
                calibration_id,
                conflict_decision: IccConflictDecision::NoConflict,
            },
        });
    };
    let source_sha256 = sha256_hex(&source.bytes);
    if source_sha256 == configured.sha256() {
        return Ok(ResolvedIccProfile {
            bytes: Some(configured.bytes().to_vec()),
            report: IccProfileReport {
                source: configured_source,
                sha256: Some(source_sha256),
                calibration_id,
                conflict_decision: IccConflictDecision::DigestsMatch,
            },
        });
    }

    match conflict {
        IccConflictPolicy::Fail => Err(Error::Metadata {
            reason: format!(
                "ICC profile conflict for color scene {scene_idx} series {series_idx}: source digest {source_sha256} differs from configured profile '{}' digest {}",
                configured.id(),
                configured.sha256()
            ),
        }),
        IccConflictPolicy::PreferConfigured => Ok(ResolvedIccProfile {
            bytes: Some(configured.bytes().to_vec()),
            report: IccProfileReport {
                source: configured_source,
                sha256: Some(configured.sha256().to_string()),
                calibration_id,
                conflict_decision: IccConflictDecision::PreferredConfigured,
            },
        }),
        IccConflictPolicy::PreferSource => Ok(ResolvedIccProfile {
            bytes: Some(source.bytes),
            report: IccProfileReport {
                source: source.source,
                sha256: Some(source_sha256),
                calibration_id,
                conflict_decision: IccConflictDecision::PreferredSource,
            },
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
        if raw.compression() != Compression::Jpeg {
            continue;
        }
        let Some(raw_profile) = jpeg_icc_profile(raw.data())? else {
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
        _ => {}
    }

    coords
        .into_iter()
        .take(JPEG_ICC_SAMPLE_TILE_LIMIT)
        .map(|(col, row)| {
            TileRequest::new(scene_idx, series_idx, level_idx, col, row)
                .with_plane(PlaneSelection::default())
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
