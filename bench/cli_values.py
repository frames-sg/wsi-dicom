"""Shared command-line value parsers for benchmark entry points."""

from __future__ import annotations

import argparse


def positive_int(value: str) -> int:
    """Parse a strictly positive integer for ``argparse``."""
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("value must be greater than zero")
    return parsed
