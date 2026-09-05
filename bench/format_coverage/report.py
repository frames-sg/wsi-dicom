"""Format-coverage artifact rendering and checksum finalization."""

from __future__ import annotations

from pathlib import Path

from bench.file_digest import sha256_file

from .manifest import FormatCoverageError, validate_relative_path


def render_summary(report: dict) -> str:
    lines = [
        "# WSI-DICOM Bench format coverage v2",
        "",
        f"- Status: `{report['status']}`",
        f"- Cases: {len(report['cases'])}",
        "- Scope: bounded native-level conversion and DICOM-side workbench evaluation",
        "",
        "| Format | Case | Level | Expected | Route | Source stage | Workbench | Result |",
        "| --- | --- | ---: | --- | --- | --- | --- | --- |",
    ]
    for case in report["cases"]:
        workbench_status = (
            case["workbench"]["status"] if case["workbench"] else "not_applicable"
        )
        lines.append(
            f"| {case['format']} | `{case['id']}` | {case['level']} | "
            f"{case['expected']['outcome']} | {case['conversion']['route_classification']} | "
            f"{case['conversion']['source_stage']['classification']} | {workbench_status} | "
            f"{case['status']} |"
        )
    lines.extend(
        [
            "",
            "## Interpretation boundary",
            "",
            "A pass establishes conversion and bounded DICOM-side evaluation only for the pinned source artifact and native level listed. It does not establish support for every vendor variant, complete pyramids, source-pixel equality, scanner calibration accuracy, or receiver interoperability.",
            "",
        ]
    )
    return "\n".join(lines)


def write_checksums(root: Path) -> None:
    lines = []
    for path in sorted(path for path in root.rglob("*") if path.is_file()):
        if path.name == "SHA256SUMS":
            continue
        lines.append(f"{sha256_file(path)}  {path.relative_to(root)}")
    (root / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")


def verify_checksums(root: Path) -> None:
    seal = root / "SHA256SUMS"
    if not seal.is_file():
        raise FormatCoverageError(f"missing format coverage checksum seal: {seal}")
    entries: dict[str, str] = {}
    for line in seal.read_text(encoding="utf-8").splitlines():
        if len(line) < 67 or line[64:66] != "  ":
            raise FormatCoverageError(f"invalid checksum entry in {seal}")
        digest, raw_path = line[:64], line[66:]
        if any(character not in "0123456789abcdef" for character in digest):
            raise FormatCoverageError(f"invalid SHA-256 in {seal}")
        relative = validate_relative_path(raw_path, "checksum path").as_posix()
        if relative in entries:
            raise FormatCoverageError(f"duplicate checksum path in {seal}: {relative}")
        entries[relative] = digest
    actual = {}
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise FormatCoverageError(f"format coverage bundle contains a symlink: {path}")
        if path.is_file() and path != seal:
            actual[path.relative_to(root).as_posix()] = path
    if set(entries) != set(actual):
        raise FormatCoverageError(f"checksum file set differs from bundle: {root}")
    for relative, expected in entries.items():
        if sha256_file(actual[relative]) != expected:
            raise FormatCoverageError(f"checksum mismatch in bundle: {relative}")
