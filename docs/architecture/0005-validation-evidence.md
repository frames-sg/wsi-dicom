# ADR 0005: validation evidence

- Status: accepted
- Date: 2026-08-20

## Decision

Intrinsic DICOM structure checks run independently of optional external tools.
Encapsulated frame validation follows Basic or Extended Offset Table semantics,
uses bounded reconstruction, and rejects ambiguous multi-frame layouts rather than
assuming one fragment per frame.

An external decoder passes only when it exits successfully and produces a bounded,
regular, nonempty raster with the expected dimensions, components, and sample
width. Tool launch failure, timeout, output overflow, malformed output, structural
mismatch, and cleanup failure remain distinct evidence.

The advertised conformance CI job installs and proves every named validator before
running integration tests. Optional local absence is reported as skipped; it is not
represented as conformance success.

## Consequences

Validation is evidence, not certification. Process execution and captured output
remain bounded, timeouts supervise descendant processes within documented platform
limits, and reports retain enough context to diagnose the failed stage.
