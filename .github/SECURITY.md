# Security Policy

## Reporting a Vulnerability

`wsi-dicom` ingests whole-slide images and writes DICOM objects. If you find a
crash, memory-safety issue, malformed-output bug, metadata leak, or unexpected
file-system behavior, please report it privately rather than opening a public
issue.

Use GitHub's private vulnerability reporting for the repository, or contact the
maintainer through the repository owner profile if private reporting is not yet
enabled.

Please include:

- A minimal reproducer, including the smallest input file or generated fixture
  you can share.
- Rust version, target triple, operating system, and cargo features used.
- The CLI command or Rust API call.
- Expected vs. observed behavior.

Reports are acknowledged within 7 days. Patches are issued as soon as possible,
generally within 30 days for high-severity issues.

## Supported Versions

Until `1.0.0`, the supported release line is the latest published pre-1.0
minor release and the current `main` branch. Pre-1.0 releases may still make
API-breaking changes when needed to harden correctness, conformance, or
long-term maintainability before the stable contract is finalized.

For 1.0 and later, critical security fixes are backported to the latest minor
release when practical, and users should otherwise track the newest compatible
minor version. GPU feature issues are triaged on supported hardware when that
hardware is available.
