//! ICC profiles embedded in JPEG APP2 marker segments, compared with the ICC
//! Profile (0028,2000) that describes the Frame (PS3.5 8.2.1).

use std::borrow::Cow;

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::FrameReport;
use crate::validation::iod::items;

const EMBEDDED: &str = "PS3.5 8.2.1";

/// One APP2 "ICC_PROFILE" chunk (ISO 15076-1 Annex B).
pub(super) struct Chunk<'a> {
    sequence: u8,
    count: u8,
    data: &'a [u8],
}

impl<'a> Chunk<'a> {
    pub(super) fn parse(payload: &'a [u8]) -> Option<Self> {
        let rest = payload.strip_prefix(b"ICC_PROFILE\0")?;
        let [sequence, count, data @ ..] = rest else {
            return None;
        };
        Some(Self {
            sequence: *sequence,
            count: *count,
            data,
        })
    }
}

/// The ICC Profile describing one Frame, and where it was found.
pub(super) struct Profile<'a> {
    bytes: &'a [u8],
    source: Cow<'static, str>,
}

/// Reports APP2 chunks whose concatenation is not `profile`, excluding the
/// trailing pad byte of the DICOM Value.
pub(super) fn compare(chunks: &[Chunk<'_>], profile: &Profile<'_>, report: &mut FrameReport<'_>) {
    if chunks.is_empty() {
        return;
    }
    let count = chunks[0].count;
    let mut ordered: Vec<&Chunk<'_>> = chunks.iter().collect();
    ordered.sort_by_key(|chunk| chunk.sequence);
    let complete = count != 0
        && ordered.len() == usize::from(count)
        && ordered
            .iter()
            .zip(1..=count)
            .all(|(chunk, sequence)| chunk.sequence == sequence && chunk.count == count);
    if !complete {
        report.violation(EMBEDDED, || {
            format!(
                "its {} APP2 ICC_PROFILE segments are not one complete chunk sequence of {count} (ISO 15076-1 Annex B), so their concatenation cannot equal ICC Profile (0028,2000) of {}",
                chunks.len(),
                profile.source
            )
        });
        return;
    }
    let embedded_len: usize = ordered.iter().map(|chunk| chunk.data.len()).sum();
    let declared = match profile.bytes {
        [bytes @ .., 0] if bytes.len() == embedded_len => bytes,
        bytes => bytes,
    };
    let mut remaining = declared;
    let identical = declared.len() == embedded_len
        && ordered.iter().all(|chunk| {
            let (head, tail) = remaining.split_at(chunk.data.len());
            remaining = tail;
            head == chunk.data
        });
    if !identical {
        report.violation(EMBEDDED, || {
            format!(
                "APP2 ICC_PROFILE segments concatenate to a {embedded_len}-byte profile that is not identical to the {}-byte ICC Profile (0028,2000) of {}",
                declared.len(),
                profile.source
            )
        });
    }
}

/// Where each Frame's ICC Profile is: the Item of the Optical Path the Frame
/// belongs to, else the top-level attribute.
pub(super) struct FrameProfiles<'a> {
    object: &'a InMemDicomObject,
    top_level: Option<Cow<'a, [u8]>>,
    paths: Vec<OpticalPath<'a>>,
    shared_path: Option<String>,
    /// Frames per optical path when TILED_FULL orders Frames implicitly.
    tiled_full_frames_per_path: Option<usize>,
    concatenation_offset: usize,
}

impl<'a> FrameProfiles<'a> {
    pub(super) fn read(object: &'a InMemDicomObject) -> Self {
        let bytes = |item: &'a InMemDicomObject| {
            item.get(tags::ICC_PROFILE)
                .and_then(|element| element.to_bytes().ok())
        };
        let paths = items(object, tags::OPTICAL_PATH_SEQUENCE)
            .iter()
            .map(|item| OpticalPath {
                identifier: text(item, tags::OPTICAL_PATH_IDENTIFIER),
                profile: bytes(item),
            })
            .collect();
        let shared_path = items(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
            .first()
            .and_then(optical_path_identifier);
        Self {
            object,
            top_level: bytes(object),
            paths,
            shared_path,
            tiled_full_frames_per_path: tiled_full_frames_per_path(object),
            concatenation_offset: unsigned(object, tags::CONCATENATION_FRAME_OFFSET_NUMBER)
                .unwrap_or(0),
        }
    }

    pub(super) fn for_frame(&self, frame: usize) -> Option<Profile<'_>> {
        match self.optical_path(frame) {
            Some(index) => self.paths.get(index).and_then(|path| {
                Some(Profile {
                    bytes: path.profile.as_deref()?,
                    source: format!("Optical Path Sequence Item {}", index + 1).into(),
                })
            }),
            None => self.top_level.as_deref().map(|bytes| Profile {
                bytes,
                source: "the top-level Data Set".into(),
            }),
        }
    }

    /// The Optical Path Sequence Item index of `frame`: the only Item, the one
    /// its Functional Groups identify, or its place in the TILED_FULL order
    /// (PS3.3 C.7.6.17.3).
    fn optical_path(&self, frame: usize) -> Option<usize> {
        if self.paths.len() == 1 {
            return Some(0);
        }
        let per_frame = items(self.object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
            .get(frame)
            .and_then(optical_path_identifier);
        if let Some(identifier) = per_frame.as_deref().or(self.shared_path.as_deref()) {
            return self
                .paths
                .iter()
                .position(|path| path.identifier.as_deref() == Some(identifier));
        }
        self.tiled_full_frames_per_path
            .map(|per_path| (frame + self.concatenation_offset) / per_path)
    }
}

/// One Optical Path Sequence Item's identifier and ICC Profile.
struct OpticalPath<'a> {
    identifier: Option<String>,
    profile: Option<Cow<'a, [u8]>>,
}

fn optical_path_identifier(group: &InMemDicomObject) -> Option<String> {
    items(group, tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE)
        .first()
        .and_then(|item| text(item, tags::OPTICAL_PATH_IDENTIFIER))
}

/// Tiles across, times tiles down, times focal planes (PS3.3 C.7.6.17.3).
fn tiled_full_frames_per_path(object: &InMemDicomObject) -> Option<usize> {
    if text(object, tags::DIMENSION_ORGANIZATION_TYPE).as_deref() != Some("TILED_FULL") {
        return None;
    }
    let tiles = |total, size| -> Option<usize> {
        let size = unsigned(object, size).filter(|size| *size > 0)?;
        Some(unsigned(object, total)?.div_ceil(size))
    };
    let across = tiles(tags::TOTAL_PIXEL_MATRIX_COLUMNS, tags::COLUMNS)?;
    let down = tiles(tags::TOTAL_PIXEL_MATRIX_ROWS, tags::ROWS)?;
    let planes = unsigned(object, tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES).unwrap_or(1);
    across
        .checked_mul(down)?
        .checked_mul(planes)
        .filter(|frames| *frames > 0)
}

fn text(object: &InMemDicomObject, tag: dicom_core::Tag) -> Option<String> {
    object
        .get(tag)
        .and_then(|element| element.to_str().ok())
        .map(|value| value.trim_matches([' ', '\0']).to_string())
        .filter(|value| !value.is_empty())
}

fn unsigned(object: &InMemDicomObject, tag: dicom_core::Tag) -> Option<usize> {
    object
        .get(tag)
        .and_then(|element| element.to_int::<usize>().ok())
}
