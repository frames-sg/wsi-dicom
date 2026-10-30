//! Specific Character Set (0008,0005) as encoded: its Defined Terms (PS3.3
//! C.12.1.1.2 Tables C.12-2 to C.12-5) and the repertoire they put in force
//! for the text of a Data Set (PS3.5 6.1.2.5.4).

use dicom_dictionary_std::tags;

use crate::validation::disclosure::shown;
use crate::validation::iod::Family;

/// A graphic character set that Specific Character Set can designate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Set {
    Ir6,
    Ir13,
    Ir14,
    Ir58,
    Ir87,
    Ir100,
    Ir101,
    Ir109,
    Ir110,
    Ir126,
    Ir127,
    Ir138,
    Ir144,
    Ir148,
    Ir149,
    Ir159,
    Ir166,
    Ir203,
}

/// The code element a set is designated to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Element {
    G0,
    G1,
}

/// Tables C.12-3 and C.12-4: each set's designating escape sequence, as the
/// bytes after ESC.
const DESIGNATIONS: &[(Set, &[u8])] = &[
    (Set::Ir6, b"(B"),
    (Set::Ir14, b"(J"),
    (Set::Ir13, b")I"),
    (Set::Ir100, b"-A"),
    (Set::Ir101, b"-B"),
    (Set::Ir109, b"-C"),
    (Set::Ir110, b"-D"),
    (Set::Ir144, b"-L"),
    (Set::Ir127, b"-G"),
    (Set::Ir126, b"-F"),
    (Set::Ir138, b"-H"),
    (Set::Ir148, b"-M"),
    (Set::Ir203, b"-b"),
    (Set::Ir166, b"-T"),
    (Set::Ir87, b"$B"),
    (Set::Ir159, b"$(D"),
    (Set::Ir149, b"$)C"),
    (Set::Ir58, b"$)A"),
];

impl Set {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Ir6 => "ISO-IR 6",
            Self::Ir13 => "ISO-IR 13",
            Self::Ir14 => "ISO-IR 14",
            Self::Ir58 => "ISO-IR 58",
            Self::Ir87 => "ISO-IR 87",
            Self::Ir100 => "ISO-IR 100",
            Self::Ir101 => "ISO-IR 101",
            Self::Ir109 => "ISO-IR 109",
            Self::Ir110 => "ISO-IR 110",
            Self::Ir126 => "ISO-IR 126",
            Self::Ir127 => "ISO-IR 127",
            Self::Ir138 => "ISO-IR 138",
            Self::Ir144 => "ISO-IR 144",
            Self::Ir148 => "ISO-IR 148",
            Self::Ir149 => "ISO-IR 149",
            Self::Ir159 => "ISO-IR 159",
            Self::Ir166 => "ISO-IR 166",
            Self::Ir203 => "ISO-IR 203",
        }
    }

    pub(super) fn element(self) -> Element {
        match self {
            Self::Ir6 | Self::Ir14 | Self::Ir87 | Self::Ir159 => Element::G0,
            _ => Element::G1,
        }
    }

    /// The 94x94 sets of Table C.12-4 encode each character in two bytes.
    pub(super) fn double_byte(self) -> bool {
        matches!(self, Self::Ir58 | Self::Ir87 | Self::Ir149 | Self::Ir159)
    }

    /// The set an escape sequence (the bytes after ESC) designates.
    pub(super) fn designated_by(sequence: &[u8]) -> Option<Self> {
        DESIGNATIONS
            .iter()
            .find(|(_, escape)| *escape == sequence)
            .map(|(set, _)| *set)
    }
}

/// One Value of Specific Character Set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Term {
    /// An empty Value 1, or "ISO_IR 6": the Default Character Repertoire.
    Default,
    /// Tables C.12-2 to C.12-4: the G0 and G1 sets of a term, either absent.
    Iso2022 {
        g0: Option<Set>,
        g1: Option<Set>,
    },
    /// Table C.12-5: "ISO_IR 192", "GB18030" or "GBK", which prohibit Code
    /// Extension.
    Unextendable(Unextendable),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unextendable {
    Utf8,
    Gb18030,
    Gbk,
}

impl Unextendable {
    pub(super) fn term(self) -> &'static str {
        match self {
            Self::Utf8 => "ISO_IR 192",
            Self::Gb18030 => "GB18030",
            Self::Gbk => "GBK",
        }
    }
}

fn term(value: &str) -> Term {
    let single = |g1| Term::Iso2022 {
        g0: Some(Set::Ir6),
        g1: Some(g1),
    };
    let number = value
        .strip_prefix("ISO_IR ")
        .or_else(|| value.strip_prefix("ISO 2022 IR "));
    match (value, number) {
        ("", _) | ("ISO_IR 6", _) => Term::Default,
        ("ISO 2022 IR 6", _) => Term::Iso2022 {
            g0: Some(Set::Ir6),
            g1: None,
        },
        ("ISO_IR 192", _) => Term::Unextendable(Unextendable::Utf8),
        ("GB18030", _) => Term::Unextendable(Unextendable::Gb18030),
        ("GBK", _) => Term::Unextendable(Unextendable::Gbk),
        ("ISO 2022 IR 87", _) => Term::Iso2022 {
            g0: Some(Set::Ir87),
            g1: None,
        },
        ("ISO 2022 IR 159", _) => Term::Iso2022 {
            g0: Some(Set::Ir159),
            g1: None,
        },
        ("ISO 2022 IR 149", _) => Term::Iso2022 {
            g0: None,
            g1: Some(Set::Ir149),
        },
        ("ISO 2022 IR 58", _) => Term::Iso2022 {
            g0: None,
            g1: Some(Set::Ir58),
        },
        (_, Some("13")) => Term::Iso2022 {
            g0: Some(Set::Ir14),
            g1: Some(Set::Ir13),
        },
        (_, Some(number)) => match number {
            "100" => single(Set::Ir100),
            "101" => single(Set::Ir101),
            "109" => single(Set::Ir109),
            "110" => single(Set::Ir110),
            "126" => single(Set::Ir126),
            "127" => single(Set::Ir127),
            "138" => single(Set::Ir138),
            "144" => single(Set::Ir144),
            "148" => single(Set::Ir148),
            "166" => single(Set::Ir166),
            "203" => single(Set::Ir203),
            _ => Term::Unknown,
        },
        _ => Term::Unknown,
    }
}

/// The repertoire Specific Character Set puts in force for a Data Set's
/// SH, LO, UC, ST, LT, UT and PN Values (PS3.5 6.1.2.5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Repertoire {
    /// Specific Character Set absent: ISO-IR 6, 7-bit, without Code Extension.
    Absent,
    /// A single Value: one 8-bit code without Code Extension.
    Single {
        term: String,
        g0: Set,
        g1: Option<Set>,
    },
    /// Several Values: ISO 2022 Code Extension among the `declared` sets,
    /// starting from the Value 1 designations.
    Extended {
        g0: Set,
        g1: Option<Set>,
        declared: Vec<Set>,
    },
    /// "ISO_IR 192", "GB18030" or "GBK" as the single Value.
    Unextendable(Unextendable),
    /// A term outside Tables C.12-2 to C.12-5: the text is not interpreted.
    Unknown,
}

impl Repertoire {
    /// The G0 and G1 sets in force at the start of each Value, line, page
    /// and name component.
    pub(super) fn initial(&self) -> (Set, Option<Set>) {
        match self {
            Self::Single { g0, g1, .. } | Self::Extended { g0, g1, .. } => (*g0, *g1),
            _ => (Set::Ir6, None),
        }
    }
}

/// The Values of a Specific Character Set Value as encoded, without padding.
fn values(raw: &[u8]) -> Vec<String> {
    raw.split(|byte| *byte == b'\\')
        .map(|value| {
            value
                .iter()
                .map(|byte| char::from(*byte))
                .collect::<String>()
                .trim_matches([' ', '\0'])
                .to_string()
        })
        .collect()
}

/// Checks a Specific Character Set Value (PS3.3 C.12.1.1.2) and returns the
/// repertoire it puts in force.
pub(super) fn specific_character_set(raw: &[u8], label: &str, findings: &mut Family) -> Repertoire {
    let values = values(raw);
    if values.iter().all(String::is_empty) {
        return Repertoire::Absent;
    }
    let terms: Vec<Term> = values.iter().map(|value| term(value)).collect();
    for (index, term) in terms.iter().enumerate() {
        if let Term::Unextendable(set) = term {
            if index > 0 || terms.len() > 1 {
                findings.push(|| format!(
                    "{label}: \"{}\" is Value {} of {}; it may only be Value 1 and the only Value (PS3.3 C.12.1.1.2)",
                    set.term(),
                    index + 1,
                    values.len()
                ));
            }
        }
    }
    let mut seen: Vec<(Option<Set>, Option<Unextendable>, usize)> = Vec::new();
    for (index, (term, value)) in terms.iter().zip(&values).enumerate() {
        let identity = match term {
            Term::Default => (Some(Set::Ir6), None),
            Term::Iso2022 { g0, g1 } => (g1.or(*g0), None),
            Term::Unextendable(set) => (None, Some(*set)),
            Term::Unknown => continue,
        };
        if index > 0 && value.is_empty() {
            continue;
        }
        if let Some((_, _, first)) = seen
            .iter()
            .find(|(set, other, _)| (*set, *other) == identity)
        {
            let first = *first;
            findings.push(|| {
                format!(
                    "{label}: Value {} {} names the character set of Value {} {} again; a character set shall not be used more than once (PS3.3 C.12.1.1.2)",
                    index + 1,
                    shown(tags::SPECIFIC_CHARACTER_SET, value),
                    first + 1,
                    shown(tags::SPECIFIC_CHARACTER_SET, &values[first])
                )
            });
        } else {
            seen.push((identity.0, identity.1, index));
        }
    }
    repertoire(&values, &terms)
}

fn repertoire(values: &[String], terms: &[Term]) -> Repertoire {
    if terms.contains(&Term::Unknown) {
        return Repertoire::Unknown;
    }
    // Value 1 is a single-byte code of Table C.12-2 or C.12-3 (or empty);
    // the multi-byte sets of Table C.12-4 extend it from Value 2 on.
    let (g0, g1) = match terms[0] {
        Term::Unextendable(set) => return Repertoire::Unextendable(set),
        Term::Default => (Set::Ir6, None),
        Term::Iso2022 { g0: Some(g0), g1 } if !g0.double_byte() => (g0, g1),
        _ => return Repertoire::Unknown,
    };
    if terms.len() == 1 {
        return Repertoire::Single {
            term: values[0].clone(),
            g0,
            g1,
        };
    }
    let mut declared = Vec::new();
    for term in terms {
        match term {
            Term::Default => declared.push(Set::Ir6),
            Term::Iso2022 { g0, g1 } => declared.extend(g0.iter().chain(g1.iter())),
            // Reported above; the Value 1 repertoire still governs.
            Term::Unextendable(_) | Term::Unknown => {}
        }
    }
    declared.sort();
    declared.dedup();
    Repertoire::Extended { g0, g1, declared }
}
