# Execution state

**Branch:** `finish/personal-v1`
**Status:** personal-v1 in progress; not a public release candidate.

## Reconciled in this batch

- V2 compressed backup format with V1 compatibility and archive limits.
- Streaming vault scan and index rebuild; CLI and web rebuild paths wired.
- Restore staging guard, Unix atomic destination reservation, and age-based
  stale-staging cleanup.
- Failure-injection tests for corruption, unsafe restore paths, and concurrent
  indexing.
- Removed the unused `Index::sync_vault` API until a real refresh path needs
  it.
- Replaced the overstated completion and release-acceptance reports with an
  honest reconciliation report.
- G1.3: vault-backed Threadline workspace at
  `.engram/threadline/workspace.json`, with schema/size bounds, atomic private
  writes, localStorage migration/fallback, revision conflict handling, and
  explicit export/import.

## Still open

1. G1.2: automatic stale-index detection. Reintroduce incremental sync only
   with a real refresh path.
2. Re-run the full macOS takeover walkthrough and update
   `docs/QA_PERSONAL_V1.md`.
3. Tag personal v1 only after the remaining milestones and checks pass.
4. Public release remains blocked by owner, legal, credential, and independent
   review gates.

## Current evidence

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace`: 78 passed, 0 failed.
- All eight JavaScript test files: pass.
- Manual localhost API walkthrough: GET empty, PUT, GET round-trip, stale `409`,
  credential `400`, and `0600`/`0700` file/directory modes.
