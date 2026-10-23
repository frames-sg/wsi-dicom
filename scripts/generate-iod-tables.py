#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only

"""Generate the VL Whole Slide Microscopy IOD requirement tables.

Reads the pinned DICOM DocBook sources (PS3.3 for the IOD, module and macro
tables, PS3.6 for the data dictionary, PS3.10 for the File Meta Information)
and writes src/validation/iod/tables.rs and dictionary.rs deterministically.
Every 1C/2C condition, conditional module or functional group usage,
conditional enumeration and Type 3 presence restriction becomes a ConditionId
variant; the validator matches on that enum exhaustively, so a changed or new
condition text in another edition fails to compile until a predicate is
written for it.

Sources are parsed only after their SHA-256 matches the pinned digest, so the
XML parser never sees unpinned input.
"""

import argparse
import hashlib
import re
import sys
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path


EDITION = "2026c"
SOURCES = {
    "part03.xml": "7911d6ec05872a38255d12ad628c1c940e9c905bb8cc3785ee039e7f6632dd48",
    "part06.xml": "ff1dcdfb557d57db96420614fcaf6d739bb76aa74b73eba77f367be9fab0be3e",
    "part10.xml": "f28d8d3a8c3fc998bf0c64db5525d8744ccd0f064e6ab7e1de24b29fcc032762",
}
SOURCE_URL = "https://dicom.nema.org/medical/dicom/{edition}/source/docbook/{part}/{name}"
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT_DIR = REPO_ROOT / "src" / "validation" / "iod"
OUTPUT_FILES = ("tables.rs", "dictionary.rs")
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


class GenerationError(Exception):
    pass


def arguments():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--source-dir",
        type=Path,
        required=True,
        help="directory holding the pinned part03.xml, part06.xml and part10.xml",
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


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fetch_source(directory, name):
    part = name.removesuffix(".xml")
    url = SOURCE_URL.format(edition=EDITION, part=part, name=name)
    with urllib.request.urlopen(url, timeout=120) as response:
        body = response.read(MAX_SOURCE_BYTES + 1)
    if len(body) > MAX_SOURCE_BYTES:
        raise GenerationError(f"{url} exceeds {MAX_SOURCE_BYTES} bytes")
    if hashlib.sha256(body).hexdigest() != SOURCES[name]:
        raise GenerationError(f"{url} does not match the pinned SHA-256 for {name}")
    directory.mkdir(parents=True, exist_ok=True)
    (directory / name).write_bytes(body)


def verified_sources(directory, fetch):
    paths = {}
    for name, expected in SOURCES.items():
        path = directory / name
        if not path.is_file() and fetch:
            fetch_source(directory, name)
        if not path.is_file():
            raise GenerationError(f"missing source {path}; pass --fetch to download it")
        actual = sha256(path)
        if actual != expected:
            raise GenerationError(
                f"{path} has SHA-256 {actual}, expected pinned {EDITION} digest {expected}"
            )
        paths[name] = path
    return paths


class Document:
    """A DocBook part with id lookup and text rendering."""

    def __init__(self, path):
        self.root = ET.parse(path).getroot()
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
                parts.append(inner or f"{node.get('targetdoc')} {node.get('targetptr')}")
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


def normalize(text):
    return re.sub(r"\s+", " ", text.replace("​", "")).strip()


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


class Model:
    def __init__(self, part03, part06, part10):
        self.part03 = part03
        self.dictionary = parse_dictionary(part06)
        self.tables = {}
        self.order = []
        self.conditions = {}
        self.condition_order = []
        self.modules = self.parse_modules()
        self.functional_groups = self.parse_functional_groups()
        self.file_meta = self.parse_file_meta(part10)

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
                for target in targets:
                    record["rows"].append(
                        {"kind": "include", "level": len(include.group(1)), "target": target}
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
                }
            )

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
        top_level = {}
        for module in modules:
            for tag in self.top_level_tags(module["table"]):
                top_level.setdefault(tag, set()).add(module["name"])
        for module in modules:
            module["signature"] = sorted(
                tag for tag in self.top_level_tags(module["table"]) if top_level[tag] == {module["name"]}
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
                out.append(f"{pad}i(&{rust_ident(row['target'])}),")
                index = end
                continue
            head = (
                f"{pad}a({tag_hex(row['tag'])}, {rust_string(row['name'])}, "
                f"T{row['type']}, {optional_condition(row['condition'])}, "
                f"{self.enumerations(row)}, {row['items']}, "
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
        emit("    Placement, Table, Usage,")
        emit("};")
        emit("use ConditionId as C;")
        emit("use ItemCount::{AtLeast, AtMost, Exactly, NumberOfFrames, Unconstrained as ANY, ValueOf};")
        emit()
        emit(f"pub(super) const EDITION: &str = {rust_string(EDITION)};")
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
        emit("const T1: AttributeType = AttributeType::One;")
        emit("const T1C: AttributeType = AttributeType::OneC;")
        emit("const T2: AttributeType = AttributeType::Two;")
        emit("const T2C: AttributeType = AttributeType::TwoC;")
        emit("const T3: AttributeType = AttributeType::Three;")
        emit()
        emit("const fn t(")
        emit("    part: &'static str,")
        emit("    label: &'static str,")
        emit("    section: &'static str,")
        emit("    nodes: &'static [Node],")
        emit(") -> Table {")
        emit("    Table { part, label, section, nodes }")
        emit("}")
        emit()
        emit("const fn i(table: &'static Table) -> Node {")
        emit("    Node::Include(table)")
        emit("}")
        emit()
        emit("const fn a(")
        emit("    tag: u32,")
        emit("    name: &'static str,")
        emit("    kind: AttributeType,")
        emit("    condition: Option<ConditionId>,")
        emit("    enumerations: &'static [Enumeration],")
        emit("    items: ItemCount,")
        emit("    children: &'static [Node],")
        emit(") -> Node {")
        emit("    Node::Attribute(Attribute { tag, name, kind, condition, enumerations, items, children })")
        emit("}")
        emit()
        emit("const fn e(")
        emit("    value: Option<u8>,")
        emit("    condition: Option<ConditionId>,")
        emit("    values: &'static [&'static str],")
        emit(") -> Enumeration {")
        emit("    Enumeration { value, condition, values }")
        emit("}")
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
    paths = verified_sources(source_dir, fetch)
    part03 = Document(paths["part03.xml"])
    part06 = Document(paths["part06.xml"])
    part10 = Document(paths["part10.xml"])
    model = Model(part03, part06, part10)
    digests = {name: SOURCES[name] for name in sorted(SOURCES)}
    return {
        "tables.rs": Emitter(model).render(digests),
        "dictionary.rs": Emitter(model).dictionary(digests),
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
