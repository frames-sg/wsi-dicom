//! Conformant documents about a VL WSI image, built like the ones highdicom
//! writes: an "acquisition complete" Key Object Selection Document (TID 2010)
//! and a TID 1500 Measurement Report whose Measurement Group outlines a
//! region of the slide image (SCOORD SELECTED FROM IMAGE), with the Content
//! Item builders the tests break them with.

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, FileMetaTableBuilder, InMemDicomObject};

pub(super) const KOS: &str = uids::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE;
pub(super) const BASIC_TEXT_SR: &str = uids::BASIC_TEXT_SR_STORAGE;
pub(super) const ENHANCED_SR: &str = uids::ENHANCED_SR_STORAGE;
pub(super) const COMPREHENSIVE_SR: &str = uids::COMPREHENSIVE_SR_STORAGE;
pub(super) const COMPREHENSIVE_3D_SR: &str = uids::COMPREHENSIVE3_DSR_STORAGE;
pub(super) const WSI: &str = crate::VL_WSI_SOP_CLASS_UID;
pub(super) const STUDY: &str = "1.2.826.0.1.3680043.10.999.60.1";
pub(super) const OTHER_STUDY: &str = "1.2.826.0.1.3680043.10.999.60.2";
pub(super) const IMAGE: &str = "1.2.826.0.1.3680043.10.999.60.3";
pub(super) const IMAGE_SERIES: &str = "1.2.826.0.1.3680043.10.999.60.4";
pub(super) const DOCUMENT: &str = "1.2.826.0.1.3680043.10.999.60.10";
const DOCUMENT_SERIES: &str = "1.2.826.0.1.3680043.10.999.60.11";
const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";

pub(super) fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::from(value))
}

pub(super) fn item(elements: Vec<InMemElement>) -> InMemDicomObject {
    InMemDicomObject::from_element_iter(elements)
}

pub(super) fn sq(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

pub(super) fn floats(tag: Tag, values: &[f32]) -> InMemElement {
    DataElement::new(
        tag,
        VR::FL,
        PrimitiveValue::F32(values.iter().copied().collect()),
    )
}

pub(super) fn code(scheme: &str, value: &str, meaning: &str) -> InMemDicomObject {
    item(vec![
        text(tags::CODE_VALUE, VR::SH, value),
        text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, scheme),
        text(tags::CODE_MEANING, VR::LO, meaning),
    ])
}

/// A Content Item: its Relationship Type, Value Type, Concept Name if any,
/// and further Attributes.
pub(super) fn content(
    relationship: &str,
    value_type: &str,
    concept: Option<InMemDicomObject>,
    mut elements: Vec<InMemElement>,
) -> InMemDicomObject {
    elements.push(text(tags::RELATIONSHIP_TYPE, VR::CS, relationship));
    elements.push(text(tags::VALUE_TYPE, VR::CS, value_type));
    if let Some(concept) = concept {
        elements.push(sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![concept]));
    }
    item(elements)
}

/// A by-reference Content Item naming `identifier`.
pub(super) fn by_reference(relationship: &str, identifier: &[u32]) -> InMemDicomObject {
    item(vec![
        text(tags::RELATIONSHIP_TYPE, VR::CS, relationship),
        DataElement::new(
            tags::REFERENCED_CONTENT_ITEM_IDENTIFIER,
            VR::UL,
            PrimitiveValue::U32(identifier.iter().copied().collect()),
        ),
    ])
}

pub(super) fn reference(class: &str, instance: &str) -> InMemDicomObject {
    item(vec![
        text(tags::REFERENCED_SOP_CLASS_UID, VR::UI, class),
        text(tags::REFERENCED_SOP_INSTANCE_UID, VR::UI, instance),
    ])
}

/// An IMAGE Content Item referencing the slide image, without a purpose of
/// reference.
pub(super) fn image(relationship: &str) -> InMemDicomObject {
    content(
        relationship,
        "IMAGE",
        None,
        vec![sq(
            tags::REFERENCED_SOP_SEQUENCE,
            vec![reference(WSI, IMAGE)],
        )],
    )
}

/// A CONTAINER Content Item.
pub(super) fn container(
    relationship: &str,
    concept: InMemDicomObject,
    children: Vec<InMemDicomObject>,
) -> InMemDicomObject {
    content(
        relationship,
        "CONTAINER",
        Some(concept),
        vec![
            text(tags::CONTINUITY_OF_CONTENT, VR::CS, "SEPARATE"),
            sq(tags::CONTENT_SEQUENCE, children),
        ],
    )
}

/// A Hierarchical SOP Instance Reference Macro Item naming one Instance.
pub(super) fn evidence(study: &str, series: &str, class: &str, instance: &str) -> InMemDicomObject {
    item(vec![
        text(tags::STUDY_INSTANCE_UID, VR::UI, study),
        sq(
            tags::REFERENCED_SERIES_SEQUENCE,
            vec![item(vec![
                text(tags::SERIES_INSTANCE_UID, VR::UI, series),
                sq(
                    tags::REFERENCED_SOP_SEQUENCE,
                    vec![reference(class, instance)],
                ),
            ])],
        ),
    ])
}

pub(super) fn template_sequence(identifier: &str) -> InMemElement {
    sq(
        tags::CONTENT_TEMPLATE_SEQUENCE,
        vec![item(vec![
            text(tags::MAPPING_RESOURCE, VR::CS, "DCMR"),
            text(tags::TEMPLATE_IDENTIFIER, VR::CS, identifier),
        ])],
    )
}

/// The Patient, Study, Series, Equipment and SOP Common Attributes of a
/// document of `sop_class`, the root CONTAINER titled `title` and built
/// from `template`, and the slide image as evidence.
fn document(
    sop_class: &str,
    modality: &str,
    title: InMemDicomObject,
    template: &str,
) -> InMemDicomObject {
    item(vec![
        text(tags::SOP_CLASS_UID, VR::UI, sop_class),
        text(tags::SOP_INSTANCE_UID, VR::UI, DOCUMENT),
        text(tags::STUDY_DATE, VR::DA, "20261024"),
        text(tags::CONTENT_DATE, VR::DA, "20261024"),
        text(tags::STUDY_TIME, VR::TM, "120000"),
        text(tags::CONTENT_TIME, VR::TM, "120000"),
        text(tags::ACCESSION_NUMBER, VR::SH, "RESEARCH"),
        text(tags::MODALITY, VR::CS, modality),
        text(tags::MANUFACTURER, VR::LO, "WSI-DICOM test"),
        text(tags::REFERRING_PHYSICIAN_NAME, VR::PN, ""),
        sq(tags::REFERENCED_PERFORMED_PROCEDURE_STEP_SEQUENCE, vec![]),
        text(tags::PATIENT_NAME, VR::PN, "RESEARCH^PLACEHOLDER"),
        text(tags::PATIENT_ID, VR::LO, "RESEARCH"),
        text(tags::PATIENT_BIRTH_DATE, VR::DA, ""),
        text(tags::PATIENT_SEX, VR::CS, ""),
        text(tags::STUDY_INSTANCE_UID, VR::UI, STUDY),
        text(tags::SERIES_INSTANCE_UID, VR::UI, DOCUMENT_SERIES),
        text(tags::STUDY_ID, VR::SH, "1"),
        text(tags::SERIES_NUMBER, VR::IS, "98"),
        text(tags::INSTANCE_NUMBER, VR::IS, "1"),
        text(tags::VALUE_TYPE, VR::CS, "CONTAINER"),
        sq(tags::CONCEPT_NAME_CODE_SEQUENCE, vec![title]),
        text(tags::CONTINUITY_OF_CONTENT, VR::CS, "SEPARATE"),
        template_sequence(template),
        sq(
            tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
            vec![evidence(STUDY, IMAGE_SERIES, WSI, IMAGE)],
        ),
    ])
}

/// TID 2010 "Complete Acquisition Content" flagging the slide image.
pub(super) fn key_object_selection() -> InMemDicomObject {
    let mut object = document(
        KOS,
        "KO",
        code("DCM", "113034", "Complete Acquisition Content"),
        "2010",
    );
    object.put(sq(
        tags::CONTENT_SEQUENCE,
        vec![
            content(
                "CONTAINS",
                "TEXT",
                Some(code("DCM", "113012", "Key Object Description")),
                vec![text(
                    tags::TEXT_VALUE,
                    VR::UT,
                    "Scan complete\r\nAll levels",
                )],
            ),
            image("CONTAINS"),
        ],
    ));
    object
}

/// The Observer Context of the device that measured (TID 1002, TID 1004).
fn device_observer() -> Vec<InMemDicomObject> {
    vec![
        content(
            "HAS OBS CONTEXT",
            "CODE",
            Some(code("DCM", "121005", "Observer Type")),
            vec![sq(
                tags::CONCEPT_CODE_SEQUENCE,
                vec![code("DCM", "121007", "Device")],
            )],
        ),
        content(
            "HAS OBS CONTEXT",
            "UIDREF",
            Some(code("DCM", "121012", "Device Observer UID")),
            vec![text(tags::UID, VR::UI, "1.2.826.0.1.3680043.10.999.60.20")],
        ),
    ]
}

/// The SCOORD of TID 1410 Row 5: a closed outline on the slide image.
pub(super) fn region() -> InMemDicomObject {
    content(
        "CONTAINS",
        "SCOORD",
        Some(code("DCM", "111030", "Image Region")),
        vec![
            text(tags::GRAPHIC_TYPE, VR::CS, "POLYLINE"),
            floats(
                tags::GRAPHIC_DATA,
                &[10.0, 10.0, 50.0, 10.0, 50.0, 40.0, 10.0, 10.0],
            ),
            text(tags::PIXEL_ORIGIN_INTERPRETATION, VR::CS, "VOLUME"),
            sq(tags::CONTENT_SEQUENCE, vec![image("SELECTED FROM")]),
        ],
    )
}

/// A NUM Content Item of 600 square micrometers.
pub(super) fn area() -> InMemDicomObject {
    content(
        "CONTAINS",
        "NUM",
        Some(code("SCT", "42798000", "Area")),
        vec![sq(
            tags::MEASURED_VALUE_SEQUENCE,
            vec![item(vec![
                sq(
                    tags::MEASUREMENT_UNITS_CODE_SEQUENCE,
                    vec![code("UCUM", "um2", "square micrometer")],
                ),
                text(tags::NUMERIC_VALUE, VR::DS, "600"),
            ])],
        )],
    )
}

/// TID 1410 Planar ROI Measurements: a tracked region of tissue and its
/// area.
pub(super) fn measurement_group() -> InMemDicomObject {
    container(
        "CONTAINS",
        code("DCM", "125007", "Measurement Group"),
        vec![
            content(
                "HAS OBS CONTEXT",
                "TEXT",
                Some(code("DCM", "112039", "Tracking Identifier")),
                vec![text(tags::TEXT_VALUE, VR::UT, "Region 1")],
            ),
            content(
                "HAS OBS CONTEXT",
                "UIDREF",
                Some(code("DCM", "112040", "Tracking Unique Identifier")),
                vec![text(tags::UID, VR::UI, "1.2.826.0.1.3680043.10.999.60.21")],
            ),
            content(
                "CONTAINS",
                "CODE",
                Some(code("DCM", "121071", "Finding")),
                vec![sq(
                    tags::CONCEPT_CODE_SEQUENCE,
                    vec![code("SCT", "85756007", "Tissue")],
                )],
            ),
            region(),
            area(),
        ],
    )
}

/// A TID 1500 Measurement Report in an SR IOD of `sop_class`.
pub(super) fn measurement_report(sop_class: &str) -> InMemDicomObject {
    let mut object = document(
        sop_class,
        "SR",
        code("DCM", "126000", "Imaging Measurement Report"),
        "1500",
    );
    object.put(text(tags::COMPLETION_FLAG, VR::CS, "COMPLETE"));
    object.put(text(tags::VERIFICATION_FLAG, VR::CS, "UNVERIFIED"));
    object.put(sq(tags::PERFORMED_PROCEDURE_CODE_SEQUENCE, vec![]));
    let mut content_items = device_observer();
    content_items.push(content(
        "HAS CONCEPT MOD",
        "CODE",
        Some(code("DCM", "121058", "Procedure reported")),
        vec![sq(
            tags::CONCEPT_CODE_SEQUENCE,
            vec![code("DCM", "112703", "Whole Slide Imaging")],
        )],
    ));
    content_items.push(container(
        "CONTAINS",
        code("DCM", "126010", "Imaging Measurements"),
        vec![measurement_group()],
    ));
    object.put(sq(tags::CONTENT_SEQUENCE, content_items));
    object
}

/// A Basic Text SR of one finding, which declares no Template.
pub(super) fn text_report() -> InMemDicomObject {
    let mut object = measurement_report(BASIC_TEXT_SR);
    object.remove_element(tags::CONTENT_TEMPLATE_SEQUENCE);
    object.put(sq(
        tags::CONTENT_SEQUENCE,
        vec![
            content(
                "CONTAINS",
                "TEXT",
                Some(code("DCM", "121071", "Finding")),
                vec![text(tags::TEXT_VALUE, VR::UT, "Adequate tissue")],
            ),
            image("CONTAINS"),
        ],
    ));
    object
}

pub(super) fn file(object: InMemDicomObject) -> DefaultDicomObject {
    object
        .with_meta(FileMetaTableBuilder::new().transfer_syntax(EXPLICIT_VR_LITTLE_ENDIAN))
        .expect("file meta")
}

/// Applies `edit` to the Content Item at 0-based `path` of the root's
/// Content Sequences.
pub(super) fn edit_at(
    object: &mut InMemDicomObject,
    path: &[usize],
    edit: impl FnOnce(&mut InMemDicomObject),
) {
    let Some((first, rest)) = path.split_first() else {
        edit(object);
        return;
    };
    let mut element = object
        .take(tags::CONTENT_SEQUENCE)
        .expect("Content Sequence");
    edit_at(&mut element.items_mut().expect("Items")[*first], rest, edit);
    object.put(element);
}

/// Puts `element` into the Content Item at `path`.
pub(super) fn put_at(object: &mut InMemDicomObject, path: &[usize], element: InMemElement) {
    edit_at(object, path, |item| {
        item.put(element);
    });
}

/// Applies `edit` to the first Item of Sequence `tag` of the Content Item
/// at `path`.
pub(super) fn edit_first(
    object: &mut InMemDicomObject,
    path: &[usize],
    tag: Tag,
    edit: impl FnOnce(&mut InMemDicomObject),
) {
    edit_at(object, path, |item| {
        let mut sequence = item.take(tag).expect("Sequence");
        edit(&mut sequence.items_mut().expect("Items")[0]);
        item.put(sequence);
    });
}

/// Replaces the Content Sequence of the Item at `path`.
pub(super) fn set_children(
    object: &mut InMemDicomObject,
    path: &[usize],
    children: Vec<InMemDicomObject>,
) {
    put_at(object, path, sq(tags::CONTENT_SEQUENCE, children));
}

/// Appends `child` to the Content Sequence of the Item at `path`.
pub(super) fn push_child(object: &mut InMemDicomObject, path: &[usize], child: InMemDicomObject) {
    edit_at(object, path, |parent| {
        let mut children: Vec<InMemDicomObject> = parent
            .get(tags::CONTENT_SEQUENCE)
            .and_then(|element| element.items())
            .map(<[InMemDicomObject]>::to_vec)
            .unwrap_or_default();
        children.push(child);
        parent.put(sq(tags::CONTENT_SEQUENCE, children));
    });
}

/// The path of the Measurement Group in `measurement_report`.
pub(super) const GROUP: [usize; 2] = [3, 0];
/// The SCOORD in the Measurement Group.
pub(super) const REGION: [usize; 3] = [3, 0, 3];
/// The NUM in the Measurement Group.
pub(super) const AREA: [usize; 3] = [3, 0, 4];
