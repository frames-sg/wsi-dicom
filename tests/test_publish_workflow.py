import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "publish-crate.sh"


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


if __name__ == "__main__":
    unittest.main()
