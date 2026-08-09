import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
METADATA_SCRIPT = REPO_ROOT / "scripts" / "write-release-metadata.py"
EVIDENCE_SCRIPT = REPO_ROOT / "scripts" / "build-release-evidence.py"
NOTES_SCRIPT = REPO_ROOT / "scripts" / "extract-release-notes.py"
CHECKSUM_SCRIPT = REPO_ROOT / "scripts" / "verify-registry-checksum.py"


class ReleaseEvidenceTests(unittest.TestCase):
    def test_metadata_and_evidence_bind_artifact_sbom_and_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            artifact = artifacts / "wsi-dicom-0.7.2-test-target.tar.gz"
            artifact.write_bytes(b"archive candidate")
            sbom = artifacts / f"{artifact.name}.spdx.json"
            sbom.write_text('{"spdxVersion":"SPDX-2.3"}\n', encoding="utf-8")
            lockfile = root / "Cargo.lock"
            lockfile.write_bytes(b"locked dependencies")
            lock_digest = hashlib.sha256(lockfile.read_bytes()).hexdigest()
            metadata = artifacts / f"{artifact.name}.metadata.json"
            subprocess.run(
                [
                    str(METADATA_SCRIPT),
                    "--version",
                    "0.7.2",
                    "--commit",
                    "a" * 40,
                    "--target",
                    "test-target",
                    "--artifact",
                    artifact.name,
                    "--rustc",
                    "rustc 1.96.0",
                    "--cargo",
                    "cargo 1.96.0",
                    "--lock-sha256",
                    lock_digest,
                    "--output",
                    str(metadata),
                ],
                cwd=REPO_ROOT,
                check=True,
            )
            validators = root / "validators"
            validators.mkdir()
            (validators / "doctor.json").write_text(
                '{"tools":[{"name":"dciodvfy","status":"available"}]}\n',
                encoding="utf-8",
            )
            (validators / "self-test.json").write_text(
                '{"validation_report":{"checks":[{"status":"passed"}]}}\n',
                encoding="utf-8",
            )
            (validators / "versions.json").write_text(
                '{"dciodvfy":"dicom3tools 1.0"}\n', encoding="utf-8"
            )
            output = root / "release-evidence.json"
            subprocess.run(
                [
                    str(EVIDENCE_SCRIPT),
                    "--version",
                    "0.7.2",
                    "--commit",
                    "a" * 40,
                    "--artifacts",
                    str(artifacts),
                    "--lockfile",
                    str(lockfile),
                    "--validators",
                    str(validators),
                    "--workflow-identity",
                    "publish@refs/tags/v0.7.2#1/1",
                    "--approval-state",
                    "candidate_pending_explicit_approval",
                    "--syft-version",
                    "1.50.0",
                    "--syft-archive-sha256",
                    "c" * 64,
                    "--output",
                    str(output),
                ],
                cwd=REPO_ROOT,
                check=True,
            )

            evidence = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(evidence["version"], "0.7.2")
            self.assertEqual(evidence["commit"], "a" * 40)
            self.assertEqual(evidence["approval_state"], "candidate_pending_explicit_approval")
            self.assertFalse(evidence["dependency_provenance"]["cargo_vet_exemptions_are_audits"])
            self.assertEqual(evidence["validators"]["doctor"]["result"], "passed")
            self.assertEqual(evidence["validators"]["self-test"]["result"], "passed")
            self.assertEqual(len(evidence["artifacts"]), 1)
            record = evidence["artifacts"][0]
            self.assertEqual(record["name"], artifact.name)
            self.assertEqual(record["target"], "test-target")
            self.assertEqual(record["features"], [])
            self.assertEqual(record["sha256"], hashlib.sha256(artifact.read_bytes()).hexdigest())
            self.assertEqual(record["sbom"]["format"], "SPDX-2.3 JSON")
            self.assertEqual(record["sbom"]["sha256"], hashlib.sha256(sbom.read_bytes()).hexdigest())

    def test_release_notes_are_exactly_the_requested_changelog_section(self):
        with tempfile.TemporaryDirectory() as temporary:
            changelog = Path(temporary) / "CHANGELOG.md"
            changelog.write_text(
                "# Changelog\n\n## [Unreleased]\n\n## [0.7.2] - 2026-08-08\n\n"
                "### Fixed\n\n- Correctness.\n\n## [0.7.1] - 2026-07-21\n\n- Old.\n",
                encoding="utf-8",
            )
            result = subprocess.run(
                [str(NOTES_SCRIPT), "--changelog", str(changelog), "--version", "0.7.2"],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=True,
            )
            self.assertIn("### Fixed", result.stdout)
            self.assertIn("Correctness", result.stdout)
            self.assertNotIn("Unreleased", result.stdout)
            self.assertNotIn("0.7.1", result.stdout)

    def test_registry_checksum_must_equal_the_exact_candidate(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "wsi-dicom-0.7.2.crate"
            candidate.write_bytes(b"exact crate")
            digest = hashlib.sha256(candidate.read_bytes()).hexdigest()
            registry = root / "registry.json"
            registry.write_text(
                json.dumps({"version": {"num": "0.7.2", "checksum": digest}}),
                encoding="utf-8",
            )
            good = subprocess.run(
                [
                    str(CHECKSUM_SCRIPT),
                    "--candidate",
                    str(candidate),
                    "--version",
                    "0.7.2",
                    "--registry-json",
                    str(registry),
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(good.returncode, 0, good.stderr)
            registry.write_text(
                json.dumps({"version": {"num": "0.7.2", "checksum": "0" * 64}}),
                encoding="utf-8",
            )
            bad = subprocess.run(
                [
                    str(CHECKSUM_SCRIPT),
                    "--candidate",
                    str(candidate),
                    "--version",
                    "0.7.2",
                    "--registry-json",
                    str(registry),
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(bad.returncode, 0)
            self.assertIn("checksum mismatch", bad.stderr)


if __name__ == "__main__":
    unittest.main()
