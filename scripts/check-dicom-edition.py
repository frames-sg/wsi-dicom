#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only

"""Fail when NEMA publishes a DICOM edition newer than the pinned one.

The pinned edition is EDITION in scripts/generate-iod-tables.py. The current
edition is read from the title of the PS3.3 DocBook that NEMA serves under
current/, so the check needs only the first bytes of that file.
"""

import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

GENERATOR = Path(__file__).resolve().parent / "generate-iod-tables.py"
CURRENT_PART03 = "https://dicom.nema.org/medical/dicom/current/source/docbook/part03/part03.xml"
EDITION_NAME = re.compile(r"\b(\d{4})([a-e])\b")
HEAD_BYTES = 8192


def pinned_edition(generator_source):
    match = re.search(r'^EDITION = "(\d{4}[a-e])"$', generator_source, re.MULTILINE)
    if match is None:
        raise SystemExit(f"no EDITION assignment in {GENERATOR}")
    return match.group(1)


def published_edition(docbook_head):
    """The edition in the PS3.3 title, e.g. "DICOM PS3.3 2026d - Information Object Definitions"."""
    match = re.search(r"DICOM PS3\.3 (\d{4}[a-e])\b", docbook_head)
    if match is None:
        raise SystemExit("the current PS3.3 DocBook names no edition in its title")
    return match.group(1)


def order(edition):
    match = EDITION_NAME.fullmatch(edition)
    if match is None:
        raise SystemExit(f"{edition!r} is not a DICOM edition name")
    return int(match.group(1)), match.group(2)


def fetch_head():
    request = urllib.request.Request(CURRENT_PART03, headers={"Range": f"bytes=0-{HEAD_BYTES - 1}"})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read(HEAD_BYTES).decode("utf-8", errors="replace")
    except (OSError, urllib.error.URLError) as error:
        raise SystemExit(f"cannot read {CURRENT_PART03}: {error}")


def main():
    pinned = pinned_edition(GENERATOR.read_text(encoding="utf-8"))
    current = published_edition(fetch_head())
    if order(current) > order(pinned):
        raise SystemExit(
            f"DICOM {current} is published and validation pins {pinned}: update EDITION and the "
            f"SOURCES digests in {GENERATOR.name}, regenerate, and disposition the changed statements"
        )
    if order(current) < order(pinned):
        raise SystemExit(f"NEMA serves {current} as current, older than the pinned {pinned}")
    print(f"DICOM {pinned} is the current edition")


if __name__ == "__main__":
    sys.exit(main())
