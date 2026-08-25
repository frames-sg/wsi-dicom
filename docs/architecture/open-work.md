# Open architecture work

This list tracks only repository-level work that remains materially open. Detailed
behavior belongs in tests and focused issues; completed release history belongs in
the changelog.

- Capture representative CPU and available device performance evidence after the
  structural work; do not make performance claims from synthetic tests alone.
- Capture representative CUDA real-slide conformance and performance evidence.
  The synthetic require-device encode and round-trip gate covers runtime
  availability, but it does not substitute for representative WSI workloads.
