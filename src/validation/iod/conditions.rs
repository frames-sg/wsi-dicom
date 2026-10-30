//! Hand-written predicates for every condition text in the generated tables.
//!
//! The match is exhaustive over `ConditionId`; regenerating the tables from an
//! edition with a new or reworded condition adds a Variant and fails to compile
//! until its predicate is written here. Each arm restates the condition it
//! implements. Facts the object cannot carry (scheduling, patient species,
//! retrieval context) evaluate to `Tri::Unknown`, which never fails a check.

use dicom_core::Tag;
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;

use super::scope::{
    absent, forbidden_if, has, has_value, integer_value, is_private, items, permitted_only_if,
    present, required_if, string_value, tag_values, value_in, Otherwise, Requirement, Scope, Tri,
};
use super::tables::ConditionId as C;
use crate::validation::transfer_syntaxes::is_jpip_referenced;
use crate::VL_WSI_SOP_CLASS_UID;

use Otherwise::{MayBePresent, NotPresent};

const LEGACY_CONVERTED: [&str; 3] = [
    uids::LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE,
    uids::LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE,
    uids::LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE,
];
const PATIENT_POSITION_SOP_CLASSES: [&str; 6] = [
    uids::CT_IMAGE_STORAGE,
    uids::MR_IMAGE_STORAGE,
    uids::ENHANCED_CT_IMAGE_STORAGE,
    uids::ENHANCED_MR_IMAGE_STORAGE,
    uids::ENHANCED_MR_COLOR_IMAGE_STORAGE,
    uids::MR_SPECTROSCOPY_STORAGE,
];
const ENHANCED_RT_IMAGE: [&str; 2] = [
    uids::ENHANCED_RT_IMAGE_STORAGE,
    uids::ENHANCED_CONTINUOUS_RT_IMAGE_STORAGE,
];

/// Evaluates `id` for the Data Set in `scope`.
pub(super) fn evaluate(id: C, scope: &Scope<'_>) -> Requirement {
    let item = scope.item;
    let root = scope.root;
    match id {
        // ---- Table 10-17 / 10-18 HL7v2 Hierarchic Designator Macros ----
        // "Required if Universal Entity ID (0040,0032) is present."
        C::UniversalEntityIDType_3c046f => {
            required_if(present(item, tags::UNIVERSAL_ENTITY_ID), NotPresent)
        }
        // "Required if Universal Entity ID (0040,0032) is not present; may be present otherwise."
        C::LocalNamespaceEntityID_3fcb73 => {
            required_if(absent(item, tags::UNIVERSAL_ENTITY_ID), MayBePresent)
        }
        // "Required if Local Namespace Entity ID (0040,0031) is not present; may be present otherwise."
        C::UniversalEntityID_a05e96 => {
            required_if(absent(item, tags::LOCAL_NAMESPACE_ENTITY_ID), MayBePresent)
        }

        // ---- Table 8.8-1a Basic Code Sequence Macro ----
        // "Shall be present if the length of the code value is 16 characters or less,
        // and the code value is not a URN or URL."
        C::CodeValue_c75d2f => required_if(code_value_fits_short_form(item), NotPresent),
        // "Shall be present if Code Value or Long Code Value is present. May be present otherwise."
        C::CodingSchemeDesignator_117155 => required_if(
            present(item, tags::CODE_VALUE).or(present(item, tags::LONG_CODE_VALUE)),
            MayBePresent,
        ),
        // "Required if the Value of Coding Scheme Designator is present and is not sufficient to
        // identify the Code Value or Long Code Value unambiguously. Shall not be present if
        // Coding Scheme Designator is absent. May be present otherwise." Sufficiency is not
        // encoded in the object.
        C::CodingSchemeVersion_74cac7 => {
            if !has(item, tags::CODING_SCHEME_DESIGNATOR) {
                Requirement::Forbidden
            } else if has(item, tags::CODING_SCHEME_VERSION) {
                Requirement::Optional
            } else {
                Requirement::Undeterminable
            }
        }
        // "Shall be present if Code Value is not present and the Code Value is not a URN or URL."
        C::LongCodeValue_318ae8 => required_if(
            absent(item, tags::CODE_VALUE).and(code_value_is_urn(item).not()),
            NotPresent,
        ),
        // "Shall be present if Code Value is not present and the Code Value is a URN or URL."
        C::URNCodeValue_3efa2e => required_if(
            absent(item, tags::CODE_VALUE).and(code_value_is_urn(item)),
            NotPresent,
        ),

        // ---- Table 8.8-1b Enhanced Encoding Mode Macro ----
        // "Required if Context Identifier (0008,010F) is present."
        C::MappingResource_00f87e => {
            required_if(present(item, tags::CONTEXT_IDENTIFIER), NotPresent)
        }
        // "Required if the Value of Context Group Extension Flag (0008,010B) is "Y"."
        C::ContextGroupLocalVersion_b3415f => required_if(
            value_in(item, tags::CONTEXT_GROUP_EXTENSION_FLAG, 1, &["Y"]),
            NotPresent,
        ),

        // ---- Table C.7-1 Patient Module ----
        // "Required if either Patient's Birth Date in Alternative Calendar or Patient's
        // Alternative Death Date in Calendar is present."
        C::PatientAlternativeCalendar_fae075 => required_if(
            (has(item, tags::PATIENT_BIRTH_DATE_IN_ALTERNATIVE_CALENDAR)
                || has(item, tags::PATIENT_DEATH_DATE_IN_ALTERNATIVE_CALENDAR))
            .into(),
            NotPresent,
        ),
        // "Required if the Patient is a non-human organism and if Patient Species Code Sequence
        // is not present. May be present otherwise."
        C::PatientSpeciesDescription_6364e6 => required_if(
            non_human(root).and(absent(item, tags::PATIENT_SPECIES_CODE_SEQUENCE)),
            MayBePresent,
        ),
        // "Required if the Patient is a non-human organism and if Patient Species Description
        // is not present. May be present otherwise."
        C::PatientSpeciesCodeSequence_3133a2 => required_if(
            non_human(root).and(absent(item, tags::PATIENT_SPECIES_DESCRIPTION)),
            MayBePresent,
        ),
        // "Required if the Patient is a non-human organism and if Patient Breed Code Sequence
        // is empty. May be present otherwise."
        C::PatientBreedDescription_2b9079 => required_if(
            non_human(root).and(
                items(item, tags::PATIENT_BREED_CODE_SEQUENCE)
                    .is_empty()
                    .into(),
            ),
            MayBePresent,
        ),
        // "Required if the Patient is a non-human organism." (Type 2C)
        C::PatientBreedCodeSequence_878016 => required_if(non_human(root), NotPresent),
        // "Required if the Patient is a non-human organism. May be present otherwise."
        C::ResponsiblePerson_26bd73 => required_if(non_human(root), MayBePresent),
        // "Required if Responsible Person is present and has a Value."
        C::ResponsiblePersonRole_45e655 => {
            required_if(has_value(item, tags::RESPONSIBLE_PERSON).into(), NotPresent)
        }
        // "Required if Patient is a non-human organism. May be present otherwise."
        C::ResponsibleOrganization_a81ee5 => required_if(non_human(root), MayBePresent),
        // "Required if Patient Identity Removed is present and has a Value of YES and
        // De-identification Method Code Sequence is not present. May be present otherwise."
        C::DeidentificationMethod_1b21e1 => required_if(
            value_in(item, tags::PATIENT_IDENTITY_REMOVED, 1, &["YES"])
                .and(absent(item, tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE)),
            MayBePresent,
        ),
        // Mirror of the above for De-identification Method (0012,0063).
        C::DeidentificationMethodCodeSequence_1e6093 => required_if(
            value_in(item, tags::PATIENT_IDENTITY_REMOVED, 1, &["YES"])
                .and(absent(item, tags::DEIDENTIFICATION_METHOD)),
            MayBePresent,
        ),

        // ---- Table 10-3b Referenced Instances and Access Macro ----
        // "Required if Type of Instances is DICOM and the Information Model of the referenced
        // Instance contains the Study IE." The referenced Information Model is not encoded.
        C::StudyInstanceUID_56e908 | C::SeriesInstanceUID_117bc3 => required_if(
            value_in(item, tags::TYPE_OF_INSTANCES, 1, &["DICOM"]).and(Tri::Unknown),
            NotPresent,
        ),
        // ">HL7 Instance Identifier: Required if Type of Instances (0040,E020) is CDA." Type of
        // Instances is in the enclosing Item of the Referenced SOP Sequence.
        C::HL7InstanceIdentifier_194507 => required_if(
            scope.parent_item().map_or(Tri::False, |parent| {
                value_in(parent, tags::TYPE_OF_INSTANCES, 1, &["CDA"])
            }),
            NotPresent,
        ),
        // "Required if the Referenced SOP Instance is a Multi-frame Image and the reference does
        // not apply to all Frames, and Referenced Segment Number is not present."
        C::ReferencedFrameNumber_1e1f84 => required_if(
            Tri::Unknown.and(absent(item, tags::REFERENCED_SEGMENT_NUMBER)),
            NotPresent,
        ),
        // "Required if the Referenced SOP Instance is a Segmentation [or Surface Segmentation]
        // and the reference does not apply to all segments and Referenced Frame Number is not
        // present."
        C::ReferencedSegmentNumber_19fe72 | C::ReferencedSegmentNumber_f2b12f => required_if(
            Tri::Unknown.and(absent(item, tags::REFERENCED_FRAME_NUMBER)),
            NotPresent,
        ),
        // Each retrieval Sequence: "Required if <the other four> are not present. May be present
        // otherwise."
        C::DICOMRetrievalSequence_da69fd => retrieval(item, tags::DICOM_RETRIEVAL_SEQUENCE),
        C::DICOMMediaRetrievalSequence_be1086 => {
            retrieval(item, tags::DICOM_MEDIA_RETRIEVAL_SEQUENCE)
        }
        C::WADORetrievalSequence_61a093 => retrieval(item, tags::WADO_RETRIEVAL_SEQUENCE),
        C::XDSRetrievalSequence_82c637 => retrieval(item, tags::XDS_RETRIEVAL_SEQUENCE),
        C::WADORSRetrievalSequence_be3376 => retrieval(item, tags::WADORS_RETRIEVAL_SEQUENCE),

        // ---- Table C.7-2b Clinical Trial Subject Module ----
        // "Shall be present if Clinical Trial Subject Reading ID is absent. May be present otherwise."
        C::ClinicalTrialSubjectID_4bdaa7 => required_if(
            absent(item, tags::CLINICAL_TRIAL_SUBJECT_READING_ID),
            MayBePresent,
        ),
        // "Shall be present if Clinical Trial Subject ID is absent. May be present otherwise."
        C::ClinicalTrialSubjectReadingID_e427c4 => {
            required_if(absent(item, tags::CLINICAL_TRIAL_SUBJECT_ID), MayBePresent)
        }
        // "Required if Clinical Trial Protocol Ethics Committee Approval Number is present."
        C::ClinicalTrialProtocolEthicsCommitteeName_b2d50b => required_if(
            has(
                item,
                tags::CLINICAL_TRIAL_PROTOCOL_ETHICS_COMMITTEE_APPROVAL_NUMBER,
            )
            .into(),
            NotPresent,
        ),

        // ---- Table 10-1 Person Identification Macro ----
        // "Required if Institution Code Sequence is not present. May be present otherwise."
        C::InstitutionName_211fb8 => {
            required_if(absent(item, tags::INSTITUTION_CODE_SEQUENCE), MayBePresent)
        }
        // "Required if Institution Name is not present. May be present otherwise."
        C::InstitutionCodeSequence_605819 => {
            required_if(absent(item, tags::INSTITUTION_NAME), MayBePresent)
        }

        // ---- Table C.7-4a Patient Study Module ----
        // "Required if Sex Parameters for Clinical Use Category Code Sequence is
        // (131232, DCM, "Specified"). May be present otherwise."
        C::SexParametersForClinicalUseCategoryComment_c0a125 => required_if(
            code_is(
                item,
                tags::SEX_PARAMETERS_FOR_CLINICAL_USE_CATEGORY_CODE_SEQUENCE,
                "131232",
                "DCM",
            ),
            MayBePresent,
        ),

        // ---- Table C.7-4b Clinical Trial Study Module ----
        // "Required if Longitudinal Temporal Offset from Event is present."
        C::LongitudinalTemporalEventType_30a811 => required_if(
            present(item, tags::LONGITUDINAL_TEMPORAL_OFFSET_FROM_EVENT),
            NotPresent,
        ),
        // "Required if Consent for Distribution Flag equals YES or WITHDRAWN."
        C::DistributionType_606cf8 => required_if(
            value_in(
                item,
                tags::CONSENT_FOR_DISTRIBUTION_FLAG,
                1,
                &["YES", "WITHDRAWN"],
            ),
            NotPresent,
        ),
        // "Required if Distribution Type is NAMED_PROTOCOL and the protocol is not that which is
        // specified in Clinical Trial Protocol ID in the Clinical Trial Subject Module." Which
        // protocol was consented to is not encoded.
        C::ClinicalTrialProtocolID_3ce4a5 => required_if(
            value_in(item, tags::DISTRIBUTION_TYPE, 1, &["NAMED_PROTOCOL"]).and(Tri::Unknown),
            NotPresent,
        ),

        // ---- Table C.7-5a General Series Module ----
        // "Required if the body part examined is a paired structure and Image Laterality or
        // Frame Laterality or Measurement Laterality are not present."
        C::Laterality_0273c7 => required_if(
            Tri::Unknown.and(Tri::from(!any_laterality(scope))),
            NotPresent,
        ),
        // "Required for images where Patient Orientation Code Sequence is not present and whose
        // SOP Class UID is one of <CT, MR, Enhanced CT/MR/Color MR, MR Spectroscopy>. May be
        // present for other SOP Classes if Patient Orientation Code Sequence is not present."
        C::PatientPosition_3d8fad => {
            let orientation_absent = !has(root, tags::PATIENT_ORIENTATION_CODE_SEQUENCE);
            let listed = scope
                .sop_class()
                .is_some_and(|uid| PATIENT_POSITION_SOP_CLASSES.contains(&uid.as_str()));
            match (listed, orientation_absent) {
                (true, true) => Requirement::Required,
                (false, true) => Requirement::Optional,
                (_, false) => Requirement::Forbidden,
            }
        }
        // "Required if the Patient is a non-human organism and the anatomical Frame of Reference
        // is not bipedal. May be present otherwise."
        C::AnatomicalOrientationType_a42424 => required_if(non_human(root), MayBePresent),

        // ---- Table 10-9 Request Attributes Macro ----
        // "Required if procedure was scheduled. May be present otherwise."
        C::RequestedProcedureID_08c518 => required_if(Tri::Unknown, MayBePresent),
        // "Required if procedure was scheduled."
        C::ScheduledProcedureStepID_d945b5 => required_if(Tri::Unknown, NotPresent),

        // ---- Table 10-2 Content Item Macro: "Required if Value Type (0040,A040) is X." ----
        C::DateTime_e5e09c => value_type(item, &["DATETIME"]),
        C::Date_f120f4 => value_type(item, &["DATE"]),
        C::Time_ad089b => value_type(item, &["TIME"]),
        C::PersonName_a1fae7 => value_type(item, &["PNAME"]),
        C::UID_ca8b4b => value_type(item, &["UIDREF"]),
        C::TextValue_6d4c4e => value_type(item, &["TEXT"]),
        C::ConceptCodeSequence_52aed2 => value_type(item, &["CODE"]),
        C::NumericValue_8073ae => value_type(item, &["NUMERIC"]),
        C::ReferencedSOPSequence_fabd0b => value_type(item, &["COMPOSITE", "IMAGE", "WAVEFORM"]),
        // "Required if Numeric Value has insufficient precision to represent the value [a
        // rational value] as a string. May be present otherwise."
        C::FloatingPointValue_a84f01 | C::RationalNumeratorValue_5600ef => {
            required_if(Tri::Unknown, MayBePresent)
        }
        // "Required if Rational Numerator Value is present."
        C::RationalDenominatorValue_553a6d => {
            required_if(present(item, tags::RATIONAL_NUMERATOR_VALUE), NotPresent)
        }
        // "Required if the Referenced SOP Instance is a Waveform that contains multiple Channels
        // and the reference does not apply to all Channels of all Multiplex Groups."
        C::ReferencedWaveformChannels_6a695a => required_if(Tri::Unknown, NotPresent),

        // ---- Table C.8.12.3-1 Whole Slide Microscopy Series Module ----
        // "Required if a Performed Procedure Step SOP Class was involved in the creation of this
        // Series."
        C::ReferencedPerformedProcedureStepSequence_d5d6e5 => required_if(Tri::Unknown, NotPresent),

        // ---- Table A.32.8-1 Module usage ----
        // "C - Required if Image Type Value 3 is VOLUME or THUMBNAIL. May be present otherwise."
        C::ModuleFrameOfReference_10a326 => required_if(
            scope.image_type_is(3, &["VOLUME", "THUMBNAIL"]),
            MayBePresent,
        ),
        // "U - Shall be present only if Image Type Value 3 is VOLUME or THUMBNAIL."
        C::ModuleMultiResolutionPyramid_c66402 => {
            permitted_only_if(scope.image_type_is(3, &["VOLUME", "THUMBNAIL"]))
        }
        // "C - Required if Image Type (0008,0008) Value 3 is LABEL; may be present otherwise"
        C::ModuleSlideLabel_82fe14 => required_if(scope.image_type_is(3, &["LABEL"]), MayBePresent),
        // "C - Required if the SOP Instance was created in response to a Frame-Level retrieve
        // request"
        C::ModuleFrameExtraction_221a2c => required_if(Tri::Unknown, NotPresent),

        // ---- Table C.7-8 General Equipment Module ----
        // "Required if Pixel Padding Range Limit is present and either Pixel Data or Pixel Data
        // Provider URL is present. May be present otherwise only if Pixel Data or Pixel Data
        // Provider URL is present."
        C::PixelPaddingValue_894c69 => {
            let pixels = has(root, tags::PIXEL_DATA) || has(root, tags::PIXEL_DATA_PROVIDER_URL);
            if pixels && has(root, tags::PIXEL_PADDING_RANGE_LIMIT) {
                Requirement::Required
            } else if pixels {
                Requirement::Optional
            } else {
                Requirement::Forbidden
            }
        }

        // ---- Table C.7-9 General Image Module ----
        // "Required if image does not require Image Orientation (Patient) and Image Position
        // (Patient) or if image does not require Image Orientation (Slide). May be present
        // otherwise." Read as two independent alternatives, every image without Image
        // Orientation (Slide) (CT, MR, ...) would need Patient Orientation, which the Type 2C
        // design excludes; the text names the two orientation mechanisms either of which makes
        // Patient Orientation unnecessary. The pathology-slide IODs carry Image Orientation
        // (Patient) only in Plane Orientation (Patient) Functional Groups.
        C::PatientOrientation_964c1d => required_if(
            image_orientation_slide_required(scope)
                .not()
                .and(Tri::from(!patient_oriented(scope))),
            MayBePresent,
        ),
        // "Required if image is part of a Series in which the images are temporally related.
        // May be present otherwise."
        C::ContentDate_b7911d => required_if(Tri::Unknown, MayBePresent),

        // ---- Table C.7-11c Image Pixel Description Macro (also in Icon Image Items) ----
        // "Required if Samples per Pixel has a Value greater than 1."
        C::PlanarConfiguration_097db3 | C::PlanarConfiguration_290ffd => required_if(
            integer_value(item, tags::SAMPLES_PER_PIXEL)
                .is_some_and(|samples| samples > 1)
                .into(),
            NotPresent,
        ),
        // "Required if the aspect ratio values do not have a ratio of 1:1 and the physical pixel
        // spacing is not specified by Pixel Spacing, Imager Pixel Spacing or Nominal Scanned
        // Pixel Spacing, either for the entire Image or per-frame in a Functional Group Macro."
        C::PixelAspectRatio_6287c0 => required_if(
            Tri::Unknown.and(Tri::from(!pixel_spacing_specified(scope))),
            NotPresent,
        ),
        // "Required if Photometric Interpretation has a Value of PALETTE COLOR or Pixel
        // Presentation at the image level equals COLOR or MIXED."
        C::RedPaletteColorLookupTableDescriptor_cd3c5c
        | C::RedPaletteColorLookupTableData_04117f => required_if(
            value_in(
                item,
                tags::PHOTOMETRIC_INTERPRETATION,
                1,
                &["PALETTE COLOR"],
            )
            .or(value_in(
                root,
                tags::PIXEL_PRESENTATION,
                1,
                &["COLOR", "MIXED"],
            )),
            NotPresent,
        ),
        // Type 3: "Shall not be present in the top level dataset when the Optical Path Sequence
        // (0048,0105) is present."
        C::ICCProfile_e05055 => {
            forbidden_if((scope.is_top_level() && has(root, tags::OPTICAL_PATH_SEQUENCE)).into())
        }
        // Type 3: "Shall not be present when the Optical Path Sequence (0048,0105) is present."
        C::ColorSpace_709202 => forbidden_if(present(root, tags::OPTICAL_PATH_SEQUENCE)),

        // ---- Table C.7.6.16-12b Real World Value Mapping Item Macro ----
        // "Required if Pixel Data or Real World Value LUT Data is present or Double Float Real
        // World Value First [Last] Value Mapped is absent."
        C::RealWorldValueFirstValueMapped_8ca8ef => real_world_first_last(
            scope,
            tags::DOUBLE_FLOAT_REAL_WORLD_VALUE_FIRST_VALUE_MAPPED,
        ),
        C::RealWorldValueLastValueMapped_2d3315 => {
            real_world_first_last(scope, tags::DOUBLE_FLOAT_REAL_WORLD_VALUE_LAST_VALUE_MAPPED)
        }
        // "Required if Real World Value First [Last] Value Mapped is absent."
        C::DoubleFloatRealWorldValueFirstValueMapped_8dc058 => required_if(
            absent(item, tags::REAL_WORLD_VALUE_FIRST_VALUE_MAPPED),
            NotPresent,
        ),
        C::DoubleFloatRealWorldValueLastValueMapped_b10528 => required_if(
            absent(item, tags::REAL_WORLD_VALUE_LAST_VALUE_MAPPED),
            NotPresent,
        ),
        // "Required if Float Pixel Data or Double Float Pixel Data are present or Real World
        // Value LUT Data is not present."
        C::RealWorldValueIntercept_10f07f => required_if(
            (has(root, tags::FLOAT_PIXEL_DATA)
                || has(root, tags::DOUBLE_FLOAT_PIXEL_DATA)
                || !has(item, tags::REAL_WORLD_VALUE_LUT_DATA))
            .into(),
            NotPresent,
        ),
        // "Required if Real World Value Intercept is not present."
        C::RealWorldValueLUTData_797724 => {
            required_if(absent(item, tags::REAL_WORLD_VALUE_INTERCEPT), NotPresent)
        }

        // ---- Source Image Items (Table C.12-10, C.7.6.16-7) ----
        // "Required if the Value of Spatial Locations Preserved is REORIENTED_ONLY."
        C::PatientOrientation_0a4c6c => required_if(
            value_in(
                item,
                tags::SPATIAL_LOCATIONS_PRESERVED,
                1,
                &["REORIENTED_ONLY"],
            ),
            NotPresent,
        ),

        // ---- Table C.8.12.14-1 Microscope Slide Layer Tile Organization Module ----
        // "Required if Dimension Organization Type is present with a Value of TILED_FULL. May be
        // present otherwise." (Also Number of Optical Paths in Table C.8.12.5-1.)
        C::TotalPixelMatrixFocalPlanes_d443dd => required_if(
            scope.dimension_organization_type_is("TILED_FULL"),
            MayBePresent,
        ),
        // "Required if the Z offset is not zero. May be present otherwise."
        C::ZOffsetInSlideCoordinateSystem_efd490 => required_if(Tri::Unknown, MayBePresent),
        // "Required if Plane Position (Slide) Sequence is present within a Functional Group
        // Sequence or Dimension Organization Type is present with a Value of TILED_FULL. May be
        // present otherwise."
        C::ImageOrientationSlide_d008d7 => {
            required_if(image_orientation_slide_required(scope), MayBePresent)
        }

        // ---- Table C.7-11a Image Pixel Module ----
        // "Required if Pixel Data Provider URL is not present."
        C::PixelData_f3a5dd => required_if(absent(item, tags::PIXEL_DATA_PROVIDER_URL), NotPresent),
        // "Required if the image is to be transferred in one of the following presentation
        // contexts identified by Transfer Syntax UID: JPIP Referenced, JPIP Referenced Deflate".
        // A stored file is encoded in the Transfer Syntax of its File Meta Information. The list
        // predates the JPIP HTJ2K Referenced syntaxes, which PS3.5 A.11 and A.12 encode the same
        // way (Pixel Data replaced by this URL); PS3.5 defines the encoding, so they are included.
        C::PixelDataProviderURL_83d79f => {
            required_if(is_jpip_referenced(scope.transfer_syntax).into(), NotPresent)
        }
        // "Required if pixel padding is to be defined as a range rather than a single Value."
        C::PixelPaddingRangeLimit_0bc79e => required_if(Tri::Unknown, NotPresent),
        // "Required if Extended Offset Table (7FE0,0001) is present."
        C::ExtendedOffsetTableLengths_cde7d0 => {
            required_if(present(item, tags::EXTENDED_OFFSET_TABLE), NotPresent)
        }

        // ---- Table C.7.6.16-1 Multi-frame Functional Groups Module ----
        // "Required if for any Frame, there are Per-Frame Functional Groups that are not empty."
        // C.7.6.16.1.1 permits omitting the Sequence when every Item would be empty and
        // otherwise encoding an empty Item per Frame, so presence is allowed either way.
        C::PerFrameFunctionalGroupsSequence_2da3d8 => {
            let any_content = items(root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
                .iter()
                .any(|frame| frame.iter().next().is_some());
            required_if(any_content.into(), MayBePresent)
        }
        // "Required if Concatenation UID is present." (Also SOP Instance UID of Concatenation
        // Source and In-concatenation Number.)
        C::ConcatenationFrameOffsetNumber_e63090 => {
            required_if(present(root, tags::CONCATENATION_UID), NotPresent)
        }
        // "Required if a group of Multi-frame Image SOP Instances within a Series are part of a
        // Concatenation." Concatenation membership attributes evidence it.
        C::ConcatenationUID_3c3e36 => {
            let member = has(root, tags::IN_CONCATENATION_NUMBER)
                || has(root, tags::CONCATENATION_FRAME_OFFSET_NUMBER)
                || has(root, tags::SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE);
            required_if(if member { Tri::True } else { Tri::Unknown }, NotPresent)
        }

        // ---- Table C.7.6.17-1 Multi-frame Dimension Module ----
        // "Required if Dimension Organization Type is absent or not TILED_FULL. May be present
        // otherwise."
        C::DimensionIndexSequence_aaaa80 => required_if(
            scope.dimension_organization_type_is("TILED_FULL").not(),
            MayBePresent,
        ),
        // "Required if the Dimension Index Pointer Value is the Data Element Tag of a Private
        // Attribute."
        C::DimensionIndexPrivateCreator_252bb2 => {
            private_creator_required(item, tags::DIMENSION_INDEX_POINTER)
        }
        // "Required if the Value of Dimension Index Pointer is the Data Element Tag of an
        // Attribute that is contained within a Functional Group Sequence."
        C::FunctionalGroupPointer_59f16f => {
            required_if(pointer_in_functional_group(scope, item), NotPresent)
        }
        // "Required if the Functional Group Pointer Value is the Data Element Tag of a Private
        // Attribute."
        C::FunctionalGroupPrivateCreator_d93a4b => {
            private_creator_required(item, tags::FUNCTIONAL_GROUP_POINTER)
        }

        // ---- Table C.7.6.22-2 Specimen Description Items ----
        // "Required if multiple specimens present in the image. May be present otherwise." The
        // Sequence may also describe specimens that are not imaged, so several Items do not
        // establish several imaged specimens.
        C::SpecimenLocalizationContentItemSequence_4cd40e => {
            let described = scope.parent_item().map_or(0, |parent| {
                items(parent, tags::SPECIMEN_DESCRIPTION_SEQUENCE).len()
            });
            let condition = if described <= 1 {
                Tri::False
            } else {
                Tri::Unknown
            };
            required_if(condition, MayBePresent)
        }

        // ---- Table C.8.12.4-1 Whole Slide Microscopy Image Module ----
        // "Required if Image Type Value 3 is VOLUME. May be present otherwise." (Width, Height,
        // Depth)
        C::ImagedVolumeWidth_96eff9 => {
            required_if(scope.image_type_is(3, &["VOLUME"]), MayBePresent)
        }
        // "Enumerated Values for Samples per Pixel when Photometric Interpretation is
        // [not] MONOCHROME2"
        C::SamplesPerPixel_d24356 => applies(scope.photometric_is("MONOCHROME2")),
        C::SamplesPerPixel_0c16cf => applies(scope.photometric_is("MONOCHROME2").not()),
        // "Enumerated Values if Image Type Value 3 is THUMBNAIL, LABEL or OVERVIEW"
        C::NumberOfFrames_8113e0 => {
            applies(scope.image_type_is(3, &["THUMBNAIL", "LABEL", "OVERVIEW"]))
        }
        // "Required if Lossy Image Compression is "01"." (Ratio and Method)
        C::LossyImageCompressionRatio_84e963 => required_if(
            value_in(root, tags::LOSSY_IMAGE_COMPRESSION, 1, &["01"]),
            NotPresent,
        ),
        // "Required if Photometric Interpretation is MONOCHROME2." (Presentation LUT Shape,
        // Rescale Intercept, Rescale Slope)
        C::PresentationLUTShape_dde08b => {
            required_if(scope.photometric_is("MONOCHROME2"), NotPresent)
        }
        // "Required if Extended Depth of Field Value is YES"
        C::NumberOfFocalPlanes_2aca60 | C::DistanceBetweenFocalPlanes_70deaa => required_if(
            value_in(root, tags::EXTENDED_DEPTH_OF_FIELD, 1, &["YES"]),
            NotPresent,
        ),

        // ---- Table C.8.12.5-1 Optical Path Module ----
        // "Required if Illumination Color Code Sequence is not present. May be present otherwise."
        C::IlluminationWaveLength_66d7d2 => required_if(
            absent(item, tags::ILLUMINATION_COLOR_CODE_SEQUENCE),
            MayBePresent,
        ),
        // "Required if Illumination Wave Length is not present. May be present otherwise."
        C::IlluminationColorCodeSequence_e613f9 => {
            required_if(absent(item, tags::ILLUMINATION_WAVE_LENGTH), MayBePresent)
        }
        // "Required if this differs from the natural interpretation."
        C::ChannelDescriptionCodeSequence_5684e2 => required_if(Tri::Unknown, NotPresent),
        // "Required if Photometric Interpretation is not MONOCHROME2, or if Palette Color Lookup
        // Table Sequence is present."
        C::ICCProfile_3b45af => required_if(
            scope.photometric_is("MONOCHROME2").not().or(has(
                item,
                tags::PALETTE_COLOR_LOOKUP_TABLE_SEQUENCE,
            )
            .into()),
            NotPresent,
        ),

        // ---- Table C.7-22a Palette Color Lookup Table Module ----
        // "Required if segmented data is NOT used in an Image IOD or Section A.58, or if the IOD
        // is a Presentation State IOD or Segmentation IOD."
        C::RedPaletteColorLookupTableData_219a0b => {
            required_if((!segmented_palette(item)).into(), NotPresent)
        }
        // "Required if segmented data is used in an Image IOD or Section A.58; shall not be
        // present in a Presentation State IOD or Segmentation IOD."
        C::SegmentedRedPaletteColorLookupTableData_2d20ab => {
            required_if(segmented_palette(item).into(), NotPresent)
        }

        // ---- Table C.12-1 SOP Common Module ----
        // "Required if an expanded or replacement character set is used."
        C::SpecificCharacterSet_2f5b10 => {
            if has(item, tags::SPECIFIC_CHARACTER_SET) {
                Requirement::Undeterminable
            } else {
                required_if(non_default_characters(root).into(), NotPresent)
            }
        }
        // "Required if Coding Scheme is registered."
        C::CodingSchemeRegistry_fc29b3 => required_if(Tri::Unknown, NotPresent),
        // "Required if Coding Scheme is identified by an ISO 8824 object identifier compatible
        // with the UI VR."
        C::CodingSchemeUID_22aeb9 => required_if(Tri::Unknown, NotPresent),
        // "Required if Coding Scheme is registered and Coding Scheme UID is not present." A
        // Coding Scheme Registry Value evidences registration.
        C::CodingSchemeExternalID_1973e8 => {
            let registered = if has(item, tags::CODING_SCHEME_REGISTRY) {
                Tri::True
            } else {
                Tri::Unknown
            };
            required_if(
                registered.and(absent(item, tags::CODING_SCHEME_UID)),
                NotPresent,
            )
        }
        // "Required if Certified Timestamp (0400,0310) is present."
        C::CertifiedTimestampType_eabbec => {
            required_if(present(item, tags::CERTIFIED_TIMESTAMP), NotPresent)
        }
        // "Required if application level confidentiality is needed and certain recipients are
        // allowed to decrypt all or portions of the Encrypted Attributes Data Set."
        C::EncryptedAttributesSequence_20eb1a => required_if(Tri::Unknown, NotPresent),
        // "Required if unencapsulated HL7 Structured Documents are referenced within the Instance."
        C::HL7StructuredDocumentReferenceSequence_97a169 => required_if(Tri::Unknown, NotPresent),
        // "Required if the Instance has ever been converted from its source form as the result of
        // a C-MOVE operation with a specific view."
        C::QueryRetrieveView_01d963 => required_if(Tri::Unknown, NotPresent),
        // "Required if this Instance was created by conversion from a DICOM source, and
        // Conversion Source Attributes Sequence is not present in an Item of Shared Functional
        // Groups Sequence or Per-Frame Functional Groups Sequence."
        C::ConversionSourceAttributesSequence_78685d => {
            let in_groups = scope
                .all_functional_group_items()
                .any(|group| has(group, tags::CONVERSION_SOURCE_ATTRIBUTES_SEQUENCE));
            required_if(Tri::Unknown.and((!in_groups).into()), NotPresent)
        }
        // "Required if the Value of Private Data Element Value Representation is SQ."
        C::PrivateDataElementNumberOfItems_54e1ee => required_if(
            value_in(
                item,
                tags::PRIVATE_DATA_ELEMENT_VALUE_REPRESENTATION,
                1,
                &["SQ"],
            ),
            NotPresent,
        ),
        // "Required if Block Identifying Information Status equals MIXED."
        C::NonidentifyingPrivateElements_0bc3db => required_if(
            value_in(
                item,
                tags::BLOCK_IDENTIFYING_INFORMATION_STATUS,
                1,
                &["MIXED"],
            ),
            NotPresent,
        ),

        // ---- Table 10-20 Attribute Selector Macro ----
        // "Required if the selected content is not a Sequence Item."
        C::SelectorAttribute_8b10ca => required_if(Tri::Unknown, NotPresent),
        // "Required if the selected content is a single Attribute of any VR other than SQ."
        C::SelectorValueNumber_81714a => required_if(Tri::Unknown, NotPresent),
        // "Required if Selector Attribute is nested in one or more Sequences or is absent."
        C::SelectorSequencePointer_452bba => {
            let condition = if has(item, tags::SELECTOR_ATTRIBUTE) {
                Tri::Unknown
            } else {
                Tri::True
            };
            required_if(condition, NotPresent)
        }
        // "Required if Selector Sequence Pointer is present and one or more of its Values is the
        // Data Element Tag of a Private Attribute."
        C::SelectorSequencePointerPrivateCreator_4cb02f => {
            private_creator_required(item, tags::SELECTOR_SEQUENCE_POINTER)
        }
        // "Required if Selector Sequence Pointer is present."
        C::SelectorSequencePointerItems_60e4ce => {
            required_if(present(item, tags::SELECTOR_SEQUENCE_POINTER), NotPresent)
        }
        // "Required if the Selector Attribute Value is the Data Element Tag of a Private
        // Attribute."
        C::SelectorAttributePrivateCreator_83625b => {
            private_creator_required(item, tags::SELECTOR_ATTRIBUTE)
        }

        // ---- Table 10.41-1 Protocol Context Macro ----
        // "Required if this instance is a Performed Procedure Protocol that resulted from a
        // Defined Procedure Protocol. May be present otherwise."
        C::ReferencedDefinedProtocolSequence_f99e76 => required_if(Tri::Unknown, MayBePresent),
        // Type 3: "Shall not be present if Source Reconstruction Protocol Element Number is
        // present." and the converse.
        C::SourceAcquisitionProtocolElementNumber_90f9d5 => forbidden_if(present(
            item,
            tags::SOURCE_RECONSTRUCTION_PROTOCOL_ELEMENT_NUMBER,
        )),
        C::SourceReconstructionProtocolElementNumber_eeffde => forbidden_if(present(
            item,
            tags::SOURCE_ACQUISITION_PROTOCOL_ELEMENT_NUMBER,
        )),
        // "Required if a related Performed Procedure Protocol SOP Instance was created."
        C::ReferencedPerformedProtocolSequence_e963cd => required_if(Tri::Unknown, NotPresent),

        // ---- Table C.12-8 Common Instance Reference Module ----
        // "Required if this Instance references Instances in this [other] Study." Without any
        // SOP Instance reference outside this Module the Instance references none. A reference
        // must be listed in one of the two Sequences, so with neither present the same-Study one
        // is missing; otherwise which Study each reference is in is undeterminable.
        C::ReferencedSeriesSequence_78cb90
        | C::StudiesContainingOtherReferencedInstancesSequence_4a06a8 => {
            let listed = has(root, tags::REFERENCED_SERIES_SEQUENCE)
                || has(
                    root,
                    tags::STUDIES_CONTAINING_OTHER_REFERENCED_INSTANCES_SEQUENCE,
                );
            let condition = match (references_instances(root), listed) {
                (false, _) => Tri::False,
                (true, false) if id == C::ReferencedSeriesSequence_78cb90 => Tri::True,
                (true, _) => Tri::Unknown,
            };
            required_if(condition, NotPresent)
        }

        // ---- Table C.12-9 Frame Extraction Module ----
        // "Required if object extraction is based on a Frame Level Retrieve using <Simple Frame
        // List | Calculated Frame List | Time Range>."
        C::SimpleFrameList_116fb1 | C::CalculatedFrameList_0f7f87 | C::TimeRange_8f1b63 => {
            required_if(Tri::Unknown, NotPresent)
        }

        // ---- Table C.7.6.16-2 Pixel Measures Macro ----
        // "Required if: Volumetric Properties is other than DISTORTED or SAMPLED, and Image Type
        // Value 3 is not LABEL or OVERVIEW, or <Segmentation with Frame of Reference UID>, or
        // <Ophthalmic Tomography with volumetric flag YES>, or <OCT B-scan Volume Analysis>, or
        // <Enhanced RT Image>, or <Enhanced Continuous RT Image>. May be present otherwise."
        C::PixelSpacing_a4aaf2 => {
            let volumetric = value_in(
                root,
                tags::VOLUMETRIC_PROPERTIES,
                1,
                &["DISTORTED", "SAMPLED"],
            )
            .not()
            .and(scope.image_type_is(3, &["LABEL", "OVERVIEW"]).not());
            required_if(
                volumetric
                    .or(other_sop_class_spacing(scope))
                    .or(sop_class_in(scope, &ENHANCED_RT_IMAGE)),
                MayBePresent,
            )
        }
        // "Required if: Volumetric Properties is VOLUME or SAMPLED, and Image Type Value 3 is not
        // LABEL or OVERVIEW, or <Segmentation ...>, or <Ophthalmic Tomography ...>, or <OCT
        // B-scan Volume Analysis>. May be present otherwise, if SOP Class UID is not Enhanced RT
        // Image Storage and not Enhanced Continuous RT Image Storage."
        C::SliceThickness_d972aa => {
            let volumetric = value_in(root, tags::VOLUMETRIC_PROPERTIES, 1, &["VOLUME", "SAMPLED"])
                .and(scope.image_type_is(3, &["LABEL", "OVERVIEW"]).not());
            match volumetric.or(other_sop_class_spacing(scope)) {
                Tri::True => Requirement::Required,
                Tri::Unknown => Requirement::Undeterminable,
                Tri::False if sop_class_in(scope, &ENHANCED_RT_IMAGE) == Tri::True => {
                    Requirement::Forbidden
                }
                Tri::False => Requirement::Optional,
            }
        }
        // "Required if Dimension Organization Type is TILED_FULL and Total Pixel Matrix Focal
        // Planes is greater than 1. May be present otherwise."
        C::SpacingBetweenSlices_c4b1ab => required_if(
            scope.dimension_organization_type_is("TILED_FULL").and(
                integer_value(root, tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES)
                    .is_some_and(|planes| planes > 1)
                    .into(),
            ),
            MayBePresent,
        ),

        // ---- Table C.7.6.16-3 Frame Content Macro ----
        // "Required if Frame Type Value 1 of this Frame is ORIGINAL, and Dimension Organization
        // Type is not TILED_FULL, and the SOP Class UID is not <Legacy Converted CT, MR, PET,
        // VL Whole Slide Microscopy>. May be present otherwise." (Frame Reference DateTime,
        // Frame Acquisition Duration)
        C::FrameReferenceDateTime_b677d6 => {
            required_if(original_frame_not_tiled_full(scope), MayBePresent)
        }
        // As above, "or SOP Class UID is Photoacoustic Image Storage. May be present otherwise."
        C::FrameAcquisitionDateTime_a613a8 => required_if(
            original_frame_not_tiled_full(scope)
                .or(sop_class_in(scope, &[uids::PHOTOACOUSTIC_IMAGE_STORAGE])),
            MayBePresent,
        ),
        // "Required if the Value of Dimension Index Sequence (0020,9222) exists."
        C::DimensionIndexValues_88c44a => required_if(
            (!items(root, tags::DIMENSION_INDEX_SEQUENCE).is_empty()).into(),
            NotPresent,
        ),
        // "Required if the Value of SOP Class UID equals Enhanced PET Image Storage or Functional
        // MR Sequence is present. May be present otherwise." (Temporal Position Index, Stack ID)
        C::TemporalPositionIndex_517e64 | C::StackID_06f205 => required_if(
            sop_class_in(scope, &[uids::ENHANCED_PET_IMAGE_STORAGE])
                .or(functional_mr(scope).into()),
            MayBePresent,
        ),
        // "Required if Stack ID or Functional MR Sequence is present."
        C::InStackPositionNumber_261950 => required_if(
            (has(item, tags::STACK_ID) || functional_mr(scope)).into(),
            NotPresent,
        ),

        // ---- Tables C.7.6.16-6/-7 Referenced Image and Derivation Image Macros ----
        // "Required if SOP Class UID is not <Legacy Converted CT, MR, PET>, may be present
        // otherwise."
        C::PurposeOfReferenceCodeSequence_187ad5 => {
            required_if(sop_class_in(scope, &LEGACY_CONVERTED).not(), MayBePresent)
        }

        // ---- Table A.32.8-2 Functional Group usage ----
        // "C - Required if the image or Frame has been derived from another SOP Instance."
        C::FgDerivationImage_6f39a8 => required_if(Tri::Unknown, NotPresent),
        // "U - May be used only if Photometric Interpretation (0028,0004) is MONOCHROME2."
        C::FgRealWorldValueMapping_81c20c => permitted_only_if(scope.photometric_is("MONOCHROME2")),
        // "C - Required if Dimension Organization Type is not TILED_FULL; may be present
        // otherwise." (Plane Position (Slide), Optical Path Identification)
        C::FgPlanePositionSlide_dc3941 => required_if(
            scope.dimension_organization_type_is("TILED_FULL").not(),
            MayBePresent,
        ),

        // ---- Table C.7-18 Device Module ----
        // "Required if Device Diameter (0050,0016) is present."
        C::DeviceDiameterUnits_3c1081 => {
            required_if(present(item, tags::DEVICE_DIAMETER), NotPresent)
        }

        // ---- Table C.11-2b VOI LUT Macro ----
        // "Required if Window Center (0028,1050) is present."
        C::WindowWidth_65ee92 => required_if(present(item, tags::WINDOW_CENTER), NotPresent),

        // ---- Table C.7-7 Synchronization Module ----
        // "Required if synchronization channel or trigger is encoded in a waveform in this SOP
        // Instance." Only a Waveform Sequence (5400,0100) encodes a waveform.
        C::SynchronizationChannel_e1ebdb => {
            required_if(present(root, tags::WAVEFORM_SEQUENCE), NotPresent)
        }

        // ---- Table C.8.20-4 Segment Description Macro (also Table C.8.35.3-1) ----
        // "Required if Tracking UID (0062,0021) is present." and the converse.
        C::TrackingID_aba688 => required_if(present(item, tags::TRACKING_UID), NotPresent),
        C::TrackingUID_f608d8 => required_if(present(item, tags::TRACKING_ID), NotPresent),
        // "Required if Referenced SOP Class UID (0008,1150) is "1.2.840.10008.5.1.4.1.1.481.3"
        // (RT Structure Set Storage)."
        C::ReferencedROINumber_14d038 => required_if(
            value_in(
                item,
                tags::REFERENCED_SOP_CLASS_UID,
                1,
                &[uids::RT_STRUCTURE_SET_STORAGE],
            ),
            NotPresent,
        ),

        // ---- Tables A.51-2 and A.75-2 Functional Group usage ----
        // "C - Required if Derivation Image Functional Group is not present and the Frame of
        // Reference is defined in the patient-relative Reference Coordinate System. May be
        // present otherwise if the Frame of Reference is defined in the patient-relative
        // Reference Coordinate System." (Pixel Measures, Plane Position (Patient), Plane
        // Orientation (Patient)). A.51.5.1 specifies a Slide Coordinate System segment "using
        // the Pixel Measures and ... the Plane Position (Slide)", which the usage text forbids;
        // with that conflict a Slide Frame of Reference leaves the three undetermined.
        C::FgPixelMeasures_3dce04 => {
            match (
                patient_frame_of_reference(scope),
                slide_frame_of_reference(scope),
            ) {
                (Tri::True, _) if !groups_have(scope, tags::DERIVATION_IMAGE_SEQUENCE) => {
                    Requirement::Required
                }
                (Tri::True, _) => Requirement::Optional,
                (Tri::False, Tri::False) => Requirement::Forbidden,
                _ => Requirement::Undeterminable,
            }
        }
        // "C - Required if Derivation Image Functional Group is not present and the Frame of
        // Reference is defined in the Slide Coordinate System and Dimension Organization Type
        // (0020,9311) is not TILED_FULL. May be present otherwise if the Frame of Reference is
        // defined in the Slide Coordinate System."
        C::FgPlanePositionSlide_c41baf => slide_positions(
            scope,
            Tri::from(!groups_have(scope, tags::DERIVATION_IMAGE_SEQUENCE)),
        ),
        // "C - Required if the Frame of Reference is defined in the Slide Coordinate System and
        // Dimension Organization Type (0020,9311) is not TILED_FULL. May be present otherwise if
        // the Frame of Reference is defined in the Slide Coordinate System."
        C::FgPlanePositionSlide_c1ecaa => slide_positions(scope, Tri::True),
        // "C - Required if Pixel Measures or either Plane Position (Patient) or Plane Orientation
        // (Patient) (if the Frame of Reference is defined in the patient-relative Reference
        // Coordinate System), or Plane Position (Slide) (if the Frame of Reference is defined in
        // the Slide Coordinate System) Functional Groups are not present. May be present
        // otherwise."
        C::FgDerivationImage_a322fc => {
            let positioned = match (
                patient_frame_of_reference(scope),
                slide_frame_of_reference(scope),
            ) {
                (Tri::True, _) => Tri::from(
                    groups_have(scope, tags::PLANE_POSITION_SEQUENCE)
                        && groups_have(scope, tags::PLANE_ORIENTATION_SEQUENCE),
                ),
                (_, Tri::True) => {
                    Tri::from(groups_have(scope, tags::PLANE_POSITION_SLIDE_SEQUENCE))
                }
                (Tri::False, Tri::False) => Tri::False,
                _ => Tri::Unknown,
            };
            let measured = Tri::from(groups_have(scope, tags::PIXEL_MEASURES_SEQUENCE));
            required_if(measured.and(positioned).not(), MayBePresent)
        }
        // "C - Required if not empty." The Frame Content Macro is not empty when Dimension Index
        // Values (0020,9157) is required in it, which a Dimension Index Sequence makes it; its
        // Type 3 Attributes may fill it otherwise.
        C::FgFrameContent_506a28 => required_if(
            Tri::from(!items(root, tags::DIMENSION_INDEX_SEQUENCE).is_empty()),
            MayBePresent,
        ),
        // "C - Required if Dimension Organization Type (0020,9311) is not TILED_FULL and
        // Segmentation Type (0062,0001) is not LABELMAP."
        C::FgSegmentation_99fd82 => required_if(
            scope
                .dimension_organization_type_is("TILED_FULL")
                .not()
                .and(value_in(root, tags::SEGMENTATION_TYPE, 1, &["LABELMAP"]).not()),
            NotPresent,
        ),
        // "C - Required if the Frame of Reference is defined in the patient-relative Reference
        // Coordinate System." (Plane Position (Patient), Plane Orientation (Patient))
        C::FgPlanePositionPatient_183b05 => {
            required_if(patient_frame_of_reference(scope), NotPresent)
        }
        // "C - Required if Pixel Presentation (0008,9205) in the Parametric Map Image Module
        // equals COLOR_RANGE."
        C::FgStoredValueColorRange_fd7048 => required_if(color_range(root), NotPresent),

        // ---- Table C.37.1-2 Microscopy Bulk Simple Annotations Module (and Table A.87-1) ----
        // "C - Required if Annotation Coordinate Type (006A,0001) is 3D"
        C::ModuleFrameOfReference_9c03a6 => required_if(coordinates_3d(root), NotPresent),
        // "Required if Annotation Coordinate Type (006A,0001) is 2D[. May be present otherwise.]"
        C::PixelOriginInterpretation_577198 => required_if(coordinates_2d(root), NotPresent),
        C::ReferencedImageSequence_89465a => required_if(coordinates_2d(root), MayBePresent),
        // "Required if Annotation Coordinate Type (006A,0001) is 3D."
        C::AnnotationAppliesToAllZPlanes_867724 => required_if(coordinates_3d(root), NotPresent),
        // "Required if Annotation Generation Type (006A,0007) is AUTOMATIC or SEMIAUTOMATIC."
        C::AnnotationGroupAlgorithmIdentificationSequence_671dca => required_if(
            value_in(
                item,
                tags::ANNOTATION_GROUP_GENERATION_TYPE,
                1,
                &["AUTOMATIC", "SEMIAUTOMATIC"],
            ),
            NotPresent,
        ),
        // "Required if Annotation Applies to All Optical Paths (006A,000D) is NO."
        C::ReferencedOpticalPathIdentifier_12a10b => required_if(
            value_in(
                item,
                tags::ANNOTATION_APPLIES_TO_ALL_OPTICAL_PATHS,
                1,
                &["NO"],
            ),
            NotPresent,
        ),
        // "Required if Annotation Coordinate Type (006A,0001) is 3D, and all points in Point
        // Coordinates Data (0066,0016) or Double Point Coordinates Data (0066,0022) are in the
        // same Z plane(s)."
        C::CommonZCoordinateValue_a8e676 => {
            required_if(coordinates_3d(root).and(common_z(item)), NotPresent)
        }
        // "Required if Double Point Coordinates Data (0066,0022) is not present." and the converse.
        C::PointCoordinatesData_58d08c => required_if(
            absent(item, tags::DOUBLE_POINT_COORDINATES_DATA),
            NotPresent,
        ),
        C::DoublePointCoordinatesData_aabd15 => {
            required_if(absent(item, tags::POINT_COORDINATES_DATA), NotPresent)
        }
        // "Required if Graphic Type (0070,0023) is POLYLINE or POLYGON."
        C::LongPrimitivePointIndexList_bb9fb2 => required_if(
            value_in(item, tags::GRAPHIC_TYPE, 1, &["POLYLINE", "POLYGON"]),
            NotPresent,
        ),
        // "Required if Measurement Values stored in Floating Point Values (0066,0125) are
        // associated with only a subset of Annotations." Fewer Values than the Annotation Group's
        // Number of Annotations (006A,000C) are a subset.
        C::AnnotationIndexList_0474f0 => required_if(measured_subset(scope), NotPresent),

        // ---- Table A.51-1 Module usage ----
        // "C - Required if Derivation Image Functional Group is not present. May be present
        // otherwise."
        C::ModuleFrameOfReference_e53396 => required_if(
            Tri::from(!groups_have(scope, tags::DERIVATION_IMAGE_SEQUENCE)),
            MayBePresent,
        ),
        // "C - Required if Derivation Image Functional Group is present."
        C::ModuleCommonInstanceReference_ef2f7b => required_if(
            Tri::from(groups_have(scope, tags::DERIVATION_IMAGE_SEQUENCE)),
            NotPresent,
        ),
        // "C - Required if Photometric Interpretation (0028,0004) has a Value of PALETTE COLOR."
        // (Palette Color Lookup Table, ICC Profile)
        C::ModulePaletteColorLookupTable_42e847 => {
            required_if(scope.photometric_is("PALETTE COLOR"), NotPresent)
        }

        // ---- Table C.8.20-2 Segmentation Image Module ----
        // "Enumerated Values if Segmentation Type (0062,0001) is BINARY or FRACTIONAL", "...is
        // LABELMAP", "...is BINARY", "...is FRACTIONAL"
        C::PhotometricInterpretation_48f82a => {
            applies(segmentation_type(root, &["BINARY", "FRACTIONAL"]))
        }
        C::PhotometricInterpretation_94f3f9 => applies(segmentation_type(root, &["LABELMAP"])),
        C::BitsAllocated_c43232 => applies(segmentation_type(root, &["BINARY"])),
        C::BitsAllocated_8408ab => applies(segmentation_type(root, &["FRACTIONAL"])),
        // "Enumerated Values if Segmentation Type (0062,0001) is LABELMAP and Bits Allocated
        // (0028,0100) is 8 [16]"
        C::BitsStored_ded854 => {
            applies(segmentation_type(root, &["LABELMAP"]).and(bits_allocated(root, 8)))
        }
        C::BitsStored_a7abfb => {
            applies(segmentation_type(root, &["LABELMAP"]).and(bits_allocated(root, 16)))
        }
        // "Required if present in the source images or this IOD [SOP] Instance has been
        // compressed." Lossy Image Compression "01" shows lossy compression; the source images
        // are not in the object. (Also Table C.8.32-2.)
        C::LossyImageCompressionRatio_611047
        | C::LossyImageCompressionMethod_090992
        | C::LossyImageCompressionRatio_0b9ecd
        | C::LossyImageCompressionMethod_e4e641 => {
            let compressed = value_in(root, tags::LOSSY_IMAGE_COMPRESSION, 1, &["01"]);
            required_if(compressed.or(Tri::Unknown), NotPresent)
        }
        // "Required if Segmentation Type (0062,0001) is FRACTIONAL." (Segmentation Fractional
        // Type, Maximum Fractional Value)
        C::SegmentationFractionalType_63bbc3 => {
            required_if(segmentation_type(root, &["FRACTIONAL"]), NotPresent)
        }
        // "Required if Segment Algorithm Type (0062,0008) is not MANUAL."
        C::SegmentAlgorithmName_01435f => required_if(
            value_other_than(item, tags::SEGMENT_ALGORITHM_TYPE, &["MANUAL"]),
            NotPresent,
        ),
        // Type 3: "Shall not be present if Segmentation Type (0062,0001) is LABELMAP and
        // Photometric Interpretation (0028,0004) is PALETTE COLOR."
        C::RecommendedDisplayCIELabValue_79ebd0 => forbidden_if(
            segmentation_type(root, &["LABELMAP"]).and(scope.photometric_is("PALETTE COLOR")),
        ),

        // ---- Table A.51 Content Constraints (A.51.4) on shared Modules ----
        // "Pixel Padding Value (0028,0120) shall not be present if Segmentation Type (0062,0001)
        // is not LABELMAP." Otherwise the General Equipment Module condition applies.
        C::PixelPaddingValue_d4f36e => match segmentation_type(root, &["LABELMAP"]) {
            Tri::True => evaluate(C::PixelPaddingValue_894c69, scope),
            Tri::False | Tri::Unknown => Requirement::Forbidden,
        },
        // "Pixel Padding Range Limit (0028,0121) shall not be present."
        C::PixelPaddingRangeLimit_ae4d69 => Requirement::Forbidden,

        // ---- PS3.4 B.5.1.25 Segmentation Storage SOP Classes ----
        // "The [Label Map] Segmentation Storage SOP Class shall use the PS3.3 Section A.51 with an
        // Enumerated Value of BINARY or FRACTIONAL [LABELMAP] for Segmentation Type (0062,0001)."
        C::SegmentationType_c84858 => applies(sop_class_in(scope, &[uids::SEGMENTATION_STORAGE])),
        C::SegmentationType_fd2ec6 => {
            applies(sop_class_in(scope, &[uids::LABEL_MAP_SEGMENTATION_STORAGE]))
        }

        // ---- Tables C.7.6.16-4 and C.7.6.16-5 Plane Position and Orientation (Patient) ----
        // "Required if: Frame Type (0008,9007) Value 1 of this Frame is ORIGINAL and Volumetric
        // Properties (0008,9206) of this Frame is other than DISTORTED, or <Segmentation with
        // Frame of Reference UID>, or <Ophthalmic Tomography with volumetric flag YES>, or <OCT
        // B-scan Volume Analysis>[, or <Enhanced RT Image>, or <Enhanced Continuous RT Image>].
        // May be present otherwise."
        C::ImagePositionPatient_fcc660 => required_if(
            original_undistorted_frame(scope).or(other_sop_class_spacing(scope)),
            MayBePresent,
        ),
        C::ImageOrientationPatient_13af2f => required_if(
            original_undistorted_frame(scope)
                .or(other_sop_class_spacing(scope))
                .or(sop_class_in(scope, &ENHANCED_RT_IMAGE)),
            MayBePresent,
        ),

        // ---- Tables C.7.6.24-1 and C.7.6.25-1 Floating Point Image Pixel Modules ----
        // "Required if [Double] Float Pixel Padding Value (0028,0122 [0123]) is present."
        C::FloatPixelPaddingRangeLimit_fa212b => {
            required_if(present(item, tags::FLOAT_PIXEL_PADDING_VALUE), NotPresent)
        }
        C::DoubleFloatPixelPaddingRangeLimit_0e89d4 => required_if(
            present(item, tags::DOUBLE_FLOAT_PIXEL_PADDING_VALUE),
            NotPresent,
        ),

        // ---- Table A.75-1 Module usage ----
        // "C - Required if integer pixels" / "32 bit floating point pixels" / "64 bit floating
        // point pixels": the Pixel Data Attribute that holds them (A.75.4).
        C::ModuleImagePixel_f11529 => required_if(integer_pixels(root), NotPresent),
        C::ModuleFloatingPointImagePixel_202cb1 => {
            required_if(present(root, tags::FLOAT_PIXEL_DATA), NotPresent)
        }
        C::ModuleDoubleFloatingPointImagePixel_0a0896 => {
            required_if(present(root, tags::DOUBLE_FLOAT_PIXEL_DATA), NotPresent)
        }
        // "C - Required if Pixel Presentation (0008,9205) in the Parametric Map image Module equals
        // COLOR_RANGE and Palette Color Lookup Table UID (0028,1199) is not present."
        C::ModulePaletteColorLookupTable_501a20 => required_if(
            color_range(root).and(absent(root, tags::PALETTE_COLOR_LOOKUP_TABLE_UID)),
            NotPresent,
        ),
        // "C - Required if Referenced Image Functional Group or Derivation Image Functional Group
        // is present."
        C::ModuleCommonInstanceReference_a6c5de => required_if(
            Tri::from(
                groups_have(scope, tags::REFERENCED_IMAGE_SEQUENCE)
                    || groups_have(scope, tags::DERIVATION_IMAGE_SEQUENCE),
            ),
            NotPresent,
        ),

        // ---- Table C.8.32-2 Parametric Map Image Module ----
        // "Enumerated Values if Pixel Data (7FE0,0010) or Pixel Data Provider URL (0028,7FE0) is
        // present", "...if Float Pixel Data (7FE0,0008) is present", "...if Double Float Pixel
        // Data (7FE0,0009) is present"
        C::BitsAllocated_38792c => applies(integer_pixels(root)),
        C::BitsAllocated_2af7e7 => applies(present(root, tags::FLOAT_PIXEL_DATA)),
        C::BitsAllocated_c03b30 => applies(present(root, tags::DOUBLE_FLOAT_PIXEL_DATA)),
        // "Required if Pixel Data (7FE0,0010) or Pixel Data Provider URL (0028,7FE0) is present."
        // (Bits Stored, High Bit)
        C::BitsStored_44fd83 => required_if(integer_pixels(root), NotPresent),
        // "Required if Pixel Presentation (0008,9205) equals COLOR_RANGE and the Section C.7.9 is
        // not present."
        C::PaletteColorLookupTableUID_c285e7 => required_if(
            color_range(root).and(Tri::from(!palette_tables_present(root))),
            NotPresent,
        ),
        // "Required if Pixel Presentation (0008,9205) has a Value of COLOR_RANGE."
        C::ICCProfile_875087 => required_if(color_range(root), NotPresent),

        // ---- Tables C.7.6.18-1 to -3 Cardiac, Respiratory and Bulk Motion Synchronization ----
        // "Required if Image Type (0008,0008) Value 1 is ORIGINAL or MIXED. May be present
        // otherwise."
        C::CardiacSynchronizationTechnique_aff23f => {
            required_if(original_or_mixed(scope), MayBePresent)
        }
        // "Required if Image Type Value 1 is ORIGINAL or MIXED and Cardiac Synchronization
        // Technique (0018,9037) equals other than NONE."
        C::CardiacSignalSource_ac9a6c => required_if(
            original_or_mixed(scope).and(value_other_than(
                item,
                tags::CARDIAC_SYNCHRONIZATION_TECHNIQUE,
                &["NONE"],
            )),
            NotPresent,
        ),
        // "... and Cardiac Synchronization Technique (0018,9037) equals PROSPECTIVE or
        // RETROSPECTIVE."
        C::CardiacBeatRejectionTechnique_a4dce8 => required_if(
            original_or_mixed(scope).and(value_in(
                item,
                tags::CARDIAC_SYNCHRONIZATION_TECHNIQUE,
                1,
                &["PROSPECTIVE", "RETROSPECTIVE"],
            )),
            NotPresent,
        ),
        // "Required if type of framing is not time forward from trigger, may be present
        // otherwise." The framing is described by this Attribute alone.
        C::CardiacFramingType_634767 => required_if(Tri::Unknown, MayBePresent),
        // "Required if Image Type Value 1 is ORIGINAL or MIXED and Respiratory Motion
        // Compensation Technique (0018,9170) equals other than NONE."
        C::RespiratorySignalSource_c4318c => required_if(
            original_or_mixed(scope).and(value_other_than(
                item,
                tags::RESPIRATORY_MOTION_COMPENSATION_TECHNIQUE,
                &["NONE"],
            )),
            NotPresent,
        ),
        // "Required if Respiratory Motion Compensation Technique (0018,9170) equals other than
        // NONE, REALTIME or BREATH_HOLD and if Image Type Value 1 is ORIGINAL or MIXED. May be
        // present otherwise."
        C::RespiratoryTriggerDelayThreshold_f11c4b => required_if(
            value_other_than(
                item,
                tags::RESPIRATORY_MOTION_COMPENSATION_TECHNIQUE,
                &["NONE", "REALTIME", "BREATH_HOLD"],
            )
            .and(original_or_mixed(scope)),
            MayBePresent,
        ),
        // "Required if the Value is not TIME, may be present otherwise." Its absence states TIME.
        C::RespiratoryTriggerType_58178c => required_if(Tri::False, MayBePresent),
        // "Required if Image Type Value 1 is ORIGINAL or MIXED and Bulk Motion Compensation
        // Technique (0018,9172) equals other than NONE."
        C::BulkMotionSignalSource_bb311d => required_if(
            original_or_mixed(scope).and(value_other_than(
                item,
                tags::BULK_MOTION_COMPENSATION_TECHNIQUE,
                &["NONE"],
            )),
            NotPresent,
        ),

        // ---- Tables C.7.6.16-8 and C.7.6.16-18 Cardiac and Respiratory Synchronization Macros ----
        // "Required if used as a dimension index, may be present otherwise." (Nominal Percentage
        // of Cardiac Phase, Nominal Percentage of Respiratory Phase). The text is shared, so a
        // Dimension Index Pointer to either leaves which one is meant undetermined.
        C::NominalPercentageOfCardiacPhase_88e8a2 => {
            let indexed = dimension_pointers(root).into_iter().any(|pointer| {
                pointer == tags::NOMINAL_PERCENTAGE_OF_CARDIAC_PHASE
                    || pointer == tags::NOMINAL_PERCENTAGE_OF_RESPIRATORY_PHASE
            });
            required_if(
                if indexed { Tri::Unknown } else { Tri::False },
                MayBePresent,
            )
        }
        // "Required if Intervals Acquired (0018,1083) is present and has a Value of 1. May be
        // present otherwise."
        C::ActualCardiacTriggerDelayTime_4f4dea => required_if(
            Tri::from(integer_value(root, tags::INTERVALS_ACQUIRED) == Some(1)),
            MayBePresent,
        ),
        // "Required if Cardiac Synchronization Technique (0018,9037) equals other than NONE or
        // REALTIME. May be present otherwise."
        C::RRIntervalTimeNominal_0c4d07 => required_if(
            value_other_than(
                root,
                tags::CARDIAC_SYNCHRONIZATION_TECHNIQUE,
                &["NONE", "REALTIME"],
            ),
            MayBePresent,
        ),
        // "Required if Respiratory Motion Compensation Technique (0018,9170) equals other than
        // NONE or REALTIME and Respiratory Trigger Type (0020,9250) is absent or has a Value of
        // TIME or BOTH."
        C::RespiratoryIntervalTime_909e53 => required_if(
            value_other_than(
                root,
                tags::RESPIRATORY_MOTION_COMPENSATION_TECHNIQUE,
                &["NONE", "REALTIME"],
            )
            .and(absent(root, tags::RESPIRATORY_TRIGGER_TYPE).or(value_in(
                root,
                tags::RESPIRATORY_TRIGGER_TYPE,
                1,
                &["TIME", "BOTH"],
            ))),
            NotPresent,
        ),
        // "Required if Respiratory Trigger Type (0020,9250) is TIME or BOTH."
        C::ActualRespiratoryTriggerDelayTime_3944c9 => required_if(
            value_in(root, tags::RESPIRATORY_TRIGGER_TYPE, 1, &["TIME", "BOTH"]),
            NotPresent,
        ),
        // "Required if Respiratory Trigger Type (0020,9250) is AMPLITUDE or BOTH." (Starting and
        // Ending Respiratory Amplitude)
        C::StartingRespiratoryAmplitude_b10e33 => required_if(
            value_in(
                root,
                tags::RESPIRATORY_TRIGGER_TYPE,
                1,
                &["AMPLITUDE", "BOTH"],
            ),
            NotPresent,
        ),
        // "Required if Starting [Ending] Respiratory Amplitude (0020,9246 [9248]) is present."
        C::StartingRespiratoryPhase_4d0ba5 => required_if(
            present(item, tags::STARTING_RESPIRATORY_AMPLITUDE),
            NotPresent,
        ),
        C::EndingRespiratoryPhase_9adda6 => required_if(
            present(item, tags::ENDING_RESPIRATORY_AMPLITUDE),
            NotPresent,
        ),

        // ---- Table C.11-2b VOI LUT Macro ----
        // "Required if Window Center (0028,1050) is not present. May be present otherwise." and
        // the converse for Window Center.
        C::VOILUTSequence_ddfce9 => required_if(absent(item, tags::WINDOW_CENTER), MayBePresent),
        C::WindowCenter_f9b098 => required_if(absent(item, tags::VOILUT_SEQUENCE), MayBePresent),

        // ---- Table C.7.16-13 Contrast/Bolus Usage Macro ----
        // "Required if Contrast/Bolus Administration Route Sequence (0018,0014) for the
        // Contrast/Bolus Agent Number (0018,9337) defined in the Contrast/Bolus Agent Sequence
        // (0018,0012) is (47625008, SCT, "Intravenous route") ...; may be present otherwise."
        C::ContrastBolusAgentPhase_52f965 => required_if(intravenous_agent(scope), MayBePresent),

        // ---- Tables C.7.6.16.2.25.1-1 and -2.1 Unassigned Converted Attributes Macros ----
        // "Required if any unassigned shared [per-frame] Attributes are present [for this Frame]."
        // Which Attributes a conversion left unassigned is not encoded apart from these Sequences.
        C::UnassignedSharedConvertedAttributesSequence_1df2d1
        | C::UnassignedPerFrameConvertedAttributesSequence_30ffda => {
            required_if(Tri::Unknown, NotPresent)
        }

        // ---- PS3.10 Table 7.1-1 File Meta Information ----
        // "Required if Private Information Creator UID (0002,0100) is present."
        C::PrivateInformation_5c0ac1 => required_if(
            present(item, tags::PRIVATE_INFORMATION_CREATOR_UID),
            NotPresent,
        ),

        // ---- Table F.3-2 File-set Identification Module ----
        // "Required to specify the expanded or replacement character set." Which
        // characters the File-set Descriptor File holds is not encoded; without that
        // File the Attribute describes nothing.
        C::SpecificCharacterSetOfFileSetDescriptorFile_793b31 => required_if(
            present(item, tags::FILE_SET_DESCRIPTOR_FILE_ID).and(Tri::Unknown),
            NotPresent,
        ),

        // ---- Table F.3-3 Directory Information Module ----
        // "Required if the Directory Record Type (0004,1430) is of Value PRIVATE."
        C::PrivateRecordUID_399277 => required_if(
            value_in(item, tags::DIRECTORY_RECORD_TYPE, 1, &["PRIVATE"]),
            NotPresent,
        ),
        // "When the Directory Record does not reference any SOP Instance this Attribute
        // shall not be present." (Type 1C: required when it does.)
        C::ReferencedFileID_3760d0 => required_if(references_sop_instance(item), NotPresent),
        // "Required if the Directory Record references a SOP Instance."
        C::ReferencedSOPClassUIDInFile_b5b28b => {
            required_if(references_sop_instance(item), NotPresent)
        }
        // "Required if the Directory Record references a SOP Instance that encodes the
        // Related General SOP Class UID (0008,001A)." The referenced File is not read.
        C::ReferencedRelatedGeneralSOPClassUIDInFile_f4b354 => {
            required_if(references_sop_instance(item).and(Tri::Unknown), NotPresent)
        }

        // ---- Tables F.5-x Record Selection Keys ----
        // "Required if an extended or replacement character set is used in one of the
        // Keys." As for the SOP Common Module, a present Value is not judged.
        C::SpecificCharacterSet_ebd0ce => item_character_set(item),
        // "Required if Verification Flag (0040,A493) is VERIFIED." (Also Verifying
        // Observer Sequence of Table C.17-2.)
        C::VerifyingObserverSequence_bea66d => required_if(
            value_in(item, tags::VERIFICATION_FLAG, 1, &["VERIFIED"]),
            NotPresent,
        ),
        // "Required if Anatomic Region Sequence (0008,2218) is not present. May be
        // present otherwise."
        C::Modality_12e144 => {
            required_if(absent(item, tags::ANATOMIC_REGION_SEQUENCE), MayBePresent)
        }
        // "Required if Modality (0008,0060) is not present. May be present otherwise."
        C::AnatomicRegionSequence_e8c3b8 => required_if(absent(item, tags::MODALITY), MayBePresent),
        // "Required if Anatomic Region Sequence (0008,2218) is present." (Type 2C)
        C::Laterality_463eac => {
            required_if(present(item, tags::ANATOMIC_REGION_SEQUENCE), NotPresent)
        }
        // "Required if Referenced Instance Sequence (0008,114A) is not present."
        C::ReferencedWaveformSequence_f42349 => {
            required_if(absent(item, tags::REFERENCED_INSTANCE_SEQUENCE), NotPresent)
        }
        // Key conditions on the content of the referenced Instance, which a
        // DICOMDIR does not hold: "the IOD of the Presentation State SOP Instance
        // referenced by this Directory Record includes the Section C.11.11", "the
        // SOP Instance referenced by this Directory Record includes Blending Sequence",
        // "the root Content Item is the Source Content Item of HAS CONCEPT MOD
        // relationships", "present in the Spectroscopy Instance", "encapsulated
        // document is an HL7 Structured Document", "present in the referenced
        // Instance", "User Content [Long] Label (3010,0033/0034) is present", "the IOD
        // of the Waveform Presentation State SOP Instance ... includes the Waveform
        // Presentation State Relationship Module", "Structured Waveform Annotation
        // Sequence (0040,B030) is present".
        C::ReferencedSeriesSequence_5fba33
        | C::BlendingSequence_942c1d
        | C::ContentSequence_fd3381
        | C::ReferencedImageEvidenceSequence_52b821
        | C::HL7InstanceIdentifier_bf2ac9
        | C::ImplantSize_62ba2f
        | C::UserContentLabel_c9e668
        | C::UserContentLongLabel_22bf5d
        | C::ReferencedSeriesSequence_8c3b36
        | C::ReferencedInstanceSequence_e2c71a => required_if(Tri::Unknown, NotPresent),

        // ---- Table C.38.2-1 Inventory Scope (Inventory Directory Record keys) ----
        // "Required if extended matching mechanisms were applied."; "Required if Range
        // Matching / List of UID Matching / Single Value, Multiple Value, or Wild Card
        // matching is applied to any Key Attribute." How the scope was matched is
        // not encoded apart from these Attributes.
        C::ExtendedMatchingMechanisms_af2f2b
        | C::RangeMatchingSequence_ce4c8a
        | C::ListOfUIDMatchingSequence_e8cd11
        | C::GeneralMatchingSequence_e9665b => required_if(Tri::Unknown, NotPresent),
        // "Required if Attributes within the Sequence Item use an expanded or
        // replacement character set."
        C::SpecificCharacterSet_00ceec => item_character_set(item),
        // "Required if Extended Matching Mechanisms (0008,040F) includes Value
        // TIMEZONE_ADJUST."
        C::TimezoneOffsetFromUTC_68b8ba => required_if(
            includes_value(item, tags::EXTENDED_MATCHING_MECHANISMS, "TIMEZONE_ADJUST"),
            NotPresent,
        ),
        // "Required if Extended Matching Mechanisms (0008,040F) includes Value RELATIONAL."
        C::QueryRetrieveLevel_29856c => required_if(
            includes_value(item, tags::EXTENDED_MATCHING_MECHANISMS, "RELATIONAL"),
            NotPresent,
        ),
        // "Required if Extended Matching Mechanisms (0008,040F) includes Value
        // EMPTY_VALUE and Empty Value Matching is applied to any Key Attribute."
        C::EmptyValueMatchingSequence_714fca => required_if(
            includes_value(item, tags::EXTENDED_MATCHING_MECHANISMS, "EMPTY_VALUE")
                .and(Tri::Unknown),
            NotPresent,
        ),

        // ---- Table C.17-6 Document Relationship Macro ----
        // "Required if the date and time are different from Content Date (0008,0023) and
        // Content Time (0008,0033) or Observation DateTime (0040,A032) defined in higher
        // Items. May be present otherwise." When the observation took place is not encoded
        // apart from this Attribute.
        C::ObservationDateTime_51faf7 => required_if(Tri::Unknown, MayBePresent),
        // "Required if the enclosing Content Item has relationships." Its relationships are
        // the Items of this Sequence.
        C::ContentSequence_2857ac => required_if(present(item, tags::CONTENT_SEQUENCE), NotPresent),
        // "Include Table C.17-6 / C.17-5 if the Target Content Item is included by-value in
        // the Source Content Item. The Macro shall not be present if the relationship is
        // by-reference." A by-reference Item is the one with a Referenced Content Item
        // Identifier.
        C::IncludeC176_aad59d => required_if(
            absent(item, tags::REFERENCED_CONTENT_ITEM_IDENTIFIER),
            NotPresent,
        ),
        // "Required if the Target Content Item is denoted by-reference, i.e., the Section
        // C.17.3.4 and Section C.17.3.3 are not included." The Document Content Macro
        // (C.17.3.3) begins with the Type 1 Value Type.
        C::ReferencedContentItemIdentifier_74d9aa => {
            required_if(absent(item, tags::VALUE_TYPE), NotPresent)
        }

        // ---- Table C.17-2 SR Document General Module ----
        // "Required if this document includes content from other documents."; "Required if
        // this document is stored with different SOP Instance UIDs in one or more other
        // Studies."; "Required if pertinent evidence from other Requested Procedures needs to
        // be recorded." What else exists is not encoded in the document.
        C::PredecessorDocumentsSequence_e99aef
        | C::IdenticalDocumentsSequence_ad93a0
        | C::PertinentOtherEvidenceSequence_68205a => required_if(Tri::Unknown, NotPresent),
        // "Required if this Document fulfills at least one Requested Procedure. May be present
        // otherwise."; "Required if the identity of a CDA Document equivalent to the current
        // SOP Instance is known at the time of creation ... May be present otherwise."
        C::ReferencedRequestSequence_c43dd8 | C::ReferencedInstanceSequence_fa9b97 => {
            required_if(Tri::Unknown, MayBePresent)
        }
        // "Required if the creator is aware of Composite Objects acquired in order to satisfy
        // the Requested Procedure(s) for which the SR Document is or if Instances are
        // referenced in the Content Tree. May be present otherwise." The Content Tree shows
        // the second; the creator's awareness is not encoded.
        C::CurrentRequestedProcedureEvidenceSequence_0cdfc8 => required_if(
            Tri::from(content_tree_references(root)).or(Tri::Unknown),
            MayBePresent,
        ),
        // ---- Table C.17-3b Identified Person or Device Macro ----
        // "Required if Observer Type value is PSN."
        C::PersonName_5a02d8 => {
            required_if(value_in(item, tags::OBSERVER_TYPE, 1, &["PSN"]), NotPresent)
        }
        // "Required if Observer Type value is DEV."
        C::StationName_c7d092 => {
            required_if(value_in(item, tags::OBSERVER_TYPE, 1, &["DEV"]), NotPresent)
        }

        // ---- Table C.17.6-2 Key Object Document Module ----
        // "Required if this Document pertains to at least one Requested Procedure." Which
        // Requested Procedures it pertains to is not encoded apart from this Sequence.
        C::ReferencedRequestSequence_d87c48 => required_if(Tri::Unknown, NotPresent),
        // "Required if this Key Object Selection document references Instances in more than
        // one Study." Current Requested Procedure Evidence Sequence (0040,A375) lists every
        // Instance the document references, by Study.
        C::IdenticalDocumentsSequence_9954fb => {
            required_if(evidence_studies(root).into(), NotPresent)
        }

        // ---- Table C.17-5 Document Content Macro ----
        // "Required if Value Type (0040,A040) is TEXT, NUM, CODE, DATETIME, DATE, TIME,
        // UIDREF, TABLE or PNAME. Required if Value Type (0040,A040) is CONTAINER and a
        // heading is present, or this is the Root Content Item. Required if Value Type
        // (0040,A040) is COMPOSITE, IMAGE, WAVEFORM, SCOORD, SCOORD3D or TCOORD, and the
        // Purpose of Reference is conveyed in the Concept Name." A heading and a
        // Purpose of Reference are conveyed only by the Concept Name itself, which
        // these Value Types may then omit.
        C::ConceptNameCodeSequence_abde30 => {
            match string_value(item, tags::VALUE_TYPE, 1).as_deref() {
                Some(
                    "TEXT" | "NUM" | "CODE" | "DATETIME" | "DATE" | "TIME" | "UIDREF" | "TABLE"
                    | "PNAME",
                ) => Requirement::Required,
                Some("CONTAINER") if scope.is_top_level() => Requirement::Required,
                Some(
                    "CONTAINER" | "COMPOSITE" | "IMAGE" | "WAVEFORM" | "SCOORD" | "SCOORD3D"
                    | "TCOORD",
                ) => Requirement::Optional,
                // The Value Type itself is checked as a Type 1 Enumerated Value.
                _ => Requirement::Undeterminable,
            }
        }
        // "Include Table C.18.n-1 if and only if Value Type (0040,A040) is ..."
        C::IncludeC1811_b4d406 => value_type(item, &["NUM"]),
        C::IncludeC1821_a2ce9a => value_type(item, &["CODE"]),
        C::IncludeC1831_61f144 => value_type(item, &["COMPOSITE"]),
        C::IncludeC1841_08c4c7 => value_type(item, &["IMAGE"]),
        C::IncludeC1851_08552e => value_type(item, &["WAVEFORM"]),
        C::IncludeC1861_80daf7 => value_type(item, &["SCOORD"]),
        C::IncludeC1891_3cfefc => value_type(item, &["SCOORD3D"]),
        C::IncludeC1871_2dd22d => value_type(item, &["TCOORD"]),
        C::IncludeC1881_0f7a08 => value_type(item, &["CONTAINER"]),
        C::IncludeC18101_0ab121 => value_type(item, &["TABLE"]),

        // ---- Table C.18.1-1 Numeric Measurement Macro ----
        // "Required if Measured Value Sequence (0040,A300) is empty." The description
        // also makes it the "Qualification of Numeric Value (0040,A30A) in Measured
        // Value Sequence", so it may be present otherwise.
        C::NumericValueQualifierCodeSequence_2d34c8 => required_if(
            items(item, tags::MEASURED_VALUE_SEQUENCE).is_empty().into(),
            MayBePresent,
        ),
        // ---- Table C.18.6-1 Spatial Coordinates Macro ----
        // "Required if the instance referenced by Referenced Image Sequence (0008,1140) is
        // tiled ... May be present otherwise." The referenced Instance is not read.
        C::PixelOriginInterpretation_2ad28a => required_if(Tri::Unknown, MayBePresent),
        // ---- Table C.18.7-1 Temporal Coordinates Macro ----
        // "Required if the Referenced SOP Instance is a Waveform and Referenced Time
        // Offsets (0040,A138) and Referenced DateTime (0040,A13A) are not present."
        C::ReferencedSamplePositions_0a7da3 => required_if(
            Tri::Unknown
                .and(absent(item, tags::REFERENCED_TIME_OFFSETS))
                .and(absent(item, tags::REFERENCED_DATE_TIME)),
            NotPresent,
        ),
        // "Required if Referenced Sample Positions (0040,A132) and Referenced DateTime
        // (0040,A13A) are not present."
        C::ReferencedTimeOffsets_263fb1 => required_if(
            absent(item, tags::REFERENCED_SAMPLE_POSITIONS)
                .and(absent(item, tags::REFERENCED_DATE_TIME)),
            NotPresent,
        ),
        // "Required if Referenced Sample Positions (0040,A132) and Referenced Time
        // Offsets (0040,A138) are not present."
        C::ReferencedDateTime_b68814 => required_if(
            absent(item, tags::REFERENCED_SAMPLE_POSITIONS)
                .and(absent(item, tags::REFERENCED_TIME_OFFSETS)),
            NotPresent,
        ),
        // ---- Table C.18.8-1 Container Macro ----
        // "Required if a template defined and known to the implementation at the time of
        // encoding was used to define the content of this Item, ..." Which Template the
        // encoder used is not encoded apart from this Sequence.
        C::ContentTemplateSequence_613106 => required_if(Tri::Unknown, NotPresent),

        // ---- Table C.18.10-1 Table Content Macro ----
        // "Required if the concept does not apply to all rows/columns." A single Item
        // applies to all of them (C.18.10-1).
        C::TableRowNumber_c720fb => required_if(
            sibling_items(scope, tags::TABLE_ROW_DEFINITION_SEQUENCE),
            NotPresent,
        ),
        C::TableColumnNumber_a0b28f => required_if(
            sibling_items(scope, tags::TABLE_COLUMN_DEFINITION_SEQUENCE),
            NotPresent,
        ),
        // "Required if all the columns in this row (rows in this column) are numeric
        // values ..., have units (are not dimensionless), and have the same units."
        // Whether a value is dimensionless is not encoded apart from its units.
        C::MeasurementUnitsCodeSequence_95167c | C::MeasurementUnitsCodeSequence_641853 => {
            required_if(Tri::Unknown, NotPresent)
        }
        // "Required if Table Column Number is absent, or all the rows for the column
        // specified in Table Column Number are not encoded in a single Attribute ...";
        // which rows a multi-valued cell Attribute spans is not encoded.
        C::TableRowNumber_0a17c3 => required_if(
            absent(item, tags::TABLE_COLUMN_NUMBER).or(Tri::Unknown),
            NotPresent,
        ),
        C::TableColumnNumber_325185 => required_if(
            absent(item, tags::TABLE_ROW_NUMBER).or(Tri::Unknown),
            NotPresent,
        ),
        // "Required if Selector Attribute VR (0072,0050) is not present."
        C::ReferencedContentItemIdentifier_64a61e => {
            required_if(absent(item, tags::SELECTOR_ATTRIBUTE_VR), NotPresent)
        }
        // "Required if Referenced Content Item Identifier (0040,DB73) is not present."
        C::SelectorAttributeVR_b521fa => required_if(
            absent(item, tags::REFERENCED_CONTENT_ITEM_IDENTIFIER),
            NotPresent,
        ),
        // "Required if Selector Attribute VR (0072,0050) is present and the value is VR,
        // and Numeric Value Qualifier Code Sequence (0040,A301) is not present."
        C::SelectorDSValue_e13c3c => numeric_cell(item, "DS"),
        C::SelectorFDValue_3dcd6f => numeric_cell(item, "FD"),
        C::SelectorFLValue_99138e => numeric_cell(item, "FL"),
        C::SelectorISValue_eb854f => numeric_cell(item, "IS"),
        C::SelectorSLValue_ee2837 => numeric_cell(item, "SL"),
        C::SelectorSSValue_43d1ea => numeric_cell(item, "SS"),
        C::SelectorSVValue_88fa3d => numeric_cell(item, "SV"),
        C::SelectorULValue_6707ef => numeric_cell(item, "UL"),
        C::SelectorUSValue_e61eaf => numeric_cell(item, "US"),
        C::SelectorUVValue_0bfd57 => numeric_cell(item, "UV"),
        // "Required if Selector Attribute VR (0072,0050) is present and the value is DT
        // (UC, SQ)."
        C::SelectorDTValue_01484a => required_if(
            value_in(item, tags::SELECTOR_ATTRIBUTE_VR, 1, &["DT"]),
            NotPresent,
        ),
        C::SelectorUCValue_7e35c8 => required_if(
            value_in(item, tags::SELECTOR_ATTRIBUTE_VR, 1, &["UC"]),
            NotPresent,
        ),
        C::ConceptCodeSequence_9c9892 => required_if(
            value_in(item, tags::SELECTOR_ATTRIBUTE_VR, 1, &["SQ"]),
            NotPresent,
        ),
        // "Required if this cell is a numeric value (Selector Attribute VR is DS, ...) has
        // a unit (is not dimensionless), and the unit is not specified within the Table
        // Row Definition Sequence for this row or Table Column Definition Sequence for
        // this column." Whether it has a unit is not encoded apart from its units.
        C::MeasurementUnitsCodeSequence_685e98 => required_if(
            value_in(item, tags::SELECTOR_ATTRIBUTE_VR, 1, &NUMERIC_CELL_VRS).and(Tri::Unknown),
            NotPresent,
        ),
        // "Required if this cell is a numeric value (...), and the numeric value cannot
        // be provided": a numeric cell without its Selector Value.
        C::NumericValueQualifierCodeSequence_21892a => {
            let missing = string_value(item, tags::SELECTOR_ATTRIBUTE_VR, 1)
                .and_then(|vr| selector_value_tag(&vr))
                .map(|tag| Tri::from(!has(item, tag)));
            required_if(missing.unwrap_or(Tri::False), NotPresent)
        }
    }
}

/// The VRs of the numeric cells of a Table Content Item (Table C.18.10-1).
const NUMERIC_CELL_VRS: [&str; 10] = ["DS", "FD", "FL", "IS", "SL", "SS", "SV", "UL", "US", "UV"];

/// The Selector Value Attribute of a cell of `vr`.
fn selector_value_tag(vr: &str) -> Option<Tag> {
    Some(match vr {
        "DS" => tags::SELECTOR_DS_VALUE,
        "FD" => tags::SELECTOR_FD_VALUE,
        "FL" => tags::SELECTOR_FL_VALUE,
        "IS" => tags::SELECTOR_IS_VALUE,
        "SL" => tags::SELECTOR_SL_VALUE,
        "SS" => tags::SELECTOR_SS_VALUE,
        "SV" => tags::SELECTOR_SV_VALUE,
        "UL" => tags::SELECTOR_UL_VALUE,
        "US" => tags::SELECTOR_US_VALUE,
        "UV" => tags::SELECTOR_UV_VALUE,
        _ => return None,
    })
}

/// A numeric cell's Selector Value: required for its VR unless Numeric Value
/// Qualifier Code Sequence gives why the value is absent.
fn numeric_cell(item: &InMemDicomObject, vr: &str) -> Requirement {
    required_if(
        value_in(item, tags::SELECTOR_ATTRIBUTE_VR, 1, &[vr])
            .and(absent(item, tags::NUMERIC_VALUE_QUALIFIER_CODE_SEQUENCE)),
        NotPresent,
    )
}

/// Whether the Sequence holding the Item in `scope` has more than one Item.
fn sibling_items(scope: &Scope<'_>, sequence: Tag) -> Tri {
    scope.parent_item().map_or(Tri::Unknown, |parent| {
        (items(parent, sequence).len() > 1).into()
    })
}

/// Whether Value `value` is among the Values of `tag`.
fn includes_value(item: &InMemDicomObject, tag: Tag, value: &str) -> Tri {
    Tri::from(
        item.get(tag)
            .and_then(|element| element.to_multi_str().ok())
            .is_some_and(|values| values.iter().any(|own| own.trim() == value)),
    )
}

/// Specific Character Set of an Item that holds its own Keys: required when
/// a text Value uses a character outside the default repertoire; a present
/// Value is not judged, as for the SOP Common Module.
fn item_character_set(item: &InMemDicomObject) -> Requirement {
    if has(item, tags::SPECIFIC_CHARACTER_SET) {
        Requirement::Undeterminable
    } else {
        required_if(non_default_characters(item).into(), NotPresent)
    }
}

/// Whether a Content Item of the SR Document Content references an Instance.
fn content_tree_references(root: &InMemDicomObject) -> bool {
    items(root, tags::CONTENT_SEQUENCE)
        .iter()
        .any(|item| contains_tag(item, tags::REFERENCED_SOP_INSTANCE_UID, 0))
}

/// Whether Current Requested Procedure Evidence Sequence (0040,A375) names
/// Instances of more than one Study.
fn evidence_studies(root: &InMemDicomObject) -> bool {
    let mut studies = items(root, tags::CURRENT_REQUESTED_PROCEDURE_EVIDENCE_SEQUENCE)
        .iter()
        .filter_map(|study| string_value(study, tags::STUDY_INSTANCE_UID, 1))
        .collect::<Vec<_>>();
    studies.sort();
    studies.dedup();
    studies.len() > 1
}

/// Whether a Directory Record references a SOP Instance: a record that names
/// one does; PATIENT, STUDY and SERIES records do not; every other Standard
/// Directory Record "shall be used to reference" a SOP Instance (PS3.3 F.5.4
/// to F.5.49); a PRIVATE record may or may not.
fn references_sop_instance(item: &InMemDicomObject) -> Tri {
    let record_type = string_value(item, tags::DIRECTORY_RECORD_TYPE, 1);
    match record_type.as_deref() {
        Some("PATIENT" | "STUDY" | "SERIES") => Tri::False,
        Some("PRIVATE") | None => {
            let names = [
                tags::REFERENCED_FILE_ID,
                tags::REFERENCED_SOP_CLASS_UID_IN_FILE,
                tags::REFERENCED_SOP_INSTANCE_UID_IN_FILE,
                tags::REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE,
            ]
            .into_iter()
            .any(|tag| has(item, tag));
            if names {
                Tri::True
            } else {
                Tri::Unknown
            }
        }
        Some(_) => Tri::True,
    }
}

/// Condition of Image Orientation (Slide) (0048,0102) in Table C.8.12.14-1.
fn image_orientation_slide_required(scope: &Scope<'_>) -> Tri {
    Tri::from(
        scope
            .all_functional_group_items()
            .any(|group| has(group, tags::PLANE_POSITION_SLIDE_SEQUENCE)),
    )
    .or(scope.dimension_organization_type_is("TILED_FULL"))
}

/// Whether a conditional Enumerated Values list applies.
fn applies(condition: Tri) -> Requirement {
    required_if(condition, MayBePresent)
}

/// Homo sapiens in Patient Species Code Sequence (CID 7454), with its legacy
/// SNOMED-RT and NCBI Taxonomy identifiers.
const HOMO_SAPIENS: [(&str, &str); 3] = [
    ("SCT", "337915000"),
    ("SRT", "L-85B00"),
    ("NCBITaxon", "9606"),
];

/// Whether the Patient is a non-human organism, as its Patient Species Code
/// Sequence states: Homo sapiens is human and any other species is not.
/// Without a coded species nothing encoded states it, since a Patient Species
/// Description is free text.
fn non_human(root: &InMemDicomObject) -> Tri {
    let Some(species) = items(root, tags::PATIENT_SPECIES_CODE_SEQUENCE).first() else {
        return Tri::Unknown;
    };
    match (
        string_value(species, tags::CODING_SCHEME_DESIGNATOR, 1),
        code_identifier(species),
    ) {
        (Some(scheme), Some(value)) => {
            Tri::from(!HOMO_SAPIENS.iter().any(|(human_scheme, human_value)| {
                *human_scheme == scheme && *human_value == value
            }))
        }
        _ => Tri::Unknown,
    }
}

fn value_type(item: &InMemDicomObject, values: &[&str]) -> Requirement {
    required_if(value_in(item, tags::VALUE_TYPE, 1, values), NotPresent)
}

fn retrieval(item: &InMemDicomObject, own: Tag) -> Requirement {
    let others_absent = [
        tags::DICOM_RETRIEVAL_SEQUENCE,
        tags::DICOM_MEDIA_RETRIEVAL_SEQUENCE,
        tags::WADO_RETRIEVAL_SEQUENCE,
        tags::XDS_RETRIEVAL_SEQUENCE,
        tags::WADORS_RETRIEVAL_SEQUENCE,
    ]
    .into_iter()
    .filter(|tag| *tag != own)
    .all(|tag| !has(item, tag));
    required_if(others_absent.into(), MayBePresent)
}

/// The coded entry's identifier, from whichever of the three Attributes holds it.
fn code_identifier(item: &InMemDicomObject) -> Option<String> {
    [
        tags::CODE_VALUE,
        tags::LONG_CODE_VALUE,
        tags::URN_CODE_VALUE,
    ]
    .into_iter()
    .find_map(|tag| string_value(item, tag, 1))
}

pub(super) fn is_urn_or_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("urn:") || lower.contains("://")
}

/// "the code value is a URN or URL"; unknown when no identifier is encoded.
fn code_value_is_urn(item: &InMemDicomObject) -> Tri {
    code_identifier(item).map_or(Tri::Unknown, |value| is_urn_or_url(&value).into())
}

/// Code Value condition. Long Code Value and URN Code Value partition the other
/// code values, so with no identifier encoded at all the Code Value is reported.
fn code_value_fits_short_form(item: &InMemDicomObject) -> Tri {
    match code_identifier(item) {
        Some(value) => Tri::from(value.chars().count() <= 16 && !is_urn_or_url(&value)),
        None if has(item, tags::LONG_CODE_VALUE) || has(item, tags::URN_CODE_VALUE) => Tri::Unknown,
        None => Tri::True,
    }
}

/// Whether the first Item of `sequence` is the coded concept (`value`, `scheme`).
fn code_is(item: &InMemDicomObject, sequence: Tag, value: &str, scheme: &str) -> Tri {
    Tri::from(items(item, sequence).first().is_some_and(|code| {
        string_value(code, tags::CODE_VALUE, 1).as_deref() == Some(value)
            && string_value(code, tags::CODING_SCHEME_DESIGNATOR, 1).as_deref() == Some(scheme)
    }))
}

fn any_laterality(scope: &Scope<'_>) -> bool {
    has(scope.root, tags::IMAGE_LATERALITY)
        || has(scope.root, tags::MEASUREMENT_LATERALITY)
        || scope.all_functional_group_items().any(|group| {
            items(group, tags::FRAME_ANATOMY_SEQUENCE)
                .iter()
                .any(|anatomy| has(anatomy, tags::FRAME_LATERALITY))
        })
}

fn pixel_spacing_specified(scope: &Scope<'_>) -> bool {
    let spacing_tags = [
        tags::PIXEL_SPACING,
        tags::IMAGER_PIXEL_SPACING,
        tags::NOMINAL_SCANNED_PIXEL_SPACING,
    ];
    spacing_tags.iter().any(|tag| has(scope.item, *tag))
        || (scope.is_top_level()
            && scope.all_functional_group_items().any(|group| {
                items(group, tags::PIXEL_MEASURES_SEQUENCE)
                    .iter()
                    .any(|measures| spacing_tags.iter().any(|tag| has(measures, *tag)))
            }))
}

fn real_world_first_last(scope: &Scope<'_>, double_float: Tag) -> Requirement {
    required_if(
        (has(scope.root, tags::PIXEL_DATA)
            || has(scope.item, tags::REAL_WORLD_VALUE_LUT_DATA)
            || !has(scope.item, double_float))
        .into(),
        NotPresent,
    )
}

/// Whether the Dimension Index Pointer names an Attribute inside a Functional
/// Group Sequence; unknown when the pointed-to Attribute is not encoded at all.
fn pointer_in_functional_group(scope: &Scope<'_>, item: &InMemDicomObject) -> Tri {
    let pointers = tag_values(item, tags::DIMENSION_INDEX_POINTER);
    if pointers.is_empty() {
        return Tri::False;
    }
    let in_group = scope.all_functional_group_items().any(|group| {
        group.iter().any(|element| {
            element.items().is_some_and(|nested| {
                nested
                    .iter()
                    .any(|entry| pointers.iter().any(|pointer| has(entry, *pointer)))
            })
        })
    });
    if in_group {
        Tri::True
    } else if pointers.iter().all(|pointer| has(scope.root, *pointer)) {
        Tri::False
    } else {
        Tri::Unknown
    }
}

fn segmented_palette(item: &InMemDicomObject) -> bool {
    [
        tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ]
    .into_iter()
    .any(|tag| has(item, tag))
}

/// Whether any text Value uses a character outside the default repertoire.
fn non_default_characters(root: &InMemDicomObject) -> bool {
    super::values::any_text(root, |text| !text.is_ascii())
}

fn references_instances(root: &InMemDicomObject) -> bool {
    root.iter().any(|element| {
        let tag = element.header().tag;
        tag != tags::REFERENCED_SERIES_SEQUENCE
            && tag != tags::STUDIES_CONTAINING_OTHER_REFERENCED_INSTANCES_SEQUENCE
            && (tag == tags::REFERENCED_SOP_INSTANCE_UID
                || element.items().is_some_and(|nested| {
                    nested
                        .iter()
                        .any(|entry| contains_tag(entry, tags::REFERENCED_SOP_INSTANCE_UID, 0))
                }))
    })
}

fn contains_tag(object: &InMemDicomObject, target: Tag, depth: usize) -> bool {
    if depth > super::MAX_NESTING {
        return false;
    }
    object.iter().any(|element| {
        element.header().tag == target
            || element.items().is_some_and(|nested| {
                nested
                    .iter()
                    .any(|entry| contains_tag(entry, target, depth + 1))
            })
    })
}

/// A Private Creator is required when a pointer Attribute names a Private
/// Data Element.
fn private_creator_required(item: &InMemDicomObject, pointer: Tag) -> Requirement {
    required_if(
        tag_values(item, pointer).into_iter().any(is_private).into(),
        NotPresent,
    )
}

fn sop_class_in(scope: &Scope<'_>, classes: &[&str]) -> Tri {
    Tri::from(
        scope
            .sop_class()
            .is_some_and(|uid| classes.contains(&uid.as_str())),
    )
}

/// The Segmentation and Ophthalmic alternatives shared by Pixel Spacing and Slice Thickness.
fn other_sop_class_spacing(scope: &Scope<'_>) -> Tri {
    let root = scope.root;
    sop_class_in(scope, &[uids::SEGMENTATION_STORAGE])
        .and(present(root, tags::FRAME_OF_REFERENCE_UID))
        .or(
            sop_class_in(scope, &[uids::OPHTHALMIC_TOMOGRAPHY_IMAGE_STORAGE]).and(value_in(
                root,
                tags::OPHTHALMIC_VOLUMETRIC_PROPERTIES_FLAG,
                1,
                &["YES"],
            )),
        )
        .or(sop_class_in(
            scope,
            &[uids::OPHTHALMIC_OPTICAL_COHERENCE_TOMOGRAPHY_BSCAN_VOLUME_ANALYSIS_STORAGE],
        ))
}

/// Frame Type Value 1 of this Frame is ORIGINAL, Dimension Organization Type is
/// not TILED_FULL, and the SOP Class is none of the excluded classes.
fn original_frame_not_tiled_full(scope: &Scope<'_>) -> Tri {
    let original = scope.frame_groups().find_map(|group| {
        items(
            group,
            tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
        )
        .first()
        .map(|frame_type| value_in(frame_type, tags::FRAME_TYPE, 1, &["ORIGINAL"]))
    });
    original
        .unwrap_or(Tri::Unknown)
        .and(scope.dimension_organization_type_is("TILED_FULL").not())
        .and(
            sop_class_in(scope, &LEGACY_CONVERTED)
                .or(sop_class_in(scope, &[VL_WSI_SOP_CLASS_UID]))
                .not(),
        )
}

fn functional_mr(scope: &Scope<'_>) -> bool {
    has(scope.root, tags::FUNCTIONAL_MR_SEQUENCE)
        || scope
            .frame_groups()
            .any(|group| has(group, tags::FUNCTIONAL_MR_SEQUENCE))
}

/// Whether Image Orientation (Patient) orients the image, at the top level or in
/// a Plane Orientation (Patient) Functional Group.
fn patient_oriented(scope: &Scope<'_>) -> bool {
    has(scope.root, tags::IMAGE_ORIENTATION_PATIENT)
        || groups_have(scope, tags::PLANE_ORIENTATION_SEQUENCE)
}

/// Whether any Shared or Per-Frame Functional Groups Sequence Item holds `tag`.
fn groups_have(scope: &Scope<'_>, tag: Tag) -> bool {
    scope
        .all_functional_group_items()
        .any(|group| has(group, tag))
}

/// Whether the object locates its Frames in the Slide Coordinate System (PS3.3
/// C.8.12.2.1): Position Reference Indicator SLIDE_CORNER (C.7.4.1.1.2), Image
/// Orientation (Slide), Total Pixel Matrix Origin Sequence or Plane Position (Slide).
fn slide_positioned(scope: &Scope<'_>) -> bool {
    let root = scope.root;
    has(root, tags::IMAGE_ORIENTATION_SLIDE)
        || has(root, tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE)
        || groups_have(scope, tags::PLANE_POSITION_SLIDE_SEQUENCE)
        || value_in(
            root,
            tags::POSITION_REFERENCE_INDICATOR,
            1,
            &["SLIDE_CORNER"],
        ) == Tri::True
}

/// Whether the object locates its Frames in the patient-relative Reference
/// Coordinate System: Plane Position or Orientation (Patient), or Image Position
/// or Orientation (Patient).
fn patient_positioned(scope: &Scope<'_>) -> bool {
    let root = scope.root;
    groups_have(scope, tags::PLANE_POSITION_SEQUENCE)
        || groups_have(scope, tags::PLANE_ORIENTATION_SEQUENCE)
        || has(root, tags::IMAGE_POSITION_PATIENT)
        || has(root, tags::IMAGE_ORIENTATION_PATIENT)
}

/// "the Frame of Reference is defined in the Slide Coordinate System": false
/// without a Frame of Reference UID, and unknown when no position, or positions
/// in both systems, are encoded.
fn slide_frame_of_reference(scope: &Scope<'_>) -> Tri {
    if !has(scope.root, tags::FRAME_OF_REFERENCE_UID) {
        return Tri::False;
    }
    match (slide_positioned(scope), patient_positioned(scope)) {
        (true, false) => Tri::True,
        (false, true) => Tri::False,
        _ => Tri::Unknown,
    }
}

/// "the Frame of Reference is defined in the patient-relative Reference
/// Coordinate System", read as `slide_frame_of_reference` is.
fn patient_frame_of_reference(scope: &Scope<'_>) -> Tri {
    if !has(scope.root, tags::FRAME_OF_REFERENCE_UID) {
        return Tri::False;
    }
    match (patient_positioned(scope), slide_positioned(scope)) {
        (true, false) => Tri::True,
        (false, true) => Tri::False,
        _ => Tri::Unknown,
    }
}

/// Plane Position (Slide) usage of Tables A.51-2 and A.75-2: required when
/// `gate` holds in a Slide Frame of Reference that is not TILED_FULL, permitted
/// otherwise in a Slide Frame of Reference, and forbidden outside one.
fn slide_positions(scope: &Scope<'_>, gate: Tri) -> Requirement {
    match slide_frame_of_reference(scope) {
        Tri::True => required_if(
            gate.and(scope.dimension_organization_type_is("TILED_FULL").not()),
            MayBePresent,
        ),
        Tri::False => Requirement::Forbidden,
        Tri::Unknown => Requirement::Undeterminable,
    }
}

/// Pixel Presentation (0008,9205) COLOR_RANGE of the Parametric Map Image Module.
fn color_range(root: &InMemDicomObject) -> Tri {
    value_in(root, tags::PIXEL_PRESENTATION, 1, &["COLOR_RANGE"])
}

/// Integer pixels: Pixel Data (7FE0,0010) or Pixel Data Provider URL (0028,7FE0).
fn integer_pixels(root: &InMemDicomObject) -> Tri {
    Tri::from(has(root, tags::PIXEL_DATA) || has(root, tags::PIXEL_DATA_PROVIDER_URL))
}

/// Whether the Palette Color Lookup Table Module's tables are encoded.
fn palette_tables_present(root: &InMemDicomObject) -> bool {
    [
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ]
    .into_iter()
    .any(|tag| has(root, tag))
}

fn segmentation_type(root: &InMemDicomObject, values: &[&str]) -> Tri {
    value_in(root, tags::SEGMENTATION_TYPE, 1, values)
}

fn bits_allocated(root: &InMemDicomObject, bits: i64) -> Tri {
    Tri::from(integer_value(root, tags::BITS_ALLOCATED) == Some(bits))
}

/// "equals other than ...": present with Value 1 outside `values`.
fn value_other_than(object: &InMemDicomObject, tag: Tag, values: &[&str]) -> Tri {
    Tri::from(string_value(object, tag, 1).is_some_and(|value| !values.contains(&value.as_str())))
}

/// Image Type (0008,0008) Value 1 is ORIGINAL or MIXED.
fn original_or_mixed(scope: &Scope<'_>) -> Tri {
    scope.image_type_is(1, &["ORIGINAL", "MIXED"])
}

/// Annotation Coordinate Type (006A,0001) of the Microscopy Bulk Simple Annotations.
fn coordinates_3d(root: &InMemDicomObject) -> Tri {
    value_in(root, tags::ANNOTATION_COORDINATE_TYPE, 1, &["3D"])
}

fn coordinates_2d(root: &InMemDicomObject) -> Tri {
    value_in(root, tags::ANNOTATION_COORDINATE_TYPE, 1, &["2D"])
}

/// Whether all points of an Annotation Group lie in one Z plane: Common Z
/// Coordinate Value (006A,0010) says so, otherwise the (X, Y, Z) triplets of
/// Point Coordinates Data or Double Point Coordinates Data show it.
fn common_z(group: &InMemDicomObject) -> Tri {
    if has(group, tags::COMMON_Z_COORDINATE_VALUE) {
        return Tri::True;
    }
    let Some(element) = [
        tags::POINT_COORDINATES_DATA,
        tags::DOUBLE_POINT_COORDINATES_DATA,
    ]
    .into_iter()
    .find_map(|tag| group.get(tag)) else {
        return Tri::Unknown;
    };
    let Ok(values) = element.to_multi_float64() else {
        return Tri::Unknown;
    };
    if values.len() < 3 || !values.len().is_multiple_of(3) {
        return Tri::Unknown;
    }
    let first = values[2].to_bits();
    Tri::from(
        values
            .iter()
            .skip(2)
            .step_by(3)
            .all(|z| z.to_bits() == first),
    )
}

/// Whether a Measurement Values Sequence Item holds fewer Floating Point Values
/// (0066,0125) than its Annotation Group has Annotations (006A,000C).
fn measured_subset(scope: &Scope<'_>) -> Tri {
    let values = scope
        .item
        .get(tags::FLOATING_POINT_VALUES)
        .map(|element| element.value().multiplicity());
    let annotations = scope
        .parent
        .and_then(|measurements| measurements.parent_item())
        .and_then(|group| integer_value(group, tags::NUMBER_OF_ANNOTATIONS));
    match (values, annotations) {
        (Some(values), Some(annotations)) => Tri::from(i64::from(values) < annotations),
        _ => Tri::Unknown,
    }
}

/// The Item of this Frame's Functional Groups that holds its Frame Type (0008,9007).
fn frame_type_item<'a>(scope: &Scope<'a>) -> Option<&'a InMemDicomObject> {
    scope.frame_groups().find_map(|group| {
        group
            .iter()
            .filter_map(|element| element.items())
            .flatten()
            .find(|item| has(item, tags::FRAME_TYPE))
    })
}

/// "Frame Type (0008,9007) Value 1 of this Frame is ORIGINAL and Volumetric
/// Properties (0008,9206) of this Frame is other than DISTORTED"; unknown when
/// no Frame Type of this Frame is encoded.
fn original_undistorted_frame(scope: &Scope<'_>) -> Tri {
    frame_type_item(scope).map_or(Tri::Unknown, |frame_type| {
        value_in(frame_type, tags::FRAME_TYPE, 1, &["ORIGINAL"])
            .and(value_in(frame_type, tags::VOLUMETRIC_PROPERTIES, 1, &["DISTORTED"]).not())
    })
}

/// The Dimension Index Pointer (0020,9165) Values of the Dimension Index Sequence.
fn dimension_pointers(root: &InMemDicomObject) -> Vec<Tag> {
    items(root, tags::DIMENSION_INDEX_SEQUENCE)
        .iter()
        .flat_map(|index| tag_values(index, tags::DIMENSION_INDEX_POINTER))
        .collect()
}

/// Whether the Contrast/Bolus Agent that this Item's Contrast/Bolus Agent Number
/// (0018,9337) names is administered by the intravenous route (47625008, SCT);
/// unknown when the Contrast/Bolus Agent Sequence (0018,0012) does not list it.
fn intravenous_agent(scope: &Scope<'_>) -> Tri {
    let Some(number) = integer_value(scope.item, tags::CONTRAST_BOLUS_AGENT_NUMBER) else {
        return Tri::Unknown;
    };
    items(scope.root, tags::CONTRAST_BOLUS_AGENT_SEQUENCE)
        .iter()
        .find(|agent| integer_value(agent, tags::CONTRAST_BOLUS_AGENT_NUMBER) == Some(number))
        .map_or(Tri::Unknown, |agent| {
            code_is(
                agent,
                tags::CONTRAST_BOLUS_ADMINISTRATION_ROUTE_SEQUENCE,
                "47625008",
                "SCT",
            )
        })
}
