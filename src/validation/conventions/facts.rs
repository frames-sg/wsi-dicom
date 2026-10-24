//! What the set results of the convention profiles read from each object,
//! kept so that the objects themselves need not be.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dicom_core::Tag;
use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use crate::validation::iod::{integer_value, items, string_value, MAX_NESTING};

/// The SOP Class of an object, as far as the profiles distinguish them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// VL Whole Slide Microscopy Image.
    Wsi,
    /// Microscopy Bulk Simple Annotations.
    Annotation,
    /// Key Object Selection Document.
    KeyObject,
    Other,
}

/// Patient Module identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Patient {
    pub(super) name: Option<String>,
    pub(super) id: Option<String>,
    pub(super) birth_date: Option<String>,
    pub(super) sex: Option<String>,
}

/// A referenced SOP Instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Reference {
    /// The Series it is listed under, in a hierarchical reference.
    pub(super) series: Option<String>,
    pub(super) instance: String,
    /// Whether the reference selects Frames or Segments.
    pub(super) selects_frames: bool,
}

/// Total Pixel Matrix Origin Sequence offsets, in mm.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Origin {
    pub(super) x: Option<f64>,
    pub(super) y: Option<f64>,
    pub(super) z: Option<f64>,
}

#[derive(Clone, Debug)]
pub(super) struct Facts {
    pub(super) path: PathBuf,
    pub(super) kind: Kind,
    pub(super) sop_instance_uid: Option<String>,
    pub(super) study_uid: Option<String>,
    pub(super) series_uid: Option<String>,
    pub(super) patient: Patient,
    pub(super) accession: Option<String>,
    pub(super) manufacturer: Option<String>,
    pub(super) model: Option<String>,
    /// Image Type Value 3.
    pub(super) flavor: Option<String>,
    pub(super) frame_of_reference_uid: Option<String>,
    pub(super) pyramid_uid: Option<String>,
    pub(super) acquisition_uid: Option<String>,
    pub(super) container_identifier: Option<String>,
    pub(super) barcode_value: Option<String>,
    pub(super) specimen_identifiers: Vec<String>,
    pub(super) concatenation_uid: Option<String>,
    /// Concatenation Frame Offset Number, In-concatenation Number or SOP
    /// Instance UID of Concatenation Source present.
    pub(super) concatenation_member: bool,
    pub(super) concatenation_source: Option<String>,
    /// Total Pixel Matrix Rows and Columns.
    pub(super) matrix: Option<(u64, u64)>,
    /// Rows and Columns of a Frame.
    pub(super) tile: Option<(u64, u64)>,
    /// Pixel Spacing of the Shared Functional Groups, row then column, mm.
    pub(super) spacing: Option<(f64, f64)>,
    pub(super) focal_planes: Option<i64>,
    pub(super) origin: Option<Origin>,
    pub(super) optical_paths: Option<i64>,
    pub(super) transfer_syntax: String,
    pub(super) photometric: Option<String>,
    pub(super) objective_lens_power: Option<f64>,
    pub(super) imaged_volume: (Option<f64>, Option<f64>),
    /// Images an annotation references, or the evidence a Key Object
    /// Selection lists.
    pub(super) references: Vec<Reference>,
    /// The IMAGE Content Items of a Key Object Selection.
    pub(super) content_references: Vec<Reference>,
    /// Whether its content tree nests deeper than the levels read.
    pub(super) content_truncated: bool,
    /// A Key Object Selection titled (113034, DCM, "Complete Acquisition
    /// Content").
    pub(super) completion: bool,
}

impl Facts {
    pub(super) fn of(path: &Path, object: &DefaultDicomObject) -> Self {
        let root: &InMemDicomObject = object;
        let kind = match text(root, tags::SOP_CLASS_UID).as_deref() {
            Some(crate::VL_WSI_SOP_CLASS_UID) => Kind::Wsi,
            Some(uids::MICROSCOPY_BULK_SIMPLE_ANNOTATIONS_STORAGE) => Kind::Annotation,
            Some(uids::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE) => Kind::KeyObject,
            _ => Kind::Other,
        };
        let (references, (content_references, content_truncated)) = match kind {
            Kind::Annotation => (annotation_references(root), (Vec::new(), false)),
            Kind::KeyObject => (evidence_references(root), content_references(root)),
            _ => (Vec::new(), (Vec::new(), false)),
        };
        Self {
            path: path.to_path_buf(),
            kind,
            sop_instance_uid: text(root, tags::SOP_INSTANCE_UID),
            study_uid: text(root, tags::STUDY_INSTANCE_UID),
            series_uid: text(root, tags::SERIES_INSTANCE_UID),
            patient: Patient {
                name: text(root, tags::PATIENT_NAME),
                id: text(root, tags::PATIENT_ID),
                birth_date: text(root, tags::PATIENT_BIRTH_DATE),
                sex: text(root, tags::PATIENT_SEX),
            },
            accession: text(root, tags::ACCESSION_NUMBER),
            manufacturer: text(root, tags::MANUFACTURER),
            model: text(root, tags::MANUFACTURER_MODEL_NAME),
            flavor: string_value(root, tags::IMAGE_TYPE, 3),
            frame_of_reference_uid: text(root, tags::FRAME_OF_REFERENCE_UID),
            pyramid_uid: text(root, tags::PYRAMID_UID),
            acquisition_uid: text(root, tags::ACQUISITION_UID),
            container_identifier: text(root, tags::CONTAINER_IDENTIFIER),
            barcode_value: text(root, tags::BARCODE_VALUE),
            specimen_identifiers: items(root, tags::SPECIMEN_DESCRIPTION_SEQUENCE)
                .iter()
                .filter_map(|item| text(item, tags::SPECIMEN_IDENTIFIER))
                .collect(),
            concatenation_uid: text(root, tags::CONCATENATION_UID),
            concatenation_member: [
                tags::CONCATENATION_FRAME_OFFSET_NUMBER,
                tags::IN_CONCATENATION_NUMBER,
                tags::SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
            ]
            .into_iter()
            .any(|tag| root.get(tag).is_some()),
            concatenation_source: text(root, tags::SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE),
            matrix: unsigned(root, tags::TOTAL_PIXEL_MATRIX_ROWS)
                .zip(unsigned(root, tags::TOTAL_PIXEL_MATRIX_COLUMNS)),
            tile: unsigned(root, tags::ROWS).zip(unsigned(root, tags::COLUMNS)),
            spacing: shared_pixel_spacing(root),
            focal_planes: integer_value(root, tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES),
            origin: items(root, tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE)
                .first()
                .map(|item| Origin {
                    x: number(item, tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, 1),
                    y: number(item, tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, 1),
                    z: number(item, tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, 1),
                }),
            optical_paths: integer_value(root, tags::NUMBER_OF_OPTICAL_PATHS),
            transfer_syntax: object
                .meta()
                .transfer_syntax()
                .trim_end_matches(['\0', ' '])
                .to_string(),
            photometric: text(root, tags::PHOTOMETRIC_INTERPRETATION),
            objective_lens_power: items(root, tags::OPTICAL_PATH_SEQUENCE)
                .first()
                .and_then(|item| number(item, tags::OBJECTIVE_LENS_POWER, 1)),
            imaged_volume: (
                number(root, tags::IMAGED_VOLUME_WIDTH, 1),
                number(root, tags::IMAGED_VOLUME_HEIGHT, 1),
            ),
            references,
            content_references,
            content_truncated,
            completion: kind == Kind::KeyObject
                && items(root, tags::CONCEPT_NAME_CODE_SEQUENCE)
                    .first()
                    .is_some_and(|code| is_code(code, "DCM", "113034")),
        }
    }

    pub(super) fn is_wsi(&self) -> bool {
        self.kind == Kind::Wsi
    }

    pub(super) fn flavor_is(&self, flavors: &[&str]) -> bool {
        self.flavor
            .as_deref()
            .is_some_and(|flavor| flavors.contains(&flavor))
    }

    /// A VOLUME or THUMBNAIL image: a layer of the Multi-Resolution Pyramid.
    pub(super) fn is_layer(&self) -> bool {
        self.is_wsi() && self.flavor_is(&["VOLUME", "THUMBNAIL"])
    }

    pub(super) fn is_volume(&self) -> bool {
        self.is_wsi() && self.flavor_is(&["VOLUME"])
    }

    /// A LABEL or OVERVIEW image, which shows the slide label.
    pub(super) fn shows_label(&self) -> bool {
        self.is_wsi() && self.flavor_is(&["LABEL", "OVERVIEW"])
    }

    /// Total Pixel Matrix pixels, for ordering layers by resolution.
    pub(super) fn area(&self) -> Option<u64> {
        self.matrix
            .map(|(rows, columns)| rows.saturating_mul(columns))
    }
}

/// The VL WSI images of one Series and the Acquisition UIDs among them.
pub(super) struct Series<'a> {
    pub(super) uid: &'a str,
    /// In the order of the set.
    pub(super) members: Vec<&'a Facts>,
    /// The members' positions in the set.
    positions: Vec<usize>,
    pub(super) acquisitions: BTreeSet<&'a str>,
}

/// Indexes of the validated set, built once per run and shared by every
/// profile.
pub(super) struct SetIndex<'a> {
    facts: &'a [Facts],
    /// The VL WSI images by Series Instance UID, in the order each Series
    /// first occurs. Images without one are left out; the DICOM checks report
    /// the missing Type 1 Attribute.
    pub(super) series: Vec<Series<'a>>,
    series_positions: BTreeMap<&'a str, usize>,
    /// The Series, by position in `series`, each Acquisition UID occurs in.
    acquisitions: BTreeMap<&'a str, BTreeSet<usize>>,
    /// The set positions of the VL WSI images by SOP Instance UID.
    instances: BTreeMap<&'a str, Vec<usize>>,
}

impl<'a> SetIndex<'a> {
    pub(super) fn new(facts: &'a [Facts]) -> Self {
        let mut index = Self {
            facts,
            series: Vec::new(),
            series_positions: BTreeMap::new(),
            acquisitions: BTreeMap::new(),
            instances: BTreeMap::new(),
        };
        for (position, fact) in facts.iter().enumerate() {
            if fact.is_wsi() {
                index.add(position, fact);
            }
        }
        index
    }

    fn add(&mut self, position: usize, fact: &'a Facts) {
        if let Some(uid) = fact.sop_instance_uid.as_deref() {
            self.instances.entry(uid).or_default().push(position);
        }
        let Some(uid) = fact.series_uid.as_deref() else {
            return;
        };
        let next = self.series.len();
        let series_position = *self.series_positions.entry(uid).or_insert(next);
        if series_position == next {
            self.series.push(Series {
                uid,
                members: Vec::new(),
                positions: Vec::new(),
                acquisitions: BTreeSet::new(),
            });
        }
        let series = &mut self.series[series_position];
        series.members.push(fact);
        series.positions.push(position);
        if let Some(acquisition) = fact.acquisition_uid.as_deref() {
            series.acquisitions.insert(acquisition);
            self.acquisitions
                .entry(acquisition)
                .or_default()
                .insert(series_position);
        }
    }

    /// The other Series that share an Acquisition UID with Series
    /// `position`, in Series order, each with the shared UIDs ascending.
    pub(super) fn shared_acquisitions(&self, position: usize) -> Vec<(&'a str, Vec<&'a str>)> {
        let mut shared = BTreeMap::<usize, Vec<&'a str>>::new();
        for &acquisition in &self.series[position].acquisitions {
            let others = self.acquisitions.get(acquisition).into_iter().flatten();
            for &other in others.filter(|&&other| other != position) {
                shared.entry(other).or_default().push(acquisition);
            }
        }
        shared
            .into_iter()
            .map(|(other, uids)| (self.series[other].uid, uids))
            .collect()
    }

    /// The VL WSI images that `references` name, each once, in set order.
    pub(super) fn referenced<'r>(
        &self,
        references: impl IntoIterator<Item = &'r Reference>,
    ) -> Vec<&'a Facts> {
        let positions: BTreeSet<usize> = references
            .into_iter()
            .filter_map(|reference| self.instances.get(reference.instance.as_str()))
            .flatten()
            .copied()
            .collect();
        positions
            .into_iter()
            .map(|position| &self.facts[position])
            .collect()
    }

    /// The VL WSI images of the Series `uids`, in set order.
    pub(super) fn members_of<'u>(&self, uids: impl IntoIterator<Item = &'u str>) -> Vec<&'a Facts> {
        let positions: BTreeSet<usize> = uids
            .into_iter()
            .filter_map(|uid| self.series_positions.get(uid))
            .flat_map(|&series| self.series[series].positions.iter().copied())
            .collect();
        positions
            .into_iter()
            .map(|position| &self.facts[position])
            .collect()
    }
}

/// The distinct Total Pixel Matrix sizes of the VOLUME images of one Series,
/// largest first: its resolution layers.
pub(super) fn volume_layers(members: &[&Facts]) -> Vec<(u64, u64)> {
    let mut layers: Vec<(u64, u64)> = members
        .iter()
        .filter(|fact| fact.is_volume())
        .filter_map(|fact| fact.matrix)
        .collect();
    layers.sort_unstable_by_key(|(rows, columns)| std::cmp::Reverse(rows * columns));
    layers.dedup();
    layers
}

pub(super) fn text(object: &InMemDicomObject, tag: Tag) -> Option<String> {
    string_value(object, tag, 1)
}

/// Value `index` (1-based) of a decimal or floating point Attribute.
pub(super) fn number(object: &InMemDicomObject, tag: Tag, index: usize) -> Option<f64> {
    let element = object.get(tag)?;
    match element.value() {
        dicom_core::DicomValue::Primitive(
            value @ (dicom_core::PrimitiveValue::F32(_) | dicom_core::PrimitiveValue::F64(_)),
        ) => value
            .to_multi_float64()
            .ok()?
            .get(index.checked_sub(1)?)
            .copied(),
        _ => string_value(object, tag, index)?.trim().parse().ok(),
    }
    .filter(|value: &f64| value.is_finite())
}

fn unsigned(object: &InMemDicomObject, tag: Tag) -> Option<u64> {
    integer_value(object, tag).and_then(|value| u64::try_from(value).ok())
}

pub(super) fn is_code(item: &InMemDicomObject, scheme: &str, value: &str) -> bool {
    text(item, tags::CODING_SCHEME_DESIGNATOR).as_deref() == Some(scheme)
        && text(item, tags::CODE_VALUE).as_deref() == Some(value)
}

/// "(value, scheme, "meaning")" of a code Item, as encoded.
pub(super) fn describe_code(item: &InMemDicomObject) -> String {
    let part = |tag| text(item, tag).unwrap_or_else(|| "?".into());
    format!(
        "({}, {}, \"{}\")",
        text(item, tags::CODE_VALUE)
            .or_else(|| text(item, tags::LONG_CODE_VALUE))
            .or_else(|| text(item, tags::URN_CODE_VALUE))
            .unwrap_or_else(|| "?".into()),
        part(tags::CODING_SCHEME_DESIGNATOR),
        text(item, tags::CODE_MEANING).unwrap_or_default()
    )
}

fn shared_pixel_spacing(root: &InMemDicomObject) -> Option<(f64, f64)> {
    let shared = items(root, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE).first()?;
    let measures = items(shared, tags::PIXEL_MEASURES_SEQUENCE).first()?;
    let row = number(measures, tags::PIXEL_SPACING, 1)?;
    let column = number(measures, tags::PIXEL_SPACING, 2)?;
    (row > 0.0 && column > 0.0).then_some((row, column))
}

fn reference(item: &InMemDicomObject, series: Option<&str>) -> Option<Reference> {
    Some(Reference {
        series: series.map(str::to_string),
        instance: text(item, tags::REFERENCED_SOP_INSTANCE_UID)?,
        selects_frames: item.get(tags::REFERENCED_FRAME_NUMBER).is_some()
            || item.get(tags::REFERENCED_SEGMENT_NUMBER).is_some(),
    })
}

/// Referenced Image Sequence, and the Common Instance Reference Module's
/// Referenced Series Sequence (PS3.3 C.12.2).
fn annotation_references(root: &InMemDicomObject) -> Vec<Reference> {
    let mut found: Vec<Reference> = items(root, tags::REFERENCED_IMAGE_SEQUENCE)
        .iter()
        .filter_map(|item| reference(item, None))
        .collect();
    for series in items(root, tags::REFERENCED_SERIES_SEQUENCE) {
        let uid = text(series, tags::SERIES_INSTANCE_UID);
        found.extend(
            items(series, tags::REFERENCED_INSTANCE_SEQUENCE)
                .iter()
                .filter_map(|item| reference(item, uid.as_deref())),
        );
    }
    found
}

/// Current Requested Procedure Evidence Sequence (PS3.3 C.17.2).
fn evidence_references(root: &InMemDicomObject) -> Vec<Reference> {
    let mut found = Vec::new();
    for study in items(root, tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE) {
        for series in items(study, tags::REFERENCED_SERIES_SEQUENCE) {
            let uid = text(series, tags::SERIES_INSTANCE_UID);
            found.extend(
                items(series, tags::REFERENCED_SOP_SEQUENCE)
                    .iter()
                    .filter_map(|item| reference(item, uid.as_deref())),
            );
        }
    }
    found
}

/// The Referenced SOP Sequence Items of the IMAGE Content Items, to
/// `MAX_NESTING` levels of the content tree, and whether it nests deeper.
fn content_references(root: &InMemDicomObject) -> (Vec<Reference>, bool) {
    let mut found = Vec::new();
    let mut truncated = false;
    let mut pending: Vec<(&InMemDicomObject, usize)> = vec![(root, 0)];
    while let Some((node, depth)) = pending.pop() {
        if depth >= MAX_NESTING {
            truncated |= !items(node, tags::CONTENT_SEQUENCE).is_empty();
            continue;
        }
        for item in items(node, tags::CONTENT_SEQUENCE) {
            if text(item, tags::VALUE_TYPE).as_deref() == Some("IMAGE") {
                found.extend(
                    items(item, tags::REFERENCED_SOP_SEQUENCE)
                        .iter()
                        .filter_map(|reference_item| reference(reference_item, None)),
                );
            }
            pending.push((item, depth + 1));
        }
    }
    (found, truncated)
}
