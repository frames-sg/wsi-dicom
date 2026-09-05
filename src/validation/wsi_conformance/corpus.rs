use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::super::ValidationCheck;
use super::fields::{required_positive_u64, required_string, PixelSpacingMm};
use super::instance::{
    clinical_identity, is_vl_wsi, slide_coordinate_identity, specimen_identities, ClinicalIdentity,
    IssuerIdentity, SlideCoordinateIdentity, SpecimenIdentity,
};
use super::rule_check;
use crate::uid::is_valid_dicom_uid;

const SPECIMEN_SET_RULE: &str = "intrinsic-wsi-dicom-2026c-specimen-uid-set";
const IDENTITY_SET_RULE: &str = "intrinsic-wsi-dicom-2026c-identity-set";
const PYRAMID_GEOMETRY_RULE: &str = "intrinsic-wsi-dicom-2026c-pyramid-geometry";
const SOURCE_RELATIONSHIP_RULE: &str = "intrinsic-wsi-dicom-2026c-source-relationship";
const CLINICAL_IDENTITY_SET_RULE: &str = "intrinsic-wsi-dicom-2026c-clinical-identity-set";

#[derive(Debug, Default)]
pub(in crate::validation) struct WsiConformanceCorpus {
    instances: Vec<WsiInstanceFacts>,
}

#[derive(Debug)]
struct WsiInstanceFacts {
    path: PathBuf,
    sop_instance_uid: Result<String, String>,
    sop_class_uid: Result<String, String>,
    specimens: Result<Vec<SpecimenIdentity>, String>,
    clinical_identity: Result<ClinicalIdentity, String>,
    slide_coordinate: Result<SlideCoordinateIdentity, String>,
    pyramid: Option<PyramidFact>,
    sources: Option<Result<Vec<SourceReference>, String>>,
}

impl WsiConformanceCorpus {
    pub(in crate::validation) fn observe(&mut self, path: &Path, object: &DefaultDicomObject) {
        if !is_vl_wsi(object) {
            return;
        }
        let sop_instance_uid = required_string(object, tags::SOP_INSTANCE_UID, "SOP Instance UID");
        let sop_class_uid = required_string(object, tags::SOP_CLASS_UID, "SOP Class UID");
        self.instances.push(WsiInstanceFacts {
            path: path.to_path_buf(),
            sop_instance_uid: sop_instance_uid.clone(),
            sop_class_uid,
            specimens: specimen_identities(object),
            clinical_identity: clinical_identity(object),
            slide_coordinate: slide_coordinate_identity(object),
            pyramid: pyramid_fact(object, path),
            sources: source_facts(object, path),
        });
    }
}

pub(in crate::validation) fn run_slide_coordinate_set_check(
    corpus: &WsiConformanceCorpus,
) -> Option<ValidationCheck> {
    let mut pyramids = BTreeMap::<String, (&SlideCoordinateIdentity, &Path)>::new();
    let mut compared = false;
    let mut failure = None;
    for facts in &corpus.instances {
        let Some(pyramid) = &facts.pyramid else {
            continue;
        };
        let Ok(uid) = &pyramid.uid else {
            continue;
        };
        let coordinate = match &facts.slide_coordinate {
            Ok(coordinate) => coordinate,
            Err(reason) => {
                failure = Some(format!(
                    "cannot compare slide coordinates in {}: {reason}",
                    facts.path.display()
                ));
                break;
            }
        };
        if let Some((existing, existing_path)) = pyramids.get(uid) {
            compared = true;
            if !coordinates_equal(existing, coordinate) {
                failure = Some(format!(
                    "Pyramid UID {uid} has conflicting total-matrix origin or orientation in {} and {}",
                    existing_path.display(),
                    facts.path.display()
                ));
                break;
            }
        } else {
            pyramids.insert(uid.clone(), (coordinate, facts.path.as_path()));
        }
    }
    compared.then(|| {
        rule_check(
            Path::new(""),
            "intrinsic-wsi-dicom-2026c-slide-coordinate-system",
            "DICOM PS3.3 2026c C.8.12.14",
            failure.map_or(Ok(()), Err),
        )
    })
}

fn coordinates_equal(left: &SlideCoordinateIdentity, right: &SlideCoordinateIdentity) -> bool {
    left.origin
        .iter()
        .chain(left.orientation.iter())
        .zip(right.origin.iter().chain(right.orientation.iter()))
        .all(|(left, right)| (left - right).abs() <= 1e-6)
}

pub(in crate::validation) fn run_clinical_identity_set_check(
    corpus: &WsiConformanceCorpus,
) -> Option<ValidationCheck> {
    let mut studies = BTreeMap::<String, (String, String, PathBuf)>::new();
    let mut series = BTreeMap::<String, (String, String, PathBuf)>::new();
    let mut frames =
        BTreeMap::<String, (String, String, String, Option<IssuerIdentity>, PathBuf)>::new();
    let mut failure = None;

    for facts in &corpus.instances {
        let identity = match &facts.clinical_identity {
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
                    identity.study_uid,
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
            if study_uid != &identity.study_uid || frame_uid != &identity.frame_of_reference_uid {
                failure = Some(format!(
                    "Series Instance UID {} maps to conflicting Study or Frame of Reference UIDs in {} and {}",
                    identity.series_uid,
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

        if let Some((study_uid, patient_name, container, issuer, path)) =
            frames.get(&identity.frame_of_reference_uid)
        {
            if study_uid != &identity.study_uid
                || patient_name != &identity.patient_name
                || container != &identity.container_identifier
                || issuer != &identity.container_issuer
            {
                failure = Some(format!(
                    "Frame of Reference UID {} maps to conflicting study, patient, or container identity in {} and {}",
                    identity.frame_of_reference_uid,
                    path.display(),
                    facts.path.display()
                ));
                break;
            }
        } else {
            frames.insert(
                identity.frame_of_reference_uid.clone(),
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

    (!corpus.instances.is_empty()).then(|| {
        rule_check(
            Path::new(""),
            CLINICAL_IDENTITY_SET_RULE,
            "DICOM PS3.3 2026c A.32.8, C.7.1, C.7.2, C.7.3, C.7.4.1, and C.7.6.22",
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

pub(in crate::validation) fn run_identity_set_check(
    corpus: &WsiConformanceCorpus,
) -> Option<ValidationCheck> {
    let mut instances = BTreeMap::<String, PathBuf>::new();
    let mut failure = None;
    for facts in &corpus.instances {
        let uid = match &facts.sop_instance_uid {
            Ok(uid) if is_valid_dicom_uid(uid) => uid.clone(),
            Ok(uid) => {
                failure = Some(format!(
                    "invalid SOP Instance UID {uid:?} in {}",
                    facts.path.display()
                ));
                break;
            }
            Err(reason) => {
                failure = Some(format!(
                    "cannot identify {}: {reason}",
                    facts.path.display()
                ));
                break;
            }
        };
        if let Some(existing) = instances.insert(uid.clone(), facts.path.clone()) {
            failure = Some(format!(
                "SOP Instance UID {uid} occurs in both {} and {}",
                existing.display(),
                facts.path.display()
            ));
            break;
        }
    }
    (!corpus.instances.is_empty()).then(|| {
        rule_check(
            Path::new(""),
            IDENTITY_SET_RULE,
            "DICOM PS3.3 2026c C.12.1",
            failure.map_or(Ok(()), Err),
        )
    })
}

pub(in crate::validation) fn run_specimen_uid_set_check(
    corpus: &WsiConformanceCorpus,
) -> Option<ValidationCheck> {
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
                                specimen.uid,
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
        "DICOM PS3.3 2026c C.7.6.22",
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

pub(in crate::validation) fn run_pyramid_geometry_set_check(
    corpus: &WsiConformanceCorpus,
) -> Option<ValidationCheck> {
    let mut pyramids = BTreeMap::<String, Vec<&PyramidFact>>::new();
    let mut failure = None;
    for facts in &corpus.instances {
        let Some(pyramid) = &facts.pyramid else {
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
                "invalid Pyramid UID {uid:?} in {}",
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
                        "Pyramid UID {uid} has inconsistent physical extents: {} is {}x{} mm and {} is {}x{} mm",
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
        "DICOM PS3.3 2026c C.8.12.6",
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

pub(in crate::validation) fn run_source_relationship_set_check(
    corpus: &WsiConformanceCorpus,
) -> Option<ValidationCheck> {
    if !corpus.instances.iter().any(|facts| facts.sources.is_some()) {
        return None;
    }
    let mut instances = BTreeMap::<String, (String, PathBuf)>::new();
    let mut references = Vec::<(String, PathBuf, SourceReference)>::new();
    let mut failure = None;
    for facts in &corpus.instances {
        let instance_uid = match &facts.sop_instance_uid {
            Ok(value) => value.clone(),
            Err(reason) => {
                failure = Some(format!("{}: {reason}", facts.path.display()));
                break;
            }
        };
        let source_references = match &facts.sources {
            None => &[][..],
            Some(Ok(value)) => value,
            Some(Err(reason)) => {
                failure = Some(reason.clone());
                break;
            }
        };
        let class_uid = match &facts.sop_class_uid {
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
                        reference.instance_uid,
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
        "DICOM PS3.3 2026c C.7.6.1 and Table 10-11",
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
