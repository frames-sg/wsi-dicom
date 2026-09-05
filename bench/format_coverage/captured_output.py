"""Read and interpret bounded subprocess output artifacts."""

from __future__ import annotations

import json
from pathlib import Path


def parse_json_output(path: Path) -> object | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return None
