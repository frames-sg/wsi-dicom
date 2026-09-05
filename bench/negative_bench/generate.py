"""Generate a locked WSI-DICOM Negative Bench package atomically."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

if __package__ in {None, ""}:
    sys.dont_write_bytecode = True
    repository_root = str(Path(__file__).resolve().parents[2])
    if repository_root not in sys.path:
        sys.path.insert(0, repository_root)

import pydicom

from bench.file_digest import sha256_file
from bench.core_profile import CoreProfileError, load_profile, validate_profile_coverage
from bench.json_document import write_json
from bench.negative_bench.identifiers import manifest_identifier_items
from bench.negative_bench.model import GenerationError
from bench.negative_bench.mutations import generate_case as _generate_case
from bench.negative_bench.mutations.dicom import deterministic_uid
from bench.path_identifiers import require_portable_identifier


EXPECTED_PYDICOM_VERSION = "3.0.2"
VL_WSI_SOP_CLASS_UID = "1.2.840.10008.5.1.4.1.1.77.1.6"


def _find_repository_root(path: Path) -> Path | None:
    for candidate in [path.parent, *path.parents]:
        if (candidate / "bench" / "negative_bench" / "generate.py").is_file() and (
            candidate / "rules"
        ).is_dir():
            return candidate
    return None


def load_manifest(path: Path) -> dict:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise GenerationError(f"cannot load manifest {path}: {exc}") from exc
    try:
        controls, cases = manifest_identifier_items(manifest)
    except ValueError as exc:
        raise GenerationError(str(exc)) from exc
    if manifest.get("schema_version") not in {
        "wsi-dicom-negative-bench-manifest-v1",
        "wsi-dicom-negative-bench-manifest-v2",
        "wsi-dicom-negative-bench-manifest-v3",
        "wsi-dicom-negative-bench-manifest-v4",
    }:
        raise GenerationError("unsupported challenge manifest schema")
    limits = manifest.get("resource_limits")
    if not isinstance(limits, dict):
        raise GenerationError("challenge manifest resource_limits must be an object")
    maximums = {}
    for key in ("max_controls", "max_cases"):
        value = limits.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise GenerationError(f"challenge manifest {key} must be a non-negative integer")
        maximums[key] = value
    if len(controls) > maximums["max_controls"]:
        raise GenerationError("control count exceeds manifest limit")
    if len(cases) > maximums["max_cases"]:
        raise GenerationError("case count exceeds manifest limit")
    control_ids = {item["control_id"] for item in controls}
    for index, case in enumerate(cases):
        source_ids = case.get("source_control_ids")
        if not isinstance(source_ids, list) or len(source_ids) != 1:
            raise GenerationError(
                f"challenge manifest case {index} must reference exactly one source control"
            )
        try:
            source_id = require_portable_identifier(
                source_ids[0], f"challenge manifest case {index} source control"
            )
        except ValueError as exc:
            raise GenerationError(str(exc)) from exc
        if source_id not in control_ids:
            raise GenerationError(
                f"challenge manifest case {index} references unknown control: {source_id}"
            )
    return manifest


def generate_challenge(manifest_path: Path, output: Path) -> None:
    manifest_path = manifest_path.resolve()
    output = output.resolve()
    if output.exists():
        raise GenerationError(f"output path already exists: {output}")
    if pydicom.__version__ != EXPECTED_PYDICOM_VERSION:
        raise GenerationError(
            f"pydicom {EXPECTED_PYDICOM_VERSION} is required, found {pydicom.__version__}"
        )
    manifest = load_manifest(manifest_path)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=f".{output.name}.staging-", dir=output.parent
    ) as temporary:
        staging = Path(temporary)
        _generate_into(manifest_path, manifest, staging)
        os.replace(staging, output)


def _generate_into(manifest_path: Path, manifest: dict, staging: Path) -> None:
    limits = manifest["resource_limits"]
    max_file_bytes = int(limits["max_file_bytes"])
    controls_dir = staging / "controls"
    cases_dir = staging / "cases"
    expected_dir = staging / "expected-results"
    for directory in [
        controls_dir,
        cases_dir,
        expected_dir,
        staging / "observed-results",
        staging / "validator-results",
        staging / "generator",
        staging / "analysis",
    ]:
        directory.mkdir(parents=True, exist_ok=True)

    controls = _materialize_controls(
        manifest_path, manifest, controls_dir, max_file_bytes
    )
    _materialize_cases(manifest, controls, cases_dir, max_file_bytes)
    repository = _copy_locked_evidence(
        manifest_path, manifest, staging, expected_dir
    )
    _copy_packaged_runtime(manifest_path, repository, staging)
    _write_generation_sums(staging)


def _materialize_controls(
    manifest_path: Path,
    manifest: dict,
    controls_dir: Path,
    max_file_bytes: int,
) -> dict[str, list[Path]]:
    controls: dict[str, list[Path]] = {}
    for control in manifest["valid_controls"]:
        if "original_paths" in control:
            originals = control.get("original_paths")
            packages = control.get("package_paths")
            sizes = control.get("size_bytes")
            digests = control.get("sha256")
        else:
            originals = [control.get("original_path")]
            packages = [
                control.get(
                    "package_path", f"controls/{control['control_id']}.dcm"
                )
            ]
            sizes = [control.get("size_bytes")]
            digests = [control.get("sha256")]
        if not all(isinstance(values, list) for values in (originals, packages, sizes, digests)):
            raise GenerationError(
                f"control file metadata is invalid: {control['control_id']}"
            )
        lengths = {len(originals), len(packages), len(sizes), len(digests)}
        if lengths != {len(originals)} or not originals:
            raise GenerationError(
                f"control file metadata lengths differ: {control['control_id']}"
            )
        copied = []
        for original, package_path, expected_size, expected_digest in zip(
            originals, packages, sizes, digests, strict=True
        ):
            if not isinstance(original, str) or not isinstance(package_path, str):
                raise GenerationError(
                    f"control file paths are invalid: {control['control_id']}"
                )
            package = Path(package_path)
            if package.is_absolute() or ".." in package.parts or package.parts[:1] != ("controls",):
                raise GenerationError(
                    f"control package path is unsafe: {package_path}"
                )
            source = Path(original)
            if not source.is_file():
                source = manifest_path.parent / package
            if not source.is_file():
                raise GenerationError(f"control source does not exist: {source}")
            size = source.stat().st_size
            if size != int(expected_size) or size > max_file_bytes:
                raise GenerationError(f"control size differs from manifest: {source}")
            digest = sha256_file(source)
            if digest != expected_digest:
                raise GenerationError(f"control SHA-256 differs from manifest: {source}")
            _reject_identifying_metadata(source)
            destination = controls_dir.parent / package
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
            if sha256_file(destination) != digest:
                raise GenerationError(f"copied control SHA-256 mismatch: {destination}")
            copied.append(destination)
        controls[control["control_id"]] = copied

    return controls


def _materialize_cases(
    manifest: dict,
    controls: dict[str, list[Path]],
    cases_dir: Path,
    max_file_bytes: int,
) -> None:
    for case in manifest["evaluation_cases"]:
        case_dir = cases_dir / case["case_id"]
        input_dir = case_dir / "input"
        input_dir.mkdir(parents=True)
        source_id = case["source_control_ids"][0]
        sources = controls.get(source_id)
        if sources is None:
            raise GenerationError(f"case references unknown control: {source_id}")
        derived_pair = case["mutation_name"] == "remove_derivation_image" and [p.name for p in sources] == ["level-0.dcm", "level-1.dcm"]
        if len(sources) != 1 and not derived_pair:
            raise GenerationError(
                f"case source must be a single-file control: {case['case_id']}"
            )
        source = sources[0]
        actual_source_digest = [sha256_file(path) for path in sources] if derived_pair else sha256_file(source)
        if actual_source_digest != case["source_control_sha256"]:
            raise GenerationError(f"case source SHA-256 mismatch: {case['case_id']}")
        outputs, change_log = _generate_case(case, source, input_dir)
        for path in outputs:
            if path.stat().st_size > max_file_bytes:
                raise GenerationError(f"generated file exceeds size limit: {path}")
            _reject_identifying_metadata(path)
        report = {
            "schema_version": "wsi-dicom-negative-bench-generation-report-v1",
            "case_id": case["case_id"],
            "source_control_id": source_id,
            "source_control_sha256": case["source_control_sha256"],
            "mutation_name": case["mutation_name"],
            "deterministic_seed": case["deterministic_seed"],
            "changes": change_log,
            "outputs": [
                {
                    "path": str(path.relative_to(case_dir)),
                    "size_bytes": path.stat().st_size,
                    "sha256": sha256_file(path),
                }
                for path in outputs
            ],
        }
        write_json(case_dir / "generation.json", report)


def _copy_locked_evidence(
    manifest_path: Path,
    manifest: dict,
    staging: Path,
    expected_dir: Path,
) -> Path | None:
    shutil.copyfile(manifest_path, staging / "manifest.json")
    protocol = manifest_path.with_name(manifest.get("protocol", "protocol-v1.md"))
    if not protocol.is_file():
        protocol = manifest_path.with_name("protocol.md")
    if not protocol.is_file():
        raise GenerationError(f"protocol does not exist: {protocol}")
    shutil.copyfile(protocol, staging / "protocol.md")
    repository = _find_repository_root(manifest_path)
    catalog = (
        repository / manifest["rule_catalog"]["path"]
        if repository is not None
        else Path("__repository_not_available__")
    )
    if not catalog.is_file():
        catalog = manifest_path.parent / "expected-results" / Path(
            manifest["rule_catalog"]["path"]
        ).name
    if not catalog.is_file():
        raise GenerationError(f"rule catalog does not exist: {catalog}")
    shutil.copyfile(catalog, expected_dir / catalog.name)
    profile = None
    core_profile = manifest.get("core_profile")
    if core_profile is not None:
        profile_path = core_profile.get("path") if isinstance(core_profile, dict) else None
        if not isinstance(profile_path, str) or not profile_path:
            raise GenerationError("core_profile.path must be a non-empty string")
        profile = (
            repository / profile_path
            if repository is not None
            else Path("__repository_not_available__")
        )
        if not profile.is_file():
            profile = manifest_path.parent / "expected-results" / Path(profile_path).name
        if not profile.is_file():
            raise GenerationError(f"core profile does not exist: {profile}")
        try:
            validate_profile_coverage(
                load_profile(profile),
                json.loads(catalog.read_text(encoding="utf-8")),
                manifest,
            )
        except (CoreProfileError, json.JSONDecodeError) as exc:
            raise GenerationError(f"core-profile coverage gate failed: {exc}") from exc
        shutil.copyfile(profile, expected_dir / profile.name)
    lock = manifest_path.with_name(
        manifest.get("expected_results_lock", "expected-results-lock-v1.json")
    )
    if not lock.is_file():
        lock = manifest_path.parent / "expected-results" / lock.name
    if not lock.is_file():
        raise GenerationError(f"expected-results lock does not exist: {lock}")
    _verify_expected_results_lock(lock, manifest_path, protocol, catalog, profile)
    shutil.copyfile(lock, expected_dir / lock.name)
    return repository


def _copy_packaged_runtime(
    manifest_path: Path,
    repository: Path | None,
    staging: Path,
) -> None:
    shutil.copyfile(Path(__file__), staging / "generator" / "generate.py")
    packaged_source = manifest_path.parent / "generator"
    source_root = repository or Path("__repository_not_available__")
    run_source = source_root / "bench" / "negative_bench" / "run.py"
    finalize_source = source_root / "bench" / "negative_bench" / "finalize.py"
    analyze_source = source_root / "bench" / "negative_bench" / "analyze.py"
    workbench_source = source_root / "bench" / "wsi_dicom_bench.py"
    requirements_source = source_root / "bench" / "negative_bench" / "requirements.txt"
    bench_source = source_root / "bench"
    if not run_source.is_file():
        run_source = packaged_source / "run.py"
        finalize_source = packaged_source / "finalize.py"
        analyze_source = manifest_path.parent / "analysis" / "analyze.py"
        workbench_source = packaged_source / "wsi_dicom_bench.py"
        requirements_source = packaged_source / "requirements.txt"
        bench_source = packaged_source / "bench"
    shutil.copyfile(run_source, staging / "generator" / "run.py")
    shutil.copyfile(finalize_source, staging / "generator" / "finalize.py")
    shutil.copyfile(
        analyze_source,
        staging / "analysis" / "analyze.py",
    )
    shutil.copyfile(
        analyze_source.with_name("analysis_artifacts.py"),
        staging / "analysis" / "analysis_artifacts.py",
    )
    shutil.copyfile(
        bench_source / "file_digest.py",
        staging / "analysis" / "file_digest.py",
    )
    adjudications_source = analyze_source.with_name("adjudications-v1.json")
    if not adjudications_source.is_file():
        raise GenerationError(f"adjudications do not exist: {adjudications_source}")
    shutil.copyfile(
        adjudications_source,
        staging / "analysis" / "adjudications-v1.json",
    )
    shutil.copyfile(workbench_source, staging / "generator" / "wsi_dicom_bench.py")
    shutil.copyfile(requirements_source, staging / "generator" / "requirements.txt")
    packaged_bench = staging / "generator" / "bench"
    packaged_bench.mkdir()
    shutil.copyfile(bench_source / "__init__.py", packaged_bench / "__init__.py")
    shutil.copyfile(bench_source / "cli_values.py", packaged_bench / "cli_values.py")
    shutil.copyfile(bench_source / "file_digest.py", packaged_bench / "file_digest.py")
    shutil.copyfile(bench_source / "core_profile.py", packaged_bench / "core_profile.py")
    shutil.copyfile(bench_source / "json_document.py", packaged_bench / "json_document.py")
    shutil.copyfile(
        bench_source / "path_identifiers.py", packaged_bench / "path_identifiers.py"
    )
    shutil.copyfile(
        bench_source / "process_evidence.py", packaged_bench / "process_evidence.py"
    )
    packaged_negative_bench = packaged_bench / "negative_bench"
    packaged_negative_bench.mkdir()
    shutil.copyfile(
        bench_source / "negative_bench" / "__init__.py",
        packaged_negative_bench / "__init__.py",
    )
    shutil.copyfile(
        bench_source / "negative_bench" / "model.py",
        packaged_negative_bench / "model.py",
    )
    shutil.copyfile(
        bench_source / "negative_bench" / "identifiers.py",
        packaged_negative_bench / "identifiers.py",
    )
    shutil.copytree(
        bench_source / "negative_bench" / "mutations",
        packaged_negative_bench / "mutations",
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
    )
    shutil.copytree(
        bench_source / "workbench",
        packaged_bench / "workbench",
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
    )
    package_assets = source_root / "bench" / "negative_bench" / "package"
    if not package_assets.is_dir():
        package_assets = manifest_path.parent
    if package_assets.is_dir():
        for name in ["README.md", "LICENSE", "CITATION.cff", ".zenodo.json"]:
            shutil.copyfile(package_assets / name, staging / name)
    if repository is not None:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        challenge = manifest["challenge_id"]
        title = manifest.get("title", challenge)
        description = f"{len(manifest['evaluation_cases'])} authored negative cases and {len(manifest['valid_controls'])} controls; catalog {manifest['rule_catalog']['catalog_version']}. Selected rule-family evidence, not certification or exhaustive normative coverage."
        (staging / "README.md").write_text(
            f"# {title}\n\n{description}\n\nUnpublished research package. See protocol.md, manifest.json, and expected-results for scope and locked expectations.\n\n"
            "Reproduce into a new directory with `python generator/generate.py --manifest manifest.json --output ../reproduction`. "
            "Run with `python generator/run.py --package . --workbench generator/wsi_dicom_bench.py --wsi-dicom /absolute/path/to/wsi-dicom`. "
            "Then run `python analysis/analyze.py --package .` and `python generator/finalize.py --package .`. "
            "Finalization validates execution evidence before sealing. Sealed packages must not be edited.\n",
            encoding="utf-8",
        )
        import re
        citation = (staging / "CITATION.cff").read_text(encoding="utf-8")
        for key, value in (("title", title), ("version", challenge)):
            citation = re.sub(rf"^{key}:.*$", f"{key}: {json.dumps(value)}", citation, flags=re.MULTILINE)
        (staging / "CITATION.cff").write_text(citation, encoding="utf-8")
        metadata = json.loads((staging / ".zenodo.json").read_text(encoding="utf-8"))
        metadata.update(title=title, version=challenge, description=description)
        (staging / ".zenodo.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _reject_identifying_metadata(path: Path) -> None:
    dataset = pydicom.dcmread(
        path,
        stop_before_pixels=True,
        specific_tags=["SOPClassUID", "PatientName", "PatientID"],
    )
    if str(getattr(dataset, "SOPClassUID", "")) != VL_WSI_SOP_CLASS_UID:
        raise GenerationError(f"control is not VL Whole Slide Microscopy: {path}")
    patient_name = str(getattr(dataset, "PatientName", "")).strip().upper()
    patient_id = str(getattr(dataset, "PatientID", "")).strip().upper()
    allowed = {"", "RESEARCH", "RESEARCH^PLACEHOLDER", "ANONYMOUS", "ANON"}
    if patient_name not in allowed or patient_id not in allowed:
        raise GenerationError(f"control failed patient identity policy: {path}")


def _verify_expected_results_lock(
    lock_path: Path,
    manifest_path: Path,
    protocol_path: Path,
    catalog_path: Path,
    profile_path: Path | None = None,
) -> None:
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest_schema = manifest.get("schema_version")
    version = {
        "wsi-dicom-negative-bench-manifest-v1": "v1",
        "wsi-dicom-negative-bench-manifest-v2": "v2",
        "wsi-dicom-negative-bench-manifest-v3": "v3",
        "wsi-dicom-negative-bench-manifest-v4": "v4",
    }.get(manifest_schema)
    if version is None:
        raise GenerationError("expected-results lock received an unsupported manifest")
    if lock.get("schema_version") != f"wsi-dicom-negative-bench-expected-results-lock-{version}":
        raise GenerationError("expected-results lock schema does not match the manifest")
    if lock.get("challenge_id") != manifest.get("challenge_id"):
        raise GenerationError("expected-results lock challenge does not match the manifest")

    expected_counts = {
        "development_cases": len(manifest.get("development_cases", [])),
        "evaluation_cases": len(manifest.get("evaluation_cases", [])),
        "valid_controls": len(manifest.get("valid_controls", [])),
        "compound_cases": len(manifest.get("compound_cases", [])),
    }
    if lock.get("expected_counts") != expected_counts:
        raise GenerationError("expected-results lock counts do not match the manifest")

    files = lock.get("files")
    expected_file_count = 4 if version in {"v3", "v4"} else 3
    if not isinstance(files, list) or len(files) != expected_file_count:
        raise GenerationError(
            f"expected-results lock must contain exactly {expected_file_count} files"
        )
    required_paths = {
        f"bench/negative_bench/{Path(manifest['protocol']).name}",
        f"bench/negative_bench/manifest-{version}.json",
        manifest["rule_catalog"]["path"],
    }
    if version in {"v3", "v4"}:
        profile_reference = manifest.get("core_profile", {}).get("path")
        if not isinstance(profile_reference, str) or profile_path is None:
            raise GenerationError("v3 expected-results lock requires a core profile")
        required_paths.add(profile_reference)
    expected: dict[str, str] = {}
    for item in files:
        if not isinstance(item, dict) or set(item) != {"path", "sha256"}:
            raise GenerationError("expected-results lock file entries are invalid")
        path = item["path"]
        digest = item["sha256"]
        if path in expected:
            raise GenerationError(f"expected-results lock duplicates path: {path}")
        if (
            not isinstance(path, str)
            or not isinstance(digest, str)
            or len(digest) != 64
            or any(character not in "0123456789abcdef" for character in digest)
        ):
            raise GenerationError("expected-results lock file provenance is invalid")
        expected[path] = digest
    if set(expected) != required_paths:
        raise GenerationError("expected-results lock file set does not match the manifest")

    actual = {
        f"bench/negative_bench/{Path(manifest['protocol']).name}": sha256_file(
            protocol_path
        ),
        f"bench/negative_bench/manifest-{version}.json": sha256_file(manifest_path),
        manifest["rule_catalog"]["path"]: sha256_file(catalog_path),
    }
    if version in {"v3", "v4"}:
        actual[manifest["core_profile"]["path"]] = sha256_file(profile_path)
        try:
            profile = load_profile(profile_path)
        except CoreProfileError as exc:
            raise GenerationError(f"cannot load locked core profile: {exc}") from exc
        expected_profile_lock = {
            "profile_id": profile["profile_id"],
            "version": profile["profile_version"],
            "sha256": actual[manifest["core_profile"]["path"]],
        }
        if lock.get("core_profile") != expected_profile_lock:
            raise GenerationError(
                "expected-results lock core-profile identity, version, or digest is invalid"
            )
    if expected != actual:
        raise GenerationError(
            "expected-results lock does not match protocol, manifest, catalog, and profile"
        )


def _write_generation_sums(root: Path) -> None:
    entries = []
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        if path.name == "generation-SHA256SUMS":
            continue
        entries.append(f"{sha256_file(path)}  {path.relative_to(root)}")
    (root / "generation-SHA256SUMS").write_text("\n".join(entries) + "\n", encoding="utf-8")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        generate_challenge(args.manifest, args.output)
    except (GenerationError, OSError, ValueError) as exc:
        print(f"generation failed: {exc}", file=sys.stderr)
        return 2
    print(json.dumps({"status": "generated", "output": str(args.output.resolve())}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
