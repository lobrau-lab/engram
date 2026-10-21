# Engram reconciliation report

**Status:** personal-v1 hardening batch. Not a public release candidate.
**Base:** `9f8306f` (`finish/personal-v1`)
**Date:** 2026-10-21

This report replaces the earlier `ENGRAM-COMPLETE` and
`ENGRAM-RELEASE-ACCEPTANCE` documents. Those documents declared the project
production-ready and included unverified performance claims. Those claims have
been removed.

## What changed

- **Compressed, authenticated backup format (V2).** Backups use a binary
  DEFLATE payload inside the existing AES-256-GCM envelope. V1 archives still
  restore. Archive, per-file, and decompressed-size limits are enforced.
- **Streaming index rebuilds.** `Vault::scan_each` and
  `Index::rebuild_from_vault` rebuild the SQLite index without holding every
  note body in memory. The CLI and the local web rebuild path both use it.
- **Restore hardening.** Staging directories are cleaned up only when they are
  older than one hour, so a reused PID cannot cause a live restore to be
  deleted. On Unix, the final destination name is reserved with an atomic
  directory creation before publication, preventing a rename from silently
  replacing a file or directory that appeared during the restore.
- **Failure-injection coverage.** Corrupt archives, wrong passwords, unsafe
  paths, restore destinations, and concurrent search during indexing are
  tested.
- **Removed unused incremental sync.** `Index::sync_vault` and
  `IndexSyncStats` had no production caller. Incremental refresh can return
  when a real stale-index path exists.

## Verification

Run from `/Users/tk/engram` on macOS:

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | 73 passed, 0 failed |
| `node crates/engram-cli/web/tests/*.test.mjs` (all seven files) | pass |

The takeover retention and tool-call guard are covered by the previous commit
`ff6e00f` plus the JavaScript tests.

## Limits and non-claims

- No public release, signing, updater, hosted inference, cloud sync, or
  multi-device sync is included.
- No independent security review has been performed.
- The earlier 50,000-note benchmark and release-artifact checksum claims were
  not reproducible from the recorded commands and are not repeated here.
- The full macOS takeover walkthrough was not re-run as part of this batch.
  See `docs/QA_PERSONAL_V1.md` for the open walkthrough item.
- Restore publication is atomic on Unix. On non-Unix targets the existing
  check-then-rename path remains; Windows `rename` refuses to replace an
  existing directory.
- G1.2 (automatic stale-index detection) and G1.3 (vault-backed workspace)
  remain open. Personal v1 is not tagged.

## Personal-use boundary

The project remains local-first: localhost-only HTTP, no account, no default
cloud, explicit disclosure before any configured remote inference, and an
armed-session boundary for GUI takeover. Workshop proposals remain
review-only.
