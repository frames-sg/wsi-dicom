import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "publish-crate.sh"
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "publish.yml"
GITATTRIBUTES_PATH = REPO_ROOT / ".gitattributes"


class PublishScriptTests(unittest.TestCase):
    def run_script(self, arguments, *, cargo_token=None, legacy_token=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary_dir = root / "bin"
            binary_dir.mkdir()
            argument_log = root / "cargo-arguments"
            token_log = root / "cargo-token"
            fake_cargo = binary_dir / "cargo"
            fake_cargo.write_text(
                "#!/usr/bin/env bash\n"
                "set -euo pipefail\n"
                "printf '%s\\n' \"$@\" > \"$FAKE_CARGO_ARGUMENTS\"\n"
                "printf '%s' \"${CARGO_REGISTRY_TOKEN:-}\" > \"$FAKE_CARGO_TOKEN\"\n",
                encoding="utf-8",
            )
            fake_cargo.chmod(0o755)
            environment = os.environ.copy()
            environment.pop("CARGO_REGISTRY_TOKEN", None)
            environment.pop("CRATES_IO_API_TOKEN", None)
            environment["PATH"] = f"{binary_dir}{os.pathsep}{environment['PATH']}"
            environment["FAKE_CARGO_ARGUMENTS"] = str(argument_log)
            environment["FAKE_CARGO_TOKEN"] = str(token_log)
            if cargo_token is not None:
                environment["CARGO_REGISTRY_TOKEN"] = cargo_token
            if legacy_token is not None:
                environment["CRATES_IO_API_TOKEN"] = legacy_token
            result = subprocess.run(
                [str(SCRIPT_PATH), *arguments],
                cwd=REPO_ROOT,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )
            cargo_arguments = (
                argument_log.read_text(encoding="utf-8").splitlines()
                if argument_log.exists()
                else None
            )
            observed_token = (
                token_log.read_text(encoding="utf-8") if token_log.exists() else None
            )
            return result, cargo_arguments, observed_token

    def test_invalid_modes_fail_before_cargo(self):
        invalid_arguments = [[], ["unknown"], ["--dry-run", "extra"]]
        for arguments in invalid_arguments:
            with self.subTest(arguments=arguments):
                result, cargo_arguments, _ = self.run_script(arguments)
                self.assertEqual(result.returncode, 2)
                self.assertIsNone(cargo_arguments)

    def test_hostile_mode_text_is_inert(self):
        hostile_arguments = [
            "--publish; touch should-not-exist",
            "$(touch should-not-exist)",
            "--publish\n--dry-run",
            '\"--publish\"',
        ]
        for argument in hostile_arguments:
            with self.subTest(argument=argument):
                result, cargo_arguments, _ = self.run_script([argument])
                self.assertEqual(result.returncode, 2)
                self.assertIsNone(cargo_arguments)
        self.assertFalse((REPO_ROOT / "should-not-exist").exists())

    def test_dry_run_uses_locked_publish_without_credentials(self):
        result, cargo_arguments, observed_token = self.run_script(["--dry-run"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            cargo_arguments,
            [
                "publish",
                "--package",
                "wsi-dicom",
                "--registry",
                "crates-io",
                "--locked",
                "--dry-run",
            ],
        )
        self.assertEqual(observed_token, "")

    def test_dry_run_refuses_any_registry_token(self):
        result, cargo_arguments, _ = self.run_script(
            ["--dry-run"], cargo_token="temporary-sentinel"
        )
        self.assertEqual(result.returncode, 2)
        self.assertIsNone(cargo_arguments)

    def test_publish_requires_temporary_token(self):
        result, cargo_arguments, _ = self.run_script(["--publish"])
        self.assertEqual(result.returncode, 2)
        self.assertIsNone(cargo_arguments)

    def test_legacy_token_cannot_publish(self):
        result, cargo_arguments, _ = self.run_script(
            ["--publish"], legacy_token="legacy-sentinel"
        )
        self.assertEqual(result.returncode, 2)
        self.assertIsNone(cargo_arguments)

    def test_publish_uses_locked_no_verify_with_temporary_token(self):
        result, cargo_arguments, observed_token = self.run_script(
            ["--publish"], cargo_token="temporary-sentinel"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            cargo_arguments,
            [
                "publish",
                "--package",
                "wsi-dicom",
                "--registry",
                "crates-io",
                "--locked",
                "--no-verify",
            ],
        )
        self.assertEqual(observed_token, "temporary-sentinel")


class ReleaseWorkflowPolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

    def job(self, name):
        match = re.search(
            rf"(?ms)^  {re.escape(name)}:\n(?P<body>.*?)(?=^  [a-zA-Z0-9_-]+:\n|\Z)",
            self.workflow,
        )
        self.assertIsNotNone(match, f"missing workflow job {name}")
        return match.group("body")

    def test_every_action_is_pinned_to_an_immutable_commit(self):
        action_uses = re.findall(r"(?m)^\s*- uses:\s*([^\s#]+)", self.workflow)
        self.assertTrue(action_uses)
        for action in action_uses:
            with self.subTest(action=action):
                self.assertRegex(action, r"^[^@]+@[0-9a-f]{40}$")

    def test_manual_rehearsal_cannot_publish_or_create_a_release(self):
        rehearsal = self.job("rehearsal")
        self.assertIn("github.event_name == 'workflow_dispatch'", rehearsal)
        self.assertIn("scripts/publish-crate.sh --dry-run", rehearsal)
        self.assertNotIn("--publish", rehearsal)
        self.assertNotIn("gh release", rehearsal)
        self.assertNotIn("CARGO_REGISTRY_TOKEN", rehearsal)

        publish = self.job("publish")
        draft_release = self.job("draft_release")
        for protected_job in (publish, draft_release):
            self.assertIn("github.event_name == 'push'", protected_job)
            self.assertIn("github.ref_type == 'tag'", protected_job)

    def test_cpu_archives_cover_the_four_release_targets(self):
        build = self.job("build_cli_archives")
        for target in (
            "x86_64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ):
            self.assertEqual(build.count(target), 1)
        self.assertIn("--no-default-features", build)
        self.assertIn(".tar.gz", build)
        self.assertIn(".zip", build)
        for required_file in ("README.md", "LICENSE-MIT", "LICENSE-APACHE", "VERSION.json"):
            self.assertIn(required_file, build)

    def test_dependency_lock_digest_is_portable_across_release_runners(self):
        attributes = GITATTRIBUTES_PATH.read_text(encoding="utf-8")
        self.assertRegex(attributes, r"(?m)^Cargo\.lock\s+text\s+eol=lf$")

    def test_candidate_contains_crate_checksums_spdx_sboms_and_evidence(self):
        crate = self.job("crate_candidate")
        evidence = self.job("release_evidence")
        self.assertIn("cargo package --locked", crate)
        self.assertIn(".crate", crate)
        self.assertIn('cat dist/validation/doctor.json', crate)
        self.assertIn('cat dist/validation/self-test.json', crate)
        self.assertIn("SYFT_VERSION: 1.50.0", evidence)
        self.assertIn(
            "bf7b29ff57f06da30918266a0e1c2885a8f99784798d1bdb1628886aa015d788",
            evidence,
        )
        self.assertIn("spdx-json", evidence)
        self.assertIn("SHA256SUMS", evidence)
        self.assertIn("scripts/build-release-evidence.py", evidence)

    def test_attestation_permissions_are_isolated_from_crates_credentials(self):
        attest = self.job("attest")
        publish = self.job("publish")
        self.assertIn("id-token: write", attest)
        self.assertIn("attestations: write", attest)
        self.assertIn("artifact-metadata: write", attest)
        self.assertIn(
            "actions/attest@508db95dd578ae2727ebd6217d5ba78e4fbda05d", attest
        )
        self.assertIn("subject-path", attest)
        self.assertIn("sbom-path", attest)
        self.assertNotIn("crates-io-auth-action", attest)
        self.assertNotIn("CARGO_REGISTRY_TOKEN", attest)

        self.assertIn("environment: crates-io", publish)
        self.assertIn("crates-io-auth-action", publish)
        self.assertNotIn("attestations: write", publish)

    def test_tag_binding_registry_checksum_and_draft_release_are_controlled(self):
        verify = self.job("verify_release")
        registry = self.job("registry_status")
        publish = self.job("publish")
        checksum = self.job("verify_registry_checksum")
        draft = self.job("draft_release")
        self.assertIn('expected_tag="v${version}"', verify)
        self.assertIn("successful CI", verify)
        self.assertIn("state: ${{ steps.registry.outputs.state }}", registry)
        self.assertIn("state=unpublished", registry)
        self.assertIn("always() &&", publish)
        for required_job in ("verify_release", "registry_status", "release_evidence", "attest"):
            self.assertIn(f"needs.{required_job}.result == 'success'", publish)
        self.assertIn("needs.registry_status.outputs.state == 'unpublished'", publish)
        self.assertIn('path: ${{ runner.temp }}/candidate', publish)
        self.assertIn('candidate="${RUNNER_TEMP}/candidate/wsi-dicom-${version}.crate"', publish)
        self.assertIn("needs.registry_status.outputs.state == 'published'", checksum)
        self.assertNotIn("outputs.published", self.workflow)
        self.assertIn("scripts/verify-registry-checksum.py", checksum)
        self.assertIn("always() &&", draft)
        self.assertIn("needs.verify_registry_checksum.result == 'success'", draft)
        self.assertIn("scripts/extract-release-notes.py", draft)
        self.assertIn("gh release create", draft)
        self.assertIn("--draft", draft)
        self.assertNotIn("--latest", draft)


if __name__ == "__main__":
    unittest.main()
