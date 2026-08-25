#!/usr/bin/env python3
"""Compatibility entry point for the modular GDC WSI benchmark harness."""

from __future__ import annotations

import sys
from pathlib import Path


if __package__ in {None, ""}:
    repository_root = str(Path(__file__).resolve().parents[1])
    if repository_root not in sys.path:
        sys.path.insert(0, repository_root)

from bench.gdc.cli import default_python_command, main, parse_args
from bench.gdc.commands import (
    build_wsi_dicom_command,
    build_wsi_dicom_profile_command,
    build_wsidicomizer_command,
    command_for_tool,
)
from bench.gdc.discovery import (
    discover_gdc_slides,
    manifest_entry_for_slide,
    parse_manifest,
    read_slide_metadata,
    select_slides,
    slide_to_json,
)
from bench.gdc.environment import (
    cargo_package_version,
    collect_environment,
    command_output,
    host_accelerator_info,
    python_package_version,
)
from bench.gdc.models import (
    PROFILE_CHOICES,
    SCOPE_CHOICES,
    SUPPORTED_SUFFIXES,
    TOOL_CHOICES,
    ManifestEntry,
    Slide,
    safe_slug,
    split_command,
)
from bench.gdc.outputs import (
    collect_dicom_outputs,
    count_output_files,
    dicom_metadata_from_dataset,
)
from bench.gdc.preflight import (
    evaluate_device_preflight,
    preflight_failure_row,
    run_device_preflight,
)
from bench.gdc.reporting import (
    append_jsonl,
    attach_result_context,
    average_passed_seconds,
    completed_result_keys,
    format_seconds,
    format_speedup,
    has_rows_for_cell,
    infer_result_label,
    read_jsonl,
    render_markdown_summary,
    result_label_sort_key,
    rows_from_result_dirs,
    status_summary,
    write_csv,
    write_json,
    write_planned_commands,
)
from bench.gdc.runner import run_command
from bench.gdc.trial import benchmark_trial
from bench.gdc.validation import (
    first_text_line,
    profile_metric,
    read_profile_report,
    validate_output,
    validation_failure_message,
)


if __name__ == "__main__":
    raise SystemExit(main())
