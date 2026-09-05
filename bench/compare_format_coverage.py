#!/usr/bin/env python3
"""Entry point for matched CPU-versus-Metal format coverage comparison."""

from __future__ import annotations

import sys
from pathlib import Path


if __package__ in {None, ""}:
    repository_root = str(Path(__file__).resolve().parents[1])
    if repository_root not in sys.path:
        sys.path.insert(0, repository_root)

from bench.format_coverage.compare import main


if __name__ == "__main__":
    raise SystemExit(main())
