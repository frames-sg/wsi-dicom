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
use crate::VL_WSI_SOP_CLASS_UID;

use Otherwise::{MayBePresent, NotPresent};

const LEGACY_CONVERTED: [&str; 3] = [
    uids::LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE,
    uids::LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE,
    uids::LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE,
];
/// Excluded from the Frame Reference and Frame Acquisition DateTime conditions.
const LEGACY_CONVERTED_AND_VL_WSI: [&str; 4] = [
    uids::LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE,
    uids::LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE,
    uids::LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE,
    VL_WSI_SOP_CLASS_UID,
];
const JPIP_REFERENCED: [&str; 2] = [uids::JPIP_REFERENCED, uids::JPIP_REFERENCED_DEFLATE];
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
            (has(item, tags::CODE_VALUE) || has(item, tags::LONG_CODE_VALUE)).into(),
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
            Tri::from(!has(item, tags::CODE_VALUE)).and(code_value_is_urn(item).not()),
            NotPresent,
        ),
        // "Shall be present if Code Value is not present and the Code Value is a URN or URL."
        C::URNCodeValue_3efa2e => required_if(
            Tri::from(!has(item, tags::CODE_VALUE)).and(code_value_is_urn(item)),
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
            non_human().and(absent(item, tags::PATIENT_SPECIES_CODE_SEQUENCE)),
            MayBePresent,
        ),
        // "Required if the Patient is a non-human organism and if Patient Species Description
        // is not present. May be present otherwise."
        C::PatientSpeciesCodeSequence_3133a2 => required_if(
            non_human().and(absent(item, tags::PATIENT_SPECIES_DESCRIPTION)),
            MayBePresent,
        ),
        // "Required if the Patient is a non-human organism and if Patient Breed Code Sequence
        // is empty. May be present otherwise."
        C::PatientBreedDescription_2b9079 => required_if(
            non_human().and(
                items(item, tags::PATIENT_BREED_CODE_SEQUENCE)
                    .is_empty()
                    .into(),
            ),
            MayBePresent,
        ),
        // "Required if the Patient is a non-human organism." (Type 2C)
        C::PatientBreedCodeSequence_878016 => required_if(non_human(), NotPresent),
        // "Required if the Patient is a non-human organism. May be present otherwise."
        C::ResponsiblePerson_26bd73 => required_if(non_human(), MayBePresent),
        // "Required if Responsible Person is present and has a Value."
        C::ResponsiblePersonRole_45e655 => {
            required_if(has_value(item, tags::RESPONSIBLE_PERSON).into(), NotPresent)
        }
        // "Required if Patient is a non-human organism. May be present otherwise."
        C::ResponsibleOrganization_a81ee5 => required_if(non_human(), MayBePresent),
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
        C::AnatomicalOrientationType_a42424 => required_if(non_human(), MayBePresent),

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
        // Patient Orientation unnecessary. Table A.32.8-1 includes no Module defining Image
        // Orientation (Patient), so the condition reduces to Image Orientation (Slide) being
        // not required.
        C::PatientOrientation_964c1d => {
            required_if(image_orientation_slide_required(scope).not(), MayBePresent)
        }
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
        // A stored file is encoded in the Transfer Syntax of its File Meta Information.
        C::PixelDataProviderURL_83d79f => required_if(
            JPIP_REFERENCED.contains(&scope.transfer_syntax).into(),
            NotPresent,
        ),
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
        C::DimensionIndexPrivateCreator_252bb2 => required_if(
            tag_values(item, tags::DIMENSION_INDEX_POINTER)
                .into_iter()
                .any(is_private)
                .into(),
            NotPresent,
        ),
        // "Required if the Value of Dimension Index Pointer is the Data Element Tag of an
        // Attribute that is contained within a Functional Group Sequence."
        C::FunctionalGroupPointer_59f16f => {
            required_if(pointer_in_functional_group(scope, item), NotPresent)
        }
        // "Required if the Functional Group Pointer Value is the Data Element Tag of a Private
        // Attribute."
        C::FunctionalGroupPrivateCreator_d93a4b => required_if(
            tag_values(item, tags::FUNCTIONAL_GROUP_POINTER)
                .into_iter()
                .any(is_private)
                .into(),
            NotPresent,
        ),

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
        C::SelectorSequencePointerPrivateCreator_4cb02f => required_if(
            tag_values(item, tags::SELECTOR_SEQUENCE_POINTER)
                .into_iter()
                .any(is_private)
                .into(),
            NotPresent,
        ),
        // "Required if Selector Sequence Pointer is present."
        C::SelectorSequencePointerItems_60e4ce => {
            required_if(present(item, tags::SELECTOR_SEQUENCE_POINTER), NotPresent)
        }
        // "Required if the Selector Attribute Value is the Data Element Tag of a Private
        // Attribute."
        C::SelectorAttributePrivateCreator_83625b => required_if(
            tag_values(item, tags::SELECTOR_ATTRIBUTE)
                .into_iter()
                .any(is_private)
                .into(),
            NotPresent,
        ),

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

        // ---- PS3.10 Table 7.1-1 File Meta Information ----
        // "Required if Private Information Creator UID (0002,0100) is present."
        C::PrivateInformation_5c0ac1 => required_if(
            present(item, tags::PRIVATE_INFORMATION_CREATOR_UID),
            NotPresent,
        ),
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

/// Whether the Patient is a non-human organism: PS3.3 encodes no Attribute that
/// states this (species Attributes may also describe humans).
fn non_human() -> Tri {
    Tri::Unknown
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

fn is_urn_or_url(value: &str) -> bool {
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
        .and(sop_class_in(scope, &LEGACY_CONVERTED_AND_VL_WSI).not())
}

fn functional_mr(scope: &Scope<'_>) -> bool {
    has(scope.root, tags::FUNCTIONAL_MR_SEQUENCE)
        || scope
            .frame_groups()
            .any(|group| has(group, tags::FUNCTIONAL_MR_SEQUENCE))
}
