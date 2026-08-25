# ADR 0006: export option compatibility

- Status: accepted
- Date: 2026-08-20

## Decision

The public `ExportOptions` structure retains its flat fields for patch-release source,
serialization, and builder compatibility. Export and profiling entry points validate that
surface once, then convert it to private grouped configuration:

- durable export semantics;
- resource limits;
- execution policy;
- GPU tuning.

Planning, preflight, identity generation, worker selection, codec execution, and writing borrow
the normalized configuration. Per-frame paths do not clone or reconstruct `ExportOptions`.

Nested public option structures may be considered only in a deliberate minor release with semver
evidence, migration documentation, and compatible serde defaults or aliases where practical.

## Consequences

Internal ownership follows the semantic and execution boundaries without disguising a public API
break as a patch. Existing struct literals and serialized option documents remain compatible.
Validation remains at executable API boundaries; already validated internal code uses the grouped
representation directly.
