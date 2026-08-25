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
| `wsi-rs` | `0.6.0` | crates.io |
| `wsi-dicom-annotations` | `0.1.1` | crates.io |
| all `j2k*` crates | `0.10.0` | crates.io |

The `metal` and `cuda` crate features propagate to the corresponding `wsi-rs`
features. Root and fuzz lockfiles must resolve one J2K version and source identity.

Local multi-repository development uses an external parent workspace, Cargo patch,
or other developer-local override that does not alter the committed package
topology.

These exact upstream versions are published on crates.io, so the release manifest
uses registry sources and the standalone package proof resolves without Git access.

## Consequences

Main CI runs locked metadata, standalone-topology policy tests, and package-content
enumeration against the published dependency set. The protected publish workflow
retains the exact `cargo package --locked` and
packaged-artifact test gates. After upstream publication, the manifest moves to the
matching registry versions and those release gates must pass before publication.
Moving to a later J2K family requires a published or immutable compatible `wsi-rs`
source first.
