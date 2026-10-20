# Threadline Personal Agent Design

Status: active design; the first slices are implemented in this repository.
Date: 2026-10-20
Audience: the owner/operator of this installation.

## Intent

Engram is a private, local-first Markdown workspace. Threadline is its front
door: a daily-driver chat and agent surface that can use the vault, run reviewed
local programs, and improve its own code through a gated workshop.

This is a personal installation, not a public service. The design still assumes
untrusted input (synced folders, imported files, model output, application help
text) and fails closed when consent or provenance is missing.

## Non-negotiables

- Markdown remains the source of truth.
- The default model runs on-device, in-process, with no separate model server.
- Network access is opt-in and visible. There is no telemetry and no required
  account.
- The UI is Threadline. The vault is a workspace inside it, not the other way
  around.
- A capability cannot be granted by model output alone. The human approves the
  disclosure, and the server enforces the resulting allow-list.
- GUI control is never armed by the model. Only the human can start a timed
  takeover session, and the session is always stoppable.
- Self-improvement never silently rewrites the running binary.

## Current Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ Threadline browser UI (local, same-origin)                   │
│  chat · memory · rules · applications · takeover · workshop  │
└──────────────────────────┬───────────────────────────────────┘
                           │ HTTP on 127.0.0.1
┌──────────────────────────▼───────────────────────────────────┐
│ Engram process                                                │
│  vault + index · HTTP API · capability digest                 │
│  embedded inference (Apple Foundation Models)                 │
│  takeover sessions + audit · Swift Accessibility helper       │
│  sync/import · backup/restore                                 │
└──────────────────────────────────────────────────────────────┘
```

The shipped shell is the local `engram open` / `engram serve` process with a
WebView-compatible browser UI. There is no Tauri layer: the browser tab is the
presentation, and the Rust process owns every privileged operation. If a native
wrapper is added later, it must call the same server endpoints rather than
creating a second architecture.

### Threadline-first routing

- `/` → Threadline.
- `/vault` → Markdown vault index; notes stay at `/note?path=...`.
- `/threadline/` → the same Threadline app, kept for direct links.
- `engram open` opens `/`, so the first screen is Threadline.

### In-process inference

On macOS 26+ with Apple Intelligence enabled, inference runs in the Engram
process through Apple Foundation Models. The OS owns the model; there is no
model download, API key, localhost model port, or CORS dependency. Engram talks
to the OS model through a small Swift bridge and exposes an OpenAI-compatible
`/v1/chat/completions` endpoint to the same-origin UI. The OpenAI shape is a
convenience for the UI and for optional compatible providers, not a claim that
a server is required.

Other platforms and users who want a larger model can configure an
OpenAI-compatible endpoint. The browser calls it directly; loopback is
preferred, remote endpoints require HTTPS and an explicit disclosure before
vault context is included.

### Capability digest

Threadline can learn a local program in four increasing levels:

1. **Digest** — inspect an absolute executable path or macOS `.app`, run only
   `--help` and `--version` under a timeout, record usage, flags, version,
   bundle metadata, and a SHA-256 hash in
   `.engram/capabilities/<id>.json`.
2. **Invoke** — run only the reviewed actions (`help`, `version`, or `open` for
   an app) with no shell and no model-supplied arguments. Re-hash the
   executable before each run; fail closed if it changed.
3. **Adapt** — (planned) run the program through a typed adapter with a small,
   reviewed argument schema, still without a shell, still with per-invocation
   consent. Adapters are code-reviewed data, not model-generated commands.
4. **Take over** — implemented for macOS. A timed, human-armed session enables
   real Accessibility and CoreGraphics control. This is a separate consent path
   from the digest: inspecting a program never grants control, and the model
   cannot arm a session.

License compliance is part of step 1: the digest records where the program came
from and never copies or redistributes its code. Absorbing an app means learning
its interface, not relicensing its implementation.

### Takeover (macOS)

The `Takeover` view exposes a small Swift helper compiled at build time. It
supports status, app focus, window and accessibility-tree inspection, key
presses, typing, clicks, scroll, drag, named-control press, value setting,
application launch, AppleScript, and screen capture. macOS Accessibility
permission is required; screen capture also needs Screen Recording permission.

The safety model is a session, not a per-step dialog:

- The session is armed by the human in the UI for 1–120 minutes. The server
  returns a random token to that browser tab; the status endpoint never returns
  it. A session armed in another tab cannot be driven by this tab.
- While armed, the `computer_use` tool is approved by the session instead of a
  per-action confirmation. It is offered only to a local OpenAI-compatible
  endpoint with function-tool support, never to a remote endpoint. The built-in
  Apple Foundation Models provider currently supports text chat only.
- The top bar shows a live countdown and a Stop control. `Escape` also stops the
  session. Stopping increments a server-side epoch, clears the token, and
  terminates in-flight helper actions or AppleScript.
- Session arm, stop, expiry, and every action are appended to
  `.engram/takeover/audit.jsonl` with the action, summary, success flag, and
  timestamp. Typed text and field values are recorded by length, not content.
- The local HTTP server handles each connection on its own thread so Stop,
  status, and inference remain responsive while an action is running. On-device
  inference is serialized separately.
- The helper is extracted under `~/Library/Caches/Engram/bin` and is rebuilt if
  its embedded bytes change. On non-macOS builds the feature reports
  unavailable; the rest of Engram still works.

AppleScript is intentionally unrestricted inside an armed session and can run
shell commands. The session, audit, and kill switch are the boundaries. There is
no per-app automation allow-list yet.

### Consent, audit, and privacy

- Host, Origin, and `Sec-Fetch-Site` are validated on every state-changing
  request. Hidden paths, non-Markdown notes, symlink escapes, and traversal are
  rejected server-side.
- Tool effects have fixed policies (`none`, `confirm`). Consequential actions
  open a disclosure with the exact path, action, and data classes involved.
- Capability runs re-check the binary hash. A stale digest is not a permission
  grant.
- Takeover actions require a live session token and are written durably to
  `.engram/takeover/audit.jsonl`. Tool receipts stay in memory for the session.
- Synced folders are untrusted. Import validates every manifest path, rejects
  hidden and non-Markdown entries, canonicalizes the source root, and refuses
  symlink escapes.
- Secrets belong in the OS keychain. The current config still has a legacy
  plaintext key path; removing it is a release blocker for shared machines.
- Export and delete are first-class operations. The vault is always readable as
  plain Markdown, so the user is never locked in.

### Self-improvement workshop (first slice implemented)

The workshop is a gated loop, not autonomous self-modification:

1. Threadline reads an explicit, bounded snapshot of its own source. This is
   implemented: the Workshop view accepts at most three named files, rejects
   hidden/sensitive paths, and enforces a context budget.
2. It proposes a patch or a new adapter in `.engram/workshop/<id>.md`; the live
   tree is untouched. This is implemented.
3. A sandboxed worker runs formatting, `cargo check`, and the relevant tests
   against a copy. Not implemented yet.
4. Threadline shows the diff, test evidence, and risks. The proposal already
   includes intent, files, a suggested diff, tests, and risks.
5. The human approves; only then is the patch applied, and the old state remains
   recoverable. Not implemented yet; proposals are review-only.
6. The app never applies a patch that weakens a security test, disables a
   permission check, or changes its own approval policy without an explicit,
   separate confirmation.

Until the sandbox and apply steps exist, code changes go through the normal
project tools and the repository's tests. The workshop model cannot invoke
tools, run commands, or write to the live tree.

## Desktop shell

The current desktop shell is deliberately boring: a local Rust process plus a
browser UI. Tauri is not required and is not in the dependency tree or the active
plan. The browser tab loads only same-origin assets served by Engram, and every
sensitive operation still happens server-side. This keeps the presentation layer
replaceable without rebuilding the security boundary.

If a native wrapper is ever justified, it should:

- call the same HTTP endpoints and reuse the same session/token model;
- keep the vault/index, inference, and capability execution in the existing
  process boundaries;
- move secrets to the OS keychain;
- add signed, notarized distribution and native consent dialogs.

Those are delivery improvements, not prerequisites for the personal
installation.

## Presentation Checklist

- [x] Threadline is the first screen.
- [x] On-device model runs in-process without a model server.
- [x] Capability digest works end to end for a local CLI and a macOS app.
- [x] Capability runs are confirmed and hash-checked; takeover actions require an
      armed session.
- [x] Host/Origin/CSRF, traversal, hidden-path, and symlink protections are
      covered by tests.
- [x] JavaScript and Rust tests run in CI.
- [x] Armed macOS takeover with Accessibility, audit, persistent Stop, and
      immediate cancellation of in-flight action processes.
- [x] No Tauri dependency; the browser UI is the shipped shell.
- [x] Workshop proposal path with bounded source access (review-only).
- [ ] Workshop sandbox tests and approved apply.
- [ ] Per-app takeover adapters or allow-lists.
- [ ] Native keychain.
- [ ] Durable general audit export and delete review (takeover audit is durable).
- [ ] Accessibility pass on the Threadline shell.
