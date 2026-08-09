#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Bind release artifacts, SPDX SBOMs, toolchains, and validation evidence."""

import argparse
import hashlib
import json
import os
import re
import tempfile
from pathlib import Path


HEX_40 = re.compile(r"[0-9a-f]{40}\Z")
HEX_64 = re.compile(r"[0-9a-f]{64}\Z")


def arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--lockfile", type=Path, required=True)
    parser.add_argument("--validators", type=Path, required=True)
    parser.add_argument("--workflow-identity", required=True)
    parser.add_argument("--approval-state", required=True)
    parser.add_argument("--syft-version", required=True)
    parser.add_argument("--syft-archive-sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read JSON evidence {path}: {error}") from error


def validator_record(path, result):
    return {
        "result": result,
        "evidence_file": path.name,
        "sha256": sha256(path),
        "bytes": path.stat().st_size,
    }


def doctor_result(document):
    tools = document.get("tools")
    if not isinstance(tools, list) or not tools:
        raise SystemExit("doctor evidence has no tool results")
    return "failed" if any(tool.get("status") == "failed" for tool in tools) else "passed"


def self_test_result(document):
    checks = document.get("validation_report", {}).get("checks")
    if not isinstance(checks, list) or not checks:
        raise SystemExit("self-test evidence has no validation checks")
    return "failed" if any(check.get("status") == "failed" for check in checks) else "passed"


def artifact_records(args, lock_digest):
    records = []
    metadata_paths = sorted(args.artifacts.glob("*.metadata.json"))
    if not metadata_paths:
        raise SystemExit("no release artifact metadata files found")
    seen_artifacts = set()
    for metadata_path in metadata_paths:
        metadata = load_json(metadata_path)
        if metadata.get("version") != args.version or metadata.get("commit") != args.commit:
            raise SystemExit(f"release identity mismatch in {metadata_path}")
        if metadata.get("dependency_lock_sha256") != lock_digest:
            raise SystemExit(f"dependency lock digest mismatch in {metadata_path}")
        artifact_name = metadata.get("artifact")
        if not isinstance(artifact_name, str) or Path(artifact_name).name != artifact_name:
            raise SystemExit(f"invalid artifact name in {metadata_path}")
        if artifact_name in seen_artifacts:
            raise SystemExit(f"duplicate artifact metadata for {artifact_name}")
        seen_artifacts.add(artifact_name)
        artifact_path = args.artifacts / artifact_name
        sbom_path = args.artifacts / f"{artifact_name}.spdx.json"
        if not artifact_path.is_file():
            raise SystemExit(f"missing release artifact {artifact_path}")
        if not sbom_path.is_file():
            raise SystemExit(f"missing SPDX JSON SBOM {sbom_path}")
        sbom = load_json(sbom_path)
        if not str(sbom.get("spdxVersion", "")).startswith("SPDX-"):
            raise SystemExit(f"invalid SPDX JSON SBOM {sbom_path}")
        records.append(
            {
                "name": artifact_name,
                "bytes": artifact_path.stat().st_size,
                "sha256": sha256(artifact_path),
                "target": metadata.get("target"),
                "features": metadata.get("features", []),
                "toolchain": metadata.get("toolchain"),
                "metadata": {
                    "name": metadata_path.name,
                    "sha256": sha256(metadata_path),
                },
                "sbom": {
                    "name": sbom_path.name,
                    "format": "SPDX-2.3 JSON",
                    "sha256": sha256(sbom_path),
                    "bytes": sbom_path.stat().st_size,
                },
            }
        )
    return records


def main():
    args = arguments()
    if not HEX_40.fullmatch(args.commit):
        raise SystemExit("commit must contain 40 lowercase hexadecimal characters")
    if not HEX_64.fullmatch(args.syft_archive_sha256):
        raise SystemExit("Syft archive digest must contain 64 lowercase hexadecimal characters")
    if not args.workflow_identity.strip() or not args.approval_state.strip():
        raise SystemExit("workflow identity and approval state must not be empty")
    lock_digest = sha256(args.lockfile)
    records = artifact_records(args, lock_digest)

    doctor_path = args.validators / "doctor.json"
    self_test_path = args.validators / "self-test.json"
    versions_path = args.validators / "versions.json"
    doctor = load_json(doctor_path)
    self_test = load_json(self_test_path)
    versions = load_json(versions_path)
    if not isinstance(versions, dict) or not versions:
        raise SystemExit("validator version evidence must be a non-empty object")

    report = {
        "schema_version": 1,
        "version": args.version,
        "commit": args.commit,
        "approval_state": args.approval_state,
        "workflow_identity": args.workflow_identity,
        "dependency_lock_sha256": lock_digest,
        "artifacts": records,
        "sbom_generator": {
            "name": "Syft",
            "version": args.syft_version,
            "archive_sha256": args.syft_archive_sha256,
        },
        "validators": {
            "versions": versions,
            "doctor": validator_record(doctor_path, doctor_result(doctor)),
            "self-test": validator_record(self_test_path, self_test_result(self_test)),
        },
        "dependency_provenance": {
            "cargo_vet_exemptions_are_audits": False,
            "statement": (
                "Artifact provenance does not convert cargo-vet exemptions into "
                "audited dependency provenance."
            ),
        },
    }
    if report["validators"]["doctor"]["result"] != "passed" or report["validators"]["self-test"]["result"] != "passed":
        raise SystemExit("release validation evidence contains failures")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    descriptor, temporary_name = tempfile.mkstemp(prefix=".release-evidence-", dir=args.output.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary_name, args.output)
    finally:
        if os.path.exists(temporary_name):
            os.unlink(temporary_name)


if __name__ == "__main__":
    main()
