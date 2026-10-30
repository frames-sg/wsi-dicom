//! The Specimen Description Sequence as the profiles read it: each specimen,
//! its preparation steps and their PS3.16 TID 8001 Content Items.

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::facts::{describe_code, is_code, text};
use crate::validation::disclosure::shown;
use crate::validation::iod::items;

/// (111701, DCM, "Processing type").
pub(super) const PROCESSING_TYPE: (&str, &str) = ("DCM", "111701");
/// (121041, DCM, "Specimen Identifier").
pub(super) const SPECIMEN_IDENTIFIER: (&str, &str) = ("DCM", "121041");
/// (111724, DCM, "Issuer of Specimen Identifier").
pub(super) const ISSUER_OF_SPECIMEN_IDENTIFIER: (&str, &str) = ("DCM", "111724");
/// (111702, DCM, "DateTime of processing").
pub(super) const DATETIME_OF_PROCESSING: (&str, &str) = ("DCM", "111702");
/// (17636008, SCT, "Specimen collection"): a Processing type, and the
/// concept of the collection procedure.
pub(super) const SPECIMEN_COLLECTION: (&str, &str) = ("SCT", "17636008");
/// (9265001, SCT, "Specimen processing").
pub(super) const SPECIMEN_PROCESSING: (&str, &str) = ("SCT", "9265001");
/// (127790008, SCT, "Staining").
pub(super) const STAINING: (&str, &str) = ("SCT", "127790008");
/// (430864009, SCT, "Tissue Fixative").
pub(super) const TISSUE_FIXATIVE: (&str, &str) = ("SCT", "430864009");
/// (430863003, SCT, "Embedding medium").
pub(super) const EMBEDDING_MEDIUM: (&str, &str) = ("SCT", "430863003");
/// (424361007, SCT, "Using substance").
pub(super) const USING_SUBSTANCE: (&str, &str) = ("SCT", "424361007");

/// One Item of the Specimen Description Sequence.
pub(super) struct Specimen<'a> {
    /// 1-based position in the Sequence.
    pub(super) number: usize,
    pub(super) item: &'a InMemDicomObject,
    pub(super) identifier: Option<String>,
    /// Local Namespace Entity ID, else Universal Entity ID, of the Issuer of
    /// the Specimen Identifier Sequence.
    pub(super) issuer: Option<String>,
    pub(super) steps: Vec<Step<'a>>,
}

/// One Item of the Specimen Preparation Sequence.
pub(super) struct Step<'a> {
    /// 1-based position in the Sequence.
    pub(super) number: usize,
    pub(super) content: Vec<ContentItem<'a>>,
}

/// One Item of the Specimen Preparation Step Content Item Sequence.
pub(super) struct ContentItem<'a> {
    pub(super) value_type: Option<String>,
    pub(super) concept: Option<&'a InMemDicomObject>,
    pub(super) code: Option<&'a InMemDicomObject>,
    pub(super) text: Option<String>,
    pub(super) datetime: Option<String>,
}

pub(super) fn specimens(root: &InMemDicomObject) -> Vec<Specimen<'_>> {
    items(root, tags::SPECIMEN_DESCRIPTION_SEQUENCE)
        .iter()
        .enumerate()
        .map(|(index, item)| Specimen {
            number: index + 1,
            item,
            identifier: text(item, tags::SPECIMEN_IDENTIFIER),
            issuer: items(item, tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE)
                .first()
                .and_then(|issuer| {
                    text(issuer, tags::LOCAL_NAMESPACE_ENTITY_ID)
                        .or_else(|| text(issuer, tags::UNIVERSAL_ENTITY_ID))
                }),
            steps: items(item, tags::SPECIMEN_PREPARATION_SEQUENCE)
                .iter()
                .enumerate()
                .map(|(index, step)| Step {
                    number: index + 1,
                    content: items(step, tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE)
                        .iter()
                        .map(ContentItem::of)
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

impl<'a> ContentItem<'a> {
    fn of(item: &'a InMemDicomObject) -> Self {
        Self {
            value_type: text(item, tags::VALUE_TYPE),
            concept: items(item, tags::CONCEPT_NAME_CODE_SEQUENCE).first(),
            code: items(item, tags::CONCEPT_CODE_SEQUENCE).first(),
            text: text(item, tags::TEXT_VALUE),
            datetime: text(item, tags::DATE_TIME),
        }
    }

    pub(super) fn is(&self, (scheme, value): (&str, &str)) -> bool {
        self.concept
            .is_some_and(|code| is_code(code, scheme, value))
    }

    fn value_type_is(&self, value_type: &str) -> bool {
        self.value_type.as_deref() == Some(value_type)
    }

    /// The coded value of a CODE Content Item.
    pub(super) fn coded(&self) -> Option<&'a InMemDicomObject> {
        self.code.filter(|_| self.value_type_is("CODE"))
    }
}

impl Step<'_> {
    pub(super) fn find(&self, concept: (&str, &str)) -> Option<&ContentItem<'_>> {
        self.content.iter().find(|item| item.is(concept))
    }

    /// Whether the step's Processing type is the code `processing`.
    pub(super) fn processing_is(&self, processing: (&str, &str)) -> bool {
        self.find(PROCESSING_TYPE)
            .and_then(ContentItem::coded)
            .is_some_and(|code| is_code(code, processing.0, processing.1))
    }

    /// The CODE Content Item `concept`, or what it is instead.
    pub(super) fn coded(&self, concept: (&str, &str)) -> Result<&InMemDicomObject, String> {
        let item = self
            .find(concept)
            .ok_or_else(|| format!("has no ({}, {}) Content Item", concept.1, concept.0))?;
        item.coded().ok_or_else(|| {
            format!(
                "has a ({}, {}) Content Item of Value Type {}, not a coded CODE entry",
                concept.1,
                concept.0,
                item.value_type.as_deref().unwrap_or("(absent)")
            )
        })
    }

    /// "step N (processing type)" for messages.
    pub(super) fn describe(&self) -> String {
        match self.find(PROCESSING_TYPE).and_then(ContentItem::coded) {
            Some(code) => format!(
                "Specimen Preparation Sequence item {} (Processing type {})",
                self.number,
                describe_code(code)
            ),
            None => format!("Specimen Preparation Sequence item {}", self.number),
        }
    }
}

impl Specimen<'_> {
    pub(super) fn describe(&self) -> String {
        match &self.identifier {
            Some(identifier) => format!(
                "Specimen Description Sequence item {} (Specimen Identifier {})",
                self.number,
                shown(tags::SPECIMEN_IDENTIFIER, identifier)
            ),
            None => format!("Specimen Description Sequence item {}", self.number),
        }
    }

    /// The steps whose Processing type is `processing`.
    pub(super) fn steps_of(&self, processing: (&str, &str)) -> Vec<&Step<'_>> {
        self.steps
            .iter()
            .filter(|step| step.processing_is(processing))
            .collect()
    }

    /// The steps that record `concept` as a CODE Content Item.
    pub(super) fn steps_coding(&self, concept: (&str, &str)) -> Vec<&Step<'_>> {
        self.steps
            .iter()
            .filter(|step| step.coded(concept).is_ok())
            .collect()
    }
}
