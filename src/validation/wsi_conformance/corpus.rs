use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::super::{ValidationCheck, ValidationStatus};
pub(in crate::validation) use super::copies::Origin;
use super::copies::{Copies, CopyFacts, FirstCopy};
use super::fields::{required_positive_u64, required_string, PixelSpacingMm};
use super::geometry::{slide_coordinate_identity, SlideCoordinateIdentity};
use super::identity::{
    clinical_identity, specimen_identities, ClinicalIdentity, IssuerIdentity, SpecimenIdentity,
};
use super::series_set::{self, SeriesFacts};
use super::{iod_of, rule_check, VL_WSI};
use crate::uid::is_valid_dicom_uid;
use crate::validation::disclosure::shown;
use crate::validation::iod::dicom_edition;
use crate::validation::{intrinsic_check, ValidationProfile};

pub(super) const SPECIMEN_SET_RULE: &str = "intrinsic-wsi-dicom-specimen-uid-set";
const IDENTITY_SET_RULE: &str = "intrinsic-wsi-dicom-identity-set";
const PYRAMID_GEOMETRY_RULE: &str = "intrinsic-wsi-dicom-pyramid-geometry";
const SOURCE_RELATIONSHIP_RULE: &str = "intrinsic-wsi-dicom-source-relationship";
const CLINICAL_IDENTITY_SET_RULE: &str = "intrinsic-wsi-dicom-clinical-identity-set";

/// What the set checks compare, gathered from each object of a
/// pathology-slide IOD as it is read.
#[derive(Debug, Default)]
pub(in crate::validation) struct WsiConformanceCorpus {
    instances: Vec<InstanceFacts>,
    series: Vec<SeriesFacts>,
}

#[derive(Debug)]
struct InstanceFacts {
    path: PathBuf,
    sop_instance_uid: Result<String, String>,
    copy: CopyFacts,
    /// Every IOD's specimens; an object of another IOD than VL WSI without
    /// the Specimen Module has none.
    specimens: Result<Vec<SpecimenIdentity>, String>,
    /// The facts only VL WSI objects are compared by.
    wsi: Option<WsiFacts>,
}

#[derive(Debug)]
struct WsiFacts {
    sop_class_uid: Result<String, String>,
    clinical_identity: Result<ClinicalIdentity, String>,
    slide_coordinate: Result<SlideCoordinateIdentity, String>,
    pyramid: Option<PyramidFact>,
    sources: Option<Result<Vec<SourceReference>, String>>,
}

impl WsiConformanceCorpus {
    /// Records what the set checks need of `object`, read from `path` as
    /// `origin` says; an object of no pathology-slide IOD adds nothing.
    pub(in crate::validation) fn observe(
        &mut self,
        path: &Path,
        object: &DefaultDicomObject,
        origin: Origin,
    ) {
        // A DICOMDIR names no SOP Instance of its own in its Data Set; the
        // File-set check compares it with the files it references.
        let Some(iod) = iod_of(object).filter(|iod| *iod != super::FILE_SET) else {
            return;
        };
        let wsi = (iod == VL_WSI).then(|| WsiFacts {
            sop_class_uid: required_string(object, tags::SOP_CLASS_UID, "SOP Class UID"),
            clinical_identity: clinical_identity(object),
            slide_coordinate: slide_coordinate_identity(object),
            pyramid: pyramid_fact(object, path),
            sources: source_facts(object, path),
        });
        // C.7.6.22 is Mandatory in VL WSI and optional or conditional in
        // the other IODs, whose objects without it describe no specimen.
        let specimens =
            if wsi.is_none() && object.element(tags::SPECIMEN_DESCRIPTION_SEQUENCE).is_err() {
                Ok(Vec::new())
            } else {
                specimen_identities(object)
            };
        self.instances.push(InstanceFacts {
            path: path.to_path_buf(),
            sop_instance_uid: required_string(object, tags::SOP_INSTANCE_UID, "SOP Instance UID"),
            copy: CopyFacts::of(path, object, origin),
            specimens,
            wsi,
        });
        self.series.push(SeriesFacts::of(object, path, iod));
    }

    /// The VL WSI objects with their WSI facts.
    fn wsi(&self) -> impl Iterator<Item = (&InstanceFacts, &WsiFacts)> {
        self.instances
            .iter()
            .filter_map(|facts| facts.wsi.as_ref().map(|wsi| (facts, wsi)))
    }
}

/// The results of the set checks: their checks, and their advisories by the
/// file each concerns, which are departures from the standard's advice or
/// readings of ambiguous text rather than failures.
#[derive(Debug, Default)]
pub(in crate::validation) struct SetResults {
    pub(in crate::validation) checks: Vec<ValidationCheck>,
    pub(in crate::validation) advisories: Vec<(PathBuf, String)>,
}

/// Every set check of `corpus` in `profile`, in report order: one list for
/// every validator that reads a set of objects.
pub(in crate::validation) fn set_checks(
    corpus: &WsiConformanceCorpus,
    profile: ValidationProfile,
) -> SetResults {
    let mut results = SetResults::default();
    results.checks.extend(run_specimen_uid_set_check(corpus));
    if let Some((check, advisories)) = run_identity_set_check(corpus) {
        results.checks.push(check);
        results.advisories.extend(advisories);
    }
    if let Some((check, advisories)) = series_set::run_series_set_check(&corpus.series) {
        results.checks.push(check);
        results.advisories.extend(advisories);
    }
    if profile == ValidationProfile::Core {
        results
            .checks
            .extend(run_clinical_identity_set_check(corpus));
    }
    results
        .checks
        .extend(run_pyramid_geometry_set_check(corpus));
    results
        .checks
        .extend(run_slide_coordinate_set_check(corpus));
    results
        .checks
        .extend(run_source_relationship_set_check(corpus));
    results
}

/// A set check that passes, naming its `advisories` if any, so a report
/// without optional conformance notes still shows them.
pub(super) fn advised_check(
    name: &str,
    citation: &str,
    advisories: &[(PathBuf, String)],
) -> ValidationCheck {
    if advisories.is_empty() {
        return rule_check(Path::new(""), name, citation, Ok(()));
    }
    let shown: Vec<String> = advisories
        .iter()
        .map(|(path, advisory)| format!("{}: {advisory}", path.display()))
        .collect();
    intrinsic_check(
        name,
        None,
        ValidationStatus::Passed,
        format!("{citation}: conformant; advisory: {}", shown.join("; ")),
    )
}

fn run_slide_coordinate_set_check(corpus: &WsiConformanceCorpus) -> Option<ValidationCheck> {
    let mut pyramids = BTreeMap::<String, (&SlideCoordinateIdentity, f64, &Path)>::new();
    let mut compared = false;
    let mut failure = None;
    for (facts, wsi) in corpus.wsi() {
        let Some(pyramid) = &wsi.pyramid else {
            continue;
        };
        let Ok(uid) = &pyramid.uid else {
            continue;
        };
        let coordinate = match &wsi.slide_coordinate {
            Ok(coordinate) => coordinate,
            Err(reason) => {
                failure = Some(format!(
                    "cannot compare slide coordinates in {}: {reason}",
                    facts.path.display()
                ));
                break;
            }
        };
        // Levels place the same image data (PS3.3 C.7.11); their pixel 1\1
        // locations agree within a pixel of the coarser level.
        let pixel = pyramid.extent.as_ref().map_or(0.0, |extent| {
            extent.spacing.row().max(extent.spacing.column())
        });
        if let Some((existing, existing_pixel, existing_path)) = pyramids.get(uid) {
            compared = true;
            if !coordinates_equal(existing, coordinate, pixel.max(*existing_pixel)) {
                failure = Some(format!(
                    "Pyramid UID {} has conflicting total-matrix origin or orientation in {} and {}",
                    shown(tags::PYRAMID_UID, uid),
                    existing_path.display(),
                    facts.path.display()
                ));
                break;
            }
        } else {
            pyramids.insert(uid.clone(), (coordinate, pixel, facts.path.as_path()));
        }
    }
    compared.then(|| {
        rule_check(
            Path::new(""),
            "intrinsic-wsi-dicom-slide-coordinate-system",
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.14"),
            failure.map_or(Ok(()), Err),
        )
    })
}

/// Origins agree within `pixel` mm in X and Y, Z offsets and direction
/// cosines to 1e-6.
fn coordinates_equal(
    left: &SlideCoordinateIdentity,
    right: &SlideCoordinateIdentity,
    pixel: f64,
) -> bool {
    let planar = pixel.max(1e-6);
    let origin = left.origin[..2]
        .iter()
        .zip(&right.origin[..2])
        .all(|(left, right)| (left - right).abs() <= planar);
    origin
        && left.origin[2..]
            .iter()
            .chain(left.orientation.iter())
            .zip(right.origin[2..].iter().chain(right.orientation.iter()))
            .all(|(left, right)| (left - right).abs() <= 1e-6)
}

fn run_clinical_identity_set_check(corpus: &WsiConformanceCorpus) -> Option<ValidationCheck> {
    let mut studies = BTreeMap::<String, (String, String, PathBuf)>::new();
    let mut series = BTreeMap::<String, (String, Option<String>, PathBuf)>::new();
    let mut frames =
        BTreeMap::<String, (String, String, String, Option<IssuerIdentity>, PathBuf)>::new();
    let mut failure = None;

    for (facts, wsi) in corpus.wsi() {
        let identity = match &wsi.clinical_identity {
            Ok(identity) => identity,
            Err(reason) => {
                failure = Some(format!(
                    "cannot compare clinical identity in {}: {reason}",
                    facts.path.display()
                ));
                break;
            }
        };
        let patient = (identity.patient_name.clone(), identity.patient_id.clone());
        if let Some((name, id, path)) = studies.get(&identity.study_uid) {
            if (name, id) != (&patient.0, &patient.1) {
                failure = Some(format!(
                    "Study Instance UID {} maps to conflicting patient identities in {} and {}",
                    shown(tags::STUDY_INSTANCE_UID, &identity.study_uid),
                    path.display(),
                    facts.path.display()
                ));
                break;
            }
        } else {
            studies.insert(
                identity.study_uid.clone(),
                (patient.0.clone(), patient.1.clone(), facts.path.clone()),
            );
        }

        if let Some((study_uid, frame_uid, path)) = series.get(&identity.series_uid) {
            // A LABEL or OVERVIEW image without a Frame of Reference may share
            // the Series of registered images (PS3.3 A.32.8).
            let frames_differ = frame_uid.is_some()
                && identity.frame_of_reference_uid.is_some()
                && frame_uid != &identity.frame_of_reference_uid;
            if study_uid != &identity.study_uid || frames_differ {
                failure = Some(format!(
                    "Series Instance UID {} maps to conflicting Study or Frame of Reference UIDs in {} and {}",
                    shown(tags::SERIES_INSTANCE_UID, &identity.series_uid),
                    path.display(),
                    facts.path.display()
                ));
                break;
            }
        } else {
            series.insert(
                identity.series_uid.clone(),
                (
                    identity.study_uid.clone(),
                    identity.frame_of_reference_uid.clone(),
                    facts.path.clone(),
                ),
            );
        }

        let Some(frame_of_reference_uid) = &identity.frame_of_reference_uid else {
            continue;
        };
        if let Some((study_uid, patient_name, container, issuer, path)) =
            frames.get(frame_of_reference_uid)
        {
            if study_uid != &identity.study_uid
                || patient_name != &identity.patient_name
                || container != &identity.container_identifier
                || issuer != &identity.container_issuer
            {
                failure = Some(format!(
                    "Frame of Reference UID {} maps to conflicting study, patient, or container identity in {} and {}",
                    shown(tags::FRAME_OF_REFERENCE_UID, frame_of_reference_uid),
                    path.display(),
                    facts.path.display()
                ));
                break;
            }
        } else {
            frames.insert(
                frame_of_reference_uid.clone(),
                (
                    identity.study_uid.clone(),
                    identity.patient_name.clone(),
                    identity.container_identifier.clone(),
                    identity.container_issuer.clone(),
                    facts.path.clone(),
                ),
            );
        }
    }

    corpus.wsi().next().is_some().then(|| {
        rule_check(
            Path::new(""),
            CLINICAL_IDENTITY_SET_RULE,
            concat!(
                "DICOM PS3.3 ",
                dicom_edition!(),
                " A.32.8, C.7.1, C.7.2, C.7.3, C.7.4.1, and C.7.6.22"
            ),
            failure.map_or(Ok(()), Err),
        )
    })
}

#[derive(Debug)]
struct PyramidFact {
    path: PathBuf,
    uid: Result<String, String>,
    extent: Result<PyramidExtent, String>,
}

fn pyramid_fact(object: &DefaultDicomObject, path: &Path) -> Option<PyramidFact> {
    object.element(tags::PYRAMID_UID).ok()?;
    Some(PyramidFact {
        path: path.to_path_buf(),
        uid: required_string(object, tags::PYRAMID_UID, "Pyramid UID"),
        extent: pyramid_extent(object, path),
    })
}

fn source_facts(
    object: &DefaultDicomObject,
    path: &Path,
) -> Option<Result<Vec<SourceReference>, String>> {
    let element = object.element(tags::SOURCE_IMAGE_SEQUENCE).ok()?;
    Some((|| {
        let Some(items) = element.items() else {
            return Err(format!(
                "Source Image Sequence is not a sequence in {}",
                path.display()
            ));
        };
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                source_reference(item).map_err(|reason| {
                    format!(
                        "Source Image Sequence item {index} in {} is invalid: {reason}",
                        path.display()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()
    })())
}

/// C.12.1 and PS3.5 9: each SOP Instance UID is a valid UID naming one
/// object. Files sharing one are copies of it: byte-identical copies, and
/// copies encoding the same Attributes and Frames differently, pass with a
/// note, copies whose Attributes differ while their SOP Class and
/// Pixel Data agree are an advisory, since a Storage SCP may coerce
/// Attributes and keep the UID (PS3.4 B.4.1.3), and a different SOP Class or
/// different Pixel Data under one UID fails.
fn run_identity_set_check(
    corpus: &WsiConformanceCorpus,
) -> Option<(ValidationCheck, Vec<(PathBuf, String)>)> {
    const CITATION: &str = concat!(
        "DICOM PS3.3 ",
        dicom_edition!(),
        " C.12.1 and PS3.5 ",
        dicom_edition!(),
        " 9"
    );
    if corpus.instances.is_empty() {
        return None;
    }
    // The first file of each SOP Instance, digested once for all its copies.
    let mut first_copy = BTreeMap::<&str, (&InstanceFacts, FirstCopy)>::new();
    let mut violations = Vec::new();
    let mut notes = Vec::new();
    let mut advisories = Vec::new();
    for facts in &corpus.instances {
        let uid = match &facts.sop_instance_uid {
            Ok(uid) if is_valid_dicom_uid(uid) => uid.as_str(),
            Ok(uid) => {
                violations.push(format!(
                    "invalid SOP Instance UID {} in {}",
                    shown(tags::SOP_INSTANCE_UID, uid),
                    facts.path.display()
                ));
                continue;
            }
            Err(reason) => {
                violations.push(format!(
                    "cannot identify {}: {reason}",
                    facts.path.display()
                ));
                continue;
            }
        };
        let Some((original, first)) = first_copy.get_mut(uid) else {
            first_copy.insert(uid, (facts, FirstCopy::new(&facts.copy)));
            continue;
        };
        let pair = format!(
            "SOP Instance UID {} occurs in both {} and {}",
            shown(tags::SOP_INSTANCE_UID, uid),
            original.path.display(),
            facts.path.display()
        );
        match first.compare(&facts.copy) {
            Ok(Copies::Identical) => notes.push(format!("{pair}, identical copies")),
            Ok(Copies::Reencoded) => notes.push(format!(
                "{pair}, copies of the same Attributes and Pixel Data Frames, encoded differently (preamble, lengths, padding, offset tables or Fragments)"
            )),
            Ok(Copies::Different(why)) => violations.push(format!(
                "{pair}, but {why}; a UID identifies one object, and different objects take different UIDs (PS3.5 {} 9)",
                crate::validation::iod::EDITION
            )),
            Ok(Copies::Coerced(attributes)) => advisories.push((
                facts.path.clone(),
                format!(
                    "{pair}, with the same SOP Class and Pixel Data but different {}; a Storage SCP may coerce Attributes and keep the SOP Instance UID (PS3.4 {} B.4.1.3), so this is a copy of one object, not a reuse of its UID",
                    attributes.join(", "),
                    crate::validation::iod::EDITION
                ),
            )),
            Err(reason) => violations.push(format!("{pair}, and they cannot be compared: {reason}")),
        }
    }
    let mut check = if violations.is_empty() {
        advised_check(IDENTITY_SET_RULE, CITATION, &advisories)
    } else {
        rule_check(
            Path::new(""),
            IDENTITY_SET_RULE,
            CITATION,
            super::violation_result(violations),
        )
    };
    if !notes.is_empty() {
        check
            .message
            .push_str(&format!("; note: {}", notes.join("; ")));
    }
    Some((check, advisories))
}

fn run_specimen_uid_set_check(corpus: &WsiConformanceCorpus) -> Option<ValidationCheck> {
    let mut identities = BTreeMap::<String, (SpecimenIdentity, PathBuf)>::new();
    let mut failure = None;
    for facts in &corpus.instances {
        match &facts.specimens {
            Ok(specimens) => {
                for specimen in specimens {
                    if let Some((existing, existing_path)) = identities.get(&specimen.uid) {
                        if existing != specimen {
                            failure = Some(format!(
                                "Specimen UID {} maps to conflicting identifiers or issuers in {} and {}",
                                shown(tags::SPECIMEN_UID, &specimen.uid),
                                existing_path.display(),
                                facts.path.display()
                            ));
                            break;
                        }
                    } else {
                        identities
                            .insert(specimen.uid.clone(), (specimen.clone(), facts.path.clone()));
                    }
                }
            }
            Err(reason) => {
                failure = Some(format!(
                    "cannot compare specimen identities in {}: {reason}",
                    facts.path.display()
                ));
            }
        }
        if failure.is_some() {
            break;
        }
    }
    if corpus.instances.is_empty() {
        return None;
    }
    Some(rule_check(
        Path::new(""),
        SPECIMEN_SET_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.6.22"),
        failure.map_or(Ok(()), Err),
    ))
}

#[derive(Clone, Debug)]
struct PyramidExtent {
    path: PathBuf,
    width_mm: f64,
    height_mm: f64,
    spacing: PixelSpacingMm,
}

fn run_pyramid_geometry_set_check(corpus: &WsiConformanceCorpus) -> Option<ValidationCheck> {
    let mut pyramids = BTreeMap::<String, Vec<&PyramidFact>>::new();
    let mut failure = None;
    for (_, wsi) in corpus.wsi() {
        let Some(pyramid) = &wsi.pyramid else {
            continue;
        };
        let pyramid_uid = match &pyramid.uid {
            Ok(uid) => uid,
            Err(_) => continue,
        };
        pyramids
            .entry(pyramid_uid.clone())
            .or_default()
            .push(pyramid);
    }
    pyramids.retain(|_, levels| levels.len() >= 2);
    if pyramids.is_empty() {
        return None;
    }

    'pyramids: for (uid, levels) in &pyramids {
        if !is_valid_dicom_uid(uid) {
            failure = Some(format!(
                "invalid Pyramid UID {} in {}",
                shown(tags::PYRAMID_UID, uid),
                levels[0].path.display()
            ));
            break;
        }
        let mut extents = Vec::with_capacity(levels.len());
        for level in levels {
            match &level.extent {
                Ok(extent) => extents.push(extent),
                Err(reason) => {
                    failure = Some(format!("{}: {reason}", level.path.display()));
                    break 'pyramids;
                }
            }
        }
        let Some(reference) = extents.first() else {
            continue;
        };
        for level in &extents[1..] {
            let width_tolerance = reference
                .spacing
                .column()
                .max(level.spacing.column())
                .max(reference.width_mm.abs() * 1e-6);
            let height_tolerance = reference
                .spacing
                .row()
                .max(level.spacing.row())
                .max(reference.height_mm.abs() * 1e-6);
            if (reference.width_mm - level.width_mm).abs() > width_tolerance
                || (reference.height_mm - level.height_mm).abs() > height_tolerance
            {
                failure = Some(format!(
                        "Pyramid UID {} has inconsistent physical extents: {} is {}x{} mm and {} is {}x{} mm",
                        shown(tags::PYRAMID_UID, uid),
                        reference.path.display(),
                        reference.width_mm,
                        reference.height_mm,
                        level.path.display(),
                        level.width_mm,
                        level.height_mm
                    ));
                break 'pyramids;
            }
        }
    }

    Some(rule_check(
        Path::new(""),
        PYRAMID_GEOMETRY_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.6"),
        failure.map_or(Ok(()), Err),
    ))
}

fn pyramid_extent(object: &DefaultDicomObject, path: &Path) -> Result<PyramidExtent, String> {
    let rows = required_positive_u64(
        object,
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        "Total Pixel Matrix Rows",
    )?;
    let columns = required_positive_u64(
        object,
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
        "Total Pixel Matrix Columns",
    )?;
    let spacing = PixelSpacingMm::from_shared_functional_groups(object)?;
    Ok(PyramidExtent {
        path: path.to_path_buf(),
        width_mm: spacing.matrix_width(columns),
        height_mm: spacing.matrix_height(rows),
        spacing,
    })
}

#[derive(Clone, Debug)]
struct SourceReference {
    class_uid: String,
    instance_uid: String,
}

fn run_source_relationship_set_check(corpus: &WsiConformanceCorpus) -> Option<ValidationCheck> {
    if !corpus.wsi().any(|(_, wsi)| wsi.sources.is_some()) {
        return None;
    }
    let mut instances = BTreeMap::<String, (String, PathBuf)>::new();
    let mut references = Vec::<(String, PathBuf, SourceReference)>::new();
    let mut failure = None;
    for (facts, wsi) in corpus.wsi() {
        let instance_uid = match &facts.sop_instance_uid {
            Ok(value) => value.clone(),
            Err(reason) => {
                failure = Some(format!("{}: {reason}", facts.path.display()));
                break;
            }
        };
        let source_references = match &wsi.sources {
            None => &[][..],
            Some(Ok(value)) => value,
            Some(Err(reason)) => {
                failure = Some(reason.clone());
                break;
            }
        };
        let class_uid = match &wsi.sop_class_uid {
            Ok(value) => value.clone(),
            Err(reason) => {
                failure = Some(format!("{}: {reason}", facts.path.display()));
                break;
            }
        };
        instances.insert(instance_uid.clone(), (class_uid, facts.path.clone()));
        references.extend(
            source_references
                .iter()
                .cloned()
                .map(|reference| (instance_uid.clone(), facts.path.clone(), reference)),
        );
    }

    if failure.is_none() {
        for (owner_uid, owner_path, reference) in references {
            if reference.instance_uid == owner_uid {
                failure = Some(format!(
                    "{} has a Source Image reference to itself",
                    owner_path.display()
                ));
                break;
            }
            if let Some((actual_class, target_path)) = instances.get(&reference.instance_uid) {
                if actual_class != &reference.class_uid {
                    failure = Some(format!(
                        "{} references {} with SOP Class UID {}, but {} declares {}",
                        owner_path.display(),
                        shown(tags::REFERENCED_SOP_INSTANCE_UID, &reference.instance_uid),
                        reference.class_uid,
                        target_path.display(),
                        actual_class
                    ));
                    break;
                }
            }
        }
    }

    Some(rule_check(
        Path::new(""),
        SOURCE_RELATIONSHIP_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.6.1 and Table 10-11"),
        failure.map_or(Ok(()), Err),
    ))
}

fn source_reference(item: &InMemDicomObject) -> Result<SourceReference, String> {
    let class_uid = required_string(
        item,
        tags::REFERENCED_SOP_CLASS_UID,
        "Referenced SOP Class UID",
    )?;
    let instance_uid = required_string(
        item,
        tags::REFERENCED_SOP_INSTANCE_UID,
        "Referenced SOP Instance UID",
    )?;
    if !is_valid_dicom_uid(&class_uid) || !is_valid_dicom_uid(&instance_uid) {
        return Err("referenced SOP Class or Instance UID is invalid".into());
    }
    Ok(SourceReference {
        class_uid,
        instance_uid,
    })
}
