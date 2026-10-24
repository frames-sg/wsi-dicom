#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only

"""Generate the VL Whole Slide Microscopy IOD requirement tables.

Reads the pinned DICOM DocBook sources (PS3.3 for the IOD, module and macro
tables, PS3.5 and PS3.10 for encoding and file format statements, PS3.6 for the
data dictionary, PS3.16 for Context Groups and Templates) and writes
src/validation/iod/tables.rs, dictionary.rs, requirements.rs and value_sets.rs
deterministically. Every 1C/2C condition, conditional module or functional
group usage, conditional enumeration and Type 3 presence restriction becomes a
ConditionId variant, and every normative sentence the tables do not enforce
becomes a Requirement variant; the validator matches on both enums
exhaustively, so a changed or new condition or statement in another edition
fails to compile until it is handled.

Sources are parsed only after their SHA-256 matches the pinned digest, so the
XML parser never sees unpinned input.
"""

import argparse
import hashlib
import re
import sys
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path


EDITION = "2026d"
SOURCES = {
    "part03.xml": "3ae695b956ba57c490933a49220b515aeaedb5ece86f05c4bf7e94ee0fbcc6ef",
    "part05.xml": "05183ea889f097cba51a938bf961a62f308bc6044a12f24d71282f08469de835",
    "part06.xml": "fa9f0f8d67eba692dcf95cfeae0a8f4e071beeb7844215a860bc4b9d5dfaad25",
    "part10.xml": "4e08e2ef3ad0b57d79cf6903c0076c56b670ef2172afa2f6c42da0792ff645d5",
    "part16.xml": "293912fbc2acefe5d53dffa449070db488134da29aaa452e31d610d403888899",
}
# NEMA serves the newest edition only under current/ until the next one is
# published; the pinned digests decide which download is the edition.
SOURCE_URLS = (
    "https://dicom.nema.org/medical/dicom/{edition}/source/docbook/{part}/{name}",
    "https://dicom.nema.org/medical/dicom/current/source/docbook/{part}/{name}",
)
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT_DIR = REPO_ROOT / "src" / "validation" / "iod"
OUTPUT_FILES = ("tables.rs", "dictionary.rs", "requirements.rs", "value_sets.rs")
IOD_SECTION = "sect_A.32.8"
IOD_MODULE_TABLE = "table_A.32.8-1"
IOD_FUNCTIONAL_GROUP_TABLE = "table_A.32.8-2"
FILE_META_TABLE = "table_7.1-1"
MAX_SOURCE_BYTES = 64 * 1024 * 1024

XML_ID = "{http://www.w3.org/XML/1998/namespace}id"
TAG = re.compile(r"\(([0-9A-Fa-fx]{4}),([0-9A-Fa-fx]{4})\)\Z")
CONDITION_START = re.compile(
    r"\b(?:Required (?:if|when|for|unless)\b|Required:|Shall be present if\b)"
)
TYPE3_RESTRICTION = re.compile(r"\bShall not be present\b[^.]*\.")
ITEM_PATTERNS = (
    ("Only a single Item shall be included in this Sequence.", "Exactly(1)"),
    ("Only a single Item is permitted in this Sequence.", "AtMost(1)"),
    ("Zero or one Item shall be included in this Sequence.", "AtMost(1)"),
    ("One or more Items shall be included in this Sequence.", "AtLeast(1)"),
    (
        "The number of Items shall be the same as the number of Frames in the Multi-frame Image.",
        "NumberOfFrames",
    ),
)
ITEMS_LIKE_VALUE = re.compile(
    r"Shall have the same number of Items as the Value of [^()]+ \(([0-9A-F]{4}),([0-9A-F]{4})\)"
    r" if present, or otherwise the Value of [^()]+ \(([0-9A-F]{4}),([0-9A-F]{4})\)"
)
ENUM_TITLE = re.compile(
    r"Enumerated Values?(?: for Value (?P<value>\d+))?"
    r"(?: for (?P<attribute>[^()]+) \((?P<group>[0-9A-F]{4}),(?P<element>[0-9A-F]{4})\))?"
    r"(?: (?P<condition>(?:if|when) .+))?:\Z"
)
PLACEMENTS = {
    "Shall be used as a Shared Functional Group.": "Placement::SharedOnly",
    "Shall not be used as a Shared Functional Group.": "Placement::NotShared",
}
TYPES = {"1": "One", "1C": "OneC", "2": "Two", "2C": "TwoC", "3": "Three"}
USAGES = {"M": "Usage::Mandatory", "C": "Usage::Conditional", "U": "Usage::UserOption"}
VRS = (
    "AE AS AT CS DA DS DT FD FL IS LO LT OB OD OF OL OV OW PN SH SL SQ SS ST SV TM UC UI UL UN UR"
    " US UT UV"
).split()

# Prose constraints that PS3.3 states outside an Enumerated Values list. Each is
# bound to the exact sentence in the cited section, or in the row's own
# description cell ("cell"); generation fails if the sentence changes so the
# binding is reviewed against the new text.
PROSE_ENUMERATIONS = (
    # (module or macro table, tag, section or "cell", exact sentence, value index, values)
    (
        "table_C.8.12.4-1",
        "(0008,0008)",
        "sect_C.8.12.4.1.1",
        "Value 1 shall have a Value of ORIGINAL or DERIVED",
        1,
        ("ORIGINAL", "DERIVED"),
    ),
    (
        "table_C.8.12.4-1",
        "(0008,0008)",
        "sect_C.8.12.4.1.1",
        "Value 2 shall have a Value of PRIMARY",
        2,
        ("PRIMARY",),
    ),
    (
        "table_C.8.12.9-1",
        "(0008,9007)",
        "cell",
        "Enumerated Values and Defined Terms are the same as those for the four Values of"
        " Image Type (0008,0008).",
        1,
        ("ORIGINAL", "DERIVED"),
    ),
    (
        "table_C.8.12.9-1",
        "(0008,9007)",
        "cell",
        "Enumerated Values and Defined Terms are the same as those for the four Values of"
        " Image Type (0008,0008).",
        2,
        ("PRIMARY",),
    ),
)
# Enumerated Values lists for one Value of a multi-valued Attribute, where the
# Value is named by the paragraph that precedes the list in the cited section.
SECTION_VALUE_ENUMERATIONS = (
    # (module or macro table, tag, section, preceding paragraph, value index)
    (
        "table_C.7-9",
        "(0008,0008)",
        "sect_C.7.6.1.1.2",
        "Value 1 shall identify the Pixel Data Characteristics",
        1,
    ),
    (
        "table_C.7-9",
        "(0008,0008)",
        "sect_C.7.6.1.1.2",
        "Value 2 shall identify the Patient Examination Characteristics",
        2,
    ),
)


# Context Group and Template references (PS3.16 Annexes A and B).
VALUE_SET_TARGET = re.compile(r"sect_(CID|TID)_(\d+)\Z")
# Macros whose inclusion row names the Value Set of the Sequence holding it.
VALUE_SET_MACROS = {"table_8.8-1": "CID", "table_10-2": "TID", "table_10.2.1-1": "TID"}
# Value Sets a table binds, in the row including a Macro, to a Code Sequence the
# Macro defines; generation fails if the row's references change.
INCLUDE_VALUE_SETS = {
    # (including table, Macro table): (Code Sequence tag, strength, Context Groups)
    ("table_C.7-9", "table_10-7"): ("00082218", "B", (4031, 7483)),
    ("table_C.7.6.22-2", "table_10-8"): ("00082228", "B", (8134,)),
    ("table_C.7.6.16-12", "table_C.7.6.16-12b"): ("004008EA", "D", (82,)),
}
# IOD Content Constraints sections that bind Value Sets to Code Sequences of a
# Module or Functional Group Macro named by the section title.
IOD_CONSTRAINT_SECTIONS = ("sect_A.32.8.3.1", "sect_A.32.8.4.1")
# Context Groups that comprise a whole Coding Scheme without a table and
# without naming the Coding Scheme Designator in a sentence (PS3.16 7.2.2).
SCHEME_CONTEXT_GROUPS = {82: "UCUM"}
SCHEME_SENTENCE = re.compile(
    r"Coding Scheme Designator(?: \(0008,0102\))? (?:shall be|is specified as) \"?([A-Za-z0-9_.-]+?)\"?\."
)
CODED_ENTRY = re.compile(r"(EV|DT) \((.+?), (\S+?)(?: \[.*?\])?, \"(.*)\"\)\Z")
TEMPLATE_CONDITION = (
    (re.compile(r"(IFF?) Row (\w+) [\w ]*?value is \((.+?), (\S+?), \".*\"\)\Z", re.I), "value"),
    (re.compile(r"(IFF?) Row (\w+) not present\Z"), "absent"),
)

# Normative statements: every sentence saying "shall" or "must" in the IOD, the
# Module, Macro and Attribute description sections it reaches, the normative
# chapters and annexes of PS3.5, and the file format chapters of PS3.10.
NORMATIVE = re.compile(r"\b(?:shall|must)\b", re.IGNORECASE)
SENTENCE_BREAK = re.compile(r"(?<=[.;])\s+(?=[A-Z(\"“])")
# PS3.3 sections a scope section may cite: code, macro and module definitions.
LINKED_SECTION = re.compile(r"sect_(?:C|8|10)\.")
NORMATIVE_CHAPTERS = {
    "PS3.5": ("chapter_6", "chapter_7", "chapter_8", "chapter_9", "chapter_10", "chapter_A", "chapter_E", "chapter_G"),
    "PS3.10": ("chapter_7", "chapter_8"),
}
# Description-cell sentences the generated tables enforce besides conditions,
# Item counts, Type 3 restrictions and bound prose enumerations.
TABLE_SENTENCES = ("Zero or more Items shall be included in this Sequence.",)


# Fixed constructors of the generated tables.rs, which keep each row on a line.
TABLE_CONSTRUCTORS = """const T1: AttributeType = AttributeType::One;
const T1C: AttributeType = AttributeType::OneC;
const T2: AttributeType = AttributeType::Two;
const T2C: AttributeType = AttributeType::TwoC;
const T3: AttributeType = AttributeType::Three;

const fn t(
    part: &'static str,
    label: &'static str,
    section: &'static str,
    nodes: &'static [Node],
) -> Table {
    Table { part, label, section, nodes }
}

const fn i(table: &'static Table) -> Node {
    Node::Include(table, &[])
}

const fn ib(table: &'static Table, value_sets: &'static [(u32, ValueSet)]) -> Node {
    Node::Include(table, value_sets)
}

#[allow(clippy::too_many_arguments)]
const fn a(
    tag: u32,
    name: &'static str,
    kind: AttributeType,
    condition: Option<ConditionId>,
    enumerations: &'static [Enumeration],
    items: ItemCount,
    value_set: Option<ValueSet>,
    children: &'static [Node],
) -> Node {
    Node::Attribute(Attribute { tag, name, kind, condition, enumerations, items, value_set, children })
}

const fn e(
    value: Option<u8>,
    condition: Option<ConditionId>,
    values: &'static [&'static str],
) -> Enumeration {
    Enumeration { value, condition, values }
}"""


class GenerationError(Exception):
    pass


def arguments():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--source-dir",
        type=Path,
        required=True,
        help="directory holding the pinned " + ", ".join(SOURCES),
    )
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if the checked-in outputs differ from a fresh generation",
    )
    parser.add_argument(
        "--fetch",
        action="store_true",
        help=f"download missing sources for edition {EDITION} into --source-dir",
    )
    return parser.parse_args()


def fetch_source(directory, name):
    part = name.removesuffix(".xml")
    failures = []
    for template in SOURCE_URLS:
        url = template.format(edition=EDITION, part=part, name=name)
        try:
            with urllib.request.urlopen(url, timeout=120) as response:
                body = response.read(MAX_SOURCE_BYTES + 1)
        except urllib.error.URLError as error:
            failures.append(f"{url}: {error}")
            continue
        if len(body) > MAX_SOURCE_BYTES:
            failures.append(f"{url} exceeds {MAX_SOURCE_BYTES} bytes")
        elif hashlib.sha256(body).hexdigest() != SOURCES[name]:
            failures.append(f"{url} does not match the pinned {EDITION} SHA-256 for {name}")
        else:
            directory.mkdir(parents=True, exist_ok=True)
            (directory / name).write_bytes(body)
            return
    raise GenerationError("; ".join(failures))


def verified_sources(directory, fetch):
    """The bytes of each pinned source, read once so that what is parsed is
    what was verified."""
    sources = {}
    for name, expected in SOURCES.items():
        path = directory / name
        if not path.is_file() and fetch:
            fetch_source(directory, name)
        if not path.is_file():
            raise GenerationError(f"missing source {path}; pass --fetch to download it")
        if path.stat().st_size > MAX_SOURCE_BYTES:
            raise GenerationError(f"{path} exceeds {MAX_SOURCE_BYTES} bytes")
        body = path.read_bytes()
        actual = hashlib.sha256(body).hexdigest()
        if actual != expected:
            raise GenerationError(
                f"{path} has SHA-256 {actual}, expected pinned {EDITION} digest {expected}"
            )
        sources[name] = body
    return sources


class Document:
    """A DocBook part with id lookup and text rendering."""

    def __init__(self, source):
        self.root = ET.fromstring(source)
        self.ns = self.root.tag.split("}")[0] + "}"
        self.ids = {}
        self.parents = {}
        for parent in self.root.iter():
            for child in parent:
                self.parents[child] = parent
            identifier = parent.get(XML_ID)
            if identifier:
                self.ids[identifier] = parent

    def q(self, name):
        return self.ns + name

    def element(self, identifier):
        element = self.ids.get(identifier)
        if element is None:
            raise GenerationError(f"missing DocBook element {identifier}")
        return element

    def reference_label(self, identifier):
        target = self.ids.get(identifier)
        if target is None:
            return f"[{identifier}]"
        label = target.get("label")
        kind = target.tag.removeprefix(self.ns)
        if label and kind in ("section", "chapter", "appendix"):
            return f"Section {label}"
        if label and kind == "table":
            return f"Table {label}"
        if label and kind == "figure":
            return f"Figure {label}"
        return f"[{identifier}]"

    def render(self, element, skip_notes=True):
        parts = []

        def walk(node):
            name = node.tag.removeprefix(self.ns)
            if skip_notes and name == "note":
                return
            if name == "xref":
                parts.append(self.reference_label(node.get("linkend")))
                return
            if name == "olink":
                inner = "".join(node.itertext()).strip()
                parts.append(inner or olink_label(node.get("targetdoc"), node.get("targetptr")))
                return
            if node.text:
                parts.append(node.text)
            for child in node:
                walk(child)
                if child.tail:
                    parts.append(child.tail)

        walk(element)
        return normalize("".join(parts))

    def section_label(self, element):
        node = element
        while node is not None:
            if node.tag == self.q("section") and node.get("label"):
                return node.get("label")
            node = self.parents.get(node)
        raise GenerationError(f"element {element.get(XML_ID)} is outside a labelled section")


def olink_label(document, target):
    """'CID 8101', 'TID 8001', 'PS3.5 Section 8.2' or 'PS3.5 Table 8-1' for a
    reference into another Part."""
    match = VALUE_SET_TARGET.match(target or "")
    if match:
        return f"{match.group(1)} {match.group(2)}"
    for prefix, kind in (("sect_", "Section"), ("table_", "Table")):
        if (target or "").startswith(prefix):
            return f"{document} {kind} {target.removeprefix(prefix)}"
    return f"{document} {target}"


def normalize(text):
    return re.sub(r"\s+", " ", text.replace("\u200b", "")).strip()


def cells(document, row):
    return [cell for cell in row if cell.tag in (document.q("td"), document.q("th"))]


def links(cell):
    return [node.get("linkend") for node in cell.iter() if node.get("linkend")]


def first_table(document, identifier):
    element = document.element(identifier)
    if element.tag == document.q("table"):
        return element
    table = next(element.iter(document.q("table")), None)
    if table is None:
        raise GenerationError(f"{identifier} contains no table")
    return table


def parse_tag(text):
    match = TAG.match(text)
    if not match:
        return None
    group, element = match.groups()
    return f"{group.upper()}{element.upper()}".replace("X", "x")


def tag_display(tag):
    return f"({tag[:4]},{tag[4:]})"


def description_blocks(document, cell):
    """Return (kind, element) for each normative block of a description cell."""
    blocks = []
    for child in cell:
        name = child.tag.removeprefix(document.ns)
        if name == "note":
            continue
        blocks.append((name, child))
    return blocks


def condition_text(document, cell, where):
    blocks = description_blocks(document, cell)
    for index, (name, block) in enumerate(blocks):
        if name != "para":
            continue
        text = document.render(block)
        match = CONDITION_START.search(text)
        if not match:
            continue
        condition = [text[match.start():]]
        for follow_name, follow in blocks[index + 1:]:
            follow_text = document.render(follow)
            if follow_name in ("itemizedlist", "orderedlist") and not condition[-1].endswith("."):
                # The sentence continues into the list ("Required if:",
                # "Required if", "May be present otherwise, if").
                condition.append(follow_text)
            elif follow_name == "para" and re.match(
                r"(May be present|May not be present|Shall not be present|Shall be absent)\b", follow_text
            ):
                condition.append(follow_text)
            else:
                break
        return normalize(" ".join(condition))
    raise GenerationError(f"{where}: conditional Attribute has no condition text")


def item_count(document, cell, where):
    text = " ".join(document.render(block) for _, block in description_blocks(document, cell))
    found = [rust for sentence, rust in ITEM_PATTERNS if sentence in text]
    like_value = ITEMS_LIKE_VALUE.search(text)
    if like_value:
        primary = like_value.group(1) + like_value.group(2)
        fallback = like_value.group(3) + like_value.group(4)
        found.append(f"ValueOf({tag_hex(primary)}, {tag_hex(fallback)})")
    if "NumberOfFrames" in found and "AtLeast(1)" in found:
        # Number of Frames is Type 1 and positive, so it implies one or more.
        found.remove("AtLeast(1)")
    if len(found) > 1:
        raise GenerationError(f"{where}: conflicting Item count constraints {found}")
    return found[0] if found else "ANY"


def enumeration_value(term, where):
    if not term:
        raise GenerationError(f"{where}: empty Enumerated Value term")
    if re.fullmatch(r"[0-9A-F]{4}H", term):
        return str(int(term[:-1], 16))
    return term


def variablelist_enumeration(document, variablelist, tag, where):
    title = variablelist.find(document.q("title"))
    if title is None:
        return None
    title_text = document.render(title)
    if not title_text.startswith("Enumerated Value"):
        return None
    match = ENUM_TITLE.match(title_text)
    if not match:
        raise GenerationError(f"{where}: unrecognized enumeration title {title_text!r}")
    if match.group("group") and match.group("group") + match.group("element") != tag:
        return None
    terms = [
        enumeration_value(normalize("".join(entry.find(document.q("term")).itertext())), where)
        for entry in variablelist.findall(document.q("varlistentry"))
    ]
    if not terms:
        raise GenerationError(f"{where}: Enumerated Values list without terms")
    value = int(match.group("value")) if match.group("value") else None
    return {"value": value, "condition": match.group("condition"), "values": terms}


def cell_enumerations(document, cell, tag, where):
    enumerations = []
    for _, block in description_blocks(document, cell):
        if block.tag == document.q("variablelist"):
            lists = [block]
        else:
            lists = block.iter(document.q("variablelist"))
        for variablelist in lists:
            found = variablelist_enumeration(document, variablelist, tag, where)
            if found:
                enumerations.append(found)
    return enumerations


def section_enumerations(document, cell, tag, name, where):
    """Enumerations in sections referenced from the description cell.

    A list binds to the Attribute when its title names the Attribute's tag, or
    when the section is titled with the Attribute name and holds exactly one
    unqualified Enumerated Values list. A section the cell cites "for Defined
    Terms" is not bound, whatever its lists are titled.
    """
    enumerations = []
    cell_text = document.render(cell)
    for identifier in links(cell):
        if not identifier.startswith("sect_"):
            continue
        section = document.ids.get(identifier)
        if section is None:
            continue
        if f"{document.reference_label(identifier)} for Defined Terms" in cell_text:
            continue
        title = section.find(document.q("title"))
        section_title = document.render(title) if title is not None else ""
        named, unqualified = [], []
        for variablelist in section.iter(document.q("variablelist")):
            list_title = variablelist.find(document.q("title"))
            if list_title is None:
                continue
            match = ENUM_TITLE.match(document.render(list_title))
            if not match:
                continue
            found = variablelist_enumeration(document, variablelist, tag, where)
            if not found:
                continue
            if match.group("group"):
                named.append(found)
            elif match.group("value") is None and match.group("condition") is None:
                unqualified.append(found)
        enumerations.extend(named)
        if section_title == name and len(unqualified) == 1:
            enumerations.extend(unqualified)
    return enumerations


def bound_section_enumerations(document, table_id, tag, cell):
    enumerations = []
    for bound_table, bound_tag, section_id, sentence, value, values in PROSE_ENUMERATIONS:
        if bound_table != table_id or bound_tag != tag_display(tag):
            continue
        if section_id == "cell":
            found = sentence in document.render(cell)
        else:
            section = document.element(section_id)
            found = sentence in [document.render(para) for para in section.iter(document.q("para"))]
        if not found:
            raise GenerationError(f"{table_id} {section_id}: bound sentence {sentence!r} not found")
        enumerations.append({"value": value, "condition": None, "values": list(values)})
    for bound_table, bound_tag, section_id, preceding, value in SECTION_VALUE_ENUMERATIONS:
        if bound_table != table_id or bound_tag != tag_display(tag):
            continue
        section = document.element(section_id)
        found = None
        for container in section.iter():
            children = list(container)
            for index, child in enumerate(children[:-1]):
                if child.tag == document.q("para") and document.render(child) == preceding:
                    following = children[index + 1]
                    if following.tag == document.q("variablelist"):
                        found = variablelist_enumeration(
                            document, following, tag, f"{section_id} Value {value}"
                        )
        if not found or found["value"] is not None:
            raise GenerationError(f"{section_id}: no Enumerated Values after {preceding!r}")
        enumerations.append({"value": value, "condition": None, "values": found["values"]})
    return enumerations


def value_set_references(document, element):
    """(strength, kind, number) of each Context Group or Template an element
    cites, strength "B" (Baseline), "D" (Defined) or "" from the letter before
    the reference."""
    found = []
    for parent in element.iter():
        children = list(parent)
        for index, child in enumerate(children):
            target = child.get("targetptr") or child.get("linkend") or ""
            match = VALUE_SET_TARGET.match(target)
            if not match or child.tag not in (document.q("olink"), document.q("xref")):
                continue
            before = parent.text if index == 0 else children[index - 1].tail
            strength = re.search(r"([BD]?)\s*\Z", before or "").group(1)
            found.append((strength, match.group(1), int(match.group(2))))
    return found


def value_set(references, kind, where):
    strengths = {strength for strength, _, _ in references}
    if strengths - {"B", "D"} or len(strengths) != 1 or any(found != kind for _, found, _ in references):
        raise GenerationError(f"{where}: expected one {kind} constraint, found {references}")
    return {"kind": kind, "strength": strengths.pop(), "ids": [number for _, _, number in references]}


def bind_value_sets(record):
    """Moves the Value Set cited by a Code Sequence or Content Item Macro
    inclusion onto the Sequence that holds it, and binds the Value Sets cited by
    other inclusions to the Code Sequence the Macro defines."""
    rows = record["rows"]
    for index, row in enumerate(rows):
        if row["kind"] != "include" or not row["references"]:
            continue
        where = f"{record['id']} include {row['target']}"
        kind = VALUE_SET_MACROS.get(row["target"])
        if kind is None:
            bound = INCLUDE_VALUE_SETS.get((record["id"], row["target"]))
            if bound is None:
                raise GenerationError(f"{where}: unbound Value Set {row['references']}")
            tag, strength, ids = bound
            if row["references"] != [(strength, "CID", number) for number in ids]:
                raise GenerationError(f"{where}: Value Sets changed to {row['references']}")
            row["bindings"].append((tag, {"kind": "CID", "strength": strength, "ids": list(ids)}))
            continue
        parent = next(
            (candidate for candidate in reversed(rows[:index])
             if candidate["kind"] == "attribute" and candidate["level"] < row["level"]),
            None,
        )
        if parent is None or parent["level"] != row["level"] - 1:
            raise GenerationError(f"{where}: Value Set outside a Sequence")
        parent["value_set"] = value_set(row["references"], kind, where)


class Model:
    def __init__(self, part03, part06, part10):
        self.part03 = part03
        self.dictionary = parse_dictionary(part06)
        self.image_storage = parse_image_storage(part06)
        self.registered_uids = parse_registered_uids(part06)
        self.tables = {}
        self.order = []
        self.conditions = {}
        self.condition_order = []
        self.modules = self.parse_modules()
        self.functional_groups = self.parse_functional_groups()
        self.file_meta = self.parse_file_meta(part10)
        self.constraint_sentences = self.bind_iod_constraints()

    def bind_iod_constraints(self):
        """Binds the Value Sets of the IOD Content Constraints sections to the
        Code Sequences of the Module or Macro each section names, overriding the
        Macro's own; returns the binding sentences."""
        document = self.part03
        tables = {entry["name"]: entry["table"] for entry in self.modules + self.functional_groups}
        bound = []
        for parent_id in IOD_CONSTRAINT_SECTIONS:
            for section in document.element(parent_id).findall(document.q("section")):
                title = document.render(section.find(document.q("title"))).removesuffix(" Module")
                if title not in tables:
                    raise GenerationError(f"{section.get('label')}: no Module or Macro named {title!r}")
                rows = self.tables[tables[title]]["rows"]
                for para in own_elements(document, section, "para"):
                    references = value_set_references(document, para)
                    if not references:
                        continue
                    text = document.render(para)
                    where = f"{section.get('label')} {text}"
                    tags = [f"{group}{element}" for group, element in re.findall(r"\(([0-9A-F]{4}),([0-9A-F]{4})\)", text)]
                    targets = [row for row in rows if row["kind"] == "attribute" and row["tag"] in tags]
                    if not tags or {row["tag"] for row in targets} != set(tags):
                        raise GenerationError(f"{where}: Code Sequences not found in {tables[title]}")
                    for row in targets:
                        row["value_set"] = value_set(references, "CID", where)
                    bound.append(text)
        return bound

    def condition(self, text, keyword, where):
        text = normalize(text)
        if text not in self.conditions:
            digest = hashlib.sha256(text.encode("utf-8")).hexdigest()[:6]
            name = f"{keyword}_{digest}"
            if name in {entry["name"] for entry in self.conditions.values()}:
                raise GenerationError(f"condition name collision {name}")
            self.conditions[text] = {"name": name, "text": text, "uses": []}
            self.condition_order.append(text)
        self.conditions[text]["uses"].append(where)
        return self.conditions[text]["name"]

    def keyword(self, tag, where):
        entry = self.dictionary.get(tag)
        if entry is None:
            raise GenerationError(f"{where}: tag {tag_display(tag)} is not in PS3.6")
        return entry["keyword"]

    def parse_table(self, table_id):
        if table_id in self.tables:
            return
        document = self.part03
        table = document.element(table_id)
        caption = table.find(document.q("caption"))
        record = {
            "id": table_id,
            "part": "PS3.3",
            "label": table.get("label"),
            "section": document.section_label(table),
            "caption": document.render(caption) if caption is not None else "",
            "rows": [],
        }
        self.tables[table_id] = record
        self.order.append(table_id)
        body = table.find(document.q("tbody"))
        if body is None:
            raise GenerationError(f"{table_id} has no tbody")
        for row in body.iter(document.q("tr")):
            row_cells = cells(document, row)
            if not row_cells:
                continue
            first = document.render(row_cells[0])
            include = re.match(r"(>*)\s*Include\b", first)
            if include:
                targets = [link for link in links(row_cells[0]) if link.startswith("table_")]
                if not targets:
                    # Functional Group Macro placeholders in the Multi-frame
                    # Functional Groups Module; the IOD lists the macros.
                    continue
                references = [found for cell in row_cells for found in value_set_references(document, cell)]
                for target in targets:
                    record["rows"].append(
                        {
                            "kind": "include",
                            "level": len(include.group(1)),
                            "target": target,
                            "references": references,
                            "bindings": [],
                        }
                    )
                    self.parse_table(target)
                continue
            if len(row_cells) != 4:
                if first.startswith(">") and "Any Attribute" in first:
                    continue  # wildcard row in Original Attributes Sequence
                if len(row_cells) == 1:
                    continue  # section heading row inside a macro
                raise GenerationError(f"{table_id}: unexpected row {first!r}")
            tag = parse_tag(document.render(row_cells[1]))
            if tag is None:
                raise GenerationError(f"{table_id}: row {first!r} has no tag")
            if "x" in tag:
                raise GenerationError(f"{table_id}: repeating group tag {tag} in the IOD")
            level = len(re.match(r">*", first).group(0))
            name = first[level:].strip()
            kind = document.render(row_cells[2])
            if kind not in TYPES:
                raise GenerationError(f"{table_id}: {name} has unknown Type {kind!r}")
            where = f"{table_id} {name} {tag_display(tag)}"
            keyword = self.keyword(tag, where)
            description = row_cells[3]
            condition = None
            if kind in ("1C", "2C"):
                condition = self.condition(condition_text(document, description, where), keyword, where)
            elif kind == "3":
                text = " ".join(document.render(b) for _, b in description_blocks(document, description))
                restriction = TYPE3_RESTRICTION.search(text)
                if restriction:
                    condition = self.condition(restriction.group(0), keyword, where)
            enumerations = cell_enumerations(document, description, tag, where)
            enumerations += section_enumerations(document, description, tag, name, where)
            enumerations += bound_section_enumerations(document, table_id, tag, description)
            for enumeration in enumerations:
                if enumeration["condition"]:
                    enumeration["condition"] = self.condition(
                        "Enumerated Values " + enumeration["condition"], keyword, where
                    )
            is_sequence = self.dictionary[tag]["vrs"] == ["SQ"]
            record["rows"].append(
                {
                    "kind": "attribute",
                    "level": level,
                    "name": name,
                    "tag": tag,
                    "type": kind,
                    "condition": condition,
                    "enumerations": dedupe(enumerations),
                    "items": item_count(document, description, where)
                    if is_sequence
                    else "ANY",
                    "value_set": None,
                }
            )
        bind_value_sets(record)

    def usage(self, text, name, keyword_prefix, allow_placement):
        match = re.match(r"([MCU])(?:\s*-\s*(.*))?\Z", text)
        if not match:
            raise GenerationError(f"{name}: unrecognized usage {text!r}")
        usage, rest = match.group(1), (match.group(2) or "").strip()
        placement = "Placement::Any"
        condition = None
        if rest in PLACEMENTS and allow_placement:
            placement = PLACEMENTS[rest]
        elif rest:
            keyword = keyword_prefix + re.sub(r"[^A-Za-z0-9]", "", name.title())
            condition = self.condition(rest, keyword, name)
        if usage == "C" and condition is None:
            raise GenerationError(f"{name}: conditional usage without condition")
        return USAGES[usage], placement, condition

    def parse_modules(self):
        document = self.part03
        table = document.element(IOD_MODULE_TABLE)
        modules = []
        for row in table.find(document.q("tbody")).iter(document.q("tr")):
            row_cells = cells(document, row)
            references = [cell for cell in row_cells if links(cell)]
            if len(row_cells) < 3 or not references:
                raise GenerationError(f"{IOD_MODULE_TABLE}: unexpected row")
            name = document.render(row_cells[-3])
            section_id = links(references[0])[0]
            module_table = first_table(document, section_id).get(XML_ID)
            self.parse_table(module_table)
            usage, placement, condition = self.usage(
                document.render(row_cells[-1]), name, "Module", False
            )
            modules.append(
                {
                    "name": name,
                    "section": document.element(section_id).get("label"),
                    "table": module_table,
                    "usage": usage,
                    "condition": condition,
                }
            )
        tags = {module["name"]: self.top_level_tags(module["table"]) for module in modules}
        top_level = {}
        for module in modules:
            for tag in tags[module["name"]]:
                top_level.setdefault(tag, set()).add(module["name"])
        for module in modules:
            module["signature"] = sorted(
                tag for tag in tags[module["name"]] if top_level[tag] == {module["name"]}
            )
            if module["usage"] != "Usage::Mandatory" and not module["signature"]:
                raise GenerationError(f"{module['name']}: no Attribute identifies the Module")
        return modules

    def top_level_tags(self, table_id, level=0, seen=None):
        seen = set() if seen is None else seen
        if table_id in seen:
            return set()
        seen = seen | {table_id}
        tags = set()
        for row in self.tables[table_id]["rows"]:
            if row["kind"] == "attribute" and row["level"] + level == 0:
                tags.add(row["tag"])
            elif row["kind"] == "include" and row["level"] + level == 0:
                tags |= self.top_level_tags(row["target"], row["level"] + level, seen)
        return tags

    def parse_functional_groups(self):
        document = self.part03
        table = document.element(IOD_FUNCTIONAL_GROUP_TABLE)
        groups = []
        for row in table.find(document.q("tbody")).iter(document.q("tr")):
            row_cells = cells(document, row)
            references = [link for cell in row_cells for link in links(cell)]
            if len(row_cells) != 3 or not references:
                raise GenerationError(f"{IOD_FUNCTIONAL_GROUP_TABLE}: unexpected row")
            name = document.render(row_cells[0])
            macro_table = first_table(document, references[0]).get(XML_ID)
            self.parse_table(macro_table)
            usage, placement, condition = self.usage(
                document.render(row_cells[2]), name, "Fg", True
            )
            sequences = sorted(self.top_level_tags(macro_table))
            if not sequences or any(self.dictionary[tag]["vrs"] != ["SQ"] for tag in sequences):
                raise GenerationError(f"{name}: Functional Group Macro without a top-level Sequence")
            groups.append(
                {
                    "name": name,
                    "section": document.element(references[0]).get("label"),
                    "table": macro_table,
                    "usage": usage,
                    "placement": placement,
                    "condition": condition,
                    "sequences": sequences,
                }
            )
        return groups

    def parse_file_meta(self, part10):
        table = part10.element(FILE_META_TABLE)
        rows = []
        for row in table.find(part10.q("tbody")).iter(part10.q("tr")):
            row_cells = cells(part10, row)
            if len(row_cells) != 4:
                raise GenerationError(f"{FILE_META_TABLE}: unexpected row")
            tag = parse_tag(part10.render(row_cells[1]))
            if tag is None:
                continue  # preamble and prefix carry no tag
            name = part10.render(row_cells[0])
            kind = part10.render(row_cells[2])
            if kind not in TYPES:
                raise GenerationError(f"{FILE_META_TABLE}: {name} has unknown Type {kind!r}")
            where = f"PS3.10 {FILE_META_TABLE} {name}"
            condition = None
            if kind in ("1C", "2C"):
                condition = self.condition(
                    condition_text(part10, row_cells[3], where), self.keyword(tag, where), where
                )
            rows.append(
                {
                    "kind": "attribute",
                    "level": 0,
                    "name": name,
                    "tag": tag,
                    "type": kind,
                    "condition": condition,
                    "enumerations": [],
                    "items": "ANY",
                    "value_set": None,
                }
            )
        return {
            "id": FILE_META_TABLE,
            "part": "PS3.10",
            "label": table.get("label"),
            "section": part10.section_label(table),
            "caption": part10.render(table.find(part10.q("caption"))),
            "rows": rows,
        }


def sentences(text):
    return [sentence for sentence in SENTENCE_BREAK.split(text) if sentence]


def enclosing_section(document, element):
    node = document.parents.get(element)
    while node is not None and node.tag != document.q("section"):
        node = document.parents.get(node)
    if node is None:
        raise GenerationError(f"element {element.get(XML_ID)} is outside a section")
    return node


def own_elements(document, section, name):
    """Elements called `name` in a section, outside notes and its subsections."""
    found = []

    def walk(node):
        kind = node.tag.removeprefix(document.ns)
        if kind in ("section", "note"):
            return
        if kind == name:
            found.append(node)
            return
        for child in node:
            walk(child)

    for child in section:
        walk(child)
    return found


def is_attribute_table(document, table):
    head = table.find(document.q("thead"))
    if head is None:
        return False
    header = [document.render(cell) for row in head for cell in cells(document, row)]
    return "Tag" in header and "Type" in header


def statement(part, section, text, keyword=None, enforced=False):
    return {"part": part, "section": section, "keyword": keyword, "text": text, "enforced": enforced}


class Requirements:
    """Normative statements in the scope of the IOD, the tables included."""

    def __init__(self, model, parts):
        self.model = model
        self.parts = parts
        self.statements = []
        self.seen = set()
        self.parse_scope()
        self.parse_cells()
        for part, chapters in NORMATIVE_CHAPTERS.items():
            self.parse_chapters(part, chapters)
        self.name_statements()

    def add(self, record):
        key = (record["part"], record["section"], record["keyword"], record["text"])
        if key not in self.seen:
            self.seen.add(key)
            self.statements.append(record)

    def add_text(self, part, section, text, keyword=None, enforced=False):
        enforced = enforced or text in self.model.constraint_sentences
        for sentence in sentences(text):
            if NORMATIVE.search(sentence):
                self.add(statement(part, section, sentence, keyword, enforced))

    def scope_sections(self):
        """The IOD section, the sections holding its tables, and the PS3.3
        sections they cite, each with its subsections. A subsection holding the
        table of a Module or Macro outside the IOD belongs to that table."""
        document = self.model.part03
        in_scope = set(self.model.order) | {IOD_MODULE_TABLE, IOD_FUNCTIONAL_GROUP_TABLE}

        def excluded(section):
            return any(
                is_attribute_table(document, table) and table.get(XML_ID) not in in_scope
                for table in own_elements(document, section, "table")
            )

        pending = [document.element(IOD_SECTION)]
        pending += [enclosing_section(document, document.element(table)) for table in self.model.order]
        for table_id in self.model.order:
            pending += [document.ids[link] for link in links(document.element(table_id))
                        if LINKED_SECTION.match(link) and link in document.ids]
        found, visited = [], set()
        while pending:
            section = pending.pop()
            identifier = section.get(XML_ID)
            if identifier in visited or excluded(section):
                continue
            visited.add(identifier)
            found.append(section)
            pending += [child for child in section if child.tag == document.q("section")]
            for para in own_elements(document, section, "para"):
                pending += [document.ids[link] for link in links(para)
                            if LINKED_SECTION.match(link) and link in document.ids]
        return sorted(found, key=lambda section: section.get(XML_ID))

    def parse_scope(self):
        document = self.model.part03
        for section in self.scope_sections():
            label = section.get("label")
            for para in own_elements(document, section, "para"):
                if not self.inside_attribute_table(para):
                    self.add_text("PS3.3", label, document.render(para))

    def inside_attribute_table(self, element):
        document = self.model.part03
        node = document.parents.get(element)
        while node is not None and node.tag != document.q("section"):
            if node.tag == document.q("table"):
                return is_attribute_table(document, node)
            node = document.parents.get(node)
        return False

    def parse_cells(self):
        """Description-cell sentences of the in-scope tables, marking those the
        generated tables enforce."""
        document = self.model.part03
        for table_id in [IOD_MODULE_TABLE, IOD_FUNCTIONAL_GROUP_TABLE] + self.model.order:
            table = document.element(table_id)
            label = table.get("label")
            iod_table = table_id in (IOD_MODULE_TABLE, IOD_FUNCTIONAL_GROUP_TABLE)
            for row in table.find(document.q("tbody")).iter(document.q("tr")):
                row_cells = cells(document, row)
                if iod_table:
                    for cell in row_cells:
                        self.add_text("PS3.3", label, document.render(cell), enforced=True)
                    continue
                if len(row_cells) != 4:
                    continue
                tag = parse_tag(document.render(row_cells[1]))
                keyword = self.model.dictionary[tag]["keyword"] if tag in self.model.dictionary else None
                enforced = self.enforced_text(table_id, tag, row_cells)
                for _, block in description_blocks(document, row_cells[3]):
                    for sentence in sentences(document.render(block)):
                        if NORMATIVE.search(sentence):
                            self.add(statement(
                                "PS3.3", label, sentence, keyword,
                                any(sentence in text or text in sentence for text in enforced),
                            ))

    def enforced_text(self, table_id, tag, row_cells):
        """Text of a row the generated tables enforce."""
        document = self.model.part03
        description = row_cells[3]
        kind = document.render(row_cells[2])
        texts = [sentence for sentence, _ in ITEM_PATTERNS] + list(TABLE_SENTENCES)
        rendered = " ".join(document.render(block) for _, block in description_blocks(document, description))
        texts += [match.group(0) for match in ITEMS_LIKE_VALUE.finditer(rendered)]
        if kind in ("1C", "2C"):
            texts.append(condition_text(document, description, table_id))
        restriction = TYPE3_RESTRICTION.search(rendered)
        if kind == "3" and restriction:
            texts.append(restriction.group(0))
        texts += [entry[3] for entry in PROSE_ENUMERATIONS
                  if entry[0] == table_id and tag and entry[1] == tag_display(tag)]
        return texts

    def parse_chapters(self, part, chapters):
        document = self.parts[part]
        for chapter_id in chapters:
            chapter = document.element(chapter_id)
            for section in [chapter, *chapter.iter(document.q("section"))]:
                label = section.get("label")
                for para in own_elements(document, section, "para"):
                    self.add_text(part, label, document.render(para))

    def name_statements(self):
        names = set()
        for record in self.statements:
            if record["enforced"]:
                continue
            digest = hashlib.sha256(
                "\0".join((record["part"], record["section"], record["keyword"] or "", record["text"])).encode()
            ).hexdigest()[:6]
            section = re.sub(r"[^A-Za-z0-9]+", "_", record["section"])
            keyword = f"_{record['keyword']}" if record["keyword"] else ""
            name = f"Ps{record['part'].removeprefix('PS3.')}_{section}{keyword}_{digest}"
            if name in names:
                raise GenerationError(f"requirement name collision {name}")
            names.add(name)
            record["name"] = name


def model_value_sets(model):
    """Every Value Set bound by a table row or a Macro inclusion of the IOD."""
    for table_id in model.order:
        for row in model.tables[table_id]["rows"]:
            if row["kind"] == "attribute" and row["value_set"]:
                yield row["value_set"]
            elif row["kind"] == "include":
                yield from (bound for _, bound in row["bindings"])


def properties(document, section):
    """The 'Type:', 'Order:' ... terms of a PS3.16 Context Group or Template."""
    found = {}
    for variablelist in section.findall(document.q("variablelist")):
        for entry in variablelist.findall(document.q("varlistentry")):
            term = document.render(entry.find(document.q("term"))).rstrip(":")
            found[term] = document.render(entry.find(document.q("listitem")))
    return found


def header_index(document, table):
    head = table.find(document.q("thead"))
    return {document.render(cell): index for index, cell in enumerate(cells(document, head.find(document.q("tr"))))}


def coded_entry(text, where):
    match = CODED_ENTRY.match(text)
    if not match:
        raise GenerationError(f"{where}: unrecognized coded entry {text!r}")
    return {"kind": match.group(1), "code": (match.group(3), match.group(2), match.group(4))}


class ValueSets:
    """The PS3.16 Context Groups and Templates the IOD's Value Sets reach."""

    def __init__(self, model, part16):
        self.document = part16
        self.groups = {}
        self.templates = {}
        for bound in model_value_sets(model):
            for number in bound["ids"]:
                (self.parse_group if bound["kind"] == "CID" else self.parse_template)(number)

    def parse_group(self, number):
        if number in self.groups:
            return
        document = self.document
        section = document.element(f"sect_CID_{number}")
        found = properties(document, section)
        record = {
            "id": number,
            "name": document.render(section.find(document.q("title"))),
            "extensible": found.get("Type", "").lower() == "extensible",
            "scheme": None,
            "codes": [],
            "includes": [],
        }
        self.groups[number] = record
        tables = section.findall(document.q("table"))
        if not tables:
            text = " ".join(document.render(para) for para in section.findall(document.q("para")))
            scheme = SCHEME_SENTENCE.search(text)
            record["scheme"] = scheme.group(1) if scheme else SCHEME_CONTEXT_GROUPS.get(number)
            if record["scheme"] is None:
                raise GenerationError(f"CID {number} has neither codes nor a Coding Scheme")
            return
        if found.get("Type", "").lower() not in ("extensible", "non-extensible"):
            raise GenerationError(f"CID {number}: unrecognized Type {found.get('Type')!r}")
        for table in tables:
            columns = header_index(document, table)
            legacy = columns.get("SNOMED-RT ID")
            for row in table.find(document.q("tbody")).iter(document.q("tr")):
                row_cells = cells(document, row)
                text = [document.render(cell) for cell in row_cells]
                if text[0].startswith("Include"):
                    references = value_set_references(document, row_cells[0])
                    if not references:
                        raise GenerationError(f"CID {number}: unrecognized row {text[0]!r}")
                    for _, kind, included in references:
                        if kind != "CID":
                            raise GenerationError(f"CID {number} includes {kind} {included}")
                        record["includes"].append(included)
                        self.parse_group(included)
                    continue
                record["codes"].append((
                    text[columns["Coding Scheme Designator"]],
                    text[columns["Code Value"]],
                    text[columns["Code Meaning"]],
                    text[legacy] if legacy is not None and legacy < len(text) else "",
                ))

    def parse_template(self, number):
        if number in self.templates:
            return
        document = self.document
        section = document.element(f"sect_TID_{number}")
        found = properties(document, section)
        record = {
            "id": number,
            "name": document.render(section.find(document.q("title"))),
            "extensible": found.get("Type") == "Extensible",
            "ordered": found.get("Order") == "Significant",
            "rows": [],
        }
        self.templates[number] = record
        table = document.element(f"table_TID_{number}")
        columns = header_index(document, table)
        for row in table.find(document.q("tbody")).iter(document.q("tr")):
            row_cells = cells(document, row)
            text = [document.render(cell) for cell in row_cells]
            where = f"TID {number} Row {text[0]}"
            value_type = text[columns["VT"]]
            concept_cell = row_cells[columns["Concept Name"]]
            references = value_set_references(document, concept_cell)
            if value_type == "INCLUDE":
                included = value_set(references, "TID", where)
                for template in included["ids"]:
                    self.parse_template(template)
                concept = {"kind": "TID", "strength": included["strength"], "ids": included["ids"]}
            elif references:
                concept = value_set(references, "CID", where)
                for group in concept["ids"]:
                    self.parse_group(group)
            elif text[columns["Concept Name"]]:
                concept = coded_entry(text[columns["Concept Name"]], where)
            else:
                concept = None
            constraint_cell = row_cells[columns["Value Set Constraint"]]
            constraint = None
            constraint_references = value_set_references(document, constraint_cell)
            if constraint_references:
                constraint = value_set(constraint_references, "CID", where)
                for group in constraint["ids"]:
                    self.parse_group(group)
            record["rows"].append({
                "row": text[0],
                "level": text[columns["NL"]].count(">") if "NL" in columns else 0,
                "value_type": value_type,
                "concept": concept,
                "vm": parse_vm(text[columns["VM"]], where),
                "requirement": text[columns["Req Type"]],
                "condition": template_condition(text[columns["Condition"]], where),
                "constraint": constraint,
            })


def template_condition(text, where):
    if not text:
        return None
    for pattern, kind in TEMPLATE_CONDITION:
        match = pattern.match(text)
        if match:
            condition = {"kind": kind, "iff": match.group(1).upper() == "IFF", "row": match.group(2)}
            if kind == "value":
                condition["code"] = (match.group(4), match.group(3))
            return condition
    raise GenerationError(f"{where}: unrecognized Template condition {text!r}")


def dedupe(enumerations):
    unique = []
    for enumeration in enumerations:
        if enumeration not in unique:
            unique.append(enumeration)
    return unique


def parse_vm(text, where):
    if " or " in text:
        # Multi-VR entries such as LUT Data (US or OW: "1-n or 1") admit the
        # union of the alternatives.
        alternatives = [parse_vm(part, where) for part in text.split(" or ")]
        if any(step != 1 for _, _, step in alternatives):
            raise GenerationError(f"{where}: unsupported VM {text!r}")
        highs = [high for _, high, _ in alternatives]
        return (
            min(low for low, _, _ in alternatives),
            None if None in highs else max(highs),
            1,
        )
    text = text.replace(" ", "")
    if re.fullmatch(r"\d+", text):
        return (int(text), int(text), 1)
    match = re.fullmatch(r"(\d+)-(\d+)", text)
    if match:
        return (int(match.group(1)), int(match.group(2)), 1)
    match = re.fullmatch(r"(\d+)-(\d*)n", text)
    if match:
        low = int(match.group(1))
        step = int(match.group(2)) if match.group(2) else 1
        if step != 1 and low != step:
            raise GenerationError(f"{where}: unsupported VM {text!r}")
        return (low, None, step)
    raise GenerationError(f"{where}: unsupported VM {text!r}")


def parse_dictionary(part06):
    entries = {}
    for table_id in ("table_6-1", "table_7-1"):
        table = part06.element(table_id)
        for row in table.find(part06.q("tbody")).iter(part06.q("tr")):
            row_cells = cells(part06, row)
            if len(row_cells) != 6:
                raise GenerationError(f"{table_id}: unexpected row")
            text = [part06.render(cell) for cell in row_cells]
            tag = parse_tag(text[0])
            if tag is None:
                raise GenerationError(f"{table_id}: invalid tag {text[0]!r}")
            where = f"PS3.6 {table_id} {text[0]}"
            vrs = [vr.strip() for vr in text[3].split(" or ")] if text[3] else []
            if any(vr not in VRS for vr in vrs):
                vrs = []  # e.g. item delimitation tags whose VR is "See Note"
            vm = parse_vm(text[4], where) if text[4] and vrs else None
            entries[tag] = {
                "tag": tag,
                "keyword": text[2],
                "vrs": vrs,
                "vm": vm,
                "retired": text[5].startswith("RET"),
            }
    return entries


def parse_image_storage(part06):
    """PS3.6 Table A-1 SOP Classes that store images, retired ones included."""
    table = part06.element("table_A-1")
    found = []
    for row in table.find(part06.q("tbody")).iter(part06.q("tr")):
        text = [part06.render(cell) for cell in cells(part06, row)]
        if len(text) >= 4 and text[3] == "SOP Class" and "Image Storage" in text[1]:
            found.append(text[0])
    if not found:
        raise GenerationError("table_A-1 lists no Image Storage SOP Class")
    return sorted(found)


DICOM_ROOT_UID = re.compile(r"1\.2\.840\.10008(?:\.(?:0|[1-9][0-9]*))+\Z")
REGISTRY_TABLES = ("table_A-1", "table_A-2", "table_A-3", "table_A-4")


def parse_registered_uids(part06):
    """PS3.6 Annex A UIDs (Tables A-1 to A-4: UID Values, Well-known Frames of
    Reference, Context Group and Template UIDs), retired ones included."""
    found = set()
    for table_id in REGISTRY_TABLES:
        table = part06.element(table_id)
        for row in table.find(part06.q("tbody")).iter(part06.q("tr")):
            text = [part06.render(cell) for cell in cells(part06, row)]
            if not text or not DICOM_ROOT_UID.match(text[0]):
                raise GenerationError(f"{table_id}: row does not begin with a DICOM UID: {text[:1]!r}")
            found.add(text[0])
    return sorted(found)


def rust_string(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def rust_ident(table_id):
    return "T_" + re.sub(r"[^A-Za-z0-9]", "_", table_id.removeprefix("table_")).upper()


def vm_ident(vm):
    low, high, step = vm
    if high is None:
        return f"VM_{low}_N" if step == 1 else f"VM_{low}_{step}N"
    return f"VM_{low}" if low == high else f"VM_{low}_{high}"


def vr_ident(vrs):
    return "_".join(vrs) if vrs else "NO_VR"


def tag_hex(tag):
    return f"0x{tag[:4]}_{tag[4:]}"


def optional_condition(name):
    return f"Some(C::{name})" if name else "None"


class Emitter:
    def __init__(self, model):
        self.model = model
        self.lines = []

    def emit(self, line=""):
        self.lines.append(line)

    def enumerations(self, row):
        if not row["enumerations"]:
            return "&[]"
        parts = []
        for enumeration in row["enumerations"]:
            value = enumeration["value"]
            values = ", ".join(rust_string(v) for v in enumeration["values"])
            parts.append(
                f"e({f'Some({value})' if value else 'None'}, "
                f"{optional_condition(enumeration['condition'])}, &[{values}])"
            )
        return "&[" + ", ".join(parts) + "]"

    def nodes(self, rows, indent):
        """Emit rows of one table as a nested Node slice, one row per line."""
        pad = "    " * indent
        out = []
        index = 0
        while index < len(rows):
            row = rows[index]
            level = row["level"]
            end = index + 1
            while end < len(rows) and rows[end]["level"] > level:
                end += 1
            children = rows[index + 1:end]
            if row["kind"] == "include":
                if children:
                    raise GenerationError(f"include of {row['target']} has nested rows")
                if row["bindings"]:
                    bindings = ", ".join(
                        f"({tag_hex(tag)}, {self.value_set_value(bound).removeprefix('Some(').removesuffix(')')})"
                        for tag, bound in row["bindings"]
                    )
                    out.append(f"{pad}ib(&{rust_ident(row['target'])}, &[{bindings}]),")
                else:
                    out.append(f"{pad}i(&{rust_ident(row['target'])}),")
                index = end
                continue
            head = (
                f"{pad}a({tag_hex(row['tag'])}, {rust_string(row['name'])}, "
                f"T{row['type']}, {optional_condition(row['condition'])}, "
                f"{self.enumerations(row)}, {row['items']}, {self.value_set_value(row['value_set'])}, "
            )
            if not children:
                out.append(head + "&[]),")
                index = end
                continue
            if self.model.dictionary[row["tag"]]["vrs"] != ["SQ"]:
                raise GenerationError(f"{row['name']} has nested rows but is not a Sequence")
            shifted = [dict(child, level=child["level"] - level - 1) for child in children]
            if any(child["level"] < 0 for child in shifted):
                raise GenerationError(f"{row['name']}: malformed nesting")
            out.append(head + "&[")
            out.extend(self.nodes(shifted, indent + 1))
            out.append(f"{pad}]),")
            index = end
        return out

    def table(self, record):
        rows = record["rows"]
        if rows and rows[0]["level"] != 0:
            raise GenerationError(f"{record['id']} starts below the top level")
        self.emit(f"/// {record['caption']}")
        self.emit(
            f"pub(super) static {rust_ident(record['id'])}: Table = "
            f"t({rust_string(record['part'])}, {rust_string(record['label'])}, "
            f"{rust_string(record['section'])}, &["
        )
        self.lines.extend(self.nodes(rows, 1))
        self.emit("]);")
        self.emit()

    def header(self, digests):
        self.emit(f"// @generated by scripts/generate-iod-tables.py from DICOM {EDITION}. Do not edit.")
        self.emit("// Regenerate with: scripts/generate-iod-tables.py --source-dir <dir> [--fetch]")
        self.emit("// Verify with:     scripts/generate-iod-tables.py --source-dir <dir> --check")
        for name, digest in digests.items():
            self.emit(f"// Source {name} SHA-256 {digest}")
        self.emit()

    def render(self, digests):
        model = self.model
        emit = self.emit
        self.header(digests)
        emit("use super::model::{")
        emit("    Attribute, AttributeType, Enumeration, FunctionalGroupMacro, ItemCount, Module, Node,")
        emit("    Placement, Strength, Table, Usage, ValueSet,")
        emit("};")
        emit("use ConditionId as C;")
        emit("use ItemCount::{AtLeast, AtMost, Exactly, NumberOfFrames, Unconstrained as ANY, ValueOf};")
        emit()
        emit("/// The pinned edition as a literal, for `concat!` in constant citations.")
        emit("macro_rules! dicom_edition {")
        emit(f"    () => {{ {rust_string(EDITION)} }};")
        emit("}")
        emit("pub(crate) use dicom_edition;")
        emit("pub(super) const EDITION: &str = dicom_edition!();")
        emit("#[cfg(test)]")
        emit("pub(super) const SOURCE_SHA256: &[(&str, &str)] = &[")
        for name, digest in digests.items():
            emit(f"    ({rust_string(name)}, {rust_string(digest)}),")
        emit("];")
        emit()
        emit("/// One Variant per distinct condition text in the IOD, its Modules and Macros.")
        emit("#[allow(non_camel_case_types)]")
        emit("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]")
        emit("pub(super) enum ConditionId {")
        for text in model.condition_order:
            condition = model.conditions[text]
            uses = sorted(set(condition["uses"]))
            emit(f"    /// {uses[0]}" + (f" (+{len(uses) - 1} more)" if len(uses) > 1 else ""))
            emit(f"    {condition['name']},")
        emit("}")
        emit()
        emit("impl ConditionId {")
        emit("    #[cfg(test)]")
        emit("    pub(super) const ALL: &'static [ConditionId] = &[")
        for text in model.condition_order:
            emit(f"        C::{model.conditions[text]['name']},")
        emit("    ];")
        emit()
        emit("    /// Normalized condition text from the standard; the Variant name hashes it.")
        emit("    pub(super) const fn text(self) -> &'static str {")
        emit("        match self {")
        for text in model.condition_order:
            emit(f"            C::{model.conditions[text]['name']} => {rust_string(text)},")
        emit("        }")
        emit("    }")
        emit("}")
        emit()
        self.lines.extend(TABLE_CONSTRUCTORS.splitlines())
        emit()
        emit("pub(super) static MODULES: &[Module] = &[")
        for module in model.modules:
            signature = ", ".join(tag_hex(tag) for tag in module["signature"])
            emit("    Module {")
            emit(f"        name: {rust_string(module['name'])},")
            emit(f"        section: {rust_string(module['section'])},")
            emit(f"        table: &{rust_ident(module['table'])},")
            emit(f"        usage: {module['usage']},")
            emit(f"        condition: {optional_condition(module['condition'])},")
            emit(f"        signature: &[{signature}],")
            emit("    },")
        emit("];")
        emit()
        emit("pub(super) static FUNCTIONAL_GROUPS: &[FunctionalGroupMacro] = &[")
        for group in model.functional_groups:
            emit("    FunctionalGroupMacro {")
            emit(f"        name: {rust_string(group['name'])},")
            emit(f"        section: {rust_string(group['section'])},")
            emit(f"        table: &{rust_ident(group['table'])},")
            emit(f"        usage: {group['usage']},")
            emit(f"        placement: {group['placement']},")
            emit(f"        condition: {optional_condition(group['condition'])},")
            emit(f"        sequences: &[{', '.join(tag_hex(tag) for tag in group['sequences'])}],")
            emit("    },")
        emit("];")
        emit()
        emit("/// PS3.10 File Meta Information.")
        emit(f"pub(super) static FILE_META: &Table = &{rust_ident(model.file_meta['id'])};")
        emit()
        for table_id in model.order:
            self.table(model.tables[table_id])
        self.table(model.file_meta)
        return "\n".join(self.lines) + "\n"

    def requirements(self, requirements, digests):
        emit = self.emit
        self.header(digests)
        emit("use super::model::Statement;")
        emit("use Requirement as R;")
        emit()
        emit("const fn s(")
        emit("    part: &'static str,")
        emit("    section: &'static str,")
        emit("    keyword: Option<&'static str>,")
        emit("    text: &'static str,")
        emit(") -> Statement {")
        emit("    Statement { part, section, keyword, text }")
        emit("}")
        emit()
        classified = [record for record in requirements.statements if not record["enforced"]]
        emit("/// One Variant per normative statement the generated tables do not enforce.")
        emit("/// `dispositions.rs` classifies each, so a changed or new statement in another")
        emit("/// edition fails to compile until it is classified.")
        emit("#[allow(non_camel_case_types)]")
        emit("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]")
        emit("pub(super) enum Requirement {")
        for record in classified:
            emit(f"    /// {record['part']} {record['section']}: {record['text']}")
            emit(f"    {record['name']},")
        emit("}")
        emit()
        emit("impl Requirement {")
        emit("    pub(super) const ALL: &'static [Requirement] = &[")
        for record in classified:
            emit(f"        R::{record['name']},")
        emit("    ];")
        emit()
        emit("    pub(super) const fn statement(self) -> Statement {")
        emit("        match self {")
        for record in classified:
            emit(f"            R::{record['name']} => {self.statement(record)},")
        emit("        }")
        emit("    }")
        emit()
        emit("    /// The stable identifier `wsi-dicom requirements --json` reports.")
        emit("    pub(super) const fn id(self) -> &'static str {")
        emit("        match self {")
        for record in classified:
            emit(f"            R::{record['name']} => \"{record['name']}\",")
        emit("        }")
        emit("    }")
        emit("}")
        emit()
        emit("/// Normative statements in description cells that the generated tables enforce.")
        emit("pub(super) static TABLE_ENFORCED: &[Statement] = &[")
        for record in requirements.statements:
            if record["enforced"]:
                emit(f"    {self.statement(record)},")
        emit("];")
        return "\n".join(self.lines) + "\n"

    def value_sets(self, value_sets, digests):
        emit = self.emit
        self.header(digests)
        emit("use super::model::{")
        emit("    Code, Concept, ContextGroup, RowRequirement as Q, Strength, Template, TemplateCondition,")
        emit("    TemplateRow, ValueSet,")
        emit("};")
        emit()
        emit("const fn c(scheme: &'static str, value: &'static str, meaning: &'static str, legacy: &'static str) -> Code {")
        emit("    Code { scheme, value, meaning, legacy }")
        emit("}")
        emit()
        emit("/// PS3.16 Context Groups the IOD's Value Sets reach, sorted by Context ID; each")
        emit("/// lists its own and its included groups' codes sorted by Coding Scheme and Value,")
        emit("/// and the Coding Schemes it or an included group comprises whole.")
        emit("pub(super) static CONTEXT_GROUPS: &[ContextGroup] = &[")
        for number in sorted(value_sets.groups):
            group = value_sets.groups[number]
            codes, schemes = {}, set()
            for included in self.included_groups(value_sets, number):
                if value_sets.groups[included]["scheme"]:
                    schemes.add(value_sets.groups[included]["scheme"])
                for scheme, value, meaning, legacy in value_sets.groups[included]["codes"]:
                    codes.setdefault((scheme, value), (meaning, legacy))
            scheme_list = ", ".join(rust_string(scheme) for scheme in sorted(schemes))
            emit(
                f"    ContextGroup {{ id: {number}, name: {rust_string(group['name'])}, "
                f"extensible: {str(group['extensible']).lower()}, schemes: &[{scheme_list}], codes: &["
            )
            for (scheme, value), (meaning, legacy) in sorted(codes.items()):
                emit(f"        c({rust_string(scheme)}, {rust_string(value)}, {rust_string(meaning)}, {rust_string(legacy)}),")
            emit("    ] },")
        emit("];")
        emit()
        emit("/// PS3.16 Templates the IOD's Value Sets reach, sorted by Template ID.")
        emit("pub(super) static TEMPLATES: &[Template] = &[")
        for number in sorted(value_sets.templates):
            template = value_sets.templates[number]
            emit(
                f"    Template {{ id: {number}, name: {rust_string(template['name'])}, "
                f"extensible: {str(template['extensible']).lower()}, "
                f"ordered: {str(template['ordered']).lower()}, rows: &["
            )
            for row in template["rows"]:
                low, high, step = row["vm"]
                if step != 1 or row["level"]:
                    raise GenerationError(f"TID {number} Row {row['row']}: nested or stepped rows")
                emit(
                    f"        TemplateRow {{ row: {rust_string(row['row'])}, "
                    f"value_type: {rust_string(row['value_type'])}, concept: {self.concept(row['concept'])}, "
                    f"min: {low}, max: {'None' if high is None else f'Some({high})'}, "
                    f"requirement: Q::{row['requirement'].title()}, "
                    f"condition: {self.template_condition(row['condition'])}, "
                    f"constraint: {self.value_set_value(row['constraint'])} }},"
                )
            emit("    ] },")
        emit("];")
        return "\n".join(self.lines) + "\n"

    @staticmethod
    def included_groups(value_sets, number):
        found, pending = [], [number]
        while pending:
            current = pending.pop()
            if current not in found:
                found.append(current)
                pending += value_sets.groups[current]["includes"]
        return found

    @staticmethod
    def value_set_value(bound):
        if bound is None:
            return "None"
        strength = "Strength::Baseline" if bound["strength"] == "B" else "Strength::Defined"
        if bound["kind"] == "TID":
            (template,) = bound["ids"]
            return f"Some(ValueSet::Template({strength}, {template}))"
        return f"Some(ValueSet::ContextGroups({strength}, &[{', '.join(map(str, bound['ids']))}]))"

    def concept(self, concept):
        if concept is None:
            raise GenerationError("a Template row without a Concept Name constraint")
        if concept["kind"] in ("CID", "TID"):
            return f"Concept::Set({self.value_set_value(concept).removeprefix('Some(').removesuffix(')')})"
        scheme, value, meaning = concept["code"]
        kind = "Enumerated" if concept["kind"] == "EV" else "Defined"
        return f"Concept::{kind}(c({rust_string(scheme)}, {rust_string(value)}, {rust_string(meaning)}, \"\"))"

    @staticmethod
    def template_condition(condition):
        if condition is None:
            return "None"
        iff = str(condition["iff"]).lower()
        if condition["kind"] == "absent":
            return f"Some(TemplateCondition::RowAbsent {{ iff: {iff}, row: {rust_string(condition['row'])} }})"
        scheme, value = condition["code"]
        return (
            f"Some(TemplateCondition::RowValue {{ iff: {iff}, row: {rust_string(condition['row'])}, "
            f"scheme: {rust_string(scheme)}, value: {rust_string(value)} }})"
        )

    @staticmethod
    def statement(record):
        keyword = f"Some({rust_string(record['keyword'])})" if record["keyword"] else "None"
        return (
            f"s({rust_string(record['part'])}, {rust_string(record['section'])}, {keyword}, "
            f"{rust_string(record['text'])})"
        )

    def dictionary(self, digests):
        self.header(digests)
        self.emit("use dicom_core::VR;")
        self.emit()
        self.emit("use super::model::{DictionaryEntry, ValueMultiplicity};")
        self.emit()
        entries = sorted(self.model.dictionary.values(), key=lambda entry: entry["tag"])
        vr_sets = sorted({tuple(entry["vrs"]) for entry in entries})
        vms = sorted(
            {entry["vm"] for entry in entries if entry["vm"]},
            key=lambda vm: (vm[0], vm[1] or 0, vm[2]),
        )
        for vrs in vr_sets:
            body = ", ".join(f"VR::{vr}" for vr in vrs)
            self.emit(f"const {vr_ident(vrs)}: &[VR] = &[{body}];")
        for low, high, step in vms:
            high_text = "None" if high is None else f"Some({high})"
            self.emit(
                f"const {vm_ident((low, high, step))}: ValueMultiplicity = "
                f"ValueMultiplicity {{ min: {low}, max: {high_text}, step: {step} }};"
            )
        self.emit("const VM_NONE: ValueMultiplicity = ValueMultiplicity { min: 0, max: None, step: 1 };")
        self.emit()
        self.emit("const fn d(tag: u32, vr: &'static [VR], vm: ValueMultiplicity) -> DictionaryEntry {")
        self.emit("    DictionaryEntry { tag, mask: u32::MAX, vr, vm, retired: false }")
        self.emit("}")
        self.emit()
        self.emit("const fn r(tag: u32, vr: &'static [VR], vm: ValueMultiplicity) -> DictionaryEntry {")
        self.emit("    DictionaryEntry { tag, mask: u32::MAX, vr, vm, retired: true }")
        self.emit("}")
        self.emit()
        self.emit("/// PS3.6 Tables 6-1 and 7-1 without repeating groups, sorted by tag; r() is retired.")
        self.emit("pub(super) static DICTIONARY: &[DictionaryEntry] = &[")
        repeating = []
        for entry in entries:
            tag = entry["tag"]
            vm = vm_ident(entry["vm"]) if entry["vm"] else "VM_NONE"
            if "x" in tag:
                repeating.append((entry, vm))
                continue
            constructor = "r" if entry["retired"] else "d"
            self.emit(f"    {constructor}(0x{tag}, {vr_ident(entry['vrs'])}, {vm}),")
        self.emit("];")
        self.emit()
        self.emit("/// PS3.6 Table A-1 Image Storage SOP Class UIDs, sorted.")
        self.emit("pub(super) static IMAGE_STORAGE_SOP_CLASSES: &[&str] = &[")
        for uid in self.model.image_storage:
            self.emit(f"    {rust_string(uid)},")
        self.emit("];")
        self.emit()
        self.emit("/// PS3.6 Annex A registered UIDs (Tables A-1 to A-4), sorted.")
        self.emit("pub(super) static REGISTERED_UIDS: &[&str] = &[")
        for uid in self.model.registered_uids:
            self.emit(f"    {rust_string(uid)},")
        self.emit("];")
        self.emit()
        self.emit("/// PS3.6 repeating-group entries; x nibbles are zero in tag and mask.")
        self.emit("pub(super) static DICTIONARY_REPEATING: &[DictionaryEntry] = &[")
        for entry, vm in repeating:
            tag = entry["tag"]
            value = tag.replace("x", "0")
            mask = "".join("0" if c == "x" else "F" for c in tag)
            retired = "true" if entry["retired"] else "false"
            self.emit(
                f"    DictionaryEntry {{ tag: 0x{value}, mask: 0x{mask}, vr: {vr_ident(entry['vrs'])}, "
                f"vm: {vm}, retired: {retired} }},"
            )
        self.emit("];")
        return "\n".join(self.lines) + "\n"


def generate(source_dir, fetch):
    sources = verified_sources(source_dir, fetch)
    part10 = Document(sources["part10.xml"])
    model = Model(Document(sources["part03.xml"]), Document(sources["part06.xml"]), part10)
    requirements = Requirements(model, {"PS3.5": Document(sources["part05.xml"]), "PS3.10": part10})
    digests = {name: SOURCES[name] for name in sorted(SOURCES)}
    return {
        "tables.rs": Emitter(model).render(digests),
        "dictionary.rs": Emitter(model).dictionary(digests),
        "requirements.rs": Emitter(model).requirements(requirements, digests),
        "value_sets.rs": Emitter(model).value_sets(ValueSets(model, Document(sources["part16.xml"])), digests),
    }


def main():
    args = arguments()
    try:
        generated = generate(args.source_dir, args.fetch)
    except (GenerationError, OSError, ET.ParseError) as error:
        raise SystemExit(f"generate-iod-tables: {error}") from error
    if args.check:
        stale = []
        for name in OUTPUT_FILES:
            path = args.output_dir / name
            try:
                current = path.read_text(encoding="utf-8")
            except OSError as error:
                raise SystemExit(f"generate-iod-tables: cannot read {path}: {error}") from error
            if current != generated[name]:
                stale.append(str(path))
        if stale:
            raise SystemExit(f"generate-iod-tables: stale generated tables: {', '.join(stale)}")
        print(f"{args.output_dir} tables are up to date with DICOM {EDITION}")
        return
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for name in OUTPUT_FILES:
        (args.output_dir / name).write_text(generated[name], encoding="utf-8")
        print(f"wrote {args.output_dir / name}", file=sys.stderr)


if __name__ == "__main__":
    main()
