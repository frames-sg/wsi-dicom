//! The document-level prose of C.17: Completion and Verification, Verifying
//! Observer and Attestor, evidence against the Content Tree, the Studies of
//! a KOS, Identical and Predecessor Documents, and the Hierarchical SOP
//! Instance Reference Macro's Display URI and MAC Transfer Syntax.

use dicom_core::VR;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::fixtures::*;
use super::{assert_fails, assert_passes};

const OTHER_IMAGE: &str = "1.2.826.0.1.3680043.10.999.60.50";
const OTHER_SERIES: &str = "1.2.826.0.1.3680043.10.999.60.51";

fn withheld(value: &str) -> String {
    format!(
        "(value withheld: identifying; {} characters)",
        value.chars().count()
    )
}

fn report() -> InMemDicomObject {
    measurement_report(COMPREHENSIVE_SR)
}

#[test]
fn verified_documents_are_complete() {
    let mut object = report();
    object.put(text(tags::VERIFICATION_FLAG, VR::CS, "VERIFIED"));
    object.put(sq(
        tags::VERIFYING_OBSERVER_SEQUENCE,
        vec![item(vec![
            text(tags::VERIFYING_ORGANIZATION, VR::LO, "Pathology"),
            text(tags::VERIFICATION_DATE_TIME, VR::DT, "20261024120000"),
            text(tags::VERIFYING_OBSERVER_NAME, VR::PN, "Pathologist^Test"),
            sq(
                tags::VERIFYING_OBSERVER_IDENTIFICATION_CODE_SEQUENCE,
                vec![],
            ),
        ])],
    ));
    assert_passes(object.clone());
    object.put(text(tags::COMPLETION_FLAG, VR::CS, "PARTIAL"));
    assert_fails(
        object,
        "Verification Flag (0040,A493) is VERIFIED but Completion Flag (0040,A491) is PARTIAL; a Value of \"VERIFIED\" shall be used only when the Value of Completion Flag is \"COMPLETE\" (PS3.3 2026d Table C.17-2)",
    );
}

#[test]
fn a_verifying_observer_is_not_also_an_attestor() {
    let participant = |kind: &str| {
        item(vec![
            text(tags::PARTICIPATION_TYPE, VR::CS, kind),
            text(tags::OBSERVER_TYPE, VR::CS, "PSN"),
            text(tags::PERSON_NAME, VR::PN, "Pathologist^Test"),
            text(tags::PARTICIPATION_DATE_TIME, VR::DT, "20261024120000"),
        ])
    };
    let mut object = report();
    object.put(sq(
        tags::VERIFYING_OBSERVER_SEQUENCE,
        vec![item(vec![text(
            tags::VERIFYING_OBSERVER_NAME,
            VR::PN,
            "Pathologist^Test^^",
        )])],
    ));
    object.put(sq(tags::PARTICIPANT_SEQUENCE, vec![participant("ENT")]));
    assert_passes(object.clone());
    object.put(sq(tags::PARTICIPANT_SEQUENCE, vec![participant("ATTEST")]));
    assert_fails(
        object,
        "Participant Sequence (0040,A07A)[1], an Attestor, identifies the individual named (value withheld: identifying; 16 characters) who is also in Verifying Observer Sequence (0040,A073); an individual shall not be identified in both",
    );
}

#[test]
fn a_report_lists_every_instance_its_tree_references_as_evidence() {
    let mut object = report();
    object.remove_element(tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE);
    object.put(sq(
        tags::PERTINENT_OTHER_EVIDENCE_SEQUENCE,
        vec![evidence(STUDY, IMAGE_SERIES, WSI, IMAGE)],
    ));
    assert_passes(object);
    let mut object = report();
    object.remove_element(tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE);
    assert_fails(
        object,
        &format!("the Content Tree references SOP Instance {}, which neither Current Requested Procedure Evidence Sequence (0040,A375) nor Pertinent Other Evidence Sequence (0040,A385) lists; the Current Requested Procedure Evidence Sequence shall include all current evidence referenced in the Content Tree", withheld(IMAGE)),
    );
}

#[test]
fn an_instance_is_current_or_other_evidence_not_both() {
    let mut object = report();
    object.put(sq(
        tags::PERTINENT_OTHER_EVIDENCE_SEQUENCE,
        vec![evidence(STUDY, IMAGE_SERIES, WSI, IMAGE)],
    ));
    assert_fails(
        object,
        "is in both Current Requested Procedure Evidence Sequence (0040,A375) and Pertinent Other Evidence Sequence (0040,A385); the same SOP Instance shall not be referenced in both of these Sequences (PS3.3 2026d C.17.2.3)",
    );
}

#[test]
fn presentation_states_an_image_references_are_evidence_too() {
    let mut object = report();
    let state = reference("1.2.840.10008.5.1.4.1.1.11.1", OTHER_IMAGE);
    edit_first(
        &mut object,
        &[3, 0, 3, 0],
        tags::REFERENCED_SOP_SEQUENCE,
        |sop| {
            sop.put(sq(tags::REFERENCED_SOP_SEQUENCE, vec![state]));
        },
    );
    assert_fails(
        object,
        &format!(
            "the Content Tree references SOP Instance {}, which neither",
            withheld(OTHER_IMAGE)
        ),
    );
}

#[test]
fn a_kos_lists_exactly_the_instances_its_content_references() {
    let mut object = key_object_selection();
    object.put(sq(
        tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
        vec![
            evidence(STUDY, IMAGE_SERIES, WSI, IMAGE),
            evidence(STUDY, OTHER_SERIES, WSI, OTHER_IMAGE),
        ],
    ));
    assert_fails(
        object,
        &format!("Current Requested Procedure Evidence Sequence (0040,A375)[2] lists SOP Instance {}, which no Content Item references; no other instances than those referenced in the Content Sequence shall be included in this Sequence (PS3.3 2026d Table C.17.6-2)", withheld(OTHER_IMAGE)),
    );
    let mut object = key_object_selection();
    push_child(
        &mut object,
        &[],
        content(
            "CONTAINS",
            "IMAGE",
            None,
            vec![sq(
                tags::REFERENCED_SOP_SEQUENCE,
                vec![reference(WSI, OTHER_IMAGE)],
            )],
        ),
    );
    assert_fails(
        object,
        &format!("Content Sequence[3] references SOP Instance {}, which Current Requested Procedure Evidence Sequence (0040,A375) does not list", withheld(OTHER_IMAGE)),
    );
}

#[test]
fn the_tree_and_the_evidence_name_an_instance_by_one_sop_class() {
    let mut object = key_object_selection();
    object.put(sq(
        tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
        vec![evidence(
            STUDY,
            IMAGE_SERIES,
            "1.2.840.10008.5.1.4.1.1.66.4",
            IMAGE,
        )],
    ));
    assert_fails(
        object,
        &format!("the Content Tree references SOP Instance {} with Referenced SOP Class UID {WSI}, but Current Requested Procedure Evidence Sequence (0040,A375)[1] lists it with 1.2.840.10008.5.1.4.1.1.66.4", withheld(IMAGE)),
    );
}

/// A KOS referencing the slide image and one in `study`.
fn two_studies(study: &str) -> InMemDicomObject {
    let mut object = key_object_selection();
    push_child(
        &mut object,
        &[],
        content(
            "CONTAINS",
            "IMAGE",
            None,
            vec![sq(
                tags::REFERENCED_SOP_SEQUENCE,
                vec![reference(WSI, OTHER_IMAGE)],
            )],
        ),
    );
    object.put(sq(
        tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
        vec![
            evidence(STUDY, IMAGE_SERIES, WSI, IMAGE),
            evidence(study, OTHER_SERIES, WSI, OTHER_IMAGE),
        ],
    ));
    object
}

#[test]
fn a_kos_of_two_studies_is_duplicated_into_the_other() {
    let mut object = two_studies(OTHER_STUDY);
    object.put(sq(
        tags::IDENTICAL_DOCUMENTS_SEQUENCE,
        vec![evidence(
            STUDY,
            OTHER_SERIES,
            KOS,
            "1.2.826.0.1.3680043.10.999.60.52",
        )],
    ));
    assert_fails(
        object,
        &format!("references SOP Instances in the document's Study and in Studies {}, for which Identical Documents Sequence (0040,A525) references no duplicate; this document shall be duplicated into each of those other Studies, and the duplicates shall be referenced in the Identical Documents Sequence (PS3.3 2026d C.17.6.2.1)", withheld(OTHER_STUDY)),
    );
    let mut object = two_studies(OTHER_STUDY);
    object.put(sq(
        tags::IDENTICAL_DOCUMENTS_SEQUENCE,
        vec![evidence(
            OTHER_STUDY,
            OTHER_SERIES,
            KOS,
            "1.2.826.0.1.3680043.10.999.60.52",
        )],
    ));
    assert_passes(object);
}

#[test]
fn a_manifest_with_description_references_its_own_study() {
    let mut object = two_studies(OTHER_STUDY);
    object.put(sq(
        tags::CONCEPT_NAME_CODE_SEQUENCE,
        vec![code("DCM", "131560", "Manifest with Description")],
    ));
    object.put(sq(
        tags::IDENTICAL_DOCUMENTS_SEQUENCE,
        vec![evidence(
            OTHER_STUDY,
            OTHER_SERIES,
            KOS,
            "1.2.826.0.1.3680043.10.999.60.52",
        )],
    ));
    push_child(&mut object, &[], image_library(&[IMAGE]));
    assert_fails(
        object,
        &format!("the Manifest with Description references SOP Instance {} in Study {}, not the KOS's Study {}", withheld(OTHER_IMAGE), withheld(OTHER_STUDY), withheld(STUDY)),
    );
}

/// A TID 1600 Image Library with an entry for each image.
pub(super) fn image_library(images: &[&str]) -> InMemDicomObject {
    let entries = images
        .iter()
        .map(|uid| {
            content(
                "CONTAINS",
                "IMAGE",
                None,
                vec![sq(tags::REFERENCED_SOP_SEQUENCE, vec![reference(WSI, uid)])],
            )
        })
        .collect();
    container(
        "CONTAINS",
        code("DCM", "111028", "Image Library"),
        vec![container(
            "CONTAINS",
            code("DCM", "126200", "Image Library Group"),
            entries,
        )],
    )
}

#[test]
fn the_image_library_describes_only_the_referenced_instances() {
    let manifest = || {
        let mut object = key_object_selection();
        object.put(sq(
            tags::CONCEPT_NAME_CODE_SEQUENCE,
            vec![code("DCM", "131560", "Manifest with Description")],
        ));
        object
    };
    let mut object = manifest();
    push_child(&mut object, &[], image_library(&[IMAGE]));
    assert_passes(object);
    let mut object = manifest();
    push_child(&mut object, &[], image_library(&[IMAGE, OTHER_IMAGE]));
    object.put(sq(
        tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
        vec![
            evidence(STUDY, IMAGE_SERIES, WSI, IMAGE),
            evidence(STUDY, IMAGE_SERIES, WSI, OTHER_IMAGE),
        ],
    ));
    assert_fails(
        object,
        &format!("an Image Library entry, references SOP Instance {}, which no IMAGE, WAVEFORM or COMPOSITE Content Item of the root references; the Image Library shall not include entries for instances not referenced in Rows 8, 9 and 10", withheld(OTHER_IMAGE)),
    );
}

#[test]
fn predecessors_are_other_sr_documents() {
    let mut object = report();
    object.put(sq(
        tags::PREDECESSOR_DOCUMENTS_SEQUENCE,
        vec![evidence(STUDY, OTHER_SERIES, COMPREHENSIVE_SR, OTHER_IMAGE)],
    ));
    assert_passes(object.clone());
    object.put(sq(
        tags::PREDECESSOR_DOCUMENTS_SEQUENCE,
        vec![evidence(STUDY, OTHER_SERIES, WSI, DOCUMENT)],
    ));
    let check = assert_fails(
        object,
        &format!("Predecessor Documents Sequence (0040,A360) references this document itself, SOP Instance {}", withheld(DOCUMENT)),
    );
    assert!(
        check.message.contains(&format!(
            "of SOP Class {WSI}, which is not an SR SOP Class; it shall refer to SR SOP Instances"
        )),
        "{}",
        check.message
    );
}

#[test]
fn a_document_is_not_its_own_duplicate() {
    let mut object = key_object_selection();
    object.put(sq(
        tags::IDENTICAL_DOCUMENTS_SEQUENCE,
        vec![evidence(OTHER_STUDY, OTHER_SERIES, KOS, DOCUMENT)],
    ));
    assert_fails(
        object,
        &format!("Identical Documents Sequence (0040,A525) references this document itself, SOP Instance {}; it shall contain references to all other duplicate SOP Instances", withheld(DOCUMENT)),
    );
}

#[test]
fn a_display_uri_has_a_scheme_authority_path_and_query() {
    let with_uri = |uri: &str| {
        let mut object = report();
        let mut study = evidence(STUDY, IMAGE_SERIES, WSI, IMAGE);
        study.put(text(dicom_core::Tag(0x0040, 0xE012), VR::UR, uri));
        object.put(sq(
            tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
            vec![study],
        ));
        object
    };
    assert_passes(with_uri("https://viewer.example/studies/view?study=1.2.3"));
    for (uri, departure) in [
        ("viewer.example/studies", "has no scheme"),
        ("https:/studies?study=1", "has no authority"),
        ("https://viewer.example?study=1", "has no path"),
        ("https://viewer.example/studies", "has no query"),
        (
            "https://viewer.example/a b?study=1",
            "holds a character RFC 3986 does not allow unencoded",
        ),
    ] {
        assert_fails(
            with_uri(uri),
            // The Basic Profile removes Display URI (PS3.15 Table E.1-1,
            // X), so its Value is withheld and its length given.
            &format!("Current Requested Procedure Evidence Sequence (0040,A375)[1] > Display URI (0040,E012) (value withheld: identifying; {} characters) {departure}; it shall include the fully specified scheme, authority, path, and query components in accordance with RFC 3986 (PS3.3 2026d Table C.17-3)", uri.chars().count()),
        );
    }
}

#[test]
fn a_mac_is_computed_in_an_explicit_vr_little_endian_syntax() {
    let with_mac = |syntax: &str| {
        let mut object = report();
        let mut study = evidence(STUDY, IMAGE_SERIES, WSI, IMAGE);
        let mut series = study.take(tags::REFERENCED_SERIES_SEQUENCE).unwrap();
        let sop = &mut series.items_mut().unwrap()[0];
        let mut sops = sop.take(tags::REFERENCED_SOP_SEQUENCE).unwrap();
        sops.items_mut().unwrap()[0].put(sq(
            tags::REFERENCED_SOP_INSTANCE_MAC_SEQUENCE,
            vec![item(vec![
                text(tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID, VR::UI, syntax),
                text(tags::MAC_ALGORITHM, VR::CS, "SHA256"),
            ])],
        ));
        sop.put(sops);
        study.put(series);
        object.put(sq(
            tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE,
            vec![study],
        ));
        object
    };
    assert_passes(with_mac("1.2.840.10008.1.2.1"));
    assert_passes(with_mac("1.2.840.10008.1.2.4.201"));
    assert_fails(
        with_mac("1.2.840.10008.1.2"),
        "MAC Calculation Transfer Syntax UID (0400,0010) is 1.2.840.10008.1.2, Implicit VR Little Endian, which does not explicitly include the VR; when computing the MAC, only Transfer Syntaxes that explicitly include the VR and use Little Endian encoding shall be used (PS3.3 2026d Table C.17-3a)",
    );
    assert_fails(
        with_mac("1.2.840.10008.1.2.2"),
        "Explicit VR Big Endian, which does not use Little Endian encoding",
    );
    let private = super::check(with_mac("1.2.826.0.1.3680043.10.999.60.99"));
    assert!(
        private.message.contains(
            "whether MAC Calculation Transfer Syntax UID 1.2.826.0.1.3680043.10.999.60.99"
        ),
        "{}",
        private.message
    );
}
