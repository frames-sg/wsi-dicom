# ADR 0001: dependency topology

- Status: accepted
- Date: 2026-08-20

## Decision

The publishable `wsi-dicom` manifest must resolve from a standalone checkout.
First-party dependencies use exact compatible registry versions when published.
If compatible code is not published, an HTTPS Git dependency may be used only with
an immutable 40-character revision and a matching package version. Dependencies may
not escape the repository through relative paths.

The coherent dependency family selected for the 0.7.2 candidate is:

| Dependency | Version | Source |
| --- | --- | --- |
| `wsi-rs` | `0.6.0` | Git revision `b940ea94f3290e54ca2c5f87823109538709c59d` |
| `wsi-dicom-annotations` | `0.1.0` | crates.io |
| all `j2k*` crates | `0.10.0` | Git revision `57b6af89c61f25e1a476415ef3a752f6d1b3f057` |

The `metal` and `cuda` crate features propagate to the corresponding `wsi-rs`
features. Root and fuzz lockfiles must resolve one J2K version and source identity.

Local multi-repository development uses an external parent workspace, Cargo patch,
or other developer-local override that does not alter the committed package
topology.

The Git revisions are temporary release-coherent sources while these exact versions
are unpublished. A publishable release replaces them with matching registry
versions after the upstream packages are published and re-runs the standalone
package proof.

## Consequences

CI runs locked metadata, topology policy tests, packaging, and tests against the
packaged artifact before release-oriented jobs. While a selected Git-pinned version
is unpublished, the package step remains an intentional release blocker rather than
being skipped or supplied by a sibling checkout. After publication, the manifest
moves to the matching registry versions and the packaged-artifact tests must pass.
Moving to a later J2K family requires a published or immutable compatible `wsi-rs`
source first.
