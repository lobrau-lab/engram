# Engram Desktop Replacement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build Engram into a private, cross-platform desktop Markdown knowledge workspace with local crawling, local inference, citations, encrypted portable backups, and safe recovery.

**Architecture:** Keep `engram-core` UI-independent and add explicit service boundaries for vaults, indexing, crawling, inference, encryption, and portability. Add a cross-platform desktop shell only after core contracts and threat-sensitive behavior are tested; retain the CLI as a headless control surface.

**Tech Stack:** Rust workspace, SQLite FTS5, hash embeddings, OpenAI-compatible local HTTP inference, a reviewed Rust desktop WebView shell, Markdown, authenticated encryption, platform-aware secret storage.

**Spec:** `docs/superpowers/specs/2026-10-16-engram-desktop-design.md`

## Global Constraints

- Markdown files remain the source of truth.
- macOS and Linux are first-class targets; Windows must remain compatible.
- Operation is on-device-only by default; network access is opt-in and visible.
- No required cloud account, cloud sync, telemetry, or build-time network calls.
- AI output must cite local source identifiers and must not silently invent support.
- Imports and backups must be staged, hash-verified, traversal-safe, and recoverable.
- Do not delete or overwrite existing user work without an explicit migration step.

## Review Focus

- Malicious archive paths and symlinks: reject traversal and unsafe extraction.
- Wrong password or corrupted backup: fail closed without destroying the vault.
- Inference endpoint failure or deceptive model output: show provenance and a
  clear unavailable/unsupported state.
- Large vaults and binary files: crawl incrementally without blocking the UI or
  copying unsupported data into the index.
- Removable-drive removal during writes: preserve the source and recover the
  incomplete operation on next launch.

---

### Task 1: Establish the core service boundaries

**Files:**
- Modify: `crates/engram-core/src/lib.rs`
- Create: `crates/engram-core/src/error.rs`
- Create: `crates/engram-core/src/events.rs`
- Test: `crates/engram-core/tests/service_contracts.rs`

**Interfaces:**
- Produce typed errors that distinguish user input, unavailable local
  services, corruption, and I/O failures.
- Produce event types for indexing, crawling, inference, backup, and recovery.
- Preserve existing CLI behavior while making new services UI-independent.

- [ ] Add failing contract tests for error classification and event serialization.
- [ ] Implement the error/event modules and export them from `lib.rs`.
- [ ] Run `cargo test -p engram-core --test service_contracts`.
- [ ] Commit as `feat: define engram core service contracts`.

### Task 2: Harden vault and index boundaries

**Files:**
- Modify: `crates/engram-core/src/vault.rs`
- Modify: `crates/engram-core/src/index.rs`
- Test: `crates/engram-core/tests/vault_security.rs`

**Interfaces:**
- Add canonical-root path validation for all vault-relative paths.
- Add incremental scan results keyed by content hash and modification metadata.
- Keep hidden `.engram` data excluded from note discovery.

- [ ] Test traversal paths, symlink escapes, unreadable files, and unchanged-note reuse.
- [ ] Implement validation and incremental scan behavior without changing Markdown source files.
- [ ] Run focused tests and a full workspace check.
- [ ] Commit as `feat: harden vault and incremental indexing`.

### Task 3: Add local crawler and provenance graph

**Files:**
- Create: `crates/engram-core/src/crawler.rs`
- Create: `crates/engram-core/src/provenance.rs`
- Modify: `crates/engram-core/src/index.rs`
- Test: `crates/engram-core/tests/crawler.rs`

**Interfaces:**
- `Crawler::scan_roots(&[PathBuf]) -> Result<CrawlReport>`.
- `CrawlReport` contains indexed documents, extracted links/tags, skipped files,
  and source hashes.
- Only explicitly allowed local roots are scanned; network URLs are rejected.

- [ ] Test Markdown, plain text, supported document types, ignored binaries,
  duplicate content, and root escape attempts.
- [ ] Implement bounded local crawling and provenance records.
- [ ] Add FTS records and relationship queries for backlinks and graph views.
- [ ] Run focused crawler tests and benchmark a generated vault fixture.
- [ ] Commit as `feat: add local crawler and provenance graph`.

### Task 4: Make local inference discovery universal

**Files:**
- Modify: `crates/engram-core/src/ai.rs`
- Modify: `crates/engram-core/src/config.rs`
- Create: `crates/engram-core/src/inference.rs`
- Test: `crates/engram-core/tests/inference.rs`

**Interfaces:**
- `InferenceRegistry::discover_local() -> Result<Vec<InferenceEndpoint>>`.
- `InferenceEndpoint` contains a user-visible name, base URL, models, and
  transport capabilities.
- `GroundedAnswerer::answer(question, sources) -> Result<GroundedAnswer>`.
- All HTTP inference calls remain explicit, local-configured, and redactable.

- [ ] Test `/v1/models` discovery, unavailable endpoints, auth headers, and
  malformed responses using a local test server.
- [ ] Implement endpoint discovery for configured localhost endpoints and
  explicit user-provided endpoints; do not scan arbitrary network ranges.
- [ ] Implement citation-preserving grounded prompts and unsupported-answer
  handling.
- [ ] Run tests without requiring an installed model.
- [ ] Commit as `feat: add universal local inference adapters`.

### Task 5: Build encrypted backup and USB portable mode

**Files:**
- Create: `crates/engram-core/src/backup.rs`
- Create: `crates/engram-core/src/crypto.rs`
- Modify: `crates/engram-core/src/sync.rs`
- Test: `crates/engram-core/tests/backup.rs`

**Interfaces:**
- `Backup::create(source, destination, password) -> Result<BackupManifest>`.
- `Backup::restore(backup, destination, password) -> Result<RestoreReport>`.
- `PortableMode::open(root) -> Result<PortableVault>`.
- Backup format includes version, hashes, timestamps, and atomic completion state.

- [ ] Test round trips, wrong passwords, corrupted chunks, path traversal,
  interrupted writes, and conflict preservation.
- [ ] Implement authenticated encryption and memory-safe password handling using
  reviewed dependencies approved in the plan review.
- [ ] Implement staged restore and atomic manifest commit.
- [ ] Run failure-injection tests and verify the original vault remains intact.
- [ ] Commit as `feat: add encrypted portable backups`.

### Task 6: Add the desktop shell and workspace UI

**Files:**
- Create: `crates/engram-desktop/Cargo.toml`
- Create: `crates/engram-desktop/src/main.rs`
- Create: `crates/engram-desktop/src/commands.rs`
- Create: `crates/engram-desktop/ui/`
- Modify: `Cargo.toml`
- Test: `crates/engram-desktop/tests/smoke.rs`

**Interfaces:**
- Expose typed commands for vault open/create, note read/write, search,
  backlinks, graph queries, crawl, inference, backup, and recovery.
- UI must never access arbitrary filesystem paths without a core validation call.

- [ ] Validate the selected desktop shell and package a minimal empty-vault app.
- [ ] Add a file tree, Markdown editor, preview, search results, backlinks,
  tags, graph navigation, command palette, and status/error surfaces.
- [ ] Add UI tests for open/edit/save/search and a redacted local-only status.
- [ ] Run the desktop smoke test on macOS and Linux; record Windows blockers.
- [ ] Commit as `feat: add engram desktop workspace`.

### Task 7: Integrate assistant, crawler, backup, and recovery UX

**Files:**
- Modify: `crates/engram-desktop/src/commands.rs`
- Modify: `crates/engram-desktop/ui/`
- Create: `crates/engram-desktop/src/recovery.rs`
- Test: `crates/engram-desktop/tests/workflows.rs`

- [ ] Test the end-to-end workflows: crawl → search → cited answer; edit →
  backup → restore; USB removal → recovery prompt.
- [ ] Implement visible source citations, model selection/discovery, network
  state, backup progress, and recovery actions.
- [ ] Ensure failures preserve the source vault and expose actionable errors.
- [ ] Run the complete desktop workflow suite.
- [ ] Commit as `feat: integrate local assistant and recovery workflows`.

### Task 8: Cross-platform release hardening

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`
- Create: `docs/THREAT_MODEL.md`
- Modify: `README.md`

- [ ] Document the threat model, local-only guarantees, supported formats, and
  backup recovery procedure.
- [ ] Add CI checks for Linux and macOS, with Windows compilation coverage.
- [ ] Add packaging checks, dependency auditing, secret scanning, and artifact
  hash generation.
- [ ] Run release smoke tests with an empty vault and a fixture vault.
- [ ] Commit as `chore: harden cross-platform release pipeline`.
