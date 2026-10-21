# Personal v1 QA Log

## 2026-10-20 — baseline and note lifecycle

**Branch:** `finish/personal-v1` from `56333e5`
**Scope:** G0 baseline gates plus the G1.1 web-vault note lifecycle.

### Baseline gates

All passed against the starting tree:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
for f in crates/engram-cli/web/tests/*.test.mjs; do node "$f" || exit 1; done
```

The full takeover walkthrough was not re-run in this session; the prior
50-run first-run and real macOS takeover evidence remains in `docs/HANDOFF.md`.

### G1.1 note lifecycle

Implemented:

- Create and save now update the SQLite index immediately.
- Rename validates source and destination, refuses overwrite, uses a hard link
  plus unlink to avoid clobbering, updates the index, and leaves wiki links
  untouched.
- Delete moves the note to `.engram/trash/<timestamp>-<sequence>/` with a
  `manifest.json`; restore refuses an existing destination and re-indexes.
- `GET /trash` lists recoverable notes.
- Note mutations append to `.engram/audit/vault.jsonl` with action, path, time,
  and result. No note body or typed content is recorded.
- The note view shows broken wiki links after a rename instead of rewriting
  links. Trash and audit directories/files are restricted to `700`/`600` on
  Unix.

Verification:

```sh
cargo test --workspace   # includes note_lifecycle_keeps_the_index_and_trash_consistent
cargo clippy --workspace --all-targets -- -D warnings
```

A live HTTP run against a fresh temporary vault also passed: create → edit →
rename → search by new path → rebuild index → delete → list trash → restore →
audit entries present.

### G1.2 index freshness

- Web note create/save/rename/delete/restore now update SQLite immediately.
- `POST /index` and a visible **Rebuild index** button rebuild the whole vault.
- The Threadline vault-search warning no longer claims the index is missing when
  hits exist.
- Still missing: automatic stale detection for Markdown files changed outside
  the web UI.

### G1.3 vault-backed workspace

Implemented:

- Workspace document at `.engram/threadline/workspace.json`, schema version 1.
- Same-origin `GET`/`PUT /api/v1/workspace` with revision checks and `409` on a
  stale tab.
- Atomic writes, `0600` file and `0700` directory on Unix, symlink rejection.
- One MiB file cap plus depth, string, array, object-key, and key-name limits.
- Credential-shaped keys (`apiKey`, `token`, `password`, and similar) are
  rejected and never stored.
- Legacy `localStorage` workspace migrates on first load; a local fallback and
  pending marker prevent silent data loss when a save fails.
- Settings expose explicit workspace JSON export/import; the clear action asks
  for confirmation. Encrypted note backups exclude `.engram`, and the README
  states that the workspace must be exported separately.

Verification:

```sh
cargo test -p engram-cli --bin engram threadline_store
node crates/engram-cli/web/tests/workspace-store.test.mjs
```

A live localhost run also passed: empty `GET`, `PUT`, `GET` round-trip, stale
`409`, credential `400`, and `0600`/`0700` modes.

### Remaining in G1

- G1.2: stale-index detection for external edits.
