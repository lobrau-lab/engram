# Engram / Threadline Handoff

**Last updated:** 2026-10-20  
**Repository:** `lobrau-lab/engram`  
**Branch:** `main`  
**Last functional commit:** `ab99c1d feat: initialize vaults on open and defer takeover probe`  
**Status:** personal presentation-ready; not public-v0.1 ready.

This report is for the next agent picking up the project. Read this first, then
`README.md`, `DECISIONS.md`, `docs/THREAT_MODEL.md`, and
`docs/superpowers/specs/2026-10-20-threadline-personal-agent.md`.

## TL;DR

Engram is a local-first Markdown second brain with Threadline as its front door.
Threadline is a browser UI served by the same local Rust process; there is no
Tauri shell, separate frontend build, model server, or cloud dependency in the
active path.

The personal demo path is complete:

- `engram open` starts the workspace and opens Threadline.
- On macOS 26+, the built-in Apple Foundation Models model is detected and used
  in-process for chat.
- An empty vault is initialized automatically by `open` (config + `Welcome.md`).
- macOS GUI takeover works through an explicitly armed, time-boxed session with
  Accessibility, a persistent Stop control, Escape, audit, and cancellation.
- Local CLI tools and macOS `.app` bundles can be digested into reviewed,
  hash-checked capability actions.
- Workshop can draft self-improvement proposals, but cannot apply them.
- Full Rust tests, all JS tests, clippy, formatting, release build, and a
  50-run first-run simulation passed.

The remaining work is mostly release/hardening work, plus one final human
walkthrough. See [What is left](#what-is-left).

## Do not regress these decisions

1. **Threadline is the first screen.** `/` redirects to `/threadline/`; `/vault`
   remains the Markdown vault.
2. **No Tauri.** The shipped shell is the local Rust process plus the browser.
   Reintroducing a native wrapper is a product decision, not a bug fix.
3. **Takeover is intentionally broad once armed.** The human arms a 1–120 minute
   session; per-action confirmation prompts are bypassed while armed. Do not add
   per-action prompts or narrow the GUI control surface without owner approval.
   Stop, Escape, expiry, and the audit log are the boundary.
4. **Self-improvement is review-only.** Workshop proposals cannot be applied
   automatically and cannot weaken tests or consent checks.
5. **Remote inference endpoints never receive `computer_use`.** The tool is
   exposed only to local tool-capable endpoints.
6. **Sync folders are untrusted.** Keep traversal, symlink, hidden-file, and
   extension protections.
7. **Secrets are not workspace settings.** Provider API keys stay in memory for
   the page session; do not persist them to `localStorage` or Markdown.
8. **No cloud by default.** The workspace binds to localhost only and validates
   Host/Origin/Sec-Fetch-Site on state-changing requests.

## Run it

From the repository root:

```sh
cargo run --release -- open --project-root .
```

`--project-root .` is required for Threadline's Workshop source-reading tools.
Without it, everything except Workshop works. Use `serve` if you do not want the
browser opened:

```sh
cargo run --release -- serve --project-root .
```

`serve` does not initialize an empty vault. `open` does, but only when the vault
has no config and no visible content. If the vault already contains notes but
has no config, `open` prints an explicit `init` warning and does not add a
starter note.

Useful CLI commands:

```sh
./target/release/engram --vault /path/to/vault init
./target/release/engram --vault /path/to/vault index
./target/release/engram --vault /path/to/vault search "query"
./target/release/engram --vault /path/to/vault serve --project-root .
./target/release/engram --vault /path/to/vault open --project-root .
```

`--vault` and `--config` are global flags and can appear before or after the
subcommand. `--port 0` binds an ephemeral port; the server prints
`Engram workspace: http://...`.

## Current architecture

```text
crates/
  engram-core/             Markdown vault, SQLite FTS index, AI client,
                           embedded Apple FM bridge, capabilities, backup,
                           sync, dump/import, crawler, atomic writes
  engram-cli/              CLI, local HTTP server, takeover, native helper,
                           embedded Threadline web assets
    src/main.rs            CLI commands, vault init, open auto-init, tests
    src/serve.rs           localhost-only HTTP API, Threadline routes, limits
    src/takeover.rs        armed sessions, tokens, audit, actions, screenshots
    native/control.swift   macOS Accessibility/CoreGraphics helper
    build.rs               compiles/embeds the Swift helper and fallback
    web/index.html         Threadline shell
    web/src/app.js         chat UI, views, takeover UI, tool callbacks
    web/src/core/          store, context packer, memory, tools, permissions,
                           agent execution, privacy policy, evaluation
    web/src/providers/     OpenAI-compatible client, Engram vault provider
    web/tests/*.test.mjs   standalone Node tests
docs/
  THREAT_MODEL.md
  superpowers/specs/2026-10-20-threadline-personal-agent.md
  ROADMAP_CHECKLIST.md
  ENGRAM_OFFLINE_BOUNDARY.md
  ENGRAM_METADATA_WRITES.md
```

Key boundary: `engram-core` has no UI dependency. `engram-cli` owns the local
server and native integration. Threadline assets are embedded in the binary;
after editing `crates/engram-cli/web/`, rebuild before testing the running binary.

## The main flows

### Threadline chat

- First load fetches `/api/v1/inference/embedded`.
- On macOS 26+ with Apple Intelligence, the embedded model auto-selects as
  `apple-on-device` and the base URL becomes the current local origin + `/v1`.
- Chat goes to same-origin `/v1/chat/completions`; embedded inference is
  serialized behind a process lock.
- The embedded Apple model is text-only. It cannot call `computer_use`.
- Optional OpenAI-compatible local endpoints can provide larger models and
  tool calling. Remote endpoints are filtered so they cannot receive
  `computer_use`.

### First-run initialization

`Cmd::Open` calls `ensure_open_vault` before `serve::open`:

- Existing config: no-op.
- Missing config + visible content: warning, no writes, server still starts.
- Missing config + empty vault (hidden files such as `.DS_Store` ignored):
  calls `cmd_init`, creating `.engram/config.toml` and `Welcome.md`.
- Initialization failure (for example read-only vault): warning, server still
  starts without a config.

`serve` deliberately remains explicit and does not auto-initialize.

### Takeover

- `Takeover` holds sessions in the server process. The human arms via
  `POST /api/v1/takeover/arm`; the model cannot arm.
- Session tokens live only in browser/server memory. Status responses never
  return the token. Stop works without it.
- Actions require `X-Threadline-Local-Control: 1`, the armed token, and an
  unexpired session. Expiry is recorded in the durable audit log.
- Audit: `.engram/takeover/audit.jsonl`.
- Screenshots: `.engram/takeover/screenshots/`; directories are `700`, files
  `600`.
- Audit summaries do not store typed text, field values, or AppleScript source.
- The helper is extracted to
  `~/Library/Caches/Engram/bin/engram-control`, is unsigned, and may need
  Accessibility re-approval after a rebuild. Screenshots also need Screen
  Recording permission.
- `pagehide` sends a best-effort disarm. If it is lost, the session expires.
- The startup status probe is deferred with `requestIdleCallback` (1 second
  timeout; 500 ms fallback). Arming or stopping increments `takeoverRequest` so
  older in-flight status responses cannot overwrite newer state.

### Capability digest and Workshop

- `engram-core/capability.rs` digests CLI executables and macOS `.app` bundles
  with SHA-256 verification and reviewed actions.
- Applications UI lists digests and runs only reviewed actions.
- Workshop reads bounded source files under the granted project root and writes
  review-only proposals under `.engram/workshop/`. It does not apply changes,
  run commands, or weaken tests/consent.

## Verification snapshot

Last full verification after the latest functional commits:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
for f in crates/engram-cli/web/tests/*.test.mjs; do node "$f" || exit 1; done
```

Results:

- Full workspace tests passed: 24 CLI unit tests, 4 core unit tests, all
  integration tests.
- All 7 JS test files passed. There is no `package.json`; run the `.mjs` files
  directly with Node.
- Release build passed.
- Real macOS control was previously verified: Finder focus, Calculator
  `7 + 3 = 10`, AppleScript `6 * 7 = 42`, Unicode/newline typing, and a valid
  screenshot.
- Concurrent Stop was verified: a 15-second AppleScript canceled in about 1.5
  seconds with no leftover `osascript`.
- Prior desktop and 390 px mobile Playwright checks passed with no console
  errors and no horizontal overflow.
- Latest change re-verified with 10/10 first-run Playwright runs: no console or
  page errors, and the deferred takeover status probe fired in every run.

### First-run simulation data

The harness was a temporary Node/Playwright script, not committed. It used a
fresh temp vault, a fresh browser context with empty `localStorage`, an
ephemeral port, the release binary, and one real on-device completion. The main
batch was 50 runs: 40 `init` + `serve`, 10 `serve` only.

| Metric | p50 | p90 | Notes |
|---|---:|---:|---|
| `init` | 18.8 ms | 19.5 ms | first-ever init was 359.5 ms |
| Server boot | 8 ms | 9 ms | binds in under 10 ms |
| HTML DOMContentLoaded | 96 ms | 104 ms | ~8.5 KB document |
| Model auto-selected | 115 ms | 124 ms | ~22 ms after DOM ready |
| First reply | 693 ms | 731 ms | cold runs 1.51–1.56 s; first-ever 2.05 s |
| Load + first reply | 813 ms | 859 ms | cold outliers 1.64–1.70 s |
| Server RSS | 21.6 MiB | 21.7 MiB | flat |
| Shutdown | 2 ms | 3 ms | clean SIGTERM |

Reliability: 50/50 succeeded. 950 HTTP requests, all 200; zero console errors,
page errors, or failed requests. All 50 selected `apple-on-device`, created one
conversation, and returned exactly `ready`.

Post-change verification was faster (warm model: chat p50 ~345 ms) but that is
not a controlled A/B; do not attribute it to the probe deferral.

## What is left

### Before the personal demo

1. **Do one human walkthrough on the current build.** Open the app, chat once,
   open Takeover, arm for 1 minute, focus an app, take a screenshot, press Stop,
   and inspect an application digest. This is the only remaining demo step.
2. **Optional:** test the model-unavailable first-run path. The UI handles
   `available: false`, but the latest simulation only covered the available
   model.

### Product hardening

- Native macOS Keychain for provider keys. Keys currently stay in memory for the
  page session.
- Per-app takeover adapters or an AppleScript allow-list. AppleScript is
  intentionally unrestricted inside an armed session today.
- General audit export/delete review. Takeover audit is durable; the rest is not.
- Accessibility pass on the Threadline shell.
- Workshop sandbox tests and an approved apply path. Apply is intentionally not
  implemented yet.
- Sync hardening: tombstones/deletion propagation, per-entry/per-device Lamport
  counters, simultaneous-export race handling, and not indexing conflict files
  as normal notes.
- English-only stopword handling; device identity is derived from host + vault
  path, so moving a vault changes the device id.

### Public v0.1 release

Not required for the personal demo:

- Linux and Windows release artifacts, macOS signing/notarization, checksums,
  reproducible release notes.
- Independent threat-model review, dependency audit, secret scan, and broader
  XSS/archive testing.
- Signed/notarized Accessibility helper, or an explicit unsigned-helper policy.
- Legal/privacy review, support and vulnerability-reporting contacts, rollback
  procedure.

See `LAUNCH_CHECKLIST.md` and README "Known gaps" for the full list.

## Testing notes and gotchas

- `open` launches the system browser. For headless tests, prepend a fake `open`
  executable to `PATH`; it receives the URL as an argument. Allow a short delay
  after the server prints the URL before asserting the browser was invoked.
- Use `--port 0` in automation and parse `Engram workspace: http://...`.
- The release binary embeds Threadline assets. Rebuild after changing
  `crates/engram-cli/web/`.
- `--project-root .` is needed for Workshop. It is canonicalized at startup.
- The helper is macOS-only. Missing `xcrun`/Swift or missing Accessibility
  permission should fail closed in the UI, not crash the server.
- Do not add per-action takeover prompts; the armed-session boundary is the
  product design.
- Keep the server localhost-only. Do not add remote bind defaults.
- The branch was clean and pushed at handoff. Check `git status` and
  `git log --oneline -5` before starting.
