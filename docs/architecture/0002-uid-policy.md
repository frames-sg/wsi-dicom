# ADR 0002: DICOM UID policy

- Status: accepted
- Date: 2026-08-20

## Decision

Generated UIDs are fresh per export by default. Reproducible pipelines may opt into
the versioned deterministic policy, which hashes source content, normalized
identity-affecting configuration, effective ICC identity, and the complete instance
coordinate. A filesystem path alone is never semantic identity.

Valid governed caller UIDs are preserved where the public metadata contract allows
it. Generated Study, Series, SOP Instance, Frame of Reference, Dimension
Organization, and Specimen UIDs have separate documented scopes. Every UID is
validated for DICOM syntax and length at the nearest input boundary.

## Consequences

Changing pixel data or identity-affecting options changes deterministic instance
identity. Moving identical content does not. Fresh exports do not accidentally
reuse generated identifiers. Any deterministic algorithm change requires explicit
versioning and migration notes.
