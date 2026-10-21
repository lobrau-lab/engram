# Engram / Threadline: Functionality Report and Completion Plan

**Date:** 2026-10-20
**Baseline:** `main` after `cbcd89d docs: add agent handoff`; functional code through `ab99c1d`
**Audience:** the owner and the next on-device coding agent
**Scope:** finish the personal daily driver, then prepare a public v0.1 without pretending the owner/legal gates are code tasks.

Read this with:

1. `docs/HANDOFF.md` — architecture, decisions, verification data, and gotchas.
2. `DECISIONS.md` — product defaults and boundaries.
3. `docs/THREAT_MODEL.md` — security model.
4. `docs/superpowers/specs/2026-10-20-threadline-personal-agent.md` — the original Threadline spec.
5. `LAUNCH_CHECKLIST.md` — public-release gates.

---

## 0. The short answer

### What Engram can do today

Engram is a working local-first Markdown second brain with Threadline as its
front door. On a Mac with macOS 26+ and Apple Intelligence, it can:

- Start with one command and open Threadline in the browser.
- Initialize an empty vault automatically.
- Chat with an on-device Apple Foundation Models model inside the Engram
  process, with no model server, download, API key, or cloud request.
- Keep multiple conversations, stream replies, pack selected vault notes into
  context, and show which notes were used.
- Derive a local conversation-memory ledger, inspect its decision rules, and run
  a synthetic memory/context benchmark.
- Search the vault, view backlinks, open a simple graph, create and edit notes,
  and use a separate dump/review/apply flow for captured text.
- Use reviewed project-file tools and checked project commands with a human
  confirmation gate.
- Inspect local CLI executables and macOS `.app` bundles, hash them, record their
  version/help/flags, and run only the reviewed `--help`, `--version`, or open
  actions.
- Take over macOS through an explicitly armed, timed session: inspect the
  accessibility tree, focus apps, press keys, type, click, scroll, drag, set
  fields, open apps, run AppleScript, and capture the screen. Stop, Escape,
  expiry, and a durable audit log bound the session.
- Draft review-only self-improvement proposals in Workshop.
- Export/import a folder-based sync copy, create encrypted backups, restore into
  an empty destination, and copy Markdown/text files into the vault.

The personal presentation path is complete. A 50-run first-run simulation passed
50/50 with 950/950 HTTP 200s and no console/page/request failures. The latest
10-run post-change simulation also passed 10/10. Rust tests, 7 JS tests, clippy,
formatting, and release builds pass. Real macOS control has been exercised.

### What it cannot do yet

The important gaps are not "does the chat work?" They are:

- **Note lifecycle is now implemented, but external edits still need a rebuild.**
  The web vault can create, edit, rename, trash, and restore notes. The index is
  updated for web writes; files changed outside the UI still require
  `engram index`, and there is no stale-index warning yet.
- **Threadline conversations/settings/memory live in browser `localStorage`.**
  Clearing site data loses them, and they do not follow the vault to another
  browser. This conflicts with the stated "metadata under `.engram/`" model.
- **There is no guided no-model first-run state.** The available-model path is
  verified; the unavailable/malformed/endpoint-setup paths are not.
- **The capability digest is shallow.** It records a CLI/app's version/help/flags
  and safe open actions, but does not map an app's GUI capabilities or learn new
  reviewed actions from its accessible controls.
- **The built-in model cannot drive takeover.** Direct Takeover controls work
  without a model, but model-driven GUI control requires a local
  OpenAI-compatible endpoint with function tools. A remote endpoint never gets
  `computer_use`.
- **Workshop cannot run tests or apply a proposal.** It writes a proposal; the
  human still edits the tree by hand.
- **Import is mostly unwired.** `import_file` supports text/Markdown/HTML and
  reports PDF unsupported, but the CLI/UI path only copies `.md`/`.txt`.
- **Sync is not deletion-safe.** No tombstones, global rather than per-entry
  Lamport counters, a race on simultaneous exports, conflict files indexed as
  normal notes, and a path-derived device id.
- **Public release is not ready.** No signed/notarized artifacts, no terms
  acceptance flow, no independent security/legal review, no support/rollback
  process, and Linux/Windows have no GUI takeover.

### Finish lines

**Personal v1 (the daily-driver target):** one local binary, one command, a
durable vault, durable conversations, complete note lifecycle, reliable
on-device/no-model setup, a meaningful capability digest, safe armed takeover,
deletion-safe sync/backup, an accessibility pass, and a human walkthrough. No
cloud dependency.

**Full vision / public v0.1:** everything above plus the reviewed
self-improvement apply loop, signed cross-platform artifacts, independent
review, legal/privacy acceptance, support, and rollback. Some of that is
owner-only. The coding agent can prepare it; it cannot approve it.

### Open owner decisions

These have defaults in this plan; change them only with the owner:

1. **Workshop apply:** default is full-vision, while personal v1 keeps Workshop
   review-only. Move it earlier only if the owner wants self-modification in the
   daily driver.
2. **Remote endpoints:** default is local-only. If remote endpoints remain
   supported, a keychain-backed proxy and a stronger privacy review are needed.
3. **Per-app takeover profiles:** default is the existing broad armed session.
   Profiles/allow-lists are optional hints, not a replacement for the owner's
   intentional broad control.
4. **Import depth:** default is text/Markdown/HTML for v1; PDF/DOCX/OCR/audio are
   later expansions.
5. **Public distribution:** owner/legal/credential gate; do not infer approval
   from a green test run.

---

## 1. Functionality report

Status vocabulary:

- **Verified** — exercised by Rust/JS/browser/manual tests at the stated baseline.
- **Implemented, partial** — the code path exists but has limitations or lacks
  verification on an important path.
- **Missing** — no user-facing implementation.
- **By design** — intentionally not built; do not "fix" without an owner decision.

### 1.1 Capability matrix

| Area | What it can do now | Status | Important limit |
|---|---|---|---|
| CLI / vault | `init`, `index`, `search` (OR/AND), `lexical`, `ask`, sync `export`/`import` with dry-run, `serve`, `open` (auto-inits empty/hidden-only vaults; `serve` is explicit), encrypted `backup`/`restore`, `import-files` | **Verified** | Index is manual for external edits; `ask` uses an OpenAI-compatible endpoint, not the embedded model; `import-files` copies only `.md`/`.markdown`/`.txt` |
| Web vault | `/vault` search, note view with backlinks/broken links, new note, edit/save, rename, trash/restore, simple SVG graph, `/ask`, dump inbox/review | **Verified** | No stale-index warning; `/ask` requires a configured endpoint |
| Threadline chat | Streaming chat, multiple conversations, context inspector, vault-context toggle, provider settings, accent themes | **Verified** | Conversations/settings/memory are browser-local; no cross-browser history; no server-side export |
| Memory | Per-conversation ledger, source-linked assertions, conflict preservation, deterministic decision-rule view, synthetic replay bench | **Implemented, prototype** | Lightweight heuristics, not truth detection; cross-chat recall is off; token counts are estimates |
| Embedded inference | Apple Foundation Models in-process on macOS 26+, auto-detection, `/api/v1/inference/embedded`, `/v1/models`, same-origin `/v1/chat/completions`, serialized inference | **Verified** | macOS 26+ and Apple Intelligence required; text only; cannot call `computer_use` |
| External inference | OpenAI-compatible endpoint, model discovery, explicit remote disclosure, tool-call fallback, fixed oMLX start/stop/restart and model load/unload | **Implemented; partial verification** | Browser calls the endpoint directly; API key is page-session-only; remote endpoints never receive `computer_use`; no real remote-model compatibility suite |
| Tools / permissions | `project_list_files`, `project_read_file`, `project_create_directory`, `project_write_file`, `run_project_check`, app inspect/list/run, `computer_use`, workspace actions | **Implemented** | Reviewed/path-restricted but not OS-sandboxed; no web/API/MCP connectors; project root is an explicit grant |
| Capability digest | CLI and `.app` inspection, SHA-256 hash, version/help/flags, reviewed actions, application UI | **Implemented** | Only `--help`, `--version`, and app open actions; no GUI capability map; hash mismatch fails closed |
| Takeover | Armed 1–120 min session, token, expiry, Stop/Escape, accessibility tree, windows, focus, key, type, click, scroll, drag, click control, set field, open app, AppleScript, screenshot, audit | **Verified on macOS; clean-account test pending** | macOS only; unsigned helper; Accessibility and Screen Recording permissions; no per-app allow-list; screenshots may contain sensitive content |
| Workshop | Read up to three named source files, write a bounded review-only proposal with intent/diff/tests/risks | **Implemented, review-only** | No sandbox test run; no approved apply; no rollback |
| Dump / capture | Submit text or file-shaped text, analyze locally or via model, review/edit, apply `create_note`, undo | **Verified** | Only `create_note` applies; append/link/tag proposals are visible but unsupported |
| Import | `import-files` copies Markdown/text; core `import_file` extracts text/Markdown/HTML and records unsupported formats | **Implemented, partial** | Core extractor is not exposed in CLI/UI; PDF/DOCX/OCR/audio/mail are unsupported |
| Sync | Folder export/import, last-write-wins, conflict files, dry-run | **Verified, partial** | No deletion propagation; global Lamport; simultaneous-export race; conflict `.md` files are indexed; device id depends on path |
| Backup | Argon2id + AES-256-GCM encrypted archive, staged restore, refuses non-empty destination, wrong-password failure | **Verified** | Restore check/rename race still needs a no-clobber guarantee across platforms |
| UI | Responsive Threadline shell, desktop and 390 px layout, accent themes, application/takeover/workshop/settings views | **Verified** | Accessibility pass is incomplete; no native shell (by design) |
| Packaging / release | Rust workspace, cross-platform CI check/test/build, local release binary | **Partial** | No signed/notarized artifacts, checksums, legal acceptance, independent review, support, rollback, or Windows/Linux takeover |
| Crawler | Core module reports documents/skips/links/tags and rejects network roots | **Core only** | No CLI or UI path; do not claim it as a shipped feature until wired |

### 1.2 What is genuinely strong

- **The local-first architecture is real.** The vault is Markdown, the index is
  disposable SQLite, and the active macOS path has no model server or cloud.
- **Threadline is the front door.** `/` redirects to `/threadline/`; the app is
  the local Rust process plus embedded same-origin assets.
- **The safety boundaries are explicit.** Host/Origin/Sec-Fetch-Site checks,
  hidden-path/traversal/symlink rejection, hash-checked capability runs,
  human-armed takeover, redacted audit, and review-only Workshop are not
  cosmetic.
- **The first-run path has evidence.** Auto-init, embedded model selection, and
  chat were exercised across 50 fresh runs, then 10 runs after the latest
  changes.
- **The takeover is not a mock.** Finder focus, Calculator arithmetic,
  AppleScript, Unicode/newline typing, screenshot capture, concurrent Stop, and
  process cancellation were verified on a real Mac.

### 1.3 What is partial or misleading if left as-is

- **"Conversation history under `.engram/`" is not true yet.** `DECISIONS.md`
  says metadata is disposable derived state under `.engram/`; the README says
  Threadline uses `localStorage`. That must be reconciled. A browser profile
  reset currently loses conversations and memory.
- **"Import supports HTML" is core-only.** The extractor and tests exist, but a
  user cannot invoke `import_file` from the CLI or UI. `import-files` only copies
  Markdown/text.
- **"Crawler" is core-only.** The module and tests exist; no command or view
  exposes it.
- **"Capability digest" is safe intake, not deep understanding.** It is a
  version/help/flags record plus a hash. It does not learn an app's menus,
  windows, or accessible controls, and it cannot synthesize new reviewed actions.
- **"Workshop improves the code" is a proposal loop only.** The model cannot run
  a test or apply a patch. The human still does the work.
- **"Memory" is a deterministic prototype.** It is useful for inspecting prompt
  behavior; it is not a general accuracy claim.
- **"Browser tests" are not committed.** The latest Playwright runs were an
  ad-hoc harness; CI does not run browser tests. The plan below makes them
  permanent.

### 1.4 What is missing

- Note rename/delete/trash/restore.
- Automatic index maintenance after note writes.
- Durable, vault-backed Threadline workspace and explicit export/import.
- No-model first-run guidance and endpoint compatibility diagnostics.
- Native keychain storage; the legacy config still has a plaintext key path.
- A capability digest that maps GUI capabilities and supports reviewed learned
  actions.
- Workshop sandbox test execution, approved apply, and rollback.
- General audit export/delete/retention; screenshot retention.
- Sync tombstones, per-entry/per-device Lamport, stable device identity, and
  simultaneous-export locking.
- Conflict files kept out of the normal note index.
- Exposed import pipeline; PDF/DOCX/OCR/audio/archive/mail import.
- Full dump append/link/tag operations.
- Permanent browser tests in CI; accessibility pass.
- Public signing/notarization, legal acceptance, independent review, support, and
  rollback.

### 1.5 By design — do not "fix"

- No Tauri/native shell in the active path.
- No per-action takeover prompts while a session is armed.
- No automatic Workshop apply.
- No remote endpoint access to `computer_use`.
- No default cloud calls, telemetry, or remote bind.
- No automatic deletion propagation without an explicit sync design (but
  tombstones are still needed).
- No claim that `--help`/`--version` parsing is the same as understanding an app.

---

## 2. Completion strategy

### 2.1 Principles

1. **Finish the daily driver before polishing the public release.** A signed
   artifact that loses conversations or deletes the wrong note is not progress.
2. **Small, testable slices.** One milestone task, one focused test, one commit.
   Do not rewrite the UI or the sync format in one pass.
3. **Evidence over checkboxes.** A task is done only when the listed commands and
   manual checks pass and the output is recorded.
4. **Security invariants are not negotiable.** If a change requires weakening
   Host/Origin checks, audit redaction, hash checks, approval gates, or the
   armed-session boundary, stop and ask the owner.
5. **Local only.** The on-device agent must not send vault content, conversations,
   screenshots, or source to a remote service. Loopback mocks are allowed.
6. **Preserve the product personality.** Threadline-first, broad armed takeover,
   review-only self-improvement, and a plain-Markdown vault are intentional.

### 2.2 Milestones at a glance

| Gate | Milestone | Why it matters | Depends on |
|---|---|---|---|
| G0 | Baseline and live walkthrough | Prove the current build before changing it | clean `main` |
| G1 | Durable vault and conversations | The daily driver must not lose notes or chat history | G0 |
| G2 | Inference setup and secret handling | A fresh Mac must never dead-end; no plaintext keys | G0 |
| G3 | Capability learning and takeover safety | Fulfil "learn another app" and make control auditable | G1, G2 |
| G4 | Workshop sandbox and approved apply | Fulfil "safely improve its own code" | G1, G3 |
| G5 | Sync, backup, import, dump | Make the second brain portable and loss-resistant | G1 |
| G6 | Accessibility, security, CI, release candidate | Make it presentable and repeatable | G1–G3, G5 (G4 for full vision) |
| G7 | Public v0.1 | Distribution; owner/credential/legal gates | G6, G4 |

Rough effort for one focused agent: G0–G3 and G5–G6 are the personal-v1 core;
G4 is the full-vision differentiator; G7 is owner-gated. Do not compress G4 or
G5 into "just add a button"; both need failure-path tests.

### 2.3 G0 — Baseline and live walkthrough

**Goal:** freeze a known-good starting point and find any current-build issues
before touching architecture.

Tasks:

- [ ] Confirm `git status` is clean and `git log --oneline -5` is understood.
- [ ] Create a working branch: `git switch -c finish/personal-v1`.
- [ ] Run the full baseline gates:
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `cargo build --workspace --release`, all seven
  `web/tests/*.test.mjs` files.
- [ ] Run the live walkthrough: `engram open --project-root .`, chat once, open
  Takeover, arm for 1 minute, focus an app, capture a screenshot, press Stop,
  and inspect a capability digest.
- [ ] Record observations in `docs/QA_PERSONAL_V1.md` (new file) with date,
  commit, commands, screenshots/behavior, and any defects.
- [ ] Reconcile stale claims in `docs/ROADMAP_CHECKLIST.md`; do not check an item
  without current evidence.
- [ ] If the walkthrough is clean, tag the personal demo (`v0.1.0-personal` or
  similar) so later work has a rollback point.

**Exit gate:** all gates green; walkthrough performed; no uncommitted changes;
known defects have owners/priorities.

### 2.4 G1 — Durable vault and conversations

**Goal:** a daily driver can create, rename, delete, search, and keep Threadline
state without relying on a browser profile.

#### G1.1 Note lifecycle

**Status:** implemented and verified in the web vault (create, edit, rename,
trash, restore, audit, index updates, broken-link display). External-file edits
still rely on `engram index`.

Implement in the web vault and HTTP layer:

- Rename a note (`.md` only) with path validation; no overwrite; no hidden path;
  no traversal/symlink; update the SQLite index. Do not silently rewrite every
  wiki link; offer an explicit "update links" follow-up if implemented.
- Delete to a vault-local trash (`.engram/trash/<timestamp>/...`) with a
  manifest, not immediate unlink. Add restore.
- Confirmation UI that names the exact path and action.
- Audit these mutations locally (path, action, time, result; no note body).
- Keep note contents plain Markdown; never lock the user in.

Files: `crates/engram-cli/src/serve.rs`, `crates/engram-core/src/vault.rs`,
`crates/engram-core/src/index.rs`, web vault templates/assets, tests.

Acceptance:

- Create → edit → rename → search finds the new path → delete → restore.
- Traversal, hidden path, symlink, non-Markdown, and overwrite attempts fail.
- Backlinks/graph do not crash after a rename; broken links are visible rather
  than silently rewritten.
- The index reflects each operation without a manual full rebuild.

#### G1.2 Index freshness

**Status:** partial. Web create/save/rename/delete/restore and dump apply update
the index; the Vault view has a one-click rebuild. Stale detection for files
changed outside the UI is still missing.

- Add incremental index/update/remove operations after web note writes and dump
  applies.
- If a full rebuild is unavoidable, show a visible "index stale" state and
  provide one-click indexing; never pretend fresh results.
- Test stale detection with mtime changes and a large fixture.

Acceptance: edit a note, then search immediately; the new text is found. Kill
the server mid-write; reopen; the index is either correct or visibly stale, not
silently wrong.

#### G1.3 Vault-backed Threadline workspace

Move conversations, non-secret settings, and derived memory to an atomic
workspace file under `.engram/threadline/` (for example `workspace.json`).

- Version the schema.
- Bound the file size and per-field lengths.
- Write atomically with `0600` permissions where supported.
- Never persist API keys, session tokens, or screenshot data.
- Migrate existing `localStorage` (`threadline.workspace.v1`) on first load;
  keep a local fallback if the server is unavailable.
- Add explicit export/import and a "clear workspace" confirmation.
- Decide whether the encrypted backup includes the workspace; if it does not,
  provide a separate export and state the limitation in the README.
- Define multi-tab behavior: last server write wins with a version check, or a
  visible conflict state. Do not silently interleave two tabs' writes.

Files: new `crates/engram-cli/src/threadline_store.rs` (or equivalent),
`serve.rs`, `web/src/core/store.js`, `app.js`, tests; update `DECISIONS.md` and
README.

Acceptance:

- New browser profile → conversations/settings/memory restored from the vault.
- API keys are absent from the file, logs, and exports.
- Corrupt workspace → safe fallback, no crash, no key exposure.
- Two tabs do not lose a conversation without a visible conflict.
- `localStorage` migration is idempotent.

**Exit gate:** note lifecycle, fresh index, and durable workspace tests pass;
browser tests cover each path; no data-loss defect remains.

### 2.5 G2 — Inference setup, diagnostics, and secrets

**Goal:** a fresh Mac always has a next step; no plaintext credential path.

#### G2.1 No-model first run

- On `/api/v1/inference/embedded` returning unavailable, malformed, or
  network error, show an explicit setup state:
  - "Use the built-in model" (macOS 26+ with Apple Intelligence), or
  - "Connect a local OpenAI-compatible endpoint."
- Disable or explain the send action; do not silently send into a dead endpoint.
- Add retry and a direct link to Connections & settings.
- Do not auto-select a remote endpoint or send context anywhere.

Acceptance tests (route mocks): `available:false`, HTTP 500, malformed JSON,
timeout, and recovery after retry. No console errors; no request to an external
host.

#### G2.2 Endpoint diagnostics

- Keep model discovery, but add a clear compatibility result: reachable, model
  present, chat completions accepted, function tools accepted/not accepted.
- Time-bound probes and allow cancellation.
- Distinguish "tools unsupported, retrying without tools" from a failed chat.
- Never offer `computer_use` to a remote endpoint.

Acceptance: unreachable host, bad model id, bad key, no-tools endpoint, and
cancelled probe all produce actionable, non-destructive states.

#### G2.3 Secret handling

- Remove the legacy plaintext `llm_key` config path or migrate it.
- Use the OS keychain for stored provider secrets where supported; otherwise use
  a documented environment variable for CLI use.
- Keep the browser's key page-session-only and out of workspace persistence.
- Never log request headers or keys; ensure error surfaces redact them.
- If remote endpoints remain a supported feature, prefer a local server-side
  proxy with an explicit host allowlist so the browser does not hold long-lived
  keys. If that is deferred, document the limitation clearly.

Acceptance: grep the vault/config/logs/export for a sentinel key and find
nothing; keychain read/write/delete tests; wrong key gives a clean error.

**Exit gate:** a fresh no-model run is understandable and recoverable; no
plaintext secret persists.

### 2.6 G3 — Capability learning and takeover safety

**Goal:** make "learn another app" substantive and make real GUI control
auditable without narrowing the owner's intentional broad-armed design.

#### G3.1 Digest v2

For CLI executables:

- Keep path, SHA-256, version, usage, and flags.
- Parse subcommands/flags into a structured summary where possible.
- Keep the reviewed action allowlist (`--help`, `--version`).
- Rehash before every run; a changed binary fails closed until re-inspected.

For macOS `.app` bundles:

- Read bundle id, display name, version, executable hash, and relevant
  `Info.plist` capabilities (document types, URL schemes, services).
- Record code-signature status where available.
- Optionally capture an accessibility summary (windows, roles, titles, menu
  items) as **read-only evidence**, clearly separated from executable actions.
- Store schema version, inspected-at, and permission status.
- Add re-inspect, diff, and revoke/delete controls.

Integration:

- Include a concise digest summary in model context when the user asks about an
  app.
- Keep `run_digested_application` restricted to reviewed actions; no arbitrary
  args, no shell.
- Do not auto-generate an executable action from a GUI tree. A proposed action
  must be reviewed and stored before it can run.

Acceptance fixtures: CLI help/version, malformed help, app with missing plist,
app with URL schemes, hash tamper, symlink, inaccessible bundle, revoked digest,
and permission-denied accessibility snapshot. No command is executed during
inspection beyond the reviewed help/version/open actions.

#### G3.2 Takeover hardening

- Add screenshot/audit retention controls and an export/delete path with
  confirmation. Warn that screenshots may contain secrets.
- Add a general audit log for local mutations, approvals, capability runs, and
  Workshop apply (takeover audit remains durable).
- Verify clean-account behavior: Accessibility grant, Screen Recording grant,
  helper extraction, arm, action, Stop, Escape, expiry, audit, and re-approval
  after rebuild.
- Keep the top-bar countdown/Stop visible; ensure Stop works from a stale tab and
  during an in-flight AppleScript.
- Keep audit redaction: no typed text, field values, or AppleScript source.
- Owner decision only: optional per-app profiles or opt-in allow-lists. Do not
  replace the broad armed session with mandatory per-app prompts.

Acceptance: the clean-account checklist passes; no orphan `osascript`/helper
process after Stop; audit/screenshot permissions are `700`/`600`; a stale
session cannot act; a remote endpoint never receives `computer_use`.

**Exit gate:** digest v2 and takeover hardening have tests and a documented
manual clean-account run.

### 2.7 G4 — Workshop sandbox and approved apply

**Goal:** satisfy "safely improve its own code" without autonomous
self-modification. This is the differentiator for the original vision and part
of the full-vision gate; the personal daily driver can ship without it because
Workshop remains review-only.

#### G4.1 Proposal contract

- Proposal records: id, base commit, files, intent, unified diff, test plan,
  risks, created-at, status.
- Validate paths under the granted project root; reject hidden/sensitive paths,
  symlinks, binary patches, oversized diffs, and paths outside the project.
- Refuse apply when the working tree is dirty, the base commit is missing, or
  the project is not a git repository.
- Flag sensitive changes (tests, security checks, approval policy, CI, secrets)
  for a second, explicit confirmation.

#### G4.2 Sandbox test run

- Create a detached git worktree or isolated copy at a fixed base commit.
- Apply the patch with `git apply --check` then `git apply`.
- Run the project's allowlisted checks (format, check, focused tests, then full
  tests if the focused set passes) with a timeout and offline dependency mode.
- Capture stdout/stderr; redact secrets; store evidence with the proposal.
- Clean up the worktree on success, failure, and cancellation.

#### G4.3 Human-approved apply

- Show the diff, test logs, risks, and sensitive-file warnings in the UI.
- Require a typed confirmation containing the proposal id.
- Apply to a new local branch (for example `workshop/<id>`), commit there, and
  never auto-merge, force-push, or push to a remote.
- Keep the old state recoverable; provide a revert/rollback path.
- The model cannot trigger apply; only the human/server action can.
- A proposal that weakens a security test, permission check, or approval policy
  requires a separate explicit owner confirmation and must never be applied by
  default.

Acceptance tests: path traversal, symlink, non-git project, dirty tree, patch
conflict, failing test, timeout, concurrent apply, sensitive-file warning,
successful apply to a branch, and rollback. Verify `main` is untouched until the
human approves and that no live tree change occurs during proposal/sandbox.

**Exit gate:** a trivial proposal can be sandbox-tested, human-approved, applied
to a branch, and reverted; all refusal paths fail closed.

### 2.8 G5 — Sync, backup, import, and dump

**Goal:** the vault survives multiple devices, interruptions, and imports.

#### G5.1 Sync correctness

- Add tombstones for deletions and propagate them.
- Move to per-entry/per-device Lamport counters.
- Store a stable device id in config; migrate the old host+path-derived id.
- Add a manifest lock/compare-and-swap for simultaneous exports; stale-lock
  timeout and recovery.
- Keep conflict files out of the normal note index; put them in a review area
  or hidden conflict directory.
- Test two devices, delete/rename, conflict, interrupted write, symlink,
  traversal, and hidden-file attempts.

#### G5.2 Backup/restore

- Guarantee no-clobber across the check/rename race; stage, fsync, and verify.
- Test wrong password, corrupted archive, truncated archive, interrupted restore,
  non-empty destination, and large vault.
- Document the exact recovery procedure.

#### G5.3 Import

- Expose the existing `import_file` extractor through a CLI command and/or UI
  upload/dump path.
- Support text/Markdown/HTML; record PDF and other unsupported formats as
  explicit unsupported rows, not silent success.
- Deduplicate by hash; enforce size limits; never index binary content; write
  imports under `.engram/imports/`.
- Optionally add PDF text extraction/OCR/DOCX later; do not let it block v1.

#### G5.4 Dump operations

- Implement append-to-note, link, and tag proposals with review and undo, or
  remove them from the UI. Do not leave visible controls that cannot work.
- Make multi-operation applies transactional or explicitly one-at-a-time.
- Test source-hash mismatch, partial failure, idempotent retry, and undo.

**Exit gate:** a two-device simulation loses nothing; deletions do not resurrect;
conflicts are reviewable but not indexed as notes; backup/restore and import
tests pass; every visible dump action works.

### 2.9 G6 — Accessibility, security, CI, and release candidate

**Goal:** make the personal app pleasant and repeatable, then prepare a public
candidate without pretending owner gates are done.

#### G6.1 Accessibility

- Keyboard path for every view: navigation, chat, composer, approvals, settings,
  takeover arm/stop, Workshop review.
- Visible focus, logical headings, labels, error/status announcements, contrast,
  reduced-motion behavior, and 390 px layout.
- Verify with a browser accessibility scan plus a manual VoiceOver pass.
- Fix the dense Takeover/Workshop forms first; they are the highest-risk screens.

#### G6.2 Security and audit

- Remove plaintext secrets; secret-scan the tree and history.
- Dependency audit and a dependency-policy decision.
- XSS tests for note rendering, search snippets, app metadata, Workshop
  proposals, and dump content.
- Archive/backup tests; local-only network tests; Host/Origin tests.
- General audit export/delete/retention; screenshot retention.
- Update `docs/THREAT_MODEL.md`, `PRIVACY.md`, and the launch checklist with what
  is actually implemented.

#### G6.3 CI and permanent browser tests

- Add `cargo fmt --all --check` and clippy to CI.
- Add the seven JS tests.
- Commit a Playwright (or equivalent) browser suite covering:
  first paint, model unavailable, settings, vault context consent, remote
  disclosure, note lifecycle, workspace persistence, takeover arm/stop (mocked
  backend), app digest, Workshop review, and no console/page errors.
- Use route mocks for model/takeover; do not require Apple Intelligence in CI.
- Add a cross-platform smoke test for `open` auto-init and `serve` no-init.
- Keep the release build job; add artifacts/checksums only when the owner enables
  distribution.

#### G6.4 Release candidate

- Choose and record a version; update README, handoff, roadmap, decisions,
  changelog, and security docs.
- Build from a clean checkout; run the full gates and manual walkthrough.
- Produce an unsigned personal release candidate with checksums if useful.
- Stop for owner sign-off before any public distribution.

**Exit gate:** all tests/CI green; accessibility and security checks pass; live
walkthrough passes; docs match code.

### 2.10 G7 — Public v0.1 (owner-gated)

The agent can prepare these, but cannot complete them alone:

- macOS Developer ID signing and notarization; signed helper or an explicit
  unsigned-helper policy.
- Linux and Windows artifacts/signing; document no GUI takeover there.
- SHA-256 checksums, reproducible version, release notes.
- Independent threat-model/security review, dependency/secret audit, archive/XSS
  testing.
- Counsel review of the proprietary license/royalty terms; ownership/provenance
  review of AI-assisted work and assets; real terms-acceptance flow.
- Jurisdiction-appropriate privacy statement; support and vulnerability contacts;
  rollback/compromised-release procedure.

Do not label an unsigned build a trusted installer. Do not publish the
repository or artifacts without the owner's explicit authorization.

---

## 3. Instructions for the on-device agent

### 3.1 Environment

Required for the full path:

- macOS 26+ with Apple Intelligence for the embedded-model tests (or use a
  no-model mock for CI).
- Xcode Command Line Tools (`xcrun`, Swift) for the takeover helper.
- Stable Rust toolchain.
- Node.js for the standalone JS tests; Chromium/Playwright for browser tests.
- No model server is required for the built-in path. A local
  OpenAI-compatible endpoint is optional for tool-calling/takeover experiments.

Start from a clean tree:

```sh
cd /Users/tk/engram
git status -sb
git log --oneline -5
git switch -c finish/personal-v1
```

If the working tree is not clean, stop and ask the owner; do not overwrite
unfamiliar changes.

### 3.2 The working loop

For every task:

1. **Read** this plan, the handoff, and the relevant module before editing.
2. **Pick one** checkbox/task; do not mix milestones in one commit.
3. **Write the failing test first** where practical: Rust integration/unit, a
   Node test, or a Playwright case.
4. **Implement the smallest change** consistent with the surrounding style.
5. **Run targeted tests**, then the full gates.
6. **Update docs/checklists** only with evidence.
7. **Commit** with a scoped message.
8. **Record** the command output and manual observations in
   `docs/QA_PERSONAL_V1.md`.

Do not claim "done" from code inspection. If a test cannot run, say which test
and why; leave the item unchecked.

### 3.3 Commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release

for f in crates/engram-cli/web/tests/*.test.mjs; do
  node "$f" || exit 1
done
```

First-run/manual:

```sh
cargo run --release -- open --project-root .
# or headless:
cargo run --release -- serve --port 0 --project-root .
```

Threadline assets are embedded at build time. After changing anything under
`crates/engram-cli/web/`, rebuild before testing the running binary; there is no
separate frontend dev server.

Automation:

- Use `--port 0` and parse `Engram workspace: http://...`.
- Prepend a fake `open` executable to `PATH` for headless `open` tests; wait a
  short time for the browser invocation.
- Use fresh temporary vaults and a fresh browser context with empty
  `localStorage`.
- Never point tests at a remote model; mock the endpoint or use loopback.

### 3.4 Evidence template

```text
Date:
Commit:
Task / milestone:
Commands run:
Results (paste the important output):
Manual checks:
Observed defects:
Security notes:
Follow-up:
```

Put this in `docs/QA_PERSONAL_V1.md` or a dated log under `docs/qa/`. Do not put
secrets, full conversations, or screenshots containing private data in the repo.

### 3.5 Security invariants — do not violate

- Keep `serve`/`open` bound to localhost by default.
- Keep Host, Origin, and `Sec-Fetch-Site` validation on state-changing requests.
- Keep hidden-path, non-Markdown, traversal, and symlink rejection.
- Keep capability hashes checked before every run; a changed binary fails closed.
- Keep capability args fixed; never introduce a shell or arbitrary model-supplied
  arguments.
- Keep `computer_use` unavailable to remote endpoints.
- Keep takeover human-armed; do not add per-action prompts while armed without
  owner approval. Keep Stop, Escape, expiry, and audit.
- Keep audit redaction: no typed text, field values, or AppleScript source.
- Keep API keys out of `localStorage`, workspace files, Markdown, logs, and
  release artifacts.
- Keep Workshop review-only until the approved apply path is fully tested; never
  auto-apply, auto-merge, or force-push.
- Treat sync folders and imported archives as untrusted.
- Do not add telemetry, cloud sync, or a remote default.

### 3.6 Stop and escalate

Stop and ask the owner before:

- Changing a product invariant (Threadline-first, no Tauri, broad armed
  takeover, review-only Workshop, no cloud).
- Changing the sync format in a way that could delete or resurrect data.
- Adding a new network destination, OAuth flow, or remote key storage.
- Publishing, signing, notarizing, or distributing anything.
- Changing the proprietary license, privacy policy, or terms.
- Making a destructive migration without a backup/rollback plan.
- Weakening a security test, approval gate, or audit behavior.
- Granting Accessibility/Screen Recording yourself: a human must approve the OS
  prompt.

If blocked, leave a precise note: what is blocked, what was tried, the exact
command/output, and the decision needed. Do not guess.

### 3.7 Definition of done

**A single task is done when:**

- The behavior is implemented, not just described.
- A focused test covers the happy path and at least one failure/abuse path.
- Full gates pass (or the exact unrelated failure is documented).
- Docs/checklists are updated with evidence.
- No secret/private data was added to the repo.
- No security invariant was weakened.

**Personal v1 is done when:**

- G0–G3 and G5–G6 pass, including the live walkthrough.
- Notes can be created, edited, renamed, deleted, restored, searched, and indexed
  without manual intervention.
- Threadline conversations/settings/memory survive a browser reset and live in
  the vault.
- A fresh Mac with no model gets a clear setup path; no plaintext key remains.
- Capability digest and armed takeover are demonstrable and audited.
- Sync/backup/import/dump pass their loss and failure tests.
- Accessibility and security checks pass; permanent browser tests run in CI.
- README/ROADMAP/DECISIONS/HANDOFF/THREAT_MODEL match reality.

**Full vision is done when:**

- G4's sandboxed, human-approved Workshop apply and rollback work.
- Public v0.1 owner/legal/credential gates are complete.
- Signed artifacts and independent review exist; support and rollback are
  documented.

### 3.8 Commit and branch protocol

- Use `finish/personal-v1` (or one branch per milestone); do not rewrite `main`
  history.
- Commit one coherent slice with a message like
  `feat(vault): add soft delete and restore` or
  `test(browser): add no-model first-run coverage`.
- Do not commit generated build output, browser profiles, screenshots with
  private content, or secrets.
- Push only when the owner asks. If you need review, provide the branch, diff
  summary, commands, and evidence.
- Update `docs/HANDOFF.md` with the new baseline when a milestone lands.

### 3.9 What not to build in this pass

Unless the owner explicitly redirects:

- A Tauri shell or native wrapper.
- Per-action takeover confirmation prompts.
- Automatic code apply/merge.
- Cloud sync, hosted inference, telemetry, or accounts.
- PDF/DOCX/OCR/audio import as a blocker for personal v1.
- Web-search/MCP/API connector marketplace.
- A mobile app.
- A public release without credentials, review, legal, and support.

Those are later products, not completion criteria for the local daily driver.

---

## 4. First actions for the next agent

1. Read `docs/HANDOFF.md`, this document, `DECISIONS.md`, and
   `docs/THREAT_MODEL.md`.
2. Run G0's commands and the live walkthrough; write
   `docs/QA_PERSONAL_V1.md`.
3. Create `finish/personal-v1`.
4. Start with **G1.1 note lifecycle** and **G1.3 workspace persistence**; these
   remove the most likely daily-driver data loss.
5. Then **G2.1 no-model setup**, then **G1.2 auto-index**, then **G3 digest
   v2/takeover audit**.
6. Do **G4 Workshop apply** only after the approval/sandbox design is reviewed;
   it is the highest-risk feature in the plan.
7. Finish with G5, G6, and a fresh live walkthrough.
8. Stop at G7's owner gates; prepare evidence, do not self-authorize release.

The measure of completion is not a longer feature list. It is a build the owner
can open on a fresh Mac, use for a day, close, reopen, and trust with the notes,
the history, and the control session.
