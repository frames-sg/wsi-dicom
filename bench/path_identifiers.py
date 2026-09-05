"""Portable one-component identifiers for benchmark filesystem boundaries."""

from __future__ import annotations

import re


PORTABLE_IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}\Z")


def require_portable_identifier(value: object, label: str) -> str:
    if not isinstance(value, str) or PORTABLE_IDENTIFIER.fullmatch(value) is None:
        raise ValueError(
            f"{label} must be a safe identifier using 1-128 ASCII letters, digits, "
            "'.', '_', or '-'"
        )
    return value
