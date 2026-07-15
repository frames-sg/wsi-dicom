import datetime
import os
import re
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "publish.yml"
CI_PATH = REPO_ROOT / ".github" / "workflows" / "ci.yml"
SCRIPT_PATH = REPO_ROOT / "scripts" / "publish-crate.sh"


def indentation(line):
    if "\t" in line:
        raise AssertionError("workflow YAML must not contain tabs")
    return len(line) - len(line.lstrip(" "))


def nested_block(lines, key, indent):
    prefix = " " * indent + f"{key}:"
    matches = [index for index, line in enumerate(lines) if line == prefix]
    if len(matches) != 1:
        raise AssertionError(f"expected exactly one `{prefix}`, found {len(matches)}")
    start = matches[0] + 1
    end = start
    while end < len(lines):
        line = lines[end]
        if line.strip() and indentation(line) <= indent:
            break
        end += 1
    return lines[start:end]


def named_blocks(lines, parent_key, parent_indent, child_indent):
    body = nested_block(lines, parent_key, parent_indent)
    child_pattern = re.compile(rf"^ {{{child_indent}}}([A-Za-z0-9_-]+):$")
    starts = []
    for index, line in enumerate(body):
        match = child_pattern.match(line)
        if match:
            starts.append((index, match.group(1)))
    blocks = {}
    for position, (start, name) in enumerate(starts):
        if name in blocks:
            raise AssertionError(f"duplicate `{parent_key}` entry `{name}`")
        end = starts[position + 1][0] if position + 1 < len(starts) else len(body)
        blocks[name] = body[start:end]
    if not blocks:
        raise AssertionError(f"`{parent_key}` has no parsed entries")
    return blocks


def step_blocks(job_lines):
    steps_body = nested_block(job_lines, "steps", 4)
    starts = [
        index
        for index, line in enumerate(steps_body)
        if re.match(r"^ {6}- (?:[A-Za-z0-9_-]+):", line)
    ]
    blocks = []
    for position, start in enumerate(starts):
        end = starts[position + 1] if position + 1 < len(starts) else len(steps_body)
        blocks.append(steps_body[start:end])
    if not blocks:
        raise AssertionError("job has no parsed steps")
    return blocks


def scalar(lines, key, indent, *, step=False):
    ordinary = re.compile(rf"^ {{{indent}}}{re.escape(key)}:\s*(.*)$")
    first_step = re.compile(rf"^ {{{indent - 2}}}- {re.escape(key)}:\s*(.*)$") if step else None
    matches = []
    for index, line in enumerate(lines):
        match = ordinary.match(line)
        if not match and first_step:
            match = first_step.match(line)
        if match:
            matches.append((index, match.group(1)))
    if not matches:
        return None
    if len(matches) != 1:
        raise AssertionError(f"duplicate scalar `{key}`")
    index, value = matches[0]
    if value not in {"|", "|-", ">", ">-"}:
        return value.strip().strip('"')

    content = []
    for line in lines[index + 1 :]:
        if line.strip() and indentation(line) <= indent:
            break
        content.append(line[indent + 2 :] if len(line) >= indent + 2 else "")
    if value.startswith(">"):
        return " ".join(part.strip() for part in content if part.strip())
    return "\n".join(content).strip()


def mapping(lines, key, indent):
    body = nested_block(lines, key, indent)
    entry_pattern = re.compile(rf"^ {{{indent + 2}}}([A-Za-z0-9_-]+):\s*(.*)$")
    result = {}
    for line in body:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        match = entry_pattern.match(line)
        if not match:
            raise AssertionError(f"unsupported `{key}` entry: {line!r}")
        name, value = match.groups()
        if name in result:
            raise AssertionError(f"duplicate `{key}` entry `{name}`")
        result[name] = value.split(" #", 1)[0].strip().strip('"')
    return result


def list_value(lines, key, indent):
    body = nested_block(lines, key, indent)
    item_pattern = re.compile(rf"^ {{{indent + 2}}}- ([A-Za-z0-9_-]+)$")
    items = []
    for line in body:
        if not line.strip():
            continue
        match = item_pattern.match(line)
        if not match:
            raise AssertionError(f"unsupported `{key}` list item: {line!r}")
        items.append(match.group(1))
    return items


class PublishWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = WORKFLOW_PATH.read_text(encoding="utf-8")
        cls.lines = cls.source.splitlines()
        if "<<:" in cls.source:
            raise AssertionError("workflow YAML must not use merge keys")
        for line in cls.lines:
            uncommented = line.split(" #", 1)[0]
            if re.search(r"(?:^|\s)[&*][A-Za-z_][A-Za-z0-9_-]*(?:\s|$)", uncommented):
                raise AssertionError("workflow YAML must not use anchors or aliases")
        cls.jobs = named_blocks(cls.lines, "jobs", 0, 2)
        cls.steps = {
            job_name: step_blocks(job_lines) for job_name, job_lines in cls.jobs.items()
        }

    def job_with_exact_run(self, command):
        matches = []
        for job_name, steps in self.steps.items():
            for step in steps:
                if scalar(step, "run", 8, step=True) == command:
                    matches.append((job_name, step))
        self.assertEqual(len(matches), 1, f"expected one exact run `{command}`")
        return matches[0]

    def test_run_blocks_never_interpolate_actions_expressions(self):
        parsed_runs = []
        for steps in self.steps.values():
            for step in steps:
                run = scalar(step, "run", 8, step=True)
                if run is not None:
                    parsed_runs.append(run)
                    self.assertNotIn("${{", run)
        raw_run_count = sum(
            1 for line in self.lines if re.match(r"^ {8}(?:- )?run:", line)
        )
        self.assertEqual(len(parsed_runs), raw_run_count)

    def test_manual_dispatch_can_only_dry_run(self):
        triggers = named_blocks(self.lines, "on", 0, 2)
        self.assertEqual(set(triggers), {"push", "workflow_dispatch"})
        self.assertNotIn("inputs:", "\n".join(triggers["workflow_dispatch"]))
        self.assertEqual(
            [line.strip() for line in triggers["push"] if line.strip()],
            ["push:", "tags:", '- "v*"'],
        )

        dry_run_jobs = []
        for job_name, steps in self.steps.items():
            if any(
                scalar(step, "run", 8, step=True)
                == "scripts/publish-crate.sh --dry-run"
                for step in steps
            ):
                dry_run_jobs.append(job_name)
        self.assertEqual(len(dry_run_jobs), 2)
        manual_jobs = [
            name
            for name in dry_run_jobs
            if scalar(self.jobs[name], "if", 4) == "github.event_name == 'workflow_dispatch'"
        ]
        self.assertEqual(len(manual_jobs), 1)

        publish_job, _ = self.job_with_exact_run("scripts/publish-crate.sh --publish")
        condition = scalar(self.jobs[publish_job], "if", 4)
        self.assertIsNotNone(condition)
        assert condition is not None
        self.assertIn("github.event_name == 'push'", condition)
        self.assertIn("github.ref_type == 'tag'", condition)
        self.assertNotIn("||", condition)
        self.assertNotIn("always()", condition)
        self.assertNotIn("failure()", condition)
        self.assertNotIn("DRY_RUN_ONLY", self.source)
        self.assertNotIn("pull_request_target", self.source)
        self.assertNotIn("workflow_run", self.source)

    def test_oidc_token_reaches_only_the_publish_step(self):
        self.assertNotIn("secrets.", self.source)
        self.assertNotIn("CRATES_IO_API_TOKEN", self.source)
        self.assertNotIn("CARGO_REGISTRIES_CRATES_IO_TOKEN", self.source)
        self.assertNotIn("CARGO_TOKEN", self.source)
        self.assertEqual(self.source.count("CARGO_REGISTRY_TOKEN"), 1)
        auth_matches = []
        for job_name, steps in self.steps.items():
            for index, step in enumerate(steps):
                uses = scalar(step, "uses", 8, step=True)
                if uses and uses.startswith("rust-lang/crates-io-auth-action@"):
                    auth_matches.append((job_name, index, step, uses))
        self.assertEqual(len(auth_matches), 1)
        auth_job, auth_index, auth_step, uses = auth_matches[0]
        self.assertRegex(
            uses,
            r"^rust-lang/crates-io-auth-action@[0-9a-f]{40}$",
        )
        self.assertEqual(
            uses,
            "rust-lang/crates-io-auth-action@c6f97d42243bad5fab37ca0427f495c86d5b1a18",
        )
        auth_id = scalar(auth_step, "id", 8, step=True)
        self.assertIsNotNone(auth_id)
        token_expression = f"${{{{ steps.{auth_id}.outputs.token }}}}"
        self.assertEqual(self.source.count(token_expression), 1)

        publish_job, publish_step = self.job_with_exact_run(
            "scripts/publish-crate.sh --publish"
        )
        self.assertEqual(publish_job, auth_job)
        self.assertEqual(mapping(publish_step, "env", 8), {"CARGO_REGISTRY_TOKEN": token_expression})
        publish_index = self.steps[publish_job].index(publish_step)
        self.assertEqual(publish_index, auth_index + 1)

    def test_every_publish_workflow_action_is_immutable(self):
        actions = []
        for steps in self.steps.values():
            for step in steps:
                uses = scalar(step, "uses", 8, step=True)
                if uses is not None:
                    actions.append(uses)
        self.assertGreaterEqual(len(actions), 7)
        for action in actions:
            self.assertRegex(action, r"^[^@\s]+@[0-9a-f]{40}$")

    def test_checkout_credentials_are_never_persisted(self):
        checkouts = []
        for job_name, steps in self.steps.items():
            for step in steps:
                uses = scalar(step, "uses", 8, step=True)
                if uses and uses.startswith("actions/checkout@"):
                    checkouts.append((job_name, step, uses))
        self.assertGreaterEqual(len(checkouts), 3)
        for job_name, step, uses in checkouts:
            self.assertRegex(uses, r"^actions/checkout@[0-9a-f]{40}$")
            options = mapping(step, "with", 8)
            self.assertEqual(options.get("persist-credentials"), "false", job_name)
            self.assertEqual(options.get("ref"), "${{ github.sha }}", job_name)

    def test_release_is_bound_to_manifest_changelog_and_exact_sha(self):
        manifest = tomllib.loads((REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        version = manifest["package"]["version"]
        changelog = (REPO_ROOT / ".github" / "CHANGELOG.md").read_text(encoding="utf-8")
        prefix = f"## [{version}] - "
        headings = [
            line
            for line in changelog.splitlines()
            if line.startswith(f"## [{version}]")
        ]
        self.assertEqual(len(headings), 1)
        self.assertTrue(headings[0].startswith(prefix))
        release_date = datetime.date.fromisoformat(headings[0][len(prefix) :])
        self.assertEqual(headings[0], f"{prefix}{release_date.isoformat()}")

        verifier_jobs = []
        for job_name, steps in self.steps.items():
            runs = "\n".join(
                run
                for step in steps
                if (run := scalar(step, "run", 8, step=True)) is not None
            )
            required = [
                "cargo metadata --locked --no-deps",
                'expected_tag="v${version}"',
                '"$GITHUB_REF_NAME"',
                ".github/CHANGELOG.md",
                'PACKAGE_VERSION="$version" python3',
                "datetime.date.fromisoformat",
                'line.startswith(f"## [{version}]")',
                "git merge-base --is-ancestor",
                "refs/remotes/origin/main",
                "gh run list",
                '--commit "$GITHUB_SHA"',
                ".conclusion == \"success\"",
            ]
            if all(fragment in runs for fragment in required):
                verifier_jobs.append(job_name)
        self.assertEqual(len(verifier_jobs), 1)
        verifier = verifier_jobs[0]
        self.assertEqual(
            scalar(self.jobs[verifier], "if", 4),
            "github.event_name == 'push' && github.ref_type == 'tag'",
        )

        registry_jobs = []
        for job_name, steps in self.steps.items():
            runs = "\n".join(
                run
                for step in steps
                if (run := scalar(step, "run", 8, step=True)) is not None
            )
            if "https://crates.io/api/v1/crates/wsi-dicom/" in runs:
                registry_jobs.append(job_name)
        self.assertEqual(len(registry_jobs), 1)
        registry = registry_jobs[0]
        self.assertEqual(scalar(self.jobs[registry], "needs", 4), verifier)
        self.assertEqual(
            scalar(self.jobs[registry], "if", 4),
            "github.event_name == 'push' && github.ref_type == 'tag'",
        )
        registry_runs = "\n".join(
            run
            for step in self.steps[registry]
            if (run := scalar(step, "run", 8, step=True)) is not None
        )
        self.assertIn("--user-agent", registry_runs)
        self.assertIn("https://github.com/frames-sg/wsi-dicom", registry_runs)

        publish_job, _ = self.job_with_exact_run("scripts/publish-crate.sh --publish")
        self.assertEqual(set(list_value(self.jobs[publish_job], "needs", 4)), {verifier, registry})
        self.assertNotIn("continue-on-error:", "\n".join(self.jobs[verifier]))
        self.assertNotIn("continue-on-error:", "\n".join(self.jobs[registry]))
        self.assertNotIn("id-token:", "\n".join(self.jobs[registry]))
        self.assertNotIn("environment:", "\n".join(self.jobs[registry]))

    def test_permissions_are_minimal_and_oidc_is_publish_only(self):
        self.assertEqual(mapping(self.lines, "permissions", 0), {"contents": "read"})
        publish_job, _ = self.job_with_exact_run("scripts/publish-crate.sh --publish")
        self.assertEqual(
            mapping(self.jobs[publish_job], "permissions", 4),
            {"contents": "read", "id-token": "write"},
        )
        verifier_jobs = [
            name
            for name, lines in self.jobs.items()
            if "gh run list" in "\n".join(lines)
        ]
        self.assertEqual(len(verifier_jobs), 1)
        self.assertEqual(
            mapping(self.jobs[verifier_jobs[0]], "permissions", 4),
            {"actions": "read", "contents": "read"},
        )
        id_token_jobs = [
            name for name, lines in self.jobs.items() if "id-token:" in "\n".join(lines)
        ]
        self.assertEqual(id_token_jobs, [publish_job])
        self.assertNotIn("write-all", self.source)
        self.assertNotIn("read-all", self.source)

    def test_only_publish_job_uses_protected_environment(self):
        publish_job, _ = self.job_with_exact_run("scripts/publish-crate.sh --publish")
        environment_jobs = [
            name
            for name, lines in self.jobs.items()
            if scalar(lines, "environment", 4) is not None
        ]
        self.assertEqual(environment_jobs, [publish_job])
        self.assertEqual(scalar(self.jobs[publish_job], "environment", 4), "crates-io")

    def test_publication_is_serialized_and_never_cancelled(self):
        self.assertEqual(
            mapping(self.lines, "concurrency", 0),
            {"group": "crates-io-publish", "cancel-in-progress": "false"},
        )
        for lines in self.jobs.values():
            self.assertNotIn("concurrency:", "\n".join(lines))

    def test_policy_is_wired_into_ci_without_cargo(self):
        ci = CI_PATH.read_text(encoding="utf-8")
        self.assertIn("workflow-security:", ci)
        self.assertIn(
            "python -m unittest discover -s tests -p 'test_publish_workflow.py'",
            ci,
        )

    def test_actionlint_install_is_versioned_and_checksum_verified(self):
        ci = CI_PATH.read_text(encoding="utf-8")
        self.assertIn("ACTIONLINT_VERSION: 1.7.12", ci)
        self.assertIn(
            "ACTIONLINT_SHA256: 8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8",
            ci,
        )
        self.assertIn("sha256sum --check --strict", ci)
        self.assertNotIn("tool: actionlint@", ci)


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
            '"--publish"',
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
