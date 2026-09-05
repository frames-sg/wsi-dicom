"""Canonical pretty JSON document serialization for benchmark evidence."""

from __future__ import annotations

import json
from pathlib import Path


def write_json(path: Path, document: object) -> None:
    """Write finite, sorted, newline-terminated JSON."""
    path.write_text(
        json.dumps(document, indent=2, sort_keys=True, allow_nan=False) + "\n",
        encoding="utf-8",
    )
