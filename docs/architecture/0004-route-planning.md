# ADR 0004: route planning

- Status: accepted
- Date: 2026-08-20

## Decision

Export and profiling consume one bounded route planner. The plan represents
JPEG passthrough, JPEG retile, JPEG encode, J2K passthrough, direct
JPEG-to-HTJ2K, J2K encode, device candidates, sparse frames, and explicit rejection
reasons without pretending that their execution semantics are interchangeable.

Planning determines semantic eligibility, output transfer syntax and pixel profile,
lossy provenance, permitted fallback, and device candidacy. Execution performs the
chosen codec work and validates produced bytes without silently re-running policy.
Planning operates at row or batch scope and does not preload a whole slide.

Raw J2K input is parsed once per logical planning step into a prepared inspection
shared by the relevant policy checks. Normalized immutable options are borrowed by
the planner and executors.

## Consequences

Profile and export route counts are directly comparable. Codec-specific
executors remain separate. Production modules use explicit imports so route-policy
ownership is visible.
