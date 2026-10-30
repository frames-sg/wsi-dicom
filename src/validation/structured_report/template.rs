//! PS3.16 Template conformance of a Content Tree: TID 2010 at the root of a
//! Key Object Selection Document (PS3.3 A.35.4.3.1.3), and the Template a
//! CONTAINER declares in Content Template Sequence (0040,A504) where the
//! Template tables hold it (e.g. TID 1500). A document that declares no
//! Template, or one the tables do not hold, is reported as such.
//!
//! The walk follows the tree: the Content Items of each Content Sequence are
//! matched to the rows nested under the row their parent matched, an INCLUDE
//! row standing for the included Template's top rows with its Relationship
//! Type. A Content Item matches a row by Relationship Type, by-value or
//! by-reference ("R-" rows), Value Type and Concept Name; one that matches
//! rows of several Templates (a Measurement Group of TID 1410, 1411 or 1501)
//! is assigned to the one whose subtree it departs from least, preferring
//! the Template it declares. Rows are then held to their Requirement Types,
//! the conditions of the forms PS3.16 Templates use (XOR, IF and IFF Row,
//! At least one of Rows, Row value and Concept Name), counts, extensibility,
//! order and the Value Sets of CODE rows. A condition in other words is
//! counted as undeterminable. Template parameters ("$Measurement") admit any
//! Concept Name: the Value Sets INCLUDE rows bind them to are not applied.

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

mod conditions;
mod matcher;

use super::tree::Tree;
use super::{Findings, CITATION, KEY_OBJECT_SELECTION};
use crate::validation::disclosure::shown;
use crate::validation::iod::structured_report::{template, DocumentConstraints, Template};
use crate::validation::iod::{items, string_value};
use matcher::Matcher;

/// The Template a KOS is constructed from at its root (A.35.4.3.1.3).
const KEY_OBJECT_SELECTION_TEMPLATE: u32 = 2010;
/// TID 2010: "no other Content Items may be added to this Template, or the
/// Templates that are included, recursively, except for TID 1600, which may
/// be extended."
const UNSEALED: [u32; 1] = [1600];
/// The Mapping Resource of the DICOM Templates (Table C.18.8-1).
const DCMR: &str = "DCMR";
/// Bound on INCLUDE nesting; the Templates the IODs reach nest five deep.
const MAX_INCLUDE_DEPTH: usize = 8;
/// Bound on row comparisons per document, alternatives included.
const MAX_WORK: usize = 2_000_000;

pub(super) fn check(
    root: &InMemDicomObject,
    tree: &Tree<'_>,
    constraints: &DocumentConstraints,
    findings: &mut Findings,
) {
    let mut matcher = Matcher::new(tree);
    let declared = declaration(root, tree.at(0), findings);
    if constraints.iod == KEY_OBJECT_SELECTION {
        key_object_root(declared.as_ref(), findings);
        if let Some(found) = template(KEY_OBJECT_SELECTION_TEMPLATE) {
            matcher.root(0, found, findings);
        }
    } else {
        match declared {
            None => findings.unexamined(
                "Template conformance: the root CONTAINER declares no Template in Content Template Sequence (0040,A504), so the content is held to the IOD's Content Constraints alone".into(),
            ),
            Some(declared) => match declared.known() {
                Some(found) => matcher.root(0, found, findings),
                None => findings.unexamined(declared.unknown("the root CONTAINER")),
            },
        }
    }
    subtrees(tree, &mut matcher, findings);
    matcher.finish(findings);
}

/// A Content Template Sequence Item: its Mapping Resource and Template
/// Identifier.
struct Declared {
    resource: String,
    identifier: String,
}

impl Declared {
    /// The DICOM Template it names, when the Template tables hold it.
    fn known(&self) -> Option<&'static Template> {
        if self.resource != DCMR {
            return None;
        }
        self.identifier.parse::<u32>().ok().and_then(template)
    }

    fn id(&self) -> Option<u32> {
        (self.resource == DCMR)
            .then(|| self.identifier.parse::<u32>().ok())
            .flatten()
    }

    fn unknown(&self, place: &str) -> String {
        if self.resource == DCMR {
            format!(
                "Template conformance of {place}: it declares TID {}, which is not among the Templates of PS3.16 the IODs reach",
                self.identifier
            )
        } else {
            format!(
                "Template conformance of {place}: it declares Template {} of Mapping Resource {}, which is not a DICOM Template",
                self.identifier, self.resource
            )
        }
    }
}

/// The Template a CONTAINER declares, with C.18.8.1.2: "The DICOM Template
/// Identifier (0040,DB00) is a string of digits, without leading zeroes, and
/// does not include the string \"TID\"."
fn declaration(
    object: &InMemDicomObject,
    at: impl std::fmt::Display,
    findings: &mut Findings,
) -> Option<Declared> {
    let item = items(object, tags::CONTENT_TEMPLATE_SEQUENCE).first()?;
    let declared = Declared {
        resource: string_value(item, tags::MAPPING_RESOURCE, 1)?,
        identifier: string_value(item, tags::TEMPLATE_IDENTIFIER, 1)?,
    };
    let digits = !declared.identifier.is_empty()
        && declared
            .identifier
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        && !(declared.identifier.len() > 1 && declared.identifier.starts_with('0'));
    if declared.resource == DCMR && !digits {
        let identifier = declared.identifier.clone();
        findings.push(|| {
            format!(
                "{at} > Content Template Sequence > Template Identifier (0040,DB00) is {}; the DICOM Template Identifier is a string of digits, without leading zeroes, and does not include the string \"TID\" ({CITATION} C.18.8.1.2)",
                shown(tags::TEMPLATE_IDENTIFIER, &identifier)
            )
        });
    }
    Some(declared)
}

/// A.35.4.3.1.3: "The document shall be constructed from TID 2010 invoked at
/// the root node", which C.18.8.1.2 has the root identify: "If created using
/// such a Template, the Root ... shall identify the Template in the CONTAINER
/// Content Item."
fn key_object_root(declared: Option<&Declared>, findings: &mut Findings) {
    match declared {
        Some(declared) if declared.id() == Some(KEY_OBJECT_SELECTION_TEMPLATE) => {}
        Some(declared) => {
            let (resource, identifier) = (declared.resource.clone(), declared.identifier.clone());
            findings.push(|| {
                format!(
                    "the root CONTAINER's Content Template Sequence (0040,A504) declares Template {identifier} of Mapping Resource {resource}; the document shall be constructed from TID 2010 invoked at the root node ({CITATION} A.35.4.3.1.3)"
                )
            });
        }
        None => findings.push(|| {
            format!(
                "the root CONTAINER has no Content Template Sequence (0040,A504) identifying TID 2010, which the document shall be constructed from at the root node; created using such a Template, the Root shall identify the Template in the CONTAINER Content Item ({CITATION} A.35.4.3.1.3 and C.18.8.1.2)"
            )
        }),
    }
}

/// The CONTAINER items below the root that declare a Template and that the root
/// Template's walk did not reach: each is held to its own.
fn subtrees(tree: &Tree<'_>, matcher: &mut Matcher<'_, '_>, findings: &mut Findings) {
    for index in 1..tree.nodes.len() {
        let node = &tree.nodes[index];
        if node.is_by_reference()
            || node.value_type() != Some("CONTAINER")
            || matcher.matched[index]
        {
            continue;
        }
        let at = tree.at(index);
        let Some(declared) = declaration(node.object, at, findings) else {
            continue;
        };
        match declared.known() {
            Some(found) => matcher.root(index, found, findings),
            None => findings.unexamined(declared.unknown(&at.to_string())),
        }
    }
}

/// Where a walk's departures go: the findings, or a count that compares the
/// alternatives a Content Item matches.
trait Report {
    /// A departure from the Template, an advisory where a Baseline Template
    /// or Value Set binds the row.
    fn departure(&mut self, baseline: bool, message: &dyn Fn() -> String);
    fn advise(&mut self, message: String);
    fn undetermined(&mut self);
    fn unexamined(&mut self, note: String);
    /// Whether Content Items it matches are recorded as reached.
    fn records(&self) -> bool;
}

impl Report for Findings {
    fn departure(&mut self, baseline: bool, message: &dyn Fn() -> String) {
        if baseline {
            self.advise(message());
        } else {
            self.push(message);
        }
    }

    fn advise(&mut self, message: String) {
        Findings::advise(self, message);
    }

    fn undetermined(&mut self) {
        Findings::undetermined(self);
    }

    fn unexamined(&mut self, note: String) {
        Findings::unexamined(self, note);
    }

    fn records(&self) -> bool {
        true
    }
}

/// The departures of an alternative, counted.
#[derive(Default)]
struct Count(usize);

impl Report for Count {
    fn departure(&mut self, baseline: bool, _: &dyn Fn() -> String) {
        if !baseline {
            self.0 += 1;
        }
    }

    fn advise(&mut self, _: String) {}

    fn undetermined(&mut self) {}

    fn unexamined(&mut self, _: String) {}

    fn records(&self) -> bool {
        false
    }
}

/// A coded entry for messages: "(value, scheme, \"meaning\")".
fn code_text(code: &InMemDicomObject) -> String {
    format!(
        "({}, {}, \"{}\")",
        string_value(code, tags::CODE_VALUE, 1)
            .or_else(|| string_value(code, tags::LONG_CODE_VALUE, 1))
            .or_else(|| string_value(code, tags::URN_CODE_VALUE, 1))
            .unwrap_or_default(),
        string_value(code, tags::CODING_SCHEME_DESIGNATOR, 1).unwrap_or_default(),
        string_value(code, tags::CODE_MEANING, 1).unwrap_or_default()
    )
}

/// A Content Item's Value Type and Concept Name for messages.
fn describe(object: &InMemDicomObject, value_type: Option<&str>) -> String {
    let name = items(object, tags::CONCEPT_NAME_CODE_SEQUENCE)
        .first()
        .map_or("without a Concept Name".to_string(), code_text);
    format!("{} {name}", value_type.unwrap_or("of no Value Type"))
}

fn code_is(code: &InMemDicomObject, scheme: Option<&str>, value: &str) -> bool {
    let encoded = string_value(code, tags::CODE_VALUE, 1)
        .or_else(|| string_value(code, tags::LONG_CODE_VALUE, 1))
        .or_else(|| string_value(code, tags::URN_CODE_VALUE, 1));
    encoded.as_deref() == Some(value)
        && scheme.is_none_or(|scheme| {
            string_value(code, tags::CODING_SCHEME_DESIGNATOR, 1).as_deref() == Some(scheme)
        })
}

/// `template(id)`, for the functions whose `template` parameter shadows it.
fn template_of(id: u32) -> Option<&'static Template> {
    template(id)
}

/// "TID 2010 \"Key Object Selection\" ", and the row's Template where it is
/// another.
fn prefix(root: Option<&'static Template>, template: &'static Template) -> String {
    match root {
        Some(root) if std::ptr::eq(root, template) => {
            format!("TID {} \"{}\" ", root.id, root.name)
        }
        Some(root) => format!("TID {} \"{}\" TID {} ", root.id, root.name, template.id),
        None => format!("TID {} ", template.id),
    }
}
