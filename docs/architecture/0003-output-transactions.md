# ADR 0003: output transactions

- Status: accepted
- Date: 2026-08-20

## Decision

One export stages its complete instance generation on the destination filesystem,
serializes publication with an output-directory lock, and journals sequential
backup and promotion operations. Ordinary commit failure removes newly installed
files and restores overwritten files. Interrupted work is recovered before another
export starts. An incomplete rollback is a distinct recovery-required error and
retains the journal and backups.

No-clobber is the default. Overwrite is explicit. Symlinks and ambiguous unrelated
files are rejected rather than followed or silently removed.

## Contract limit

A portable flat set of files cannot become visible simultaneously. The guarantee is
failure atomicity for ordinary reported errors plus deterministic crash recovery,
not snapshot visibility for concurrent readers. Stronger visibility would require a
separate versioned-directory design and compatibility decision.
