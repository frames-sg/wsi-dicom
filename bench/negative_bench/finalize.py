"""Create the exclusive final checksum seal for a completed negative-bench package."""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path, PurePosixPath

if __package__ in {None, ""}:
    sys.dont_write_bytecode = True
    repository_root = str(Path(__file__).resolve().parents[2])
    if repository_root not in sys.path:
        sys.path.insert(0, repository_root)

from bench.file_digest import sha256_file
from bench.negative_bench.identifiers import manifest_identifier_items
from bench.path_identifiers import require_portable_identifier


REQUIRED_FINAL_INPUTS = (
    "manifest.json",
    "generation-SHA256SUMS",
    "observed-results/run-summary.json",
    "analysis/summary.json",
)
REQUIRED_ANALYSIS_OUTPUTS = (
    "adjudication.csv",
    "analysis/USCAP-results.md",
    "analysis/case-validator-matrix.csv",
    "analysis/case-validator-matrix.json",
    "analysis/disagreements.csv",
    "analysis/disagreements.json",
    "analysis/domain-metrics.csv",
    "analysis/domain-metrics.json",
    "analysis/summary.json",
    "analysis/validator-detection.svg",
)
GENERATION_ANALYSIS_INPUTS = frozenset(
    {
        "analysis/adjudications-v1.json",
        "analysis/analyze.py",
        "analysis/analysis_artifacts.py",
        "analysis/file_digest.py",
    }
)


class FinalizationError(RuntimeError):
    """A completed package could not be sealed safely."""


def _generation_path(value: str) -> PurePosixPath:
    relative = PurePosixPath(value)
    if (
        not value
        or "\\" in value
        or relative.is_absolute()
        or value != relative.as_posix()
        or any(part in {"", ".", ".."} for part in relative.parts)
    ):
        raise FinalizationError(f"unsafe generation path: {value!r}")
    return relative


def _is_post_generation(relative: PurePosixPath) -> bool:
    value = relative.as_posix()
    if value in {"generation-SHA256SUMS", "SHA256SUMS", "adjudication.csv"}:
        return True
    if relative.parts[0] in {"observed-results", "validator-results", ".finalize.lock"}:
        return True
    return relative.parts[0] == "analysis" and value not in GENERATION_ANALYSIS_INPUTS


def _generation_owned_files(package: Path) -> dict[str, Path]:
    files: dict[str, Path] = {}
    for path in sorted(package.rglob("*")):
        relative = PurePosixPath(path.relative_to(package).as_posix())
        if path.is_symlink():
            raise FinalizationError(f"package contains a symlink: {relative}")
        if path.is_file() and not _is_post_generation(relative):
            files[relative.as_posix()] = path
    return files


def _verify_generation_sums(package: Path) -> None:
    sums = package / "generation-SHA256SUMS"
    try:
        text = sums.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        raise FinalizationError(f"cannot read generation-SHA256SUMS: {exc}") from exc
    if not text or not text.endswith("\n"):
        raise FinalizationError("generation-SHA256SUMS must be nonempty and newline-terminated")

    entries: dict[str, str] = {}
    ordered_paths: list[str] = []
    for line in text.splitlines():
        if len(line) < 67 or line[64:66] != "  ":
            raise FinalizationError("generation-SHA256SUMS contains an invalid entry")
        digest, raw_path = line[:64], line[66:]
        if any(character not in "0123456789abcdef" for character in digest):
            raise FinalizationError("generation-SHA256SUMS contains an invalid SHA-256")
        relative = _generation_path(raw_path).as_posix()
        if relative in entries:
            raise FinalizationError(f"generation-SHA256SUMS duplicates path: {relative}")
        entries[relative] = digest
        ordered_paths.append(relative)
    if ordered_paths != sorted(ordered_paths):
        raise FinalizationError("generation-SHA256SUMS paths are not sorted")

    generated = _generation_owned_files(package)
    if set(entries) != set(generated):
        missing = sorted(set(generated) - set(entries))
        unexpected = sorted(set(entries) - set(generated))
        raise FinalizationError(
            "generation file set differs from package "
            f"(unsealed={missing}, missing={unexpected})"
        )
    for relative, expected in entries.items():
        if sha256_file(generated[relative]) != expected:
            raise FinalizationError(f"generation SHA-256 mismatch: {relative}")


def _read_object(package: Path, relative: str) -> dict:
    try:
        value = json.loads((package / relative).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise FinalizationError(f"cannot read finalization JSON {relative}: {exc}") from exc
    if not isinstance(value, dict):
        raise FinalizationError(f"finalization JSON must be an object: {relative}")
    return value


def _validate_completed_package(package: Path) -> None:
    manifest = _read_object(package, "manifest.json")
    try:
        controls, cases = manifest_identifier_items(manifest)
    except ValueError as exc:
        raise FinalizationError(str(exc)) from exc
    challenge_id = manifest.get("challenge_id")
    if not isinstance(challenge_id, str) or not challenge_id:
        raise FinalizationError("manifest challenge_id must be a non-empty string")

    expected = {
        **{control["control_id"]: "valid-controls-v1" for control in controls},
        **{case["case_id"]: "evaluation-v1" for case in cases},
    }
    if not expected:
        raise FinalizationError("completed package must contain at least one challenge input")
    run = _read_object(package, "observed-results/run-summary.json")
    if (
        run.get("schema_version") != "wsi-dicom-negative-bench-run-summary-v1"
        or run.get("challenge_id") != challenge_id
        or not isinstance(run.get("executions"), list)
    ):
        raise FinalizationError("run summary schema or challenge does not match manifest")
    observed: dict[str, str] = {}
    for index, execution in enumerate(run["executions"]):
        if not isinstance(execution, dict):
            raise FinalizationError(f"run summary execution {index} must be an object")
        try:
            identifier = require_portable_identifier(
                execution.get("id"), f"run summary execution {index} id"
            )
        except ValueError as exc:
            raise FinalizationError(str(exc)) from exc
        if identifier in observed:
            raise FinalizationError(f"run summary duplicates execution: {identifier}")
        observed[identifier] = execution.get("cohort")
    if observed != expected:
        raise FinalizationError("run summary executions do not match manifest inputs")

    for identifier in expected:
        report_path = (
            package
            / "observed-results"
            / identifier
            / "workbench"
            / "workbench-report.json"
        )
        report = _read_object(package, str(report_path.relative_to(package)))
        if report.get("schema_version") not in {"wsi-dicom-bench-workbench-report-v1", "wsi-dicom-bench-workbench-report-v2"}:
            raise FinalizationError(f"workbench report schema is invalid: {identifier}")

    analysis = _read_object(package, "analysis/summary.json")
    if (
        analysis.get("schema_version") != "wsi-dicom-negative-bench-analysis-v1"
        or analysis.get("challenge_id") != challenge_id
        or analysis.get("evaluation_cases") != len(cases)
        or analysis.get("valid_controls") != len(controls)
    ):
        raise FinalizationError("analysis summary does not match manifest inputs")
    if manifest.get("core_profile"):
        from bench.core_profile import CoreProfileError, load_profile, validate_evidence_coverage
        try:
            profile_path = package / "expected-results" / Path(manifest["core_profile"]["path"]).name
            catalog_path = "expected-results/" + Path(manifest["rule_catalog"]["path"]).name
            validate_evidence_coverage(load_profile(profile_path), _read_object(package, catalog_path), manifest, package)
        except (CoreProfileError, OSError) as exc:
            raise FinalizationError(f"core evidence gate failed: {exc}") from exc
    for relative in REQUIRED_ANALYSIS_OUTPUTS:
        if not (package / relative).is_file():
            raise FinalizationError(f"required analysis artifact does not exist: {relative}")


def finalize_package(package: Path) -> dict:
    package = package.resolve()
    if not package.is_dir():
        raise FinalizationError(f"package does not exist: {package}")
    for relative in REQUIRED_FINAL_INPUTS:
        if not (package / relative).is_file():
            raise FinalizationError(f"required finalization input does not exist: {relative}")
    _verify_generation_sums(package)
    _validate_completed_package(package)

    seal = package / "SHA256SUMS"
    if seal.exists():
        raise FinalizationError(f"final checksum seal already exists: {seal}")
    lock = package / ".finalize.lock"
    try:
        lock.mkdir()
    except FileExistsError as exc:
        raise FinalizationError(f"another finalizer owns the package: {lock}") from exc

    temporary = lock / "SHA256SUMS"
    try:
        if seal.exists():
            raise FinalizationError(f"final checksum seal already exists: {seal}")
        files = []
        for path in sorted(package.rglob("*")):
            if path.is_symlink():
                raise FinalizationError(f"package contains a symlink: {path.relative_to(package)}")
            if path.is_file() and not path.is_relative_to(lock):
                files.append(path)
        lines = [f"{sha256_file(path)}  {path.relative_to(package)}" for path in files]
        with temporary.open("x", encoding="utf-8") as stream:
            stream.write("\n".join(lines) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        try:
            os.link(temporary, seal)
        except FileExistsError as exc:
            raise FinalizationError(f"final checksum seal already exists: {seal}") from exc
    finally:
        temporary.unlink(missing_ok=True)
        lock.rmdir()
    return {
        "schema_version": "wsi-dicom-negative-bench-finalization-v1",
        "files": len(files),
        "sha256sums": "SHA256SUMS",
        "sha256": sha256_file(seal),
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        result = finalize_package(args.package)
    except (FinalizationError, OSError) as exc:
        print(f"finalization failed: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
