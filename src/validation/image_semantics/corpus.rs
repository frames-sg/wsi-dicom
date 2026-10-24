//! The set-level requirements of the IODs beside VL WSI: those that compare
//! an object with the Instances it references or shares a Series with. Each
//! check reports once per validated set: failed when a compared pair fails,
//! passed when every compared pair passes, skipped when no pair could be
//! compared, and not at all when no object of its IODs is in the set.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::{
    includes_synchronization, items, numbers, string, FunctionalGroups, Iod, BULK_ANNOTATIONS_RULE,
    CITATION, SEGMENTATION_RULE, SYNCHRONIZATION_RULE,
};
use crate::validation::bulk_annotations::{screen_windings, GroupWinding};
use crate::validation::iod::{dicom_edition, Family};
use crate::validation::{intrinsic_check, ValidationCheck, ValidationStatus};

const NOT_IN_SET: &str = "the referenced Instance is not in the validated set";
const SERIES_NOT_IN_SET: &str = "no other Instance of the Series is in the validated set";
/// Unexamined pairs a set-level message names; the rest are counted.
const MAX_NOTES: usize = 20;

/// Pixel spacings within this relative difference are the same sampling;
/// DS Values written from one float differ by its decimal rounding.
const SPACING_TOLERANCE: f64 = 1e-6;

/// The facts of every validated object that the set-level checks compare.
#[derive(Debug, Default)]
pub(in crate::validation) struct FamilyCorpus {
    instances: BTreeMap<String, Instance>,
    segmentations: Vec<Segmentation>,
    annotations: Vec<Annotations>,
    synchronized: Vec<Synchronized>,
}

/// Any validated Instance, as a referenced image or a Series member.
#[derive(Debug)]
struct Instance {
    path: PathBuf,
    series: Option<String>,
    frame_of_reference: Option<String>,
    concatenation: bool,
    /// Rows and Columns, present for an image.
    matrix: Option<(i64, i64)>,
    spacing: Option<Vec<f64>>,
    slide_orientation: Option<Vec<f64>>,
}

#[derive(Debug)]
struct Segmentation {
    path: PathBuf,
    frame_of_reference: Option<String>,
    matrix: Option<(i64, i64)>,
    spacing: Option<Vec<f64>>,
    references: Vec<String>,
}

#[derive(Debug)]
struct Annotations {
    path: PathBuf,
    references: Vec<String>,
    /// The 2D Annotation Groups' polylines and polygons by the sign of their
    /// area in (column, row) coordinates; empty for 3D coordinates.
    windings: Vec<GroupWinding>,
}

#[derive(Debug)]
struct Synchronized {
    path: PathBuf,
    instance: String,
    series: Option<String>,
    frame_of_reference: Option<String>,
}

impl FamilyCorpus {
    /// Records the facts of one validated object.
    pub(in crate::validation) fn observe(&mut self, path: &Path, object: &DefaultDicomObject) {
        let root: &InMemDicomObject = object;
        let Some(uid) = string(root, tags::SOP_INSTANCE_UID) else {
            return;
        };
        let groups = FunctionalGroups::of(root);
        let spacing = numbers(root, tags::PIXEL_SPACING).or_else(|| {
            groups
                .macro_item(0, tags::PIXEL_MEASURES_SEQUENCE)
                .and_then(|measures| numbers(measures, tags::PIXEL_SPACING))
        });
        let matrix = matrix(root);
        let frame_of_reference = string(root, tags::FRAME_OF_REFERENCE_UID);
        self.instances
            .entry(uid.clone())
            .or_insert_with(|| Instance {
                path: path.to_path_buf(),
                series: string(root, tags::SERIES_INSTANCE_UID),
                frame_of_reference: frame_of_reference.clone(),
                concatenation: root.get(tags::CONCATENATION_UID).is_some(),
                matrix,
                spacing: spacing.clone(),
                slide_orientation: numbers(root, tags::IMAGE_ORIENTATION_SLIDE)
                    .filter(|values| values.len() == 6),
            });
        if includes_synchronization(object)
            && root
                .get(tags::SYNCHRONIZATION_FRAME_OF_REFERENCE_UID)
                .is_some()
        {
            self.synchronized.push(Synchronized {
                path: path.to_path_buf(),
                instance: uid,
                series: string(root, tags::SERIES_INSTANCE_UID),
                frame_of_reference: frame_of_reference.clone(),
            });
        }
        let Some(iod) = Iod::of(object) else {
            return;
        };
        match iod {
            Iod::Segmentation => self.segmentations.push(Segmentation {
                path: path.to_path_buf(),
                frame_of_reference,
                matrix,
                spacing,
                references: segmentation_references(root, &groups),
            }),
            Iod::BulkAnnotations => self.annotations.push(Annotations {
                path: path.to_path_buf(),
                references: references(items(root, tags::REFERENCED_IMAGE_SEQUENCE)),
                windings: screen_windings(root),
            }),
            Iod::ParametricMap => {}
        }
    }
}

fn matrix(object: &InMemDicomObject) -> Option<(i64, i64)> {
    let value = |tag| super::integer(object, tag);
    Some((value(tags::ROWS)?, value(tags::COLUMNS)?))
}

fn references(sequence: &[InMemDicomObject]) -> Vec<String> {
    sequence
        .iter()
        .filter_map(|item| string(item, tags::REFERENCED_SOP_INSTANCE_UID))
        .collect()
}

/// The images a Segmentation classifies: the Source Images of its Derivation
/// Image Functional Groups and the Instances of its Referenced Series.
fn segmentation_references(
    object: &InMemDicomObject,
    groups: &FunctionalGroups<'_>,
) -> Vec<String> {
    let mut found: Vec<String> = groups
        .macro_items(tags::DERIVATION_IMAGE_SEQUENCE)
        .flat_map(|(_, derivation)| references(items(derivation, tags::SOURCE_IMAGE_SEQUENCE)))
        .chain(
            items(object, tags::REFERENCED_SERIES_SEQUENCE)
                .iter()
                .flat_map(|series| references(items(series, tags::REFERENCED_INSTANCE_SEQUENCE))),
        )
        .collect();
    found.sort();
    found.dedup();
    found
}

/// `<from> references <to>`, formatted only when a message names the pair.
#[derive(Clone, Copy)]
struct Pair<'a> {
    from: &'a Path,
    to: &'a Path,
}

impl std::fmt::Display for Pair<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} references {}",
            self.from.display(),
            self.to.display()
        )
    }
}

/// What one set-level check compared.
#[derive(Default)]
struct Outcome {
    applies: bool,
    compared: usize,
    violations: Family,
    unexamined: Notes,
}

impl Outcome {
    fn check(self, rule: &str, citation: &str, undecided: &str) -> Option<ValidationCheck> {
        if !self.applies {
            return None;
        }
        if self.violations.is_empty() && self.compared == 0 {
            return Some(intrinsic_check(
                rule,
                None,
                ValidationStatus::Skipped,
                format!("{citation}: {undecided}"),
            ));
        }
        let mut check = self.violations.check(Path::new(""), rule, citation);
        if !self.unexamined.shown.is_empty() {
            check.message.push_str("; not examined: ");
            check.message.push_str(&self.unexamined.shown.join("; "));
            let hidden = self.unexamined.total - self.unexamined.shown.len();
            if hidden > 0 {
                check.message.push_str(&format!("; and {hidden} more"));
            }
        }
        Some(check)
    }
}

/// What a set-level check could not compare, the first few formatted.
#[derive(Default)]
struct Notes {
    shown: Vec<String>,
    total: usize,
}

impl Notes {
    fn push(&mut self, note: impl FnOnce() -> String) {
        self.total += 1;
        if self.shown.len() < MAX_NOTES {
            self.shown.push(note());
        }
    }
}

/// The set-level checks of the IOD families, one result per check name.
pub(in crate::validation) fn run_family_set_checks(corpus: &FamilyCorpus) -> Vec<ValidationCheck> {
    [
        segmentation_check(corpus).check(
            SEGMENTATION_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " A.51.1 and A.51.5.1"),
            NOT_IN_SET,
        ),
        annotation_check(corpus).check(
            BULK_ANNOTATIONS_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.37.1.2.1.1"),
            NOT_IN_SET,
        ),
        synchronization_check(corpus).check(
            SYNCHRONIZATION_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.4.2.1.1"),
            SERIES_NOT_IN_SET,
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A.51.1 and A.51.5.1: a Segmentation shares the Frame of Reference of the
/// images it references, and without one has their sampling and extent.
fn segmentation_check(corpus: &FamilyCorpus) -> Outcome {
    let mut outcome = Outcome {
        applies: !corpus.segmentations.is_empty(),
        ..Outcome::default()
    };
    for segmentation in &corpus.segmentations {
        for uid in &segmentation.references {
            let Some(image) = corpus
                .instances
                .get(uid)
                .filter(|image| image.matrix.is_some())
            else {
                continue;
            };
            outcome.compared += 1;
            let pair = Pair {
                from: &segmentation.path,
                to: &image.path,
            };
            match &image.frame_of_reference {
                Some(frame) if segmentation.frame_of_reference.as_ref() != Some(frame) => {
                    outcome.violations.push(|| format!(
                        "{pair}, whose Frame of Reference UID is {frame}, but the Segmentation's is {}; it shall have the same Frame of Reference ({CITATION} A.51.1)",
                        segmentation.frame_of_reference.as_deref().unwrap_or("absent")
                    ));
                }
                Some(_) => {}
                None => sampling(segmentation, image, pair, &mut outcome),
            }
        }
    }
    outcome
}

/// A.51.1: with no Frame of Reference in the referenced image, the same
/// spatial sampling and extent; A.51.5.1: without one in the Segmentation,
/// each pixel corresponds to a pixel of the referenced image.
fn sampling(segmentation: &Segmentation, image: &Instance, pair: Pair<'_>, outcome: &mut Outcome) {
    let sections = if segmentation.frame_of_reference.is_none() {
        "A.51.1 and A.51.5.1"
    } else {
        "A.51.1"
    };
    if segmentation.matrix != image.matrix {
        let describe = |matrix: Option<(i64, i64)>| {
            matrix.map_or("absent".to_string(), |(rows, columns)| {
                format!("{rows} x {columns}")
            })
        };
        outcome.violations.push(|| format!(
            "{pair}, which has no Frame of Reference, but the Segmentation's Rows x Columns {} differ from the image's {}; it shall have the image's spatial sampling and extent ({CITATION} {sections})",
            describe(segmentation.matrix),
            describe(image.matrix)
        ));
    }
    if let (Some(own), Some(theirs)) = (&segmentation.spacing, &image.spacing) {
        let same = own.len() == theirs.len()
            && own
                .iter()
                .zip(theirs)
                .all(|(a, b)| (a - b).abs() <= SPACING_TOLERANCE * a.abs().max(b.abs()));
        if !same {
            outcome.violations.push(|| format!(
                "{pair}, which has no Frame of Reference, but the Segmentation's Pixel Spacing {own:?} differs from the image's {theirs:?}; it shall have the image's spatial sampling ({CITATION} {sections})"
            ));
        }
    }
}

/// C.37.1.2.1.1: the referenced image is not an Instance of a Concatenation,
/// and the 2D polylines and polygons wind clockwise viewed from the top of
/// the slide, which the referenced image's Image Orientation (Slide) maps
/// (column, row) coordinates to.
fn annotation_check(corpus: &FamilyCorpus) -> Outcome {
    let mut outcome = Outcome {
        applies: !corpus.annotations.is_empty(),
        ..Outcome::default()
    };
    for annotations in &corpus.annotations {
        for uid in &annotations.references {
            let Some(image) = corpus.instances.get(uid) else {
                continue;
            };
            outcome.compared += 1;
            let pair = Pair {
                from: &annotations.path,
                to: &image.path,
            };
            if image.concatenation {
                outcome.violations.push(|| format!(
                    "{pair}, an Instance of a Concatenation (it has a Concatenation UID); the referenced image shall not be, references being relative to its Concatenation Source ({CITATION} C.37.1.2.1.1)"
                ));
            }
            if annotations.windings.is_empty() {
                continue;
            }
            let Some(orientation) = &image.slide_orientation else {
                outcome.unexamined.push(|| format!(
                    "the winding of the 2D polylines and polygons of {pair}, which has no Image Orientation (Slide) relating its pixels to the top surface of the slide"
                ));
                continue;
            };
            winding(&annotations.windings, orientation, pair, &mut outcome);
        }
    }
    outcome
}

/// Viewed from the top surface (X to the right, Y up), a clockwise outline
/// has negative area; the pixel matrix maps (column, row) to (X, Y) by the
/// row and column direction cosines, whose determinant scales the area.
fn winding(groups: &[GroupWinding], orientation: &[f64], pair: Pair<'_>, outcome: &mut Outcome) {
    let determinant = orientation[0] * orientation[4] - orientation[3] * orientation[1];
    if determinant.abs() < 1e-6 {
        outcome.unexamined.push(|| format!(
            "the winding of the 2D polylines and polygons of {pair}, whose Image Orientation (Slide) rows and columns do not span the slide plane"
        ));
        return;
    }
    for group in groups {
        // Clockwise outlines have (column, row) areas of the sign opposite
        // to the determinant.
        let wrong = if determinant > 0.0 {
            &group.positive
        } else {
            &group.negative
        };
        if let Some(first) = wrong.first {
            outcome.violations.push(|| format!(
                "{pair}: Annotation Group Sequence Item {} has {} {} Annotation(s) winding counter-clockwise viewed from the top surface of the slide (Annotation {first} the first); vertices shall be in clockwise winding order ({CITATION} C.37.1.2.1.1)",
                group.item,
                wrong.count,
                group.graphic_type
            ));
        }
    }
}

/// C.7.4.2.1.1: with a Synchronization Frame of Reference UID, every
/// Instance of the Series of a Parametric Map or SR document shares one
/// Frame of Reference. The Series are gathered once, and each member is
/// compared with the Frame of Reference of the first synchronized object of
/// its Series.
fn synchronization_check(corpus: &FamilyCorpus) -> Outcome {
    let mut outcome = Outcome {
        applies: !corpus.synchronized.is_empty(),
        ..Outcome::default()
    };
    let mut representatives: BTreeMap<&str, &Synchronized> = BTreeMap::new();
    for object in &corpus.synchronized {
        if let Some(series) = &object.series {
            representatives.entry(series).or_insert(object);
        }
    }
    let mut members: BTreeMap<&str, Vec<(&str, &Instance)>> = BTreeMap::new();
    for (uid, instance) in &corpus.instances {
        if let Some(series) = instance.series.as_deref() {
            if representatives.contains_key(series) {
                members.entry(series).or_default().push((uid, instance));
            }
        }
    }
    for (series, object) in representatives {
        let mut differing = 0;
        let mut first = None;
        for (uid, member) in members.get(series).into_iter().flatten() {
            if *uid == object.instance {
                continue;
            }
            outcome.compared += 1;
            if member.frame_of_reference != object.frame_of_reference {
                differing += 1;
                first.get_or_insert(*member);
            }
        }
        if let Some(member) = first {
            outcome.violations.push(|| format!(
                "{} has a Synchronization Frame of Reference UID and Frame of Reference UID {}, but {differing} other Instance(s) of its Series have another, {} with {}; all SOP Instances in the Series shall share the same Frame of Reference ({CITATION} C.7.4.2.1.1)",
                object.path.display(),
                object.frame_of_reference.as_deref().unwrap_or("absent"),
                member.path.display(),
                member.frame_of_reference.as_deref().unwrap_or("absent")
            ));
        }
    }
    outcome
}
