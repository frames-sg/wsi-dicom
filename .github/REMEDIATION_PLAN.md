# wsi-dicom Remediation Plan

Status: remediation active; publication remains frozen
Baseline date: 2026-07-09
Baseline commit: `dc6ae2e` on `main`, three commits ahead of `origin/main`
Scope: security, correctness, build and release reproducibility, architecture,
tests, CI, supply-chain assurance, documentation, and repository hygiene

This file is the durable source of truth for the remediation program. Keep it
current as work proceeds. It is intentionally stored under `.github/` because
the repository integrity tests prohibit additional Markdown files in the root
and `docs/`, while `.github/` is already the home for project process
documentation and is excluded from the published crate.

## Live handoff

Updated: 2026-07-09
Current phase: Phase 1 registry dependency topology; publication remains
fail-closed
Active items: BUILD-001 is blocked after completing the registry `j2k 0.6.2`
migration; owner `/root`, assigned files are the root/fuzz manifests and
lockfiles, dependency integrity tests, the published-API adaptation, `deny.toml`,
and this plan
Last completed: SEC-000 remote workflow disablement and repository-secret
removal; SEC-001 local workflow, script, CI, regression-test, trusted-publisher,
and exposed-token containment changes
Next exact action: obtain separate authority to harden and publish the `wsi-rs`
`0.5.0` release; then replace the final temporary `wsi-rs` path with `=0.5.0` from
crates.io and prove a standalone package. Add a second GitHub reviewer before
any discussion of re-enabling publication
Blocked by: no compatible published `wsi-rs` exists. Registry `0.4.0` is not
fresh-resolvable because it requires yanked `signinum` packages and would mix
the `j2k 0.5` and `0.6` families. Compatible local `0.5.0` requires security
hardening and separately authorized publication. The release environment also
remains intentionally fail-closed pending an independent reviewer
Token verification: the owner attests that the token disclosed on 2026-07-09 was
revoked. Two other `codex` tokens remain; their presence does not contradict the
owner's revocation and they are outside this incident's scope.
Tests last run: 278 Rust tests passed, including all 26 repository-integrity
tests, and 7 fixture tests were ignored. Default workspace, Metal/CUDA, and fuzz
compile checks, Clippy with warnings denied, formatting, 39 Python tests, and
Pyright passed. Package creation stops correctly because registry `wsi-rs 0.5.0`
does not yet exist
Working-tree state at plan creation: `main` ahead of `origin/main` by three;
the plan and two Cargo-independent policy tests are the intended new repository
files
Open decisions: DEC-001 through DEC-009

Update this block at every handoff. Keep it short; detailed evidence belongs in
the work item, decision log, and execution log.

## 1. Context recovery and handoff protocol

At the beginning of every implementation session:

1. Read this file completely.
2. Run `git status --short --branch` and preserve all pre-existing changes.
3. Confirm the current commit and compare it with the baseline above.
4. Read the latest entries in the execution log and decision log.
5. Resume the assigned item marked `[~]`. If none is active in the current
   workstream, take the first dependency-ready item in priority order.
6. Before editing, add or identify the behavior-focused regression test that
   will prove the defect and prevent recurrence.
7. At the end of the session, update item status, verification evidence,
   decisions, newly discovered risks, and the execution log.

Do not infer that an unchecked item is complete from code shape alone. Re-run
its stated acceptance checks. Do not begin lower-priority refactors while a P0
or P1 security/correctness blocker remains open unless the work is required to
unblock that blocker.

### Status notation

- `[ ]` not started
- `[~]` in progress; parallel items are permitted only in non-overlapping
  workstreams. Every active item must record owner, assigned files, dependency
  state, and coordination notes, with at most one active item per workstream.
- `[x]` completed and verified
- `[!]` blocked; record the blocker and the authority or external state needed
- `[-]` deliberately declined; requires a decision-log entry and rationale

### Per-item completion record

When closing an item, append these fields beneath it:

- Owner and assigned file set:
- Completion commit:
- Tests added or changed:
- Commands run and results:
- Coverage result for changed paths:
- Residual risk or follow-up:
- Documentation or compatibility note:

## 2. Operating constraints

These rules apply to all work in this plan:

- Priority is security, correctness, maintainability, then delivery speed.
- Preserve passing regression tests. Replace implementation-coupled assertions
  only with equal or stronger behavioral protection.
- Edit in place. Add modules only where they create a real ownership boundary.
- Use `trash <path>` for local deletions; never use destructive Git recovery.
- Use the project toolchain and `./.venv/bin/python` where Python is needed.
- Validate all input-derived dimensions, counts, paths, metadata, and external
  tool output before allocation or persistence.
- Surface errors. Missing validators, disconnected workers, dependency-analysis
  failures, and partial output must not be reported as success.
- Keep public Rust APIs, CLI behavior, JSON report fields, DICOM identity, output
  names, and performance as explicit compatibility surfaces.
- Target at least 80% changed-path coverage where no stronger gate exists.
- Run the narrowest relevant tests during development and the complete release
  matrix before the final release-readiness decision.
- Do not publish a crate, rotate a credential, change repository settings, or
  modify external repositories without the corresponding authority. Prepare
  and verify those steps locally first.

## 3. Baseline and audit evidence

### 3.1 Current verification state

| Check                                          | Current result | Meaning                                                                                           |
| ---------------------------------------------- | -------------- | ------------------------------------------------------------------------------------------------- |
| `cargo fmt --all -- --check`                   | Pass           | Formatting is clean.                                                                              |
| All Python tests and Pyright                   | 39 passed      | Benchmark, workflow-security, and dependency-topology policy tests are healthy.                   |
| Locked metadata/check/test/Clippy              | Pass locally   | Registry `j2k 0.6.2` resolves; 278 Rust tests passed, 7 protected-fixture tests remain ignored.   |
| Metal, CUDA, and fuzz compile checks           | Pass           | Published `j2k 0.6.2` builds across the declared codec feature surfaces on this macOS host.       |
| Clean-checkout metadata/package                | Blocked        | The last path bridge is compatible but unpublished `wsi-rs 0.5.0`; packaging refuses to continue. |
| `cargo audit --file Cargo.lock --deny unsound` | Fail           | Four quick-xml vulnerability matches remain; prior crossbeam/memmap findings no longer match.     |
| `cargo deny check bans licenses sources`       | Pass           | Registry, license, and duplicate-version policy pass without stale skip warnings.                 |
| `cargo vet --locked`                           | Blocked        | The pre-existing vet store requires formatting/review before it can evaluate the new graph.       |
| Working tree                                   | Dirty          | Active remediation changes are intentionally uncommitted.                                         |

The original audit baseline is preserved in the execution log. BUILD-001 is
only partially complete until a standalone clone and package can resolve
`wsi-rs` from the registry.

Registry baseline observed on 2026-07-09: the `j2k 0.6.2` family,
`wsi-rs 0.4.0`, and `wsi-dicom 0.2.0` were published; `wsi-rs 0.5.0`,
`wsi-dicom 0.4.0`, and `j2k 0.7.0` were not. Re-query this time-sensitive state
before any release or semver decision.

### 3.2 Structural measurements

- Approximately 35,421 first-party Rust lines across 68 non-vendor files.
- Largest files: `src/export.rs` 3,395 lines, `src/writer.rs` 2,912,
  `src/validation.rs` 2,239, and `src/main.rs` 2,225.
- Production clone scan: 45 clone groups, 562 duplicated lines, 2.01% line
  duplication. Duplication is localized rather than repository-wide.
- Twenty-three production functions have at least ten parameters; six have
  sixteen. Forty-nine `clippy::too_many_arguments` allowances exist.
- Notable large functions include `build_dicom_object` at roughly 407 lines,
  two codec instance paths above 300 lines, and Metal paths with sixteen
  parameters.
- `ExportMetrics` emits exactly 99 flattened JSON fields from four already
  grouped Rust structs. Its concentrated implementation and 307-line manual
  aggregation remain structural debt.
- `vendor/` contains 214 tracked files and about 118,000 lines of unused Rust,
  more than three times the first-party Rust surface.

These numbers are diagnostics, not standalone success criteria. Refactors must
improve ownership and behavior rather than merely move lines to satisfy a cap.

### 3.3 Controls that must not regress

- Library, CLI, and GUI forbid unsafe Rust.
- Output persistence already uses same-directory temporary files, no-clobber
  behavior, synchronization, and symlink defenses in important paths.
- Metadata and validation-directory readers have useful byte/depth limits.
- External decoder commands use argument vectors instead of shell evaluation.
- GitHub Actions are pinned by commit SHA and the Gitleaks download is verified.
- No hardcoded credential material was found by the audit scan.
- The core test inventory is substantial even though important gates are
  currently blocked or silently skipped.

## 4. Program definition of done

The remediation program is complete only when all of the following are true:

1. The release workflow cannot execute workflow input as shell code, uses no
   long-lived publishing secret, and maps the exchanged short-lived crates.io
   credential only to the exact publish step.
2. A fresh standalone clone can run metadata, formatting, tests, Clippy, docs,
   audit, deny, vet, package, and publish dry-run without undeclared sibling
   directories.
3. The committed dependency graph has no unaccepted RustSec vulnerabilities or
   unsoundness advisories. Every temporary exception has an owner and expiry.
4. Multi-scene and multi-series inputs generate collision-free, deterministic
   paths and semantically correct DICOM identifiers.
5. Export ordinary failures are failure-atomic. If rollback itself cannot
   complete, the program returns a distinct recovery-required outcome backed by
   a durable journal and blocks further writes until recovery.
6. Strict validation cannot pass unless an external decoder produced and the
   program inspected the expected output.
7. Timeouts terminate the full decoder process tree and return within a bounded
   grace interval on every supported platform.
8. Geometry, metadata, DICOM representation limits, resource budgets, and ICC
   prerequisites are validated before expensive encoding or allocation.
9. Encapsulated pixel validation handles Basic and Extended Offset Tables and
   multi-fragment frames correctly.
10. GUI worker failure/disconnection reaches a terminal visible state.
11. Route-cache updates are race-safe and atomically durable; cache persistence
    failure cannot misreport a completed export as failed.
12. Profile and export paths share the same codec decisions instead of carrying
    parallel copies that can drift.
13. Large modules and parameter lists have explicit ownership boundaries backed
    by characterization tests, without breaking public or JSON contracts.
14. Required conformance tools and fixtures run in CI; their absence is a hard
    failure in the job that advertises conformance.
15. CLI, GUI state logic, platform features, fuzz targets, and changed paths have
    meaningful automated coverage.
16. Supply-chain gates represent reviewed provenance rather than an all-exempt
    baseline, and dependency-update automation is active.
17. Dead vendor code and contradictory documentation are gone, and repository
    policy tests check behavior rather than brittle source strings.
18. The complete verification matrix in section 18 passes from a clean tree.

## 5. Decisions required before dependent implementation

Record final choices in section 22. The recommended defaults below are part of
the plan but must be confirmed against ecosystem and compatibility needs.

### DEC-001: canonical dependency topology

Recommended: published `wsi-dicom` manifests use registry-resolvable versions.
Local multi-repository development may use an explicitly documented parent
workspace or local override outside the publishable manifest. Publish compatible
`j2k` and `wsi-rs` releases before publishing `wsi-dicom`.

Reject a topology that only works because sibling directories happen to exist
on one workstation. If sibling checkouts remain necessary in CI temporarily,
pin exact repository commits and treat that as a bridge, not release readiness.

### DEC-002: output naming compatibility

Recommended: adopt one explicit v2 filename for every instance, including
zero-valued scene and source-series components, for example
`scene-0000-series-0000-level-0000-z0000-c0000-t0000.dcm`. The actual fixed
widths must cover the entire validated numeric domain; otherwise constrain that
domain explicitly. Four digits are illustrative, not sufficient if values above
9,999 are accepted. One unconditional contract is easier to reason about than a
dataset-dependent naming scheme.
This changes existing single-scene output names and therefore requires a
documented pre-1.0 migration note. If compatibility is chosen instead, prove
that the conditional scheme is collision-free and retain golden tests for both
forms. Names must always be deterministic, portable, lexically sortable, and
independent of parallel execution order.

### DEC-003: DICOM identifier semantics

Define separately how Study, Series, and SOP Instance UIDs are preserved or
generated. Recommended modes are fresh random UIDs per export by default and an
explicit content-derived deterministic mode for reproducible pipelines. The
deterministic mode may require a full streaming source hash and must document
that cost. Required properties:

- Never derive semantic identity from the source file path alone.
- Preserve valid caller/source identifiers where policy permits.
- Generated identifiers include a documented stable source identity, normalized
  export-affecting configuration, and the full instance coordinate.
- Changing pixel data or identity-affecting metadata cannot silently reuse a SOP
  Instance UID.
- Moving an otherwise identical source does not change identity.
- Random and deterministic modes are explicit serialized API/CLI choices with a
  compatibility-safe default for previously serialized options.

### DEC-004: batch commit and overwrite semantics

Recommended: encode and validate into a sibling staging generation, then use a
journaled, failure-atomic commit/recovery protocol. Define whether overwrite
uses atomic directory exchange, backup-and-rollback, or is disallowed when
platform guarantees are insufficient. Never promise simultaneous visibility or
stronger atomicity than the file system and implementation provide.

### DEC-005: DICOM character repertoire

Recommended: support validated Unicode deliberately by emitting the appropriate
Specific Character Set, while rejecting control characters, NUL, invalid value
delimiters, and values that exceed VR-specific encoded limits. A simpler
ASCII-only policy is acceptable only if it is explicit in APIs and errors.

### DEC-006: resource-budget policy

Recommended: provide conservative defaults for frames, decoded pixels, encoded
bytes, temporary storage, and per-allocation size, with an explicit advanced
override. All arithmetic must be checked before conversion or allocation.

### DEC-007: GUI panic and worker-failure policy

Recommended: choose explicitly between an unwind-capable GUI profile that can
convert worker panic into sanitized failure and the existing release-wide abort
policy. Under abort, document that panic terminates the process; channel
disconnection still must be a recoverable terminal UI state. Never claim that
`catch_unwind` works in an aborting release build.

### DEC-008: report schema evolution

Recommended: treat every JSON report as an integration surface. Add `scene` and
`source_series` (final names to be confirmed) to instance reports as additive
fields with documented defaults/backward-reading behavior, and add structured
warnings without changing the existing 99 metric field names/types. Snapshot
full export, instance, profile, coverage, validation, doctor, and warning JSON.
Any rename/removal/semantic reinterpretation requires explicit migration and
semver treatment.

### DEC-009: Metal release claim

Recommended: advertise/publish Metal support only after a clean candidate
consumer resolves every transitive dependency from the registry and passes on
supported macOS hardware. If consumer-side patches or unpublished crates remain
necessary, block the claim/release rather than weakening the gate.

## 6. Phase 0 — contain release risk and preserve evidence

Security ordering invariant: complete SEC-000 and SEC-001 before repairing Cargo. The
current dependency-resolution failure accidentally prevents the vulnerable
publish path from reaching its credentialed command. Restoring the build first
would make that path executable.

### [x] SEC-000 — freeze publication and preserve incident evidence

Priority: P0 operational containment
Dependencies: repository-owner authority for settings/secret changes
Primary surfaces: GitHub Actions/environment settings, crates.io ownership and
token activity, workflow run metadata

Implementation:

1. Disable real publication or remove the repository-level crates.io secret
   before any dependency repair. Leave at most an unprivileged manual dry run.
2. Preserve workflow run metadata/logs and review GitHub audit logs where
   available. Never print or place a credential in a diagnostic command.
3. Verify crates.io versions, owners, and token activity for unexpected changes.
4. Record the existing evidence: no vulnerable workflow revision was observed
   running after its introduction. State this as no evidence of execution, not
   proof that compromise is impossible.
5. Revoke immediately if anything is suspicious. Rotate before publication is
   re-enabled even if the review remains clean.

Acceptance criteria:

- No currently runnable job can receive a crates.io token or perform a publish.
- Evidence, dates, reviewed runs, registry state, and conclusion are recorded
  without secrets or local paths.
- Publication freeze and the authority required to lift it are explicit.

Completion record:

- Owner and assigned file set: `/root`; GitHub publish workflow state,
  repository secret inventory, crates.io public release/owner evidence, and this
  plan.
- Completion commit: not applicable; this item changed external repository state.
- Tests added or changed: none; SEC-001 owns executable workflow regression tests.
- Commands run and results: workflow/run history, repository permissions,
  environments, secret names, crates.io versions/owners, workflow disablement,
  and post-disable state were checked. Workflow ID `275496130` is
  `disabled_manually`; the repository secret list is empty.
- Coverage result for changed paths: not applicable.
- Residual risk or follow-up: the crates.io-side long-lived token was disclosed
  through an unapproved channel on 2026-07-09. It was not used by this
  remediation, and the owner attests that it was revoked. Never copy credentials
  into repository or workflow state. Trusted Publishing is configured for owner
  `frames-sg`, repository `wsi-dicom`, workflow `publish.yml`, and environment
  `crates-io`; keep publication disabled until SEC-001 and REL-001 gates pass.
- Documentation or compatibility note: the vulnerable workflow revision has no
  recorded run after its 2026-06-14 introduction. All three lifetime publish runs
  and the latest `wsi-dicom 0.2.0` publication predate it; this is no evidence of
  execution, not proof of no compromise.

Read-only evidence commands:

```sh
gh run list --workflow publish.yml --limit 100 \
  --json databaseId,event,headSha,status,conclusion,createdAt,displayTitle
cargo info wsi-dicom
cargo owner --list wsi-dicom
```

### [x] SEC-001 — remove command injection from the publish workflow

Priority: P0
Dependencies: SEC-000
Primary files: `.github/workflows/publish.yml`, `scripts/publish-crate.sh`,
`.github/workflows/ci.yml`, `tests/repo_integrity.rs`, dependency-free workflow
security test

Implementation:

1. Remove the free-form version input. Derive the package version through Cargo
   metadata, require a real release ref to equal `v<manifest-version>`, and
   require the repository's dated changelog-heading format.
2. Validate every value against the package version and strict version grammar
   before it reaches any command.
3. Keep every expansion quoted and pass values as data, not executable text.
4. Remove `CRATES_IO_API_TOKEN` entirely. Use crates.io Trusted Publishing/OIDC
   through the official auth action pinned to a full commit SHA; grant
   `id-token: write` only to the protected publish job.
5. Map the short-lived auth-action output to `CARGO_REGISTRY_TOKEN` only on the
   exact publish step. Run the authenticated upload with `--no-verify` after an
   exact-SHA locked dry run so build/dependency scripts never inherit the token.
6. Set explicit least-privilege workflow permissions, disable persisted checkout
   credentials, and use a protected GitHub environment with required review for
   actual publication.
7. Keep publish and dry-run paths visibly distinct. Manual dispatch performs
   verification/dry-run only; actual publication is tag-triggered after the
   exact release SHA has passed required checks and environment approval.
8. Query public registry state with an identifiable project user agent before
   requesting OIDC authentication, and serialize publication with
   `cancel-in-progress: false`.
9. Replace the current exact-string integrity assertion with a regression check
   that fails if workflow inputs are interpolated into `run:` blocks or if the
   token is exposed outside the publish step.
10. Add a non-Cargo workflow regression test proving manual dispatch has no
    inputs, hostile shell-like mode text remains inert, OIDC is publish-job-only,
    and the script fails closed.

Acceptance criteria:

- No `run:` script contains any Actions expression interpolation.
- Free-form version input and long-lived crates.io secret references are absent.
- Dry-run, verification, and public-registry jobs have no crates.io credential or
  OIDC permission.
- Manual dispatch cannot reach a real publish command.
- Tag, manifest, changelog, and exact tested SHA must agree.
- The real publish job alone requests OIDC, requires protected-environment
  approval, and passes its short-lived token only to `cargo publish`.
- `actionlint` and a workflow security scanner pass.

Incident note: workflow history showed no execution of the vulnerable workflow
revision after its introduction on 2026-06-14. Record that evidence. Revoke the
old crates.io token before publication is re-enabled regardless; the repository
copy has already been removed. Credential changes require crates.io-owner
authority.

Verification:

```sh
actionlint
zizmor --pedantic .github/workflows/publish.yml
./.venv/bin/python -m unittest discover -s tests -p test_publish_workflow.py
```

SEC-001 may close on the standalone workflow evidence above while Cargo remains
broken. Update the Rust integrity test in the same change, but explicitly defer
executing it until BUILD-001 restores resolution; record that deferred check in
the completion evidence.

Local implementation record:

- Owner and assigned file set: `/root`; `.github/workflows/publish.yml`,
  `scripts/publish-crate.sh`, `.github/workflows/ci.yml`,
  `tests/test_publish_workflow.py`, `tests/repo_integrity.rs`, and this plan.
- Completion commit: not committed; changes remain in the working tree for
  review.
- Tests added or changed: 10 structural workflow-policy tests and 7 fake-Cargo
  publish-script behavior tests; the Rust repository-integrity assertion was
  updated in place to stop requiring the vulnerable long-lived-secret design.
- Commands run and results: all 34 Python tests passed; Pyright reported zero
  errors or warnings; `bash -n`, ShellCheck, formatting, typos, actionlint, and
  pedantic zizmor checks passed. The descriptive crates.io user agent returned
  HTTP 200 for an existing version and 404 for a missing version. The protected
  environment and remote disabled workflow/absent repository secret were also
  verified.
- Coverage result for changed paths: no numeric line-coverage tool applies to
  the YAML/shell paths. Every script mode, credential boundary, and error branch
  is behavior-tested; workflow trigger, expression, permission, checkout,
  environment, concurrency, release-binding, CI-wiring, immutable action
  references, and token-scope policies are statically tested.
  The numerical `>=80%` target is recorded as a tooling gap rather than inferred.
- Residual risk or follow-up: the owner attests that the disclosed token was
  revoked. Two other `codex` tokens remain and are outside this incident's
  scope. Trusted Publishing is registered for owner `frames-sg`, repository
  `wsi-dicom`, workflow `publish.yml`, and environment `crates-io`, but registry
  enforcement is disabled by owner choice so API-token publication remains
  permitted. The hardened repository workflow still uses short-lived OIDC. The
  sole current reviewer cannot approve their own deployment; add an independent
  reviewer. Keep workflow ID `275496130` disabled until the complete
  REL-001/REL-002 release gates pass.
- Documentation or compatibility note: manual dispatch is now credential-free
  dry-run only. Real publishing is tag-only, exact-SHA-bound, serialized,
  protected by the `crates-io` environment, and designed for a short-lived OIDC
  token. SEC-001 is complete and publication is fail-closed. Adding an
  independent reviewer remains a REL-001 prerequisite before re-enablement.

### [ ] BASE-001 — capture a reproducible pre-fix baseline

Priority: P1
Dependencies: none to start; BUILD-001 is required to complete the green
behavior/performance portion
Primary files: this plan only, unless a dedicated test-output artifact is later
approved

Implementation:

Use two checkpoints: capture failure/tooling evidence immediately while Cargo is
broken, then capture behavior/performance after BUILD-001 and before CORE-002.
Do not mark BASE-001 complete until both are recorded.

1. Record exact Rust/Cargo versions and target triples.
2. Record the current failures for metadata, check, test compilation, Clippy,
   package, audit, deny, and vet without editing dependency files.
3. Record current test counts, ignored tests, and available external tools.
4. Record current JSON-report fixtures or representative outputs before schema
   and metrics work begins.
5. Record representative CPU throughput, peak RSS, temporary-disk peak, and
   output size across small/medium/large synthetic or approved inputs. Use this
   evidence to choose initial resource-budget defaults.

Acceptance criteria:

- A later implementer can distinguish a pre-existing failure from a regression.
- Baseline output contains no credentials, patient data, or workstation paths.

## 7. Phase 1 — restore a buildable and releasable dependency topology

### [ ] WSI-001 — patch, verify, and make `wsi-rs 0.5.0` release-ready

Priority: P1 security and BUILD-001 prerequisite
Dependencies: SEC-000, SEC-001; separate authority for sibling-repository edits
and publication
Primary files: authorized `wsi-rs` manifest/lock, `src/decode/xml.rs`, XML tests,
package/release configuration

Implementation:

1. Upgrade direct `quick-xml` from 0.36 to at least 0.41.0 without disabling
   duplicate-attribute or other parser safety checks.
2. Add hostile XML regressions for many distinct attributes, excessive
   namespaces where namespace parsing is reachable, malformed/truncated input,
   and representative valid slide metadata.
3. Enforce bounded XML input/allocation/completion behavior and surface parse
   errors rather than falling back silently.
4. Verify compatibility with the selected registry `j2k` release family.
5. Run CPU, release, MSRV, docs, package, audit-with-unsound-denied, deny, and
   applicable feature checks from a clean standalone clone.
6. Prepare `wsi-rs 0.5.0` for protected publication. Actual publication requires
   explicit authority; confirm index visibility before BUILD-001 changes this
   repository to consume it.

Acceptance criteria:

- No runtime `quick-xml` below 0.41 remains in the candidate `wsi-rs` graph.
- Hostile XML completes within configured bounds and returns explicit errors.
- Representative WSI metadata parses equivalently apart from intentional error
  hardening.
- The package builds without sibling/local state and the intended registry
  version is available before BUILD-001 proceeds.

Risk: 0.36 to 0.41 crosses parser API/behavior versions; review parsing and error
semantics rather than treating compilation as proof.

### [!] BUILD-001 — reconcile `j2k`, `wsi-rs`, and `wsi-dicom` versions

Priority: P1 blocker
Dependencies: DEC-001, WSI-001
Primary files: `Cargo.toml`, `Cargo.lock`, sibling `j2k` and `wsi-rs` manifests
when separately authorized, `tests/repo_integrity.rs`

Implementation:

1. Inventory the exact API compatibility between the current `j2k 0.7.0`
   checkout, the pinned `0.6.2` contract, and `wsi-rs 0.5.0`.
2. Start from the current registry facts: the `j2k 0.6.2` family is published;
   `wsi-rs 0.5.0` and `wsi-dicom 0.4.0` are not. Preferred path: patch and
   publish `wsi-rs 0.5.0` against registry `j2k =0.6.2`, then use registry
   `j2k =0.6.2` and `wsi-rs =0.5.0` here. Do not silently substitute the dirty
   local `j2k 0.7.0` graph.
3. If 0.7 APIs are genuinely required, choose and document a coordinated 0.7
   release sequence rather than mixing exact 0.6 versions with 0.7 paths.
4. Verify registry `j2k 0.6.2` and the WSI-001 `wsi-rs 0.5.0` release are
   resolvable. Do not publish `wsi-dicom` in BUILD-001; its publication is
   exclusively a REL-002 action after the full program closes.
5. Change the publishable manifest to normal versioned dependencies. Remove the
   unconditional sibling `[patch.crates-io]` table from the release topology.
6. Document an optional local ecosystem-development override that cannot alter
   clean-clone or package behavior.
7. Regenerate `Cargo.lock` with the project toolchain.
8. Rewrite integrity tests to inspect structured manifest/lock data instead of
   asserting exact manifest source strings. Run `cargo metadata` as a top-level
   verification/CI command rather than recursively from inside `cargo test`.

Acceptance criteria:

- `cargo metadata --locked --format-version 1` succeeds in this repository.
- The same command succeeds in a fresh standalone clone with no sibling repos.
- Local ecosystem development has a documented, reproducible setup.
- All resolved `j2k` crates come from one compatible release family.
- No test encodes a workstation-specific directory topology.

Verification:

```sh
cargo metadata --locked --format-version 1
cargo check --workspace --no-default-features --all-targets --locked
cargo test --test repo_integrity --locked
```

Partial completion record:

- Owner and assigned file set: `/root`; root/fuzz manifests and lockfiles,
  `tests/repo_integrity.rs`, `tests/test_dependency_topology.py`, CI wiring, the
  small published-API adaptation in `src/export.rs` and
  `src/export/jpeg_baseline_instance.rs`, `deny.toml`, and this plan.
- Completion commit: not committed; changes remain in the working tree for
  review.
- Tests added or changed: a Cargo-independent Python policy test semantically
  parses both manifests and lockfiles, proves every locked `j2k` package is a
  checksummed crates.io `0.6.2`, rejects codec patch tables, and permits only the
  explicitly documented temporary `wsi-rs 0.5.0` bridge. CI and the Rust
  integrity suite require that policy. A focused backend-classification
  regression covers the published JPEG backend enum.
- Commands run and results: locked root/fuzz metadata, default workspace check,
  Metal and CUDA library checks, fuzz all-target check, 278 workspace tests,
  26 integrity tests, Clippy with warnings denied, formatting, 39 Python tests,
  Pyright, and deny bans/licenses/sources passed. `cargo audit` now reports four
  quick-xml vulnerability matches; prior crossbeam and memmap matches are gone.
- Coverage result for changed paths: full default workspace behavior tests
  passed; no numeric changed-path report was produced. The backend branch and
  registry-source policy have focused regression coverage.
- Residual risk or follow-up: `cargo package --locked --allow-dirty` correctly
  fails because `wsi-rs =0.5.0` is not published. Registry `wsi-rs 0.4.0` cannot
  replace it: fresh resolution fails on yanked `signinum` dependencies and its
  graph would mix `j2k 0.5` with `0.6.2`. `cargo vet --locked` remains blocked by
  pre-existing store-format inconsistencies. Complete WSI-001 under separately
  authorized sibling-repository work, publish the hardened `0.5.0`, then remove
  the last path from root and fuzz manifests and run clean-clone/package proof.
- Documentation or compatibility note: local `j2k 0.7.0` had added a
  `JpegBackend::Cuda` variant while declaring the pinned `0.6.2` contract.
  Production matches now use the actual published `0.6.2` enum; Metal and CUDA
  feature builds both pass.

### [ ] BUILD-002 — make CI operate from the same topology as users

Priority: P1
Dependencies: BUILD-001
Primary files: `.github/workflows/ci.yml`, `.cargo/config.toml`, `xtask/src/main.rs`

Implementation:

1. Ensure ordinary CI needs only the repository checkout and registry/cache
   state declared by Cargo.
2. Checkout at the repository root and remove the current nested
   `path: wsi-dicom`/working-directory assumption unless an independently
   justified layout requires it.
3. Add an early metadata job so topology errors fail before expensive matrix
   jobs and cannot be mistaken for test failures.
4. Keep lockfile-only `cargo audit --file Cargo.lock --deny unsound`
   independently runnable so
   a broken metadata graph cannot hide known advisories; report metadata-based
   deny/vet failures separately.
5. Add `--locked` to CI build, test, Clippy, docs, package, and publish dry-run
   commands unless a job is explicitly responsible for updating the lockfile.
6. Align `xtask` flags with authoritative CI, including no-default-feature and
   lockfile policy where applicable.
7. Add a clean-source-package check that builds and tests the packaged crate,
   not only the working tree.
8. Remove duplicate Metal compile checks while retaining one authoritative
   feature job.
9. Make dependency-analysis tool errors fatal; `cargo machete` must not return a
   green job after it failed to analyze manifests.
10. Pin reviewed versions of actionlint, zizmor, cargo-audit, cargo-deny,
    cargo-vet, cargo-machete, coverage, and semver tooling. Install through
    checksum-verified artifacts, locked Cargo tools, or SHA-pinned installer
    actions, and include them in update automation.

Acceptance criteria:

- Every CI job has a clear prerequisite chain and fails early on topology error.
- The package extracted from `cargo package` compiles with declared features.
- CI and documented local commands use the same dependency model.
- No duplicate job provides a second green badge for the same Metal check.
- Security/quality tool versions and artifact integrity are reproducible and
  visible in CI logs.

### [ ] BUILD-003 — prove package and dry-run publication reproducibility

Priority: P1 release blocker
Dependencies: BUILD-001, BUILD-002, SEC-001
Primary files: `Cargo.toml`, `scripts/publish-crate.sh`, `.github/workflows/publish.yml`

Implementation:

1. Inspect the package file list and ensure no sibling, local override, vendor
   tree, fixture, benchmark output, credential, or workstation path is included.
2. Build and test the generated crate archive in an isolated directory.
3. Run a credential-free publish dry run with the exact lockfile.
4. Verify the script's dependency publication-order checks against the chosen
   ecosystem versions.
5. Reconcile changelog claims with the final dependency model.

Acceptance criteria:

- `cargo package --locked` succeeds from a clean standalone clone.
- The unpacked package builds and its package-appropriate tests pass.
- `cargo publish --dry-run --locked` succeeds without a token.
- The package manifest contains only registry-resolvable dependencies.

## 8. Phase 2 — eliminate known dependency risk and make assurance real

### [ ] DEP-001 — remediate every current RustSec finding

Priority: P1 security
Dependencies: BUILD-001
Primary files: `Cargo.lock`, `deny.toml`, dependency manifests; separately
authorized changes in `wsi-rs` and `j2k` where the vulnerable dependency enters

Implementation:

1. Once Cargo resolves, capture `cargo tree -i` paths for both `quick-xml`
   versions, `crossbeam-epoch`, and `memmap2`.
2. Verify WSI-001 removed the `quick-xml 0.36.2` path and retained its hostile
   XML regression evidence in the resolved graph.
3. Trace and remove the separate `quick-xml 0.39.4` GUI path through its owning
   `eframe`/`rfd`/zbus/Wayland parent, selecting the least disruptive compatible
   upgrade patched for RUSTSEC-2026-0194 and RUSTSEC-2026-0195.
4. Confirm whether namespace-aware parsing is reachable in each path rather than
   treating advisory presence and exploitability as identical.
5. Upgrade the Rayon/Crossbeam chain to `crossbeam-epoch >=0.9.20`.
6. Upgrade `memmap2` to at least `0.9.11` and inspect call sites for affected
   advice/flush operations before declaring the issue unreachable.
7. Trace the unmaintained `encoding` and `paste` warnings through their DICOM and
   Metal parents. Prefer maintained upstream releases; track any upstream block
   with owner and review date.
8. Review and remove or re-justify the existing `RUSTSEC-2021-0153` and
   `RUSTSEC-2024-0436` ignores and every duplicate-package skip. Any temporary
   exception must name the advisory, dependency path, reachability, upstream
   issue, rationale, owner, containment, and expiry/review date.
9. Re-run audit for the workspace, GUI graph, fuzz workspace, and package lock
   with unsound warnings denied. Move to `--deny warnings` after unmaintained
   warnings are eliminated or explicitly governed.

Acceptance criteria:

- `cargo audit --file Cargo.lock --deny unsound` reports zero unaccepted
  vulnerabilities and unsound advisories.
- `cargo deny check advisories` passes without blanket suppression.
- Crafted XML inputs remain bounded and return explicit errors where limited.
- The final report distinguishes fixed, unreachable, and temporarily accepted
  advisories with evidence.

### [ ] DEP-002 — replace the cargo-vet exemption baseline with evidence

Priority: P1 assurance
Dependencies: BUILD-001, DEP-001
Primary files: `supply-chain/config.toml`, `supply-chain/audits.toml`,
`supply-chain/imports.lock`, `.github/workflows/ci.yml`, `.github/CODEOWNERS`

Implementation:

1. Inventory all 504 exemptions by runtime/dev/build role and criticality.
2. Decide whether first-party `audit-as-crates-io` policy is intentional; do not
   let the project accidentally self-exempt its own code.
3. Run `cargo vet suggest` after the graph stabilizes and explicitly choose
   trusted import sources.
4. Import audits from explicitly trusted ecosystems where policy allows.
5. Perform local audits for high-impact parsers, codecs, unsafe/FFI, compression,
   IPC, file-format, and GPU crates not covered by trusted imports.
6. Reduce exemptions in reviewable batches; never mechanically convert an
   exemption into an audit certification.
7. Define criteria for `safe-to-run`, `safe-to-deploy`, and cryptography or
   native-code review.
8. Make the CI gate describe what is actually covered and publish an exemption
   count trend.
9. Add designated CODEOWNER review for `supply-chain/**`, `deny.toml`, and
   `Cargo.lock`; configure protection before calling the assurance gate complete.
10. Prevent the exemption count from increasing without that designated review.

Acceptance criteria:

- `cargo vet --locked` passes from a clean clone.
- Every runtime dependency has trusted/local `safe-to-deploy` evidence; build and
  dev dependencies have at least `safe-to-run` evidence.
- No blanket/generated exemption baseline remains. Any narrowly unavoidable
  exemption is justified, owned, time-bounded, and cannot increase without
  designated review.
- CI does not describe an all-exempt baseline as reviewed provenance.
- CODEOWNER protection is active for vet/deny/lockfile policy changes.

### [ ] DEP-003 — add dependency-update and multi-ecosystem vulnerability checks

Priority: P2
Dependencies: BUILD-001, DEP-001
Primary files: `.github/dependabot.yml` or approved equivalent,
`.github/workflows/ci.yml`, `bench/requirements.txt`

Implementation:

1. Add scheduled Rust, GitHub Actions, and Python dependency updates with sane
   grouping and review limits.
2. Add a Python dependency vulnerability scan for benchmark tooling.
3. Keep lockfile updates separate from unrelated feature work.
4. Require audit, deny, tests, and package checks on dependency update changes.
5. Preserve full-SHA action pinning; keep major updates separate and prohibit
   auto-merge unless every required check and designated policy review passes.

Acceptance criteria:

- Each managed ecosystem receives scheduled update proposals.
- Security updates can be merged independently and are verified by the same
  gates as handwritten dependency changes.
- Python dependencies have a lock or documented reproducibility policy.

### [ ] DEP-004 — prove Metal works for a registry-only downstream consumer

Priority: P1 for advertised Metal support
Dependencies: BUILD-001, DEP-001, DEC-009; supported macOS hardware
Primary files: dependency manifests, Metal feature CI, README support matrix

Risk: published `j2k-metal 0.6.2` uses `metal 0.33`, while the local 0.7
ecosystem may rely on workspace patches that downstream consumers do not
inherit. A local feature check is not proof that the published graph works.

Implementation:

1. Before `wsi-dicom` is published, create an isolated consumer of the candidate
   package/path while requiring every transitive dependency to resolve from the
   registry; enable `metal` with no sibling paths or consumer-side patches. An
   actual registry `wsi-dicom` consumer is the later REL-002 proof.
2. Compile and run representative Metal tests on the declared Rust/macOS
   support floor and current stable toolchain.
3. If the registry chain requires an unpublished/local patch, coordinate and
   publish the needed upstream `metal`/`j2k`/`wsi-rs` versions in topological
   order.
4. If upstream publication is blocked, mark Metal release support blocked and
   correct documentation rather than weakening the clean-consumer gate.

Acceptance criteria:

- A clean candidate consumer enables Metal with registry-only transitive
  dependencies; REL-002 later repeats with the published registry crate.
- No local patch, sibling checkout, or unpublished crate is required.
- Metal output/parity tests pass on supported hardware.
- README and release claims match the verified registry graph.

## 9. Phase 3 — establish validated domain invariants before encoding

### [ ] CORE-001 — centralize geometry and DICOM representability validation

Priority: P1 correctness and availability
Dependencies: BUILD-001
Primary files: `src/options.rs`, `src/request.rs`, `src/export.rs`,
`src/export/icc_profile.rs`, `src/writer.rs`, `src/tile.rs`

Implementation:

1. Introduce one validated geometry/domain object created during preflight. It
   must own checked forms of image dimensions, tile dimensions, frame grid,
   frame count, and every value later narrowed to DICOM `u16`/`u32` fields.
2. Reject zero tile width/height before ICC probing, routing, allocation, or
   encoder selection. Remove downstream assumptions that can divide by zero.
3. Use checked multiplication/addition/ceiling-division for tile grids, total
   pixels, frame counts, offsets, and encoded-length calculations.
4. Validate maximum row/column slide positions against DICOM SL, Dimension Index
   Values against UL, Extended Offset Table value-byte lengths against explicit-
   VR limits, checked instance/series numbering, and zero Total Pixel Matrix and
   WholeLevel dimensions. Calculate maximum grid positions directly with checked
   arithmetic rather than iterating every frame.
5. Validate all DICOM value-representation limits before the first frame is
   encoded. Error messages must identify the field and offending value.
6. Pass the validated object to downstream code instead of repeatedly
   converting unchecked primitives.
7. Search for pattern-equivalent late conversions and zero assumptions across
   all CPU, JPEG, HTJ2K, Metal, CUDA, passthrough, profiling, and synthetic
   routes.

Required tests:

- Zero virtual tile width and height each fail without panic.
- Dimensions at each legal boundary succeed; boundary-plus-one fails.
- Products that overflow `u32`, `usize`, or configured budgets fail cleanly.
- Failure occurs before any encoder, ICC probe, output writer, or cache update.
- All routes consume the same preflight invariants.

Acceptance criteria:

- No untrusted dimension participates in unchecked arithmetic or division.
- `writer.rs` does not discover basic representability errors after encoding.
- Release builds with `panic = "abort"` return normal errors for tested invalid
  geometry.

### [ ] CORE-002 — enforce explicit resource budgets and fallible allocation

Priority: P1 availability
Dependencies: CORE-001, DEC-006, BASE-001
Primary files: `src/options.rs`, `src/export.rs`,
`src/export/lossless_j2k_instance.rs`, `src/writer.rs`, `src/validation.rs`

Implementation:

1. Define a documented `ResourceBudget` covering at least instance count, frames
   per instance, total frames across the export, total decoded pixels, estimated
   and actual encoded bytes, per-allocation bytes, temporary storage, and
   external-decoder output.
2. Set conservative CLI/library defaults and expose an explicit override for
   trusted large workloads. Report which limit was exceeded and how to adjust
   it safely.
3. Preflight estimates before allocating per-frame vectors or DICOM functional
   groups.
4. Enforce incremental actual encoded/temp-byte use during execution; estimates
   alone are not a safety boundary.
5. Replace untrusted-size `Vec::with_capacity`/bulk allocation with
   `try_reserve` or streaming where practical.
6. Stream/spool per-frame functional groups or enforce a measured hard cap;
   `try_reserve` alone does not prevent later memory exhaustion.
7. Ensure parallel routes cannot each consume the entire global budget. Reserve
   or schedule resources centrally.
8. Clean up staging and decoder outputs after a budget failure.

Required tests:

- Maliciously large geometry returns a bounded error rather than aborting or
  attempting the allocation.
- Aggregate parallel work respects a global budget.
- Exact-limit inputs succeed and limit-plus-one inputs fail.
- Overrides are explicit, validated, and represented in reports without
  leaking environment details.

Acceptance criteria:

- Every allocation proportional to input geometry is preflighted or fallible.
- Resource failures leave no final or temporary output behind.
- Normal representative slides show no material throughput regression.

### [ ] META-001 — validate all numeric clinical and spatial metadata

Priority: P1 correctness
Dependencies: BUILD-001
Primary files: `src/metadata.rs`, `src/writer.rs`, metadata JSON fuzz target

Implementation:

1. Require `imaged_volume_depth_mm` to be a finite, strictly positive `f64`, to
   convert to a finite nonzero `f32` for Imaged Volume Depth (FL), and to be
   representable in the DICOM decimal-string encoding used for Slice Thickness.
2. Audit pixel spacing, objective power, focal-plane spacing, offsets, and other
   numeric metadata for the same NaN/infinity/sign/range problem.
3. Normalize and validate at metadata construction/deserialization boundaries,
   not only while writing tags.
4. Use one tested DICOM decimal formatter with explicit precision and maximum
   encoded length.
5. Return field-specific errors without echoing sensitive metadata values more
   broadly than necessary.

Required tests:

- Zero, negative, NaN, positive/negative infinity, `f32` overflow,
  positive-underflow-to-zero, excessive magnitude, and non-representable
  precision are rejected for every affected field.
- Legal boundary and round-trip values produce expected DICOM strings.
- JSON and Rust API construction enforce equivalent rules.
- Fuzzing cannot reach a panic through floating-point edge cases.

Acceptance criteria:

- Invalid spatial metadata never reaches `writer.rs`.
- All emitted decimal values meet DICOM length and syntax constraints.

### [ ] META-002 — make DICOM text and character-set handling explicit

Priority: P1 clinical interoperability
Dependencies: BUILD-001, DEC-005
Primary files: `src/metadata.rs`, `src/writer.rs`, metadata tests and fuzz target

Implementation:

1. Define validation by target VR, including PN, LO, SH, CS, and any free-text
   fields currently emitted.
2. Reject NUL, unsupported controls, unintended `\` value delimiters, malformed
   PN component structure, and encoded values beyond VR limits.
3. If Unicode is supported, emit and test the correct Specific Character Set
   and measure limits in encoded bytes. If not, reject non-ASCII with a clear
   policy error.
4. Preserve intentional multi-value fields through typed APIs rather than
   allowing callers to smuggle delimiters through scalar strings.
5. Validate strings before any output is staged.

Required tests:

- ASCII, accented, CJK, combining-character, and representative PN values under
  the selected policy.
- Backslash, NUL, newline/control, excessive byte length, and malformed PN are
  rejected or represented only through an explicit typed multi-value API.
- Independent DICOM validators accept the emitted character-set declarations.

Acceptance criteria:

- No scalar metadata field can accidentally create multiple DICOM values.
- Every non-ASCII output has an appropriate character-set declaration.
- JSON and Rust API inputs have consistent validation behavior.

## 10. Phase 4 — correct instance coordinates, paths, and DICOM identity

### [ ] IDENT-001 — introduce one complete instance coordinate

Priority: P1 data integrity
Dependencies: BUILD-001, DEC-002, DEC-008
Primary files: `src/export.rs`, `src/instance_context.rs`, `src/uid.rs`,
`src/request.rs`

Implementation:

1. Define an `InstanceKey` containing every axis that distinguishes output:
   source series, scene, pyramid level, Z, channel, time, and any future optical
   path or focal-plane discriminator.
2. Construct it once during job enumeration and pass it to path, UID, metadata,
   report, and cache logic.
3. Make output-path generation consume only this key plus the output root.
4. Implement the chosen DEC-002 contract. Under the recommended v2 policy, emit
   all axes even when scene and source series are zero.
5. Reject duplicate keys during preflight with diagnostic detail before any
   output or cache mutation.
6. Verify ordering is deterministic independent of Rayon scheduling.
7. Add scene and source-series fields to `InstanceReport` and preserve existing
   report fields/types through additive JSON compatibility tests.

Required tests:

- At least two scenes with identical level/Z/C/T coordinates.
- At least two source series with identical coordinates.
- Combined multi-scene, multi-series, multi-channel, multi-time input.
- Stable names under serial and parallel execution.
- Values at the chosen filename-width boundary and the maximum accepted index;
  unequal accepted keys remain unequal and lexical order matches numeric order.
- Existing single-scene golden tests are migrated deliberately and the filename
  contract change is documented if v2 is selected.

Acceptance criteria:

- Every enumerated job has a unique key and final path.
- Path collision is impossible for supported axis combinations unless the
  caller supplied genuinely conflicting output policy.
- Reports identify outputs using the same coordinate as the writer.

### [ ] UID-001 — implement documented DICOM UID scope and lifecycle semantics

Priority: P1 DICOM correctness
Dependencies: IDENT-001, DEC-003, META-001, META-002
Primary files: `src/uid.rs`, `src/instance_context.rs`, `src/metadata.rs`,
`src/writer.rs`, public API documentation

Implementation:

1. Separate Study, Series/Pyramid, SOP Instance, Frame of Reference, Dimension
   Organization, and Specimen identity inputs, scope, and lifecycle.
2. Define the normalized identity-affecting export configuration, including
   transfer syntax, pixel-affecting options, scene/series/level coordinates,
   dimensions, and applicable metadata.
3. Add an explicit serialized `UidGeneration` option covering random-per-export
   and versioned content-derived modes. Use a default compatible with older
   serialized options and expose it consistently through Rust API, CLI, GUI,
   reports, and documentation.
4. For content-derived mode, use a documented UID root and decimal encoding of a
   cryptographic digest, or
   another standards-compliant generator with equivalent collision properties.
5. For random mode, create one cryptographically strong export namespace and
   derive the intended Study/Series/Pyramid/SOP/Frame-of-Reference/Dimension-
   Organization/Specimen hierarchy without accidental reuse.
6. Remove source path as the sole source identity. Provide a stable identity
   seed or content/source identifier when the input format cannot supply one.
7. Validate caller-provided UIDs for DICOM syntax and length.
8. Version the deterministic UID algorithm if future changes could reinterpret
   the same seed.
9. Document migration effects for previously exported objects.

Required tests:

- Random mode gives disjoint generated UIDs across otherwise identical exports.
- Moving an identical source does not change deterministic UIDs.
- Changing pixel data, transfer syntax, scene/series coordinate, or
  identity-affecting metadata changes the appropriate UID.
- Repeating the same export produces identical deterministic UIDs.
- All generated UIDs are valid, within length limits, and unique across a large
  generated corpus.
- Preserved caller UIDs remain unchanged where policy allows.
- Hierarchy tests prove which instances intentionally share or differ in Study,
  Series/Pyramid, Frame of Reference, Dimension Organization, SOP Instance, and
  Specimen UIDs.
- Reused placeholder specimen identifiers in unrelated exports cannot create a
  globally reused Specimen UID.

Acceptance criteria:

- No two semantically distinct SOP Instances intentionally share a UID.
- UID behavior is public, tested, and independent of output path formatting.
- Compatibility impact is captured in the changelog before release.

## 11. Phase 5 — make output and route-cache state transactional

### [ ] TX-001 — stage, validate, and commit an export as one generation

Priority: P1 data integrity
Dependencies: IDENT-001, UID-001, CORE-001, DEC-004
Primary files: `src/export.rs`, `src/writer.rs`, output-path helpers, integration
tests

Implementation:

1. Represent one export invocation as an explicit generation with a manifest of
   expected instances, final paths, checksums/lengths where useful, and status.
2. Preflight every final path and overwrite conflict before encoding.
3. Write all instances to a same-file-system sibling staging location using the
   existing safe temporary-file primitives.
4. Validate staged instances before commit when strict validation is requested.
5. Commit the generation using the strongest atomic primitive available for the
   selected output layout. Document platform limitations.
6. Serialize commit operations using a cooperative output-directory lock. After
   acquiring it, revalidate every destination, type, owner marker, and symlink
   condition before changing final state. In no-clobber mode use a true
   no-replace primitive rather than a check-then-rename race. For flat multi-file
   destinations, journal backup/promote/restore transitions and sync the journal
   plus affected directories.
7. On any pre-commit error, remove staging and leave the prior final generation
   untouched.
8. On ordinary commit error, remove newly promoted entries and restore byte-
   identical backups. On process restart, detect a journal and deterministically
   restore or complete it before allowing another export.
9. If rollback itself cannot complete, return a distinct `RecoveryRequired`
   outcome with the durable journal location/state and block subsequent exports
   to that destination until recovery. Do not collapse this into an ordinary
   export error or success.
10. Ensure Ctrl-C/process interruption leaves identifiable, safely removable
    staging state and never masquerades as a completed generation.
11. Construct the final report only after commit succeeds; reports must never
    reference uncommitted final paths.

Required tests:

- Inject failure on the first, middle, and last instance.
- Inject validation, fsync, rename, permission, out-of-space, and destination
  conflict failures.
- Exercise serial and parallel execution and overwrite/no-clobber modes.
- Confirm prior outputs remain byte-identical after a failed overwrite.
- Confirm success exposes exactly the manifest set and no staging residue.

Acceptance criteria:

- An ordinary reported export failure for which rollback completed exposes no
  new partial final generation.
- A reported success exposes every expected instance exactly once.
- `RecoveryRequired` is separately typed/reported, retains enough durable state
  to recover, and cannot be mistaken for ordinary failure or success.
- Recovery behavior after interruption is deterministic and documented.
- Existing symlink, no-clobber, same-directory temp, and sync defenses remain.

Contract limit: an arbitrary set of flat files cannot become visible
simultaneously through one portable rename. Promise failure-atomic ordinary
errors and crash recovery, not instantaneous multi-file visibility. Stronger
visibility requires a versioned-directory plus atomic pointer design and a
separate compatibility decision.

### [ ] TX-002 — define and test overwrite, retry, and recovery behavior

Priority: P1
Dependencies: TX-001
Primary files: export options/API, CLI help, GUI controls, integration tests

Implementation:

1. Define behavior for an absent destination, complete existing generation,
   incomplete legacy output, abandoned staging generation, and conflicting
   non-wsi-dicom file.
2. Keep no-clobber the safe default. Make overwrite explicit and generation
   scoped.
3. Provide recovery diagnostics or a dry-run cleanup command; do not silently
   delete ambiguous user files.
4. Ensure retries after a failed generation do not collide with its staging
   names or reuse partial state without validation.

Acceptance criteria:

- Every destination state has a tested, documented outcome.
- The program never silently merges old and new generations.
- Cleanup refuses paths it cannot prove belong to an abandoned generation.

### [ ] CACHE-001 — make route-cache concurrency and persistence correct

Priority: P1 correctness
Dependencies: BUILD-001, DEC-008; coordinate with TX-001
Primary files: `src/export/route_cache.rs`, `src/export.rs`, report types

Implementation:

1. Add a monotonically increasing revision or equivalent state model so a flush
   can clear `dirty` only if no update occurred after its snapshot.
2. Acquire an interprocess cache lock, re-read and merge on-disk state under that
   lock using a deterministic conflict rule, then serialize to a same-directory
   temporary file, flush/sync, atomically replace the cache, and sync the parent
   directory where supported.
3. Preserve size limits, symlink safety, and parse-error visibility.
4. Treat optional cache load, parse, merge, lock, and write failures as visible
   structured warnings with a defined in-memory/no-cache fallback.
5. Decide cache failure semantics independently of output success. A failed
   post-export optimization write should produce a warning/status field, not a
   false claim that successfully committed DICOM output failed.
6. Avoid holding a global in-process lock during slow I/O while still proving no
   update is lost; the interprocess lock covers the read-merge-write file
   transaction.
   lost.

Required tests:

- Update during flush remains dirty and is persisted by the next flush.
- Concurrent writers cannot produce truncated or invalid JSON.
- Concurrent processes preserve both valid non-conflicting updates and apply the
  documented deterministic conflict rule.
- Simulated write, sync, and rename failures preserve the previous valid cache.
- Successful output plus cache failure reports output success with an explicit
  cache warning.
- Malformed/unreadable cache input produces a warning and defined fallback, not
  silent success or export failure.

Acceptance criteria:

- No acknowledged cache update can be lost by a concurrent flush.
- Readers see either the old or new complete cache, never a partial file.
- Cache state cannot change the truth of the export completion result.

## 12. Phase 6 — make validation evidence real and bounded

Implementation order within this phase is VAL-003, then VAL-001, then VAL-002
and VAL-004. Frame reconstruction and its byte budgets must be correct before
decoder-output postconditions are treated as evidence.

### [ ] VAL-001 — require and inspect external decoder output

Priority: P1 validation integrity
Dependencies: BUILD-001, CORE-001, CORE-002, VAL-003
Primary files: `src/validation.rs`, CLI validation options, validation tests

Implementation:

1. Give every invocation a unique, size-budgeted temporary output path.
2. After a zero exit status, require the output to exist, be a regular file, be
   nonempty, and satisfy expected decoded dimensions/component/sample size.
3. For the current `.ppm` contract, implement a bounded binary P5/P6 parser that
   handles comments, width, height, max value, component count, 8/16-bit sample
   width, checked raster length, and defined trailing-data rules. If another
   format is added, configure it explicitly rather than guessing from bytes.
   Do not treat mere file creation as pixel validation.
4. Reject pre-existing output, symlinks, unexpected extra files, and output that
   exceeds the resource budget.
5. Distinguish tool launch failure, timeout, nonzero exit, malformed output,
   decoded-raster structural mismatch, and cleanup failure in reports.
6. Ensure strict mode cannot use a template such as `/usr/bin/true` to pass.
7. Document that custom `{output}` decoder templates must produce the configured
   PGM/PPM form, including component and sample-width expectations.

Required tests:

- Zero-exit/no-output, empty output, wrong-size output, symlink output, oversized
  output, malformed output, and valid output.
- A fake decoder fixture for deterministic cross-platform testing.
- Real decoder integration cases in the dedicated conformance job.

Acceptance criteria:

- `Passed` means the decoder produced a structurally valid raster matching
  expected geometry, component count, and sample width. It does not prove pixel
  equality with the source unless a separate reference-pixel comparison is
  implemented and reported.
- Decoder output and temporary files are cleaned on success and every failure.
- Errors are actionable but do not expose sensitive source paths unnecessarily.

### [ ] VAL-002 — enforce timeouts on the full external process tree

Priority: P1 availability
Dependencies: VAL-001
Primary files: `src/validation.rs`, platform-specific process helper if needed

Implementation:

1. Start each decoder in an isolated process group/job object.
2. On timeout, send graceful termination to the entire group, wait a short
   bounded grace period, then force termination of the group.
3. Prefer bounded private capture files, or a reviewed nonblocking channel
   design, over unconditional pipe-reader joins. Monitor capture size and kill
   the group on overflow. Never call an unbounded `JoinHandle::join` after a
   timeout while a descendant may still hold the pipe.
4. Reap the child and return within timeout plus the documented grace interval.
5. Implement and test equivalent semantics on Linux, macOS, and Windows if
   Windows remains supported.
6. Configure Windows Job Objects to prohibit breakaway where supported. Document
   that Unix supervision covers the created process group and cannot absolutely
   control a deliberately escaping/daemonizing descendant without stronger
   sandboxing.

Required tests:

- Child that sleeps forever.
- Child that spawns a grandchild holding stdout/stderr open.
- Child that ignores graceful termination.
- Child that floods stdout/stderr.
- Normal child exiting near the timeout boundary.

Acceptance criteria:

- No timeout test hangs a test process.
- No process remaining in the supervised process group/Job Object survives the
  validation result. Any deliberate escape limitation is documented and does
  not weaken the normal decoder contract.
- Captured output is bounded and truncation is explicit.

### [ ] VAL-003 — validate encapsulated frames using DICOM offset semantics

Priority: P1 interoperability
Dependencies: BUILD-001, CORE-002
Primary files: `src/validation.rs`, DICOM fragment tests, fuzz target

Implementation:

1. Stop assuming one encapsulated fragment equals one frame.
2. Resolve frame boundaries using a valid Basic Offset Table, Extended Offset
   Table/Lengths where applicable, or standards-compliant fallback rules.
3. Prefer a maintained DICOM library abstraction if it preserves the needed
   validation evidence; otherwise isolate and thoroughly test the parser.
4. Validate offset monotonicity, bounds, item padding, frame count, overflow,
   truncated items, and conflicting tables.
5. Reconstruct frames through a bounded streaming/spooling path with a configured
   encoded-frame-byte limit. Do not concatenate an untrusted whole frame in
   memory merely for convenience.
6. Define `max_pixel_frames` as a count of reconstructed frames, never raw
   fragments.
7. If offset tables are empty/unusable, allow a single frame to consume all
   fragments; for multiple frames, fail an ambiguous layout unless a maintained
   library can establish an unambiguous standards-compliant mapping. Never fall
   back silently to one-fragment-per-frame.

Required tests:

- One fragment per frame, multiple fragments per frame, empty Basic Offset
  Table, valid Basic Offset Table, valid Extended Offset Table, malformed and
  conflicting tables, padding, truncation, and offset overflow.
- Real DICOM files produced by at least two independent encoders.
- Fuzz corpus seeds for every table form.

Acceptance criteria:

- Standards-compliant multi-fragment objects do not fail merely because of
  fragment layout.
- Malformed offsets return bounded errors without panic or excessive allocation.

### [ ] VAL-004 — prevent missing tools from becoming a green conformance result

Priority: P1 assurance
Dependencies: BUILD-002
Primary files: `src/export/tests/external_htj2k_tests.rs`,
`src/export/tests/export_integration_tests.rs`, `src/export/tests/support.rs`,
`.github/workflows/ci.yml`

Implementation:

1. Separate hermetic unit tests using fake tools from real conformance tests.
2. In the CI job advertised as conformance, install and version-check
   `dciodvfy`, `dcentvfy`, `grk_decompress`, `opj_decompress`, and `djpeg` as
   applicable.
3. Fail that job if a required tool is absent. Do not return early with success.
4. If local tests remain optional, mark them explicitly ignored or emit a
   machine-readable skipped status that CI checks.
5. Record tool versions and validator outcomes in CI artifacts.

Acceptance criteria:

- Removing any required tool from the conformance image makes the job fail.
- A green conformance job proves that every named validator actually ran.
- Hermetic tests still cover error mapping when system tools are unavailable.

## 13. Phase 7 — make frontend lifecycle and error contracts explicit

### [ ] GUI-001 — model worker lifecycle and handle disconnection

Priority: P1 usability and correctness
Dependencies: BUILD-001, DEC-007
Primary files: `apps/wsi-dicom-gui/src/main.rs`; new GUI-internal state/worker
modules only where they create testable ownership

Implementation:

1. Replace the `running` boolean plus optional receiver with an explicit state
   machine: idle, running, succeeded, failed, and disconnected/worker-lost.
2. Match `TryRecvError::Empty` and `TryRecvError::Disconnected` separately.
   Empty remains running; disconnection is terminal failure, clears the receiver,
   restores controls, and displays a sanitized diagnostic.
3. Retain and reap the worker `JoinHandle` so completed workers do not
   accumulate and so abnormal completion can be observed where the panic model
   permits it.
4. Extract state transitions and worker polling from egui rendering so they can
   be tested without opening a native window.
5. Do not promise recoverable worker-panic handling while the workspace release
   profile uses `panic = "abort"`. Decide explicitly whether the GUI needs an
   unwind profile; if abort is retained, document that a panic terminates the
   process rather than becoming a stranded channel.
6. Surface export warnings, including route-cache persistence warnings, without
   reclassifying successful output as failed.

Required tests:

- Empty queue leaves the state running.
- Success and ordinary error reach their terminal states.
- Dropped sender reaches worker-lost/failed and re-enables Export.
- Repeated exports reap prior handles and do not leak workers.
- Warning-only results render success plus warnings.
- If unwind behavior is selected, a simulated worker panic becomes a sanitized
  failure; otherwise test and document the abort policy at the appropriate
  boundary.

Acceptance criteria:

- No channel terminal state can leave the UI permanently running.
- The export action becomes usable after every recoverable terminal outcome.
- UI code does not silently discard a send, receive, save, or worker error.

### [ ] FRONT-001 — centralize bounded metadata-file loading

Priority: P2 maintainability and consistency
Dependencies: META-001, META-002
Primary files: `src/main.rs`, `apps/wsi-dicom-gui/src/main.rs`, `src/metadata.rs`

Implementation:

1. Replace duplicated CLI/GUI `load_metadata_source` and `read_capped_file`
   implementations with one coherent library operation.
2. Preserve byte caps, parse behavior, research-placeholder conflicts, and
   user-facing error context. Define and enforce a shared regular-file and
   symlink policy; the current duplicate loaders do not already provide that
   guarantee.
3. Keep frontend-specific presentation outside the shared parser.
4. Make the API public only if it is a supported capability rather than a
   workspace-visibility convenience.

Required tests:

- Valid, empty, oversized, malformed, directory, symlink, disappearing, and
  unreadable files through the shared layer.
- CLI and GUI adapters map the same core error to appropriate presentation.

Acceptance criteria:

- Size and parsing policy exist in one implementation.
- Neither frontend can silently weaken metadata input limits.

## 14. Phase 8 — characterize behavior, then reduce structural debt

Architecture work starts only after SEC-001, BUILD-001, and the directly
affected P1 correctness items are green. Keep pure moves separate from behavior
changes so review and history remain intelligible.

### [ ] ARCH-000 — add characterization contracts before broad moves

Priority: P1 refactor prerequisite
Dependencies: BUILD-001, DEC-008 plus applicable correctness fixes
Primary files: integration tests, report tests, DICOM writer tests

Add protection for:

1. The exact 99-key flattened `ExportMetrics` JSON field names and value types,
   plus full export, instance, profile, coverage, validation, doctor, and warning
   report JSON shapes.
2. Merge semantics for every metric: saturating sum, maximum/high-water, or
   derived-only.
3. Export/profile route and counter parity on deterministic CPU fixtures.
4. Canonical DICOM tags, values, ordering where relevant, functional groups,
   encapsulation, and offset tables for JPEG and J2K output.
5. Deterministic output/report ordering under serial and parallel execution.
6. A minimal shipped-binary CLI characterization suite covering exit status,
   stdout/stderr separation, JSON-line contracts, and one critical synthetic
   conversion. QA-003 expands this baseline before/alongside frontend moves.
7. Existing output safety: no-clobber, overwrite, symlink, temp, sync, and error
   cleanup.
8. Representative CPU performance and memory so refactoring cannot hide a major
   regression.

Acceptance criteria:

- Each module split below can be shown as behavior-preserving apart from a
  separately reviewed correctness change.
- JSON/DICOM compatibility failures produce useful diffs.
- Characterization tests do not assert source text or private function layout.

### [ ] ARCH-001 — establish shared domain types and dependency direction

Priority: P1 maintainability
Dependencies: ARCH-000, IDENT-001, CORE-001
Primary files: `src/defaults.rs`, `src/export.rs`, `src/routing.rs`, export
submodules

Implementation:

1. Reuse the validated `InstanceKey`/geometry from earlier phases rather than
   introducing parallel coordinate types.
2. Introduce focused, immutable parameter objects such as an instance job,
   instance export context, route context, and row range. Constructors enforce
   invariants; they must not be unstructured bags hiding unrelated parameters.
3. Move source-route probing/shared planning below defaults and export, likely
   into routing/planning. `defaults` may depend on routing; export may depend on
   both. Defaults must not import export orchestration internals.
4. Replace production `use super::*` with explicit imports.
5. Ratchet down `too_many_arguments` suppressions as contexts land. New
   production functions should normally have at most seven parameters.

Acceptance criteria:

- The logical defaults/export dependency cycle is gone.
- No production wildcard parent imports remain.
- No new `too_many_arguments` allowance is introduced.
- Remaining allowances are individually documented external constraints and
  have follow-up IDs.

### [ ] ARCH-002 — decompose export orchestration

Priority: P1 maintainability
Dependencies: ARCH-000, ARCH-001, TX-001
Primary files: `src/export.rs`, `src/export/`

Safe extraction order:

1. Move instance planning, jobs, keys, and target enumeration into a focused
   planning module.
2. Move session/worker orchestration and transaction coordination into a
   session/execution module.
3. Move route profiling, coverage traversal, and corpus traversal into distinct
   modules because they have different cost and output contracts.
4. Move lossless J2K execution to its codec family.
5. Move JPEG baseline/fallback execution to its codec family.
6. Leave the export facade with the existing public entry points and stable
   error/report contracts.

Review targets, not brittle test limits:

- Export facade approximately 250 lines or less.
- Production files preferably below 700 lines and normally below 1,000.
- Production functions normally below 100 lines; any function above 150 needs
  an explicit cohesive-reason justification.
- No parameter list above seven absent an external ABI/callback constraint.

Acceptance criteria:

- Each extracted module has one named responsibility and focused tests.
- Public export APIs and documented errors remain compatible unless separately
  versioned.
- CPU, CUDA, and Metal configurations compile on supported environments.
- The existing line-budget test is replaced with ownership/behavior protection,
  not merely reset to a larger number.

### [ ] ARCH-003 — eliminate profile/export codec drift

Priority: P1 correctness and maintainability
Dependencies: ARCH-000, ARCH-002
Primary files: `src/export.rs`, `src/export/jpeg_baseline_instance.rs`,
`src/export/lossless_j2k_instance.rs`, profiling modules

Implementation:

1. Introduce route-family row/instance executors returning a typed encoded
   outcome: frames, route events, metrics, pixel profile, and byte counts.
2. Export consumes the outcome through an output sink; profile validates and
   discards frame bytes. Keep coverage as cheap classification/planning unless
   its contract explicitly requires encoding.
3. Record route decisions once through typed events or focused metric methods.
4. Delete duplicated profile implementations only after parity tests pass; use
   `trash` for whole-file deletion where applicable.

Required tests:

- Export/profile parity for JPEG passthrough, JPEG retile, direct HTJ2K, J2K
  passthrough, lossless fallback, and conditional Metal routes.
- Same route decisions, counts, warnings, and codec parameters for identical
  input/options, excluding writer-only timings.

Acceptance criteria:

- The known exact clone groups between export and profile are gone.
- Codec decision logic has one implementation per route family.
- Production duplication remains no worse than the audited 2.01%, with risky
  semantic clones specifically eliminated.

### [ ] ARCH-004 — decompose writer without weakening output safety

Priority: P1 maintainability
Dependencies: ARCH-000, CORE-001, TX-001, VAL-003
Primary files: `src/writer.rs`; focused `src/writer/` modules

Suggested ownership boundaries:

- object construction and validated parameters;
- patient/study/specimen/equipment/image DICOM sections;
- dimensions and shared/per-frame functional groups;
- pixel-data spool/buffer sinks;
- encapsulated headers, fragments, offset tables, and patching;
- output temp/no-clobber/symlink/sync/persistence primitives.

Implementation rules:

1. Preserve element insertion order until independent validation proves an
   intentional change safe.
2. Reuse one output-persistence layer from TX-001; do not duplicate it in
   export modules.
3. Split `build_dicom_object` by DICOM section using validated inputs.
4. Preserve streaming and bounded-memory behavior; do not trade file size for a
   giant in-memory object graph.

Acceptance criteria:

- Canonical DICOM structure and pixel-data byte tests pass.
- Basic/Extended Offset Tables remain correct for the writer's current
  one-fragment-per-frame output model. Multi-fragment writing, if later desired,
  is a separate feature with its own design and compatibility tests; VAL-003 is
  responsible for reading valid multi-fragment input.
- Existing and new transaction/symlink/no-clobber tests pass.
- No extracted module exposes an invalid partially constructed writer state.

### [ ] ARCH-005 — decompose validation after correctness fixes land

Priority: P1 maintainability
Dependencies: ARCH-000, VAL-001, VAL-002, VAL-003
Primary files: `src/validation.rs`; focused `src/validation/` modules

Suggested ownership boundaries:

- public options, result/status/report types;
- bounded directory discovery;
- external process runner and process-tree supervision;
- tool discovery/doctor;
- DICOM set validators;
- encapsulated-frame reconstruction and pixel decoder verification;
- private temporary validation files.

Acceptance criteria:

- The public validation API and JSON shape remain stable or receive explicit
  migration treatment.
- Fake-runner tests cover success, nonzero exit, timeout, capture truncation,
  missing/empty/malformed output, and process setup failure.
- Directory traversal retains symlink, entry-count, file-size, and depth limits.
- Strict mode has no path to success when evidence is absent.

### [ ] ARCH-006 — make metrics aggregation maintainable without changing JSON

Priority: P2 maintainability
Dependencies: ARCH-000, ARCH-003
Primary files: `src/report.rs`; focused report/metrics modules

Current constraint: Rust metrics are already grouped into `RouteCounters`,
`JpegDirectHtj2kMetrics`, `GpuEncodeMetrics`, and `WriteTimings`, then flattened
for a public 99-key JSON shape. Do not describe or refactor them as one wholly
flat Rust struct.

Implementation:

1. Give each subgroup a small merge method that encodes sum, saturation,
   maximum/high-water, or derived behavior explicitly.
2. Make the top-level merge delegate instead of manually handling the 99 emitted
   fields in one 307-line method.
3. Isolate manual GPU serialization and report types by responsibility.
4. Compute derived values from authoritative fields instead of storing parallel
   mutable copies.
5. Verify serialized field count from the actual serialized object.

Required tests:

- Golden zero and fully populated JSON with all 99 names and types.
- Merge identity and associativity where mathematically valid.
- Saturation and max-versus-sum behavior for every field category.
- Backward deserialization if report types are consumed as input anywhere.

Acceptance criteria:

- All existing JSON field names and types remain stable unless separately
  approved as a compatibility change.
- Each merge policy is locally obvious and independently tested.
- `report.rs` is split by its actual responsibilities: report schemas, metric
  recording, custom serialization, and merge policy. CLI rendering already
  belongs to `cli_report.rs` and must remain separate.

### [ ] ARCH-007 — decompose encoder by backend and common policy

Priority: P2 maintainability
Dependencies: ARCH-000, ARCH-001
Primary files: `src/encode.rs`; focused encode modules

Suggested boundaries: facade/result types, shared device-selection policy, CPU,
Metal, CUDA, and option construction.

Acceptance criteria:

- CPU codestream equivalence tests pass.
- Device-used, fallback, and timing metrics remain compatible.
- Feature-gated types do not leak into no-feature builds.
- Linux CUDA and macOS Metal jobs compile/test every extracted backend module.

### [ ] ARCH-008 — separate CLI/GUI launch, state, commands, and presentation

Priority: P2 maintainability
Dependencies: GUI-001, FRONT-001, ARCH-000
Primary files: `src/main.rs`, `src/cli_report.rs`,
`apps/wsi-dicom-gui/src/main.rs`, GUI modules

Implementation:

1. Keep CLI `main.rs` to parse, dispatch, render top-level error, and choose exit
   status. Put argument definitions, command handlers, and report formatting in
   owned modules.
2. Keep GUI `main.rs` to native boot. Separate app rendering, pure model/state,
   worker execution, and reusable widgets.
3. Move the duplicated exact-duration formatter only after choosing one shared
   semantic home; do not create a generic miscellaneous helper bucket.
4. Keep core parsing/export behavior in the library and frontend presentation
   in each frontend.

Acceptance criteria:

- Both `main.rs` files are small launchers.
- Critical CLI command behavior is covered through the shipped binary.
- GUI state transitions run as headless unit tests.
- No production `console.log`-equivalent debug output or ignored channel error
  remains.

### [ ] ARCH-009 — clean up Metal route ownership without false unification

Priority: P2 maintainability
Dependencies: ARCH-001, ARCH-002, ARCH-003; supported Metal hardware
Primary files: `src/export/metal_input.rs`,
`src/export/metal_row_batch/aligned.rs`,
`src/export/metal_row_batch/whole_level.rs`, Metal compose/route modules

Implementation:

1. Split session state, automatic-route probing, input packing, batch execution,
   and metric recording by responsibility.
2. Break `pack_tiles` into layout validation, allocation planning, and bounded
   copy/packing stages.
3. Share only genuinely identical pipeline state and merge logic. Keep aligned
   and whole-level layout request generation distinct where their invariants
   differ.
4. Replace sixteen-argument functions with validated route/batch contexts.

Acceptance criteria:

- Layout tests cover edges, empty/partial tiles, overflow, partial batches,
  cancellation, device failure, and fallback.
- Metal route metrics and codestreams match characterization baselines.
- No abstraction exists solely to make visually similar but semantically
  different layouts share code.

## 15. Phase 9 — close test, CI, coverage, and fixture blind spots

### [ ] QA-001 — replace brittle repository-integrity checks with semantic gates

Priority: P1 assurance
Dependencies: BUILD-001, BUILD-002
Primary files: `tests/repo_integrity.rs`, `xtask/src/main.rs`, CI workflows

Preserve the intent of every passing protection while changing its mechanism:

| Current assertion style                    | Replacement evidence                                                                   |
| ------------------------------------------ | -------------------------------------------------------------------------------------- |
| Exact dependency strings and sibling paths | Cargo metadata/tree plus deny duplicate/source policy                                  |
| Manifest/package substring checks          | `cargo package --list` and isolated package build                                      |
| Workflow source strings                    | `actionlint`, workflow security scan, and executable dry-run tests                     |
| Public API wording                         | Doctests, compile tests, rustdoc warnings, and semver checks                           |
| Root/docs Markdown prohibition             | Explicit documentation ownership and link/lint checks                                  |
| Source line-count budgets                  | Module responsibility review plus complexity/size reporting, not pass/fail text counts |
| Assistant/vendor-name scanning             | Secret/path/provenance scanners focused on actual harmful artifacts                    |
| Local user paths                           | Focused repository scan and package-content test                                       |

Implementation:

1. Migrate one assertion family at a time and demonstrate that an intentionally
   introduced defect still fails the replacement gate.
2. Do not delete a passing integrity test until equal or stronger behavior
   protection lands in the same change.
3. Permit maintained architecture/remediation documentation, including this
   file, without allowing uncontrolled generated scratch files into packages.
4. Split remaining semantic integration tests by domain so one 949-line file is
   not the policy system for the entire project.

Acceptance criteria:

- Version updates and harmless formatting no longer require rewriting source
  literals.
- Duplicate dependencies, package leaks, broken workflows, local home paths,
  and undocumented public breakage still fail automatically.
- This plan can be tracked without special-casing its contents as private
  implementation debris.

### [ ] QA-002 — establish a licensed, de-identified real-fixture tier

Priority: P1 correctness evidence
Dependencies: BUILD-002, VAL-004
Primary files: `src/export/tests/fixture_tests.rs`, fixture manifest/tooling,
CI workflows

Implementation:

1. Inventory all thirteen ignored real-fixture tests and their NDPI, Aperio,
   Metal, codec, and environment requirements.
2. Define a fixture manifest containing logical name, format/vendor, provenance,
   license, de-identification status, SHA-256, expected size, platform/features,
   and expected route characteristics.
3. Put tiny redistributable fixtures in ordinary CI where legally and
   technically reasonable. Store larger/restricted fixtures in an immutable,
   access-controlled artifact source; never commit patient data.
4. Add a preflight command that verifies every required variable, file,
   checksum, license tier, and tool before running the suite.
5. Run protected fixtures on scheduled and release jobs; fork contributions use
   synthetic/public fixtures without secrets.
6. Upload bounded diagnostics and validator summaries, not source clinical data.

Acceptance criteria:

- Scheduled/release fixture jobs fail when a required fixture is absent; they do
  not return green after an early skip.
- CI records fixture hashes, route expectations, platform, and tests executed.
- Every fixture has documented legal and de-identification status.
- Each currently ignored regression is either executed in a named tier or has a
  documented replacement and retirement decision.

### [ ] QA-003 — test critical CLI flows through the shipped binary

Priority: P1 delivery confidence
Dependencies: BUILD-001, applicable correctness items
Primary files: new behavior-focused integration tests using
`CARGO_BIN_EXE_wsi-dicom`

Cover at minimum:

- help, version, malformed arguments, and incompatible option combinations;
- convert using deterministic synthetic single- and multi-instance sources;
- metadata file, research-placeholder conflict, oversized/malformed metadata;
- JSON output parseability, one-record-per-line behavior, warnings, and errors;
- existing output, no-clobber, overwrite transaction, and recovery states;
- validate strict/non-strict behavior, missing tool, and malformed decoder output;
- doctor and self-test success/failure exit codes;
- profile/coverage/corpus partial failures and summaries.

Sequencing: land the minimal help/error/JSON/synthetic-convert subset as part of
ARCH-000 before ARCH-008 moves CLI code. Complete the rest of this item during
the quality phase; do not postpone all shipped-binary evidence until after the
frontend refactor.

Acceptance criteria:

- Tests assert stdout, stderr, exit code, output files, and cleanup separately.
- Hermetic CLI tests use fake/synthetic dependencies and need no system decoder.
- Real system-tool behavior remains in the dedicated conformance tier.

### [ ] QA-004 — add GUI model, worker, and smoke tests

Priority: P1
Dependencies: GUI-001, ARCH-008
Primary files: GUI model/worker tests and CI workflow

Implementation:

1. Test default state, form validation, readiness, request construction, status
   transitions, warnings, report save, repeated start, and cancellation policy.
2. Inject worker execution so success, export error, disconnect, and selected
   panic policy are deterministic.
3. Add a stable native launch smoke test under a virtual display only if it
   provides useful signal; pure state coverage is mandatory regardless.
4. Run `cargo test -p wsi-dicom-gui`, not only `cargo check`, in CI.

Acceptance criteria:

- The audited disconnect bug has a regression test.
- GUI state and worker behavior are testable without a physical display.
- Rendering code owns no metadata parsing or export orchestration.

### [ ] QA-005 — add changed-path and branch-aware coverage

Priority: P1 assurance
Dependencies: BUILD-002, QA-003, QA-004
Primary files: `xtask/src/main.rs`, `.github/workflows/ci.yml`, coverage config

Implementation:

1. Retain the aggregate 80% CPU/no-feature line threshold.
2. Produce LCOV against the contribution merge base and require at least 80%
   coverage for changed executable lines.
3. Report uncovered changed lines in a durable artifact or review annotation.
4. Add branch coverage where tooling is stable, especially for error and
   transaction state machines.
5. Include CLI and GUI logic packages; report platform-only Metal/CUDA coverage
   separately rather than hiding it in an aggregate.
6. Require an explicit plan note for a justified changed-path gap.

Acceptance criteria:

- A test change with uncovered new branches demonstrates that the gate fails.
- Generated and inaccessible platform code exclusions are explicit and reviewed.
- Security, validation, metadata, UID/path, writer, and transaction changes meet
  or document the changed-path target.

### [ ] QA-006 — seed and time-bound fuzzing meaningfully

Priority: P2 security and robustness
Dependencies: BUILD-001, CORE-001, META-002, VAL-003
Primary files: `fuzz/fuzz_targets/`, tracked bounded seed corpora, CI workflow

Implementation:

1. Add small valid-minimal, boundary, truncated, oversized, duplicate-field,
   Unicode/delimiter, and malformed seeds for all five existing targets.
2. Add focused targets for encapsulated offset/frame reconstruction, instance
   planning/path generation, and metadata string validation where a stable
   harness is available.
3. Use short time-based PR smoke runs instead of only 1,000 executions.
4. Run materially longer scheduled fuzz jobs and retain reproducible crash
   artifacts.
5. Minimize each fixed crash and commit it as a regression seed.
6. Bound corpus and individual input size so fuzz infrastructure cannot become
   an uncontrolled storage/compute sink.

Acceptance criteria:

- Every target starts from a nonempty relevant corpus.
- Crash artifacts can be replayed with documented commands.
- Scheduled runs cover enough time to move beyond startup/corpus loading.

### [ ] QA-007 — align the platform/feature matrix with advertised support

Priority: P1
Dependencies: BUILD-002
Primary files: `.github/workflows/ci.yml`, README support matrix

Required tiers:

- Linux stable: formatting, Clippy, core/CLI/GUI tests, coverage, CUDA compile.
- macOS stable: core tests plus Metal compilation and tests on supported hardware.
- Windows stable: core/CLI tests, GUI compile/tests, path and process supervision.
- Linux MSRV: no-default build/tests using declared Rust version.
- Dedicated Linux external-conformance image.
- Scheduled/release real-fixture jobs.
- Hardware runtime jobs clearly distinct from feature compile-only jobs.

Acceptance criteria:

- Every advertised OS/backend has at least a compile gate on the appropriate
  platform and behavior tests where hardware is available.
- Windows path, transaction, and process-tree behavior actually runs.
- CI job names state whether they compile, unit-test, conformance-test, or run on
  hardware.
- The duplicate Metal feature job is gone without reducing coverage.

### [ ] QA-008 — protect performance and memory while correctness is hardened

Priority: P2
Dependencies: BASE-001, CORE-002, TX-001, architecture items
Primary files: benchmarks, `bench/`, CI scheduled/release jobs

Implementation:

1. Define representative small, medium, and large synthetic/approved fixtures.
2. Record CPU throughput, peak RSS, temp-disk peak, and output size for key JPEG
   and J2K routes; record GPU metrics on supported hardware separately.
3. Add broad regression thresholds that catch material degradation without
   making ordinary CI flaky.
4. Measure the cost of content-derived UID hashing, transaction staging,
   validation, and streaming per-frame metadata.
5. Keep validation correctness failures fatal in benchmark tooling.

Acceptance criteria:

- Resource-limit defaults come from measured data rather than guesswork.
- Any material performance/memory regression is documented and explicitly
  accepted for a quantified correctness/security benefit.
- Benchmark outputs never include patient data or unbounded local paths.

### [ ] QA-009 — audit lints, dead code, duplication, and architecture after moves

Priority: P2 final maintainability gate
Dependencies: all ARCH items
Primary files: workspace-wide

Implementation:

1. Re-run exact and near-duplicate scanning on production and tests separately.
2. Review every `allow(dead_code)`, `allow(clippy::too_many_arguments)`, and broad
   lint allowance. Remove obsolete suppressions; justify true feature/ABI cases.
3. Run unused-dependency and dead-code analysis with every feature combination.
4. Generate updated file/function size and complexity reports for comparison to
   the baseline, without turning arbitrary numeric thresholds into source tests.
5. Check for newly introduced generic helper buckets or cyclic module ownership.

Acceptance criteria:

- Risky profile/export and frontend duplication is removed.
- No silent or blanket lint suppression remains.
- Every remaining duplicate has an intentional semantic reason recorded in code
  structure or this plan.

## 16. Phase 10 — remove dead baggage and reconcile documentation

### [ ] CLEAN-001 — remove the unused vendor tree safely

Priority: P2 repository and supply-chain hygiene
Dependencies: BUILD-001, DEP-001
Primary files: `vendor/`, manifests, package configuration, tests, README/changelog

Implementation:

1. Prove with Cargo metadata/tree, scripts, workflows, package list, and text
   search that no active path consumes `vendor/metal-0.33-patches`.
2. Confirm the supported Metal path and its provenance before deletion.
3. Use `trash vendor` during implementation; preserve history in Git rather than
   retaining dead third-party code in the active tree.
4. Remove contradictory README/test claims and any now-useless package exclusion.
5. Re-run license, secret, dependency, package, and unused-dependency scans.

Acceptance criteria:

- Clean-clone CPU and supported Metal builds/tests pass without `vendor/`.
- No manifest, script, workflow, package, or document references the removed
  tree as active code.
- Supply-chain and unused-dependency tools no longer analyze nested dead
  manifests or emit misleading success after errors.

### [ ] CLEAN-002 — reconcile features, dependencies, release narrative, and docs

Priority: P2
Dependencies: BUILD-003, all user-visible correctness decisions
Primary files: `src/lib.rs`, `README.md`, `.github/CHANGELOG.md`,
`.github/CONTRIBUTING.md`, `.github/SECURITY.md`, API/CLI docs

Implementation:

1. Remove or correct the nonexistent aggregate `gpu` feature in `src/lib.rs`.
2. Generate/test the README feature and platform matrix against Cargo metadata.
3. Document filename v2/compatibility, UID modes and migration, transaction and
   recovery guarantees, resource limits, Unicode policy, validation decoder
   output contract, warnings, and conformance/fixture tiers.
4. Reconcile changelog claims about published dependencies with the final
   registry topology and advisory remediation.
5. Document which test tiers are hermetic, require external tools, require
   protected fixtures, or require hardware.
6. Keep examples executable and free of real clinical identifiers.

Acceptance criteria:

- No documented feature, platform, command, or validation guarantee lacks a
  matching executable check.
- Compatibility changes have migration notes appropriate for a pre-1.0 release.
- Documentation links and examples pass their gates.

### [ ] CLEAN-003 — consolidate remaining small duplicated utilities

Priority: P3
Dependencies: FRONT-001, ARCH-008
Primary files: `src/main.rs`, GUI main/modules, `src/report.rs`, bounded directory
walkers in export/validation

Implementation:

1. Move the exact-duration helper to the report/presentation layer that owns its
   semantics.
2. Consolidate genuinely identical metadata I/O through FRONT-001.
3. Compare bounded symlink-safe directory walkers carefully. Share a policy
   engine only if their entry/depth/error semantics are intentionally the same;
   otherwise retain separate typed policies rather than forcing a generic copy.
4. Re-run clone detection and document intentionally retained domain-specific
   similarities.

Acceptance criteria:

- Exact helper copies are gone.
- Consolidation does not broaden traversal, symlink, or byte-limit policy.
- No miscellaneous `utils` module becomes an ownership dumping ground.

## 17. Phase 11 — restore a protected, evidence-bound release path

### [ ] REL-001 — implement protected tag-only publication

Priority: P1 release blocker
Dependencies: SEC-000, SEC-001, BUILD-003, DEP-001, DEP-002, DEP-004, QA-007
Primary files/surfaces: `.github/workflows/publish.yml`,
`scripts/publish-crate.sh`, `.github/CODEOWNERS`, GitHub branch/tag/environment
rules, and crates.io trusted-publisher configuration

Implementation:

1. Manual dispatch performs verification and `cargo publish --dry-run --locked`
   only, without any publication credential.
2. Real publication begins only from a strictly parsed `v<semver>` tag whose
   version equals Cargo metadata and changelog release heading.
3. Require the tag commit to belong to protected `main` and require the exact
   immutable SHA—not merely a branch name—to have passed all release checks.
4. Put the publish job behind a protected `crates-io` environment with required
   reviewer approval and prevention of self-review where available.
5. Register the exact owner/repository/workflow/environment tuple as a crates.io
   trusted publisher, but keep Trusted Publishing Only enforcement disabled so
   API-token publication remains available. Do not enable registry enforcement
   without explicit owner authorization.
6. Keep `id-token: write` confined to the protected publish job and keep the
   exchanged short-lived crates.io token unavailable to checkout, verification,
   package, dry-run, manual-dispatch, and registry-query steps/jobs. GitHub OIDC
   permission is job-scoped rather than step-scoped, so minimize and
   commit-pin every action in that protected job. Expose the exchanged crates.io
   token only to the exact `cargo publish` command.
7. Query public registry state for an already-published version before requesting
   OIDC authentication.
8. Add static publication concurrency with `cancel-in-progress: false` so two
   release tags cannot publish simultaneously and an active publish cannot be
   cancelled midway by a newer tag.
9. Use least privileges, disable persisted checkout credentials, restrict `v*`
   tag creation to release maintainers, require pull requests for main, dismiss
   stale approvals, and avoid broad bypass.
10. Add CODEOWNER review for workflows, release scripts, Cargo lock/manifest,
    `deny.toml`, and `supply-chain/**`, naming a real maintainer/team before branch
    protection is enabled.
11. Derive version data through Cargo metadata, use locked dry-run plus a
    no-verify authenticated upload of the exact SHA, and never blindly retry an
    ambiguous publish failure.

Acceptance criteria:

- Publication is impossible from manual dispatch, pull request, arbitrary
  branch, or unreviewed/mismatched tag.
- The OIDC permission and temporary token are unavailable until exact-SHA checks
  and environment approval complete.
- Workflow/release-script changes require designated review.
- A publish timeout cannot trigger a blind duplicate upload attempt.
- Existing-version handling is idempotent and explicit.
- Concurrent release tags serialize without cancelling an active publication.

### [ ] REL-002 — rehearse release and verify external consumers

Priority: P1 release blocker
Dependencies: REL-001 and every P0/P1 item
Primary files/surfaces: release workflow, package archive, clean external
consumer projects, release evidence record

Implementation:

1. Run the complete release workflow with no secret and prove the publish step
   remains unreachable.
2. Inspect package contents and the normalized packaged manifest.
3. Build clean registry-only consumers for CPU default, Metal on macOS, and CUDA
   on supported systems.
4. Re-query the actual latest published `wsi-dicom` version before semver
   comparison; the audit observed `0.2.0`, but registry state is time-sensitive.
5. Confirm the required `wsi-rs` and `j2k` versions are indexed and downloadable.
6. Confirm every required check attaches to the exact candidate tag SHA.
7. Configure and independently verify environment/tag rules and crates.io trusted
   publisher claims before allowing OIDC authentication.
8. Perform one final credential-free package and publish dry run.
9. Restore tag-triggered publication only after the evidence is reviewed. Actual
   publication still requires explicit release authority.

Acceptance criteria:

- All release checks are green on the exact candidate SHA.
- Registry-only consumers pass every advertised feature on supported systems.
- No current security, correctness, package, or clean-clone blocker remains.
- Release evidence records tag, SHA, workflow run, package checksum, toolchain,
  dependency versions, validators, fixtures, and approvals.

Rollback policy:

1. Disable the protected environment or remove its token.
2. Preserve logs and registry evidence.
3. Yank a defective version when appropriate; crates.io versions cannot be
   overwritten.
4. Fix forward with a new version. Never force-move a public tag or rewrite
   published history.
5. Notify users when security, malformed output, metadata, or identity semantics
   could affect existing data.

## 18. Complete verification matrix

Run narrow checks while implementing. Run this matrix from a clean standalone
clone before closing REL-002. Commands requiring an OS, hardware backend,
credential-free external tool, or protected fixture must run in their named CI
tier rather than being silently omitted.

### 18.1 Repository and toolchain state

```sh
git status --short --branch
rustc --version --verbose
cargo --version --verbose
cargo metadata --locked --all-features --format-version 1 >/dev/null
cargo fetch --locked
```

Expected: clean tree for final release verification, declared MSRV/current
stable evidence captured, and no sibling/local path in resolved publishable
dependencies.

### 18.2 Formatting, compile, lint, tests, and documentation

```sh
cargo fmt --all -- --check
cargo check --workspace --no-default-features --all-targets --locked
cargo clippy --workspace --no-default-features --all-targets --locked -- -D warnings
cargo test --workspace --no-default-features --all-targets --locked
cargo test --workspace --all-targets --release --locked
cargo doc --workspace --no-default-features --no-deps --locked
cargo rustdoc --lib --no-default-features --locked -- -D missing_docs
./.venv/bin/python -m unittest discover -s tests -p 'test_*.py'
```

Also run the declared MSRV lane separately. Do not let a newer lockfile package
silently raise MSRV without an explicit compatibility decision.

### 18.3 Feature and platform matrix

```sh
cargo check --workspace --features cuda --all-targets --locked
cargo check --workspace --features metal --all-targets --locked
cargo test -p wsi-dicom-gui --locked
```

- CUDA executes on supported Linux/Windows hardware in its runtime tier.
- Metal compiles and executes on supported macOS hardware.
- Linux, macOS, and Windows run core/CLI tests.
- Windows runs process-tree, path, and transaction tests.
- Compile-only results are labeled as such and never substituted for hardware
  execution evidence.

### 18.4 Security and dependency policy

```sh
cargo audit --file Cargo.lock --deny unsound
cargo deny check advisories bans licenses sources
cargo vet --locked
cargo vet fmt
git diff --exit-code -- supply-chain/
cargo machete
actionlint .github/workflows/*.yml
zizmor --pedantic .github/workflows/
```

Expected: zero unaccepted vulnerability/unsoundness findings, no unexplained
deny exceptions, substantive vet coverage, no manifest-analysis errors hidden
behind a successful status, and no untrusted workflow expression in shell
source.

### 18.5 Package and clean-clone proof

```sh
cargo package --locked
cargo package --list
cargo publish --dry-run --locked
```

Then extract `git archive HEAD` into a new temporary directory and repeat Cargo
metadata, package, package test, audit, and relevant feature checks without
sibling repositories or local override configuration. Inspect the package list
for `vendor/`, fixtures, benchmark output, credentials, local paths, and process
documents that do not belong in the crate.

### 18.6 DICOM, codec, and external conformance

Required evidence:

- `dciodvfy` and `dcentvfy` run against representative generated object sets.
- Grok/OpenJPEG/JPEG reference decoders run and their exact versions are logged.
- Pixel decoder output is parsed and dimension-checked, not only exit-checked.
- JPEG passthrough, retile, lossless J2K, direct HTJ2K, CPU fallback, and
  supported device routes are covered.
- One-fragment, multi-fragment, Basic Offset Table, and Extended Offset Table
  fixtures are covered.
- Multi-scene and multi-series exports are covered in serial and parallel modes.
- Unicode, delimiter rejection, numeric boundary, resource limit, UID, path,
  overwrite, failure injection, and crash recovery tests run.
- Missing any required tool or protected release fixture fails its tier before
  the advertised tests begin.

### 18.7 Coverage, fuzzing, and performance

```sh
cargo xtask coverage
cargo xtask semver
cargo xtask docs-strict
cargo xtask release-test
```

Required evidence:

- Aggregate line coverage remains at least 80%.
- Changed executable paths meet at least 80% or have an approved documented gap.
- Branch/error-state coverage is reported for transaction, process supervision,
  GUI state, and validation.
- Every fuzz target has a seed corpus and completes PR plus scheduled time-based
  runs without crash, leak, hang, or unbounded growth.
- CPU throughput, peak RSS, temp-disk peak, and output-size comparisons show no
  unexplained material regression.

### 18.8 Final static re-audit

Re-run and record:

- first-party and vendor/package line counts;
- largest production files and functions;
- parameter counts and all lint suppressions;
- production/test clone scans separately;
- unused dependencies and feature-specific dead code;
- secrets, local paths, generated artifacts, and package contents;
- dependency graph and advisory provenance;
- documentation links, advertised features, platforms, and commands.

The goal is not zero duplication or arbitrary file-size compliance. The goal is
to prove that risky semantic duplication, unclear ownership, silent checks, and
the audited defects are gone without weakening existing defenses.

## 19. Audit finding closure ledger

This ledger is the completeness check. A work item cannot be removed without
moving its finding to another explicit item or recording an approved waiver in
the decision log.

| Finding                                                                           |            Severity | Closure item(s)                        | Closure evidence                                                                            |
| --------------------------------------------------------------------------------- | ------------------: | -------------------------------------- | ------------------------------------------------------------------------------------------- |
| F-001 Workflow input becomes shell source in credentialed release flow            |                  P0 | SEC-000, SEC-001                       | Malicious-input regression; workflow lint/security scan; protected dry-run proof            |
| F-002 Crates.io token is present during dry-run/non-publish work                  |                  P0 | SEC-000, SEC-001, REL-001              | No long-lived secret; exchanged crates.io token is mapped only to the approved publish step |
| F-003 Manifest pins local `j2k =0.6.2` to a 0.7 path                              |          P1 blocker | BUILD-001                              | Standalone locked metadata/check succeeds                                                   |
| F-004 CI checks out only this repo but requires sibling repos                     |          P1 blocker | BUILD-001, BUILD-002                   | Clean-clone full CI and package proof                                                       |
| F-005 `wsi-rs 0.5.0` is unpublished, blocking package/release                     |          P1 blocker | WSI-001, BUILD-001, BUILD-003          | Patched registry version, index, and clean package evidence                                 |
| F-006 Five RustSec matches plus `memmap2` unsoundness warning                     |                  P1 | WSI-001, DEP-001                       | Zero unaccepted audit findings with unsound warnings denied                                 |
| F-007 Metal registry graph may rely on non-inherited local patches                |                  P1 | DEP-004                                | External registry-only Metal consumer passes                                                |
| F-008 cargo-vet has 504 exemptions and no audits/imports                          |        P1 assurance | DEP-002                                | Trusted imports/local audits and justified minimal exemptions                               |
| F-009 No dependency-update automation or Python advisory gate                     |                  P2 | DEP-003                                | Scheduled updates and Python scan run                                                       |
| F-010 Multi-scene/multi-series paths collide                                      |                  P1 | IDENT-001                              | Serial/parallel multi-axis export tests                                                     |
| F-011 Source-path UID seeds are unstable and can reuse semantic identity          |                  P1 | UID-001                                | Random/content-derived identity matrix and DICOM hierarchy tests                            |
| F-012 Export leaves partial/mixed final output after later failure                |                  P1 | TX-001, TX-002                         | Failure injection plus crash recovery at every commit state                                 |
| F-013 Route-cache update can be lost during concurrent flush                      |                  P1 | CACHE-001                              | Barrier-controlled race regression                                                          |
| F-014 Route-cache write is non-atomic                                             |                  P1 | CACHE-001                              | Faulted write leaves prior complete cache                                                   |
| F-015 Cache flush can report failure after output succeeded                       |                  P1 | CACHE-001, GUI-001                     | Structured warning with committed export success                                            |
| F-016 External decoder can pass without producing output                          |                  P1 | VAL-001                                | Zero-exit/no-output and malformed raster regressions                                        |
| F-017 Timeout kills only child and can hang joining inherited pipes               |                  P1 | VAL-002                                | Grandchild/pipe/process-tree tests on supported OSes                                        |
| F-018 Validator assumes one fragment equals one frame                             |                  P1 | VAL-003                                | BOT/EOT/multi-fragment conformance and fuzz tests                                           |
| F-019 ICC probing can divide by zero                                              |                  P1 | CORE-001                               | Zero-dimension tests fail before ICC/tile read                                              |
| F-020 Clinical/spatial numeric metadata accepts invalid floats                    |                  P1 | META-001                               | NaN/infinity/sign/range/DS tests                                                            |
| F-021 DICOM scalar strings permit delimiter injection                             |                  P1 | META-002                               | Per-field backslash/control tests                                                           |
| F-022 Unicode text is emitted without Specific Character Set                      |                  P1 | META-002                               | Unicode round-trip and independent validator acceptance                                     |
| F-023 Source geometry can drive enormous infallible allocations                   |                  P1 | CORE-001, CORE-002                     | Budget, overflow, fallible-allocation, peak-RSS tests                                       |
| F-024 DICOM representability is checked after expensive encoding                  |                  P1 | CORE-001                               | Instrumented preflight-before-encoder tests                                                 |
| F-025 GUI disconnect can leave UI permanently running                             |                  P1 | GUI-001, QA-004                        | State-machine disconnect regression                                                         |
| F-026 `export.rs` is a 3,395-line orchestration hotspot                           |             P1 debt | ARCH-000, ARCH-001, ARCH-002           | Focused facade/modules with behavior parity                                                 |
| F-027 `writer.rs`, `validation.rs`, `encode.rs`, and frontend mains are oversized |          P1/P2 debt | ARCH-004, ARCH-005, ARCH-007, ARCH-008 | Ownership splits and characterization gates                                                 |
| F-028 Long functions and 12–16 argument routes                                    |          P1/P2 debt | ARCH-001, ARCH-002, ARCH-004, ARCH-009 | Validated contexts; allowance re-audit                                                      |
| F-029 Metrics aggregation/serialization is manually concentrated                  |             P2 debt | ARCH-006                               | 99-key golden JSON and subgroup merge tests                                                 |
| F-030 Profile and export duplicate codec workflows                                | P1 correctness debt | ARCH-003                               | Route parity and clone removal                                                              |
| F-031 CLI/GUI duplicate metadata loading and capped reads                         |             P2 debt | FRONT-001                              | One bounded parser with adapter tests                                                       |
| F-032 Duration and bounded-walker similarities can drift                          |             P3 debt | CLEAN-003                              | Shared semantics or documented typed separation                                             |
| F-033 Defaults/export dependency direction and parent wildcard imports            |             P2 debt | ARCH-001                               | Acyclic direction and explicit imports                                                      |
| F-034 Unused vendor tree contains 214 files/~118k Rust lines                      |                  P2 | CLEAN-001                              | No reference/package inclusion; tree removed with tests green                               |
| F-035 `src/lib.rs` advertises nonexistent `gpu` feature                           |                  P2 | CLEAN-002                              | Docs generated/checked against metadata                                                     |
| F-036 External codec/DICOM tests silently pass when tools are absent              |        P1 assurance | VAL-004                                | Tool removal deliberately fails conformance job                                             |
| F-037 Thirteen real-fixture tests are ignored without CI tier                     |        P1 assurance | QA-002                                 | Manifested scheduled/release fixture execution                                              |
| F-038 GUI has no tests and CI only compiles it                                    |        P1 assurance | QA-004                                 | GUI test job and state coverage                                                             |
| F-039 Critical CLI flows lack shipped-binary E2E tests                            |        P1 assurance | QA-003                                 | Exit/stdout/stderr/filesystem E2E matrix                                                    |
| F-040 Coverage is aggregate CPU lines only                                        |        P1 assurance | QA-005                                 | Changed-path/branch/platform reports                                                        |
| F-041 Integrity tests assert exact strings and ban documentation                  |             P2 debt | QA-001                                 | Semantic replacement gates with mutation demonstrations                                     |
| F-042 Fuzzing uses 1,000 runs and no seed corpora                                 |        P2 assurance | QA-006                                 | Nonempty seeds and time-based PR/nightly runs                                               |
| F-043 Windows is advertised without Windows CI                                    |        P1 assurance | QA-007                                 | Windows build/test/path/process evidence                                                    |
| F-044 CUDA/Metal compile evidence is confused with runtime evidence               |        P1 assurance | QA-007, DEP-004                        | Explicit compile and hardware runtime tiers                                                 |
| F-045 CI omits `--locked` and duplicates Metal compilation                        |                  P2 | BUILD-002, QA-007                      | Locked command audit; single authoritative Metal compile job                                |
| F-046 Dependency analyzer can error then report success                           |                  P2 | BUILD-002                              | Deliberate malformed-manifest failure test                                                  |
| F-047 Changelog/release story conflicts with path dependencies                    |                  P2 | BUILD-003, CLEAN-002                   | Registry topology and changelog agree                                                       |
| F-048 `vendor/` may enter package because it is not excluded                      |          P1 release | BUILD-003, CLEAN-001                   | Package list excludes it before and after removal                                           |
| F-049 Performance/memory impact of hardening is unmeasured                        |                  P2 | BASE-001, QA-008                       | Baseline and post-change benchmark evidence                                                 |
| F-050 Lint suppressions/dead code may outlive refactors                           |                  P2 | QA-009                                 | Feature-wide allowance/dead-code report                                                     |

Audit observations that do not justify a cleanup campaign:

- Production duplication was only 2.01%; target the risky clone groups rather
  than pursuing an artificial zero.
- No meaningful TODO/FIXME/HACK accumulation was found.
- Unsafe Rust is forbidden and important filesystem defenses already exist.
- These are regression guards in sections 3.3 and 18, not reasons to skip the
  open findings.

## 20. Recommended change-set sequence

Keep each change reviewable. Do not mix pure moves, security behavior,
dependency topology, DICOM semantics, and large refactors in one change.

1. **S0 — external containment:** SEC-000. No code/dependency changes.
2. **S1 — release workflow hardening:** SEC-001 and its semantic tests. No Cargo
   repair in this change.
3. **W1 — `wsi-rs` security/release prerequisite:** WSI-001 quick-xml upgrade,
   hostile XML tests, package readiness; publish only with separate authority.
4. **D1 — registry dependency topology:** BUILD-001 and lockfile regeneration.
5. **D2 — remaining advisory closure:** DEP-001 and DEP-004 compatibility work.
6. **C1 — clean-clone delivery gates:** BUILD-002, BUILD-003, BASE-001 rerun.
7. **Q0 — semantic integrity replacement needed by current work:** the narrow
   portions of QA-001 that otherwise lock in unsafe workflow/dependency strings.
8. **K1 — early invariant fixes:** CORE-001, META-001, META-002, and focused
   resource guardrails with regression tests.
9. **I1 — instance identity:** IDENT-001 and UID-001 after decisions are final.
10. **V1 — validation evidence:** VAL-003, VAL-001, VAL-002, VAL-004 as separate
    focused changes when practical.
11. **T1 — output state:** TX-001/TX-002, followed by CACHE-001. Transaction
    failpoints and recovery are mandatory before broad export movement.
12. **G1 — frontend lifecycle:** GUI-001 and FRONT-001 with headless tests.
13. **A0 — characterization:** ARCH-000, including the minimal pre-refactor
    QA-003 shipped-binary baseline.
14. **A1 through A9 — architecture:** follow dependency order in section 14;
    pure file moves precede focused rewrites, and each retains green behavior.
15. **Q1 — complete quality tiers:** QA-002 through QA-009 and remaining QA-001.
16. **SC1 — sustainable assurance:** DEP-002 and DEP-003, including ownership.
17. **CL1 — repository/docs cleanup:** CLEAN-001 through CLEAN-003.
18. **R1 — release protection/rehearsal:** REL-001, REL-002, full section 18.

Parallel work is safe only where file ownership and prerequisites do not
overlap. Examples: DEP-002 policy research can proceed while correctness tests
are written; fixture licensing can proceed while core code changes; Windows CI
design can proceed while process-runner behavior is specified. Do not parallelize
multiple rewrites of `export.rs`, `writer.rs`, `validation.rs`, or `report.rs`.

## 21. Risk register

| Risk                                                      |                                 Impact | Trigger/indicator                                                      | Required control                                                                |
| --------------------------------------------------------- | -------------------------------------: | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Cargo is repaired before release containment              |         Critical supply-chain exposure | Publish workflow becomes executable while input injection/token remain | SEC-000/SEC-001 hard gate; separate change sets                                 |
| Registry 0.6 codecs differ from local 0.7 behavior        | High malformed output/performance risk | Route/codestream/feature parity changes                                | CPU/Metal/CUDA parity and external consumer tests                               |
| quick-xml upgrade changes parsing semantics               |          High input compatibility risk | Representative WSI metadata differs or fallback activates              | Hostile plus real representative XML tests in `wsi-rs`                          |
| UID policy causes PACS duplication/replacement surprises  |         High clinical integration risk | Re-exported objects group differently                                  | Final DEC-003, hierarchy tests, migration note, explicit modes                  |
| Filename v2 breaks downstream scripts                     |              Medium/high workflow risk | Consumers assume old names                                             | Final DEC-002, pre-1.0 migration, manifest/report-based consumption             |
| Flat multi-file output is described as atomically visible |      High data-integrity misconception | Readers observe mid-commit state                                       | Promise failure atomicity/recovery only or adopt versioned-directory design     |
| Transaction staging exceeds disk capacity                 |                 High availability risk | Old + new + staging peak fills volume                                  | Preflight disk budget, streamed staging, explicit limit/error                   |
| Unicode policy exposes old consumer defects               |           Medium interoperability risk | Validator/consumer rejects ISO_IR 192                                  | Independent validators, representative systems, explicit fallback policy        |
| Fragment parser accepts ambiguous/malformed offsets       |           High validation false result | Conflicting BOT/EOT or unusual padding                                 | Streaming bounded parser, fuzzing, independent fixtures                         |
| Process supervision differs by OS                         |                  High hang/orphan risk | Windows/Unix timeout tests diverge                                     | Job object/process group abstraction and platform CI                            |
| Refactor overlaps active correctness work                 |            High regression/review risk | Large conflicts or behavior hidden in moves                            | Correctness first, characterization, separate pure-move commits                 |
| JSON metrics shape changes during cleanup                 |                 High integration break | Missing/renamed/type-changed keys                                      | 99-key golden schema and semver review                                          |
| DICOM element order/bytes change during writer split      |         Medium/high compatibility risk | Golden/external validators differ                                      | Canonical structure/byte tests and intentional-diff review                      |
| GPU code is judged from Linux CPU-only evidence           |                High feature regression | Feature compiles but fails on hardware                                 | Named hardware tiers and registry-only consumers                                |
| Fixture includes restricted or clinical data              |            Critical privacy/legal risk | Unclear provenance/license/de-identification                           | Manifest, legal review, immutable protected store, no source uploads            |
| cargo-vet exemptions are relabeled rather than audited    |                   High false assurance | Exemption count moves without review evidence                          | Trusted imports/local audits, owner review, ratchet                             |
| Stronger CI becomes prohibitively slow/flaky              |                   Medium delivery risk | Long PR queue/intermittent failures                                    | Separate PR, scheduled, release, hardware, fixture tiers                        |
| Hardening causes major throughput/RSS regression          |           Medium/high operational risk | Benchmarks exceed agreed threshold                                     | BASE-001 and QA-008 measured trade-off review                                   |
| Over-generalized helpers weaken domain policy             |                Medium correctness risk | Shared walker/route gains many flags                                   | Typed policy boundaries; retain separate implementations where semantics differ |

## 22. Decision and execution logs

### 22.1 Decision log

Change `Proposed` to `Accepted`, `Rejected`, or `Superseded` only after the
decision is made. Add the date, decision-maker, rationale, and affected work
items. Never rewrite old rationale; append a superseding row.

| Decision                             | Status   | Recommended direction                                                                                  | Final choice/date/rationale                                                                                                                                                   |
| ------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| DEC-001 dependency topology          | Accepted | Registry-first release graph; local overrides outside publishable manifest                             | 2026-07-09, owner directed `wsi-dicom` to use actual published crates; registry `j2k 0.6.2` is complete and `wsi-rs 0.5.0` must be hardened/published before final conversion |
| DEC-002 output filenames             | Proposed | Always-explicit v2 scene/series/level/Z/C/T names                                                      | —                                                                                                                                                                             |
| DEC-003 UID semantics                | Proposed | Random per export by default; versioned content-derived opt-in                                         | —                                                                                                                                                                             |
| DEC-004 transaction semantics        | Proposed | Failure-atomic ordinary errors plus journaled crash recovery; no false simultaneous-visibility promise | —                                                                                                                                                                             |
| DEC-005 text repertoire              | Proposed | Unicode with conditional `ISO_IR 192`; reject scalar delimiters/controls                               | —                                                                                                                                                                             |
| DEC-006 resource policy              | Proposed | Measured conservative defaults with explicit override and checked arithmetic                           | —                                                                                                                                                                             |
| DEC-007 GUI panic policy             | Proposed | Decide unwind/recover versus documented process abort; never claim unsupported recovery                | —                                                                                                                                                                             |
| DEC-008 report warning compatibility | Proposed | Additive structured warnings while preserving existing 99 metric fields/types                          | —                                                                                                                                                                             |
| DEC-009 Metal release claim          | Proposed | Block advertised registry release if clean external consumer cannot enable Metal                       | —                                                                                                                                                                             |
| DEC-010 registry publish policy      | Accepted | Keep the OIDC trusted publisher registered without requiring it for every registry publication          | 2026-07-09, owner directed that API-token publishing remain available; trusted-only enforcement was disabled and must not be re-enabled without explicit owner authorization   |

### 22.2 Execution log

Append one row per meaningful session or change set. Link commits/PRs when they
exist. Results must say what was not run as well as what passed.

| Date       | Work item/change                 | Result                                  | Verification                                                                                                                                                                                                                | Next action                                                      |
| ---------- | -------------------------------- | --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| 2026-07-09 | Repository-wide audit            | Complete, read-only                     | Formatting passed; 17 Python tests passed; Cargo gates blocked; audit found dependency advisories                                                                                                                           | Create remediation plan                                          |
| 2026-07-09 | Master remediation plan          | Complete; no production fix attempted   | Finding ledger, dependencies, acceptance criteria, and verification matrix reviewed                                                                                                                                         | SEC-000 with repository-owner authority, then SEC-001            |
| 2026-07-09 | SEC-000 containment              | Complete                                | Publish workflow disabled; repository token secret deleted; run and registry history preserved                                                                                                                              | Complete local SEC-001 hardening                                 |
| 2026-07-09 | SEC-001 local hardening          | Complete locally; operationally blocked | 34 Python tests, Pyright, ShellCheck, actionlint, zizmor, typos, formatting, and shell syntax passed; Rust test blocked by known dependency mismatch                                                                        | Add independent reviewer                                         |
| 2026-07-09 | Credential disclosure response   | Contained; credential not used          | Long-lived crates.io credential treated as compromised; owner attests it was revoked; workflow remains disabled and repository secret remains absent                                                                        | Add independent reviewer                                         |
| 2026-07-09 | crates.io Trusted Publishing     | Configured and enforced                 | Registered `frames-sg/wsi-dicom`, `publish.yml`, environment `crates-io`; enabled required trusted publishing for all new versions                                                                                          | Add independent reviewer                                         |
| 2026-07-09 | Trusted-publishing enforcement   | Disabled at owner direction             | Verified the `Require trusted publishing for all new versions` checkbox is off; the existing trusted publisher registration remains intact and API-token publishing is allowed                                             | Do not re-enable without explicit owner authorization             |
| 2026-07-09 | BUILD-001 registry j2k migration | Partial; blocked on `wsi-rs` release    | Root and fuzz graphs now use crates.io `j2k 0.6.2`; 278 Rust tests, Clippy, default/Metal/CUDA/fuzz checks, metadata, formatting, Python, Pyright, and deny policy passed; package fails only on unpublished `wsi-rs 0.5.0` | Obtain authority for WSI-001 and publish hardened `wsi-rs 0.5.0` |

### 22.3 Blocker log

| Date       | Work item | Blocker                                                    | Attempts/evidence                                                                                                                                                                         | Authority or state needed                                       |
| ---------- | --------- | ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| 2026-07-09 | SEC-000   | Secret/environment/repository settings were external state | Resolved: workflow disabled, repository secret deleted, environment created                                                                                                               | Resolved 2026-07-09                                             |
| 2026-07-09 | SEC-001   | Independent approval needs a second reviewer               | Owner attests that the disclosed token was revoked; Trusted Publishing is configured but optional by owner choice; only `jcwal1516` is currently available and self-review is prevented   | A second repository/org reviewer                                |
| 2026-07-09 | BUILD-001 | Compatible `wsi-rs 0.5.0` is unpublished                   | Registry `j2k 0.6.2` migration and local checks pass. Registry `wsi-rs 0.4.0` requires yanked `signinum` packages and a mixed `j2k 0.5/0.6` graph; package proof fails on missing `0.5.0` | Separate authority to harden and publish sibling `wsi-rs 0.5.0` |

### 22.4 Final closure record

Complete this only after every ledger row is closed:

- Final commit/tag:
- Clean-clone verification run:
- Release workflow rehearsal run:
- Package checksum:
- RustSec/deny/vet result:
- DICOM/codec validator versions and result:
- Fixture manifest/version and result:
- Coverage result:
- Fuzzing duration/result:
- CPU/GPU/platform matrix result:
- Performance/memory comparison:
- Remaining accepted risks and owners:
- Release approval:

## 23. Authoritative operational references

- GitHub Actions script injection:
  <https://docs.github.com/en/actions/concepts/security/script-injections>
- GitHub Actions secure use:
  <https://docs.github.com/en/actions/reference/security/secure-use>
- GitHub deployment environments and protected secrets:
  <https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments>
- GitHub Actions OIDC permissions and trust model:
  <https://docs.github.com/en/actions/reference/security/oidc>
- crates.io Trusted Publishing configuration:
  <https://crates.io/docs/trusted-publishing>
- Rust project Trusted Publishing/OIDC announcement and migration guidance:
  <https://blog.rust-lang.org/2025/07/11/crates-io-development-update-2025-07/>
- Official crates.io authentication action used for short-lived credentials:
  <https://github.com/rust-lang/crates-io-auth-action>
- Cargo publishing and dry-run/package behavior:
  <https://doc.rust-lang.org/cargo/reference/publishing.html>
- cargo-vet model; exemptions are deferred review, not audit evidence:
  <https://mozilla.github.io/cargo-vet/>
- RustSec quick-xml advisories:
  <https://rustsec.org/advisories/RUSTSEC-2026-0194.html> and
  <https://rustsec.org/advisories/RUSTSEC-2026-0195.html>
- RustSec crossbeam-epoch advisory:
  <https://rustsec.org/advisories/RUSTSEC-2026-0204.html>
- RustSec memmap2 advisory:
  <https://rustsec.org/advisories/RUSTSEC-2026-0186.html>

Re-check time-sensitive tool behavior, advisories, registry state, and platform
support against current primary documentation at implementation time.
