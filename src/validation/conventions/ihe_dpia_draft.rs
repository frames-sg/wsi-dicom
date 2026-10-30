//! `ihe-dpia-draft` and `ihe-dpia-draft-jpeg`: the DPIA options that IHE
//! PaLM CP-LAB-279 and CP-LAB-281 add. Both CPs are under ballot, so every
//! source, and so every message, says "draft (CP under ballot)".

use std::collections::BTreeSet;

use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::facts::{text, volume_layers, Facts, Kind, SetIndex};
use super::wg26::UNCOMPRESSED;
use super::{advisory, check, ConventionEntry, ConventionProfile, Out, Subject, Violations};
use crate::validation::disclosure::{difference, shown, shown_all, shown_or};

const P: ConventionProfile = ConventionProfile::IheDpiaDraft;
const JPEG: ConventionProfile = ConventionProfile::IheDpiaDraftJpeg;

/// A section of CP-LAB-279 "Specialize Radiology Transactions in DPIA to
/// add Options and Display Requirements".
macro_rules! cp279 {
    ($section:literal) => {
        concat!(
            "IHE PaLM CP-LAB-279 (last update 2026/06/10) ",
            $section,
            ", draft (CP under ballot)"
        )
    };
}

/// A section of CP-LAB-281 "DPIA Acquisition Complete".
macro_rules! cp281 {
    ($section:literal) => {
        concat!(
            "IHE PaLM CP-LAB-281 (last update 2026/06/10) ",
            $section,
            ", draft (CP under ballot)"
        )
    };
}

pub(super) const TRUE_COLOR: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-true-color",
    cp279!("X.2.3 and 3.1.4.1.2.2"),
    "True Color Tiled Full Single Z Plane Pyramid Option: Photometric Interpretation (0028,0004) is a true color one, RGB, YBR_FULL_422, YBR_ICT or YBR_RCT; monochrome and fluorescence images are outside the option.",
);
pub(super) const PYRAMID: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-pyramid",
    cp279!("3.1.4.1.2.2"),
    "\"A multi-resolution pyramid is required\": at least two VOLUME layers (distinct Total Pixel Matrix sizes) in the Series.",
);
pub(super) const TILED_FULL: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-tiled-full",
    cp279!("3.1.4.1.2.2"),
    "\"Dimension Organization Type (0020,9311) shall be TILED_FULL and the Per-Frame Functional Groups Sequence (5200,9230) shall not be present.\"",
);
pub(super) const SINGLE_Z_PLANE: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-single-z-plane",
    cp279!("3.1.4.1.2.2"),
    "\"Only a single pyramid with a single Z-plane shall be present\": the VOLUME and THUMBNAIL images of the Series have one Pyramid UID at most, Total Pixel Matrix Focal Planes (0048,0303) 1, and one Total Pixel Matrix Origin Sequence Z Offset in Slide Coordinate System (0040,074A).",
);
pub(super) const ONE_SERIES_PER_SCAN: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-one-series-per-scan",
    cp279!("3.1.4.1.2.2 and 4.8.4.1.1.1"),
    "All images of one scan of one slide are in one Series and share the Acquisition UID (0008,0017) where present: one Acquisition UID per Series, and none in two Series.",
);
pub(super) const PYRAMID_IDENTITY: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-pyramid-identity",
    cp279!("3.1.4.1.2.2"),
    "\"All of the images in the pyramid shall have the same value for Frame of Reference UID (0020,0052), and, if present, Pyramid UID (0008,0019)\": the VOLUME and THUMBNAIL images of the Series.",
);
pub(super) const NO_CONCATENATION: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-no-concatenation",
    cp279!("3.1.4.1.2.2"),
    "\"Concatenations shall not be used\": no Concatenation UID (0020,9161), Concatenation Frame Offset Number (0020,9228), In-concatenation Number (0020,9162) or SOP Instance UID of Concatenation Source (0020,0242).",
);
pub(super) const OFFSET_TABLE: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-offset-table",
    cp279!("3.1.4.1.2.2"),
    "\"Either a (non-empty) Basic Offset Table, or an Extended Offset Table, shall be present\" in encapsulated Pixel Data.",
);
pub(super) const JPEG_BASELINE: ConventionEntry = check(
    JPEG,
    "convention-ihe-dpia-draft-jpeg-baseline",
    cp279!("X.2.2 and 3.1.4.1.2.3"),
    "JPEG Storage Option: the layers of the pyramid (VOLUME and THUMBNAIL images) are in baseline JPEG, Transfer Syntax 1.2.840.10008.1.2.4.50; LABEL and OVERVIEW images may also be uncompressed.",
);
pub(super) const COMPLETION_SERIES: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-completion-series",
    cp281!("3.nn3.4.1.2"),
    "Acquisition Complete Option: a KOS titled (113034, DCM, \"Complete Acquisition Content\") is \"in a new Series of the referenced images' Study\".",
);
pub(super) const COMPLETION_WHOLE_IMAGES: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-completion-whole-images",
    cp281!("3.nn3.4.1.2"),
    "\"References to multi-frame images shall reference the entire image, and not selected frames\": no Referenced Frame Number (0008,1160) or Referenced Segment Number (0062,000B) in the completion KOS.",
);
pub(super) const COMPLETION_IDENTITY: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-completion-identity",
    cp281!("3.nn3.4.1.2"),
    "The completion KOS has \"the same Patient, Study and Specimen identifying and descriptive information that is present in the referenced images\": Patient's Name, Patient ID, Birth Date and Sex, Study Instance UID and Accession Number equal, and Container Identifier and Specimen Identifiers equal where the KOS carries them.",
);
pub(super) const COMPLETION_REFERENCES: ConventionEntry = check(
    P,
    "convention-ihe-dpia-draft-completion-references",
    cp281!("X.2.4 and 3.nn34.6.3"),
    "The completion KOS lists \"the complete set of images ... for a single acquisition (scan) of a single slide\": its Current Requested Procedure Evidence Sequence references one Series, every VL WSI image of that Series in the validated set, and every image its IMAGE Content Items reference.",
);
pub(super) const COMPLETION_SPECIMEN: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-draft-completion-specimen-standard-extended",
    concat!(
        "IHE PaLM CP-LAB-281 3.nn3.4.1.2, draft (CP under ballot), against the DICOM PS3.3 ",
        crate::validation::iod::dicom_edition!(),
        " A.35.4 Key Object Selection Document IOD; DICOM CP-2656 (Assigned)"
    ),
    "Conflict: CP-LAB-281 asks the completion KOS for the Specimen information of the images, but the KOS IOD of the pinned edition has no Specimen Module, so Specimen Attributes make it a Standard Extended SOP Instance until DICOM CP-2656 adds the Module. The Patient and Study information is required; the Specimen information is compared only where the KOS carries it.",
);

pub(super) const ENTRIES: &[&ConventionEntry] = &[
    &TRUE_COLOR,
    &PYRAMID,
    &TILED_FULL,
    &SINGLE_Z_PLANE,
    &ONE_SERIES_PER_SCAN,
    &PYRAMID_IDENTITY,
    &NO_CONCATENATION,
    &OFFSET_TABLE,
    &JPEG_BASELINE,
    &COMPLETION_SERIES,
    &COMPLETION_WHOLE_IMAGES,
    &COMPLETION_IDENTITY,
    &COMPLETION_REFERENCES,
    &COMPLETION_SPECIMEN,
];

const TRUE_COLOR_PHOTOMETRIC: &[&str] = &["RGB", "YBR_FULL_422", "YBR_ICT", "YBR_RCT"];

pub(super) fn file_checks(object: &DefaultDicomObject, facts: &Facts, out: &mut Out) {
    if facts.kind != Kind::Wsi {
        return;
    }
    let root: &InMemDicomObject = object;
    let file = Subject::File(&facts.path);
    out.check(
        &TRUE_COLOR,
        file,
        match facts.photometric.as_deref() {
            Some(value) if TRUE_COLOR_PHOTOMETRIC.contains(&value) => Ok(()),
            other => Err(format!(
                "Photometric Interpretation (0028,0004) is {}, not a true color one (RGB, YBR_FULL_422, YBR_ICT or YBR_RCT)",
                other.unwrap_or("absent")
            )),
        },
    );
    out.check(&TILED_FULL, file, tiled_full(root));
    out.check(
        &NO_CONCATENATION,
        file,
        if facts.concatenation_uid.is_some() || facts.concatenation_member {
            Err("the image is a Concatenation member (Concatenation UID, Concatenation Frame Offset Number, In-concatenation Number or SOP Instance UID of Concatenation Source is present)".into())
        } else {
            Ok(())
        },
    );
    offset_table(object, facts, out);
}

/// The JPEG Storage Option, which constrains each image alone.
pub(super) fn jpeg_file_checks(facts: &Facts, out: &mut Out) {
    if facts.kind != Kind::Wsi {
        return;
    }
    let file = Subject::File(&facts.path);
    match &facts.gaps.transfer_syntax {
        Some(reason) => out.skip(&JPEG_BASELINE, file, reason.clone()),
        None => out.check(&JPEG_BASELINE, file, jpeg_baseline(facts)),
    }
}

pub(super) fn set_checks(index: &SetIndex<'_>, facts: &[Facts], out: &mut Out) {
    for (position, series) in index.series.iter().enumerate() {
        let (uid, members) = (series.uid, &series.members);
        let subject = Subject::Series(uid);
        let layers = volume_layers(members);
        out.check(
            &PYRAMID,
            subject,
            if layers.len() >= 2 {
                Ok(())
            } else {
                Err(format!(
                    "the Series has {} VOLUME layer(s), but a multi-resolution pyramid is required",
                    layers.len()
                ))
            },
        );
        out.check(&SINGLE_Z_PLANE, subject, single_z_plane(members));
        out.check(
            &ONE_SERIES_PER_SCAN,
            subject,
            one_series_per_scan(index, position),
        );
        out.check(&PYRAMID_IDENTITY, subject, pyramid_identity(members));
    }
    completion(index, facts, out);
}

fn tiled_full(root: &InMemDicomObject) -> Result<(), String> {
    let mut found = Violations::default();
    let organization = text(root, tags::DIMENSION_ORGANIZATION_TYPE);
    if organization.as_deref() != Some("TILED_FULL") {
        found.push(|| {
            format!(
                "Dimension Organization Type (0020,9311) is {}, but shall be TILED_FULL",
                organization.as_deref().unwrap_or("absent")
            )
        });
    }
    if root
        .get(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .is_some()
    {
        found.push(|| {
            "the Per-Frame Functional Groups Sequence (5200,9230) is present, but shall not be"
                .into()
        });
    }
    found.result()
}

fn offset_table(object: &DefaultDicomObject, facts: &Facts, out: &mut Out) {
    let file = Subject::File(&facts.path);
    if let Some(reason) = &facts.gaps.pixel_data {
        out.skip(&OFFSET_TABLE, file, reason.clone());
        return;
    }
    let Some(element) = object.get(tags::PIXEL_DATA) else {
        out.skip(
            &OFFSET_TABLE,
            file,
            "the object has no Pixel Data (7FE0,0010) to hold an offset table".into(),
        );
        return;
    };
    // Native Pixel Data has no Basic Offset Table to require.
    let Some(basic) = element.value().offset_table() else {
        return;
    };
    let extended = object.get(tags::EXTENDED_OFFSET_TABLE).is_some();
    out.check(
        &OFFSET_TABLE,
        file,
        if !basic.is_empty() || extended {
            Ok(())
        } else {
            Err("the encapsulated Pixel Data has an empty Basic Offset Table and no Extended Offset Table (7FE0,0001)".into())
        },
    );
}

fn jpeg_baseline(facts: &Facts) -> Result<(), String> {
    let syntax = facts.transfer_syntax.as_str();
    if syntax == uids::JPEG_BASELINE8_BIT || (facts.shows_label() && UNCOMPRESSED.contains(&syntax))
    {
        return Ok(());
    }
    Err(format!(
        "the {} image is in Transfer Syntax {syntax}, but {}",
        facts.flavor.as_deref().unwrap_or("WSI"),
        if facts.shows_label() {
            "a LABEL or OVERVIEW image is baseline JPEG (1.2.840.10008.1.2.4.50) or uncompressed"
        } else {
            "the layers of the pyramid are baseline JPEG (1.2.840.10008.1.2.4.50)"
        }
    ))
}

fn values<'a>(
    members: &[&'a Facts],
    value: impl Fn(&'a Facts) -> Option<&'a str>,
) -> BTreeSet<&'a str> {
    members.iter().filter_map(|fact| value(fact)).collect()
}

fn single_z_plane(members: &[&Facts]) -> Result<(), String> {
    let layers: Vec<&Facts> = members
        .iter()
        .copied()
        .filter(|fact| fact.is_layer())
        .collect();
    let mut found = Violations::default();
    let pyramids = values(&layers, |fact| fact.pyramid_uid.as_deref());
    if pyramids.len() > 1 {
        found.push(|| {
            format!(
                "the Series holds {} pyramids (Pyramid UIDs {})",
                pyramids.len(),
                shown_all(tags::PYRAMID_UID, pyramids.into_iter(), ", ")
            )
        });
    }
    for fact in &layers {
        if fact.focal_planes != Some(1) {
            found.push(|| {
                format!(
                    "{} has Total Pixel Matrix Focal Planes {}, but it shall be 1",
                    fact.path.display(),
                    fact.focal_planes
                        .map_or("absent".to_string(), |planes| planes.to_string())
                )
            });
        }
    }
    let offsets: BTreeSet<String> = layers
        .iter()
        .map(|fact| {
            fact.origin
                .and_then(|origin| origin.z)
                .map_or("absent".to_string(), |z| format!("{z}"))
        })
        .collect();
    if offsets.len() > 1 {
        found.push(|| {
            format!(
                "the pyramid images have different Total Pixel Matrix Origin Z Offsets ({})",
                offsets.into_iter().collect::<Vec<_>>().join(", ")
            )
        });
    }
    found.result()
}

fn one_series_per_scan(index: &SetIndex<'_>, position: usize) -> Result<(), String> {
    let mut found = Violations::default();
    let acquisitions = &index.series[position].acquisitions;
    if acquisitions.len() > 1 {
        found.push(|| {
            format!(
                "the Series has {} Acquisition UIDs ({}), but the images of one scan share one",
                acquisitions.len(),
                shown_all(tags::ACQUISITION_UID, acquisitions.iter().copied(), ", ")
            )
        });
    }
    for (other, shared) in index.shared_acquisitions(position) {
        found.push(|| format!(
            "Acquisition UID {} also occurs in Series {}, but all images of one scan are in a single Series",
            shown_all(tags::ACQUISITION_UID, shared.iter().copied(), ", "),
            shown(tags::SERIES_INSTANCE_UID, other)
        ));
    }
    found.result()
}

fn pyramid_identity(members: &[&Facts]) -> Result<(), String> {
    let layers: Vec<&Facts> = members
        .iter()
        .copied()
        .filter(|fact| fact.is_layer())
        .collect();
    let mut found = Violations::default();
    let frames: BTreeSet<Option<&str>> = layers
        .iter()
        .map(|fact| fact.frame_of_reference_uid.as_deref())
        .collect();
    if frames.len() > 1 {
        found.push(|| {
            format!(
                "the pyramid images have different Frame of Reference UIDs ({})",
                frames
                    .into_iter()
                    .map(|uid| shown_or(tags::FRAME_OF_REFERENCE_UID, uid, "absent"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });
    }
    let pyramids = values(&layers, |fact| fact.pyramid_uid.as_deref());
    if pyramids.len() > 1 {
        found.push(|| {
            format!(
                "the pyramid images have different Pyramid UIDs ({})",
                shown_all(tags::PYRAMID_UID, pyramids.into_iter(), ", ")
            )
        });
    }
    found.result()
}

fn completion(index: &SetIndex<'_>, facts: &[Facts], out: &mut Out) {
    let documents: Vec<&Facts> = facts.iter().filter(|fact| fact.completion).collect();
    if documents.is_empty() {
        for entry in [
            &COMPLETION_SERIES,
            &COMPLETION_WHOLE_IMAGES,
            &COMPLETION_IDENTITY,
            &COMPLETION_REFERENCES,
        ] {
            out.skip(
                entry,
                Subject::Set,
                "the set has no Key Object Selection titled (113034, DCM, \"Complete Acquisition Content\"); the Acquisition Complete Option checks apply to such documents only".into(),
            );
        }
        return;
    }
    for document in documents {
        completion_document(document, index, out);
    }
}

fn completion_document(document: &Facts, index: &SetIndex<'_>, out: &mut Out) {
    let file = Subject::File(&document.path);
    let referenced = index.referenced(
        document
            .references
            .iter()
            .chain(&document.content_references),
    );
    out.check(&COMPLETION_WHOLE_IMAGES, file, whole_images(document));
    out.check(&COMPLETION_REFERENCES, file, references(document, index));
    if referenced.is_empty() {
        let reason =
            "none of the images the completion KOS references is in the validated set".to_string();
        out.skip(&COMPLETION_SERIES, file, reason.clone());
        out.skip(&COMPLETION_IDENTITY, file, reason);
        return;
    }
    let mut found = Violations::default();
    for image in &referenced {
        if image.study_uid != document.study_uid {
            found.push(|| {
                format!(
                    "the KOS is in Study {:?}, but the referenced image {} is in Study {:?}",
                    document.study_uid,
                    image.path.display(),
                    image.study_uid
                )
            });
        }
        if image.series_uid == document.series_uid {
            found.push(|| {
                format!(
                    "the KOS is in the Series of the referenced image {}, not a new Series",
                    image.path.display()
                )
            });
        }
    }
    out.check(&COMPLETION_SERIES, file, found.result());
    out.check(&COMPLETION_IDENTITY, file, identity(document, &referenced));
    let carries_specimen =
        document.container_identifier.is_some() || !document.specimen_identifiers.is_empty();
    out.advise(
        &COMPLETION_SPECIMEN,
        file,
        if carries_specimen {
            format!("the completion KOS carries Specimen Attributes, which the DICOM {} KOS IOD does not define (Standard Extended)", crate::validation::DICOM_EDITION)
        } else {
            format!("the completion KOS has no Specimen Attributes; CP-LAB-281 asks for them, but the DICOM {} KOS IOD does not define them", crate::validation::DICOM_EDITION)
        },
    );
}

fn whole_images(document: &Facts) -> Result<(), String> {
    if document.content_truncated {
        return Err(format!(
            "the content tree nests deeper than {} levels, so not every IMAGE Content Item could be read",
            crate::validation::iod::MAX_NESTING
        ));
    }
    let mut found = Violations::default();
    for reference in document
        .references
        .iter()
        .chain(&document.content_references)
        .filter(|reference| reference.selects_frames)
    {
        found.push(|| {
            format!(
                "the reference to {} selects Frames or Segments, not the entire image",
                reference.instance
            )
        });
    }
    found.result()
}

fn references(document: &Facts, index: &SetIndex<'_>) -> Result<(), String> {
    let mut found = Violations::default();
    let series: BTreeSet<&str> = document
        .references
        .iter()
        .filter_map(|reference| reference.series.as_deref())
        .collect();
    if series.len() != 1 {
        found.push(|| format!(
            "the Current Requested Procedure Evidence Sequence references {} Series ({}), but one scan of one slide is one Series",
            series.len(),
            series.iter().copied().collect::<Vec<_>>().join(", ")
        ));
    }
    let listed: BTreeSet<&str> = document
        .references
        .iter()
        .map(|reference| reference.instance.as_str())
        .collect();
    for fact in index.members_of(series.iter().copied()) {
        let Some(uid) = fact.sop_instance_uid.as_deref() else {
            continue;
        };
        if !listed.contains(uid) {
            found.push(|| {
                format!(
                    "{} of the referenced Series is not in the evidence",
                    fact.path.display()
                )
            });
        }
    }
    for reference in &document.content_references {
        if !listed.contains(reference.instance.as_str()) {
            found.push(|| format!(
                "IMAGE Content Item reference {} is not in the Current Requested Procedure Evidence Sequence",
                reference.instance
            ));
        }
    }
    found.result()
}

/// "`what` is <own> in the KOS, but <theirs> in <image>", the Values of
/// `tag` shown as `disclosure` allows.
fn differing(
    tag: dicom_core::Tag,
    what: &str,
    own: Option<&str>,
    theirs: Option<&str>,
    image: &Facts,
) -> String {
    format!(
        "{what} is {} in the KOS, but {} in {}{}",
        shown_or(tag, own, "absent"),
        shown_or(tag, theirs, "absent"),
        image.path.display(),
        own.zip(theirs)
            .map(|(own, theirs)| difference(tag, own, theirs).to_string())
            .unwrap_or_default()
    )
}

fn identity(document: &Facts, referenced: &[&Facts]) -> Result<(), String> {
    let mut found = Violations::default();
    for image in referenced {
        for (tag, what, own, theirs) in [
            (
                tags::PATIENT_NAME,
                "Patient's Name",
                &document.patient.name,
                &image.patient.name,
            ),
            (
                tags::PATIENT_ID,
                "Patient ID",
                &document.patient.id,
                &image.patient.id,
            ),
            (
                tags::PATIENT_BIRTH_DATE,
                "Patient's Birth Date",
                &document.patient.birth_date,
                &image.patient.birth_date,
            ),
            (
                tags::PATIENT_SEX,
                "Patient's Sex",
                &document.patient.sex,
                &image.patient.sex,
            ),
            (
                tags::ACCESSION_NUMBER,
                "Accession Number",
                &document.accession,
                &image.accession,
            ),
            (
                tags::CONTAINER_IDENTIFIER,
                "Container Identifier",
                &document.container_identifier,
                &image.container_identifier,
            ),
        ] {
            let compared = tag != tags::CONTAINER_IDENTIFIER || own.is_some();
            if compared && own != theirs {
                found.push(|| differing(tag, what, own.as_deref(), theirs.as_deref(), image));
            }
        }
        if !document.specimen_identifiers.is_empty()
            && document.specimen_identifiers != image.specimen_identifiers
        {
            found.push(|| {
                let list = |identifiers: &[String]| {
                    shown_all(
                        tags::SPECIMEN_IDENTIFIER,
                        identifiers.iter().map(String::as_str),
                        ", ",
                    )
                };
                format!(
                    "the Specimen Identifiers are [{}] in the KOS, but [{}] in {}",
                    list(&document.specimen_identifiers),
                    list(&image.specimen_identifiers),
                    image.path.display()
                )
            });
        }
    }
    found.result()
}
