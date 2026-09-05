"""Typed context and result boundaries for one negative-bench mutation."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any


class GenerationError(RuntimeError):
    """The challenge could not be generated without violating its lock or limits."""


@dataclass
class MutationContext:
    case: dict
    source: Path
    input_dir: Path
    dataset: Any
    changes: list[dict]

    @property
    def name(self) -> str:
        return self.case["mutation_name"]

    @property
    def seed(self) -> str:
        return self.case["deterministic_seed"]

    @property
    def parameters(self) -> dict:
        return self.case["mutation_parameters"]

    @property
    def default_output(self) -> Path:
        return self.input_dir / "instance.dcm"


@dataclass
class MutationResult:
    outputs: list[Path]
    changes: list[dict]
