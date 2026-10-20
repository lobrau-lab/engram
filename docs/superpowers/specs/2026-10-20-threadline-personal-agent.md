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
- Self-improvement never silently rewrites the running binary.

## Current Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ Tauri desktop shell (planned)                                │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ Threadline WebView                                    │  │
│  │  chat · memory · rules · applications · workshop      │  │
│  └──────────────────────────┬─────────────────────────────┘  │
│                             │ same-origin HTTP / IPC         │
│  ┌──────────────────────────▼─────────────────────────────┐  │
│  │ Engram process                                        │  │
│  │  vault + index · HTTP API · capability digest         │  │
│  │  embedded inference (Apple Foundation Models)         │  │
│  │  sync/import · backup/restore · audit                  │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

The current shipped shell is the local `engram open` / `engram serve` process
with a WebView-compatible browser UI. The same process boundaries carry over to
the Tauri shell: UI, vault/index, inference, and capability execution stay
separate modules, and sensitive actions remain server-side.

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
4. **Take over** — (deliberately out of scope for now) GUI automation or
   session takeover. It requires a separate threat model, per-app accessibility
   permissions, and a visible stop control.

License compliance is part of step 1: the digest records where the program came
from and never copies or redistributes its code. Absorbing an app means learning
its interface, not relicensing its implementation.

### Consent, audit, and privacy

- Host, Origin, and `Sec-Fetch-Site` are validated on every state-changing
  request. Hidden paths, non-Markdown notes, symlink escapes, and traversal are
  rejected server-side.
- Tool effects have fixed policies (`none`, `confirm`). Consequential actions
  open a disclosure with the exact path, action, and data classes involved.
- Capability runs re-check the binary hash. A stale digest is not a permission
  grant.
- Audit receipts are kept in memory for the session and written to the vault
  only when a durable audit log is enabled.
- Synced folders are untrusted. Import validates every manifest path, rejects
  hidden and non-Markdown entries, canonicalizes the source root, and refuses
  symlink escapes.
- Secrets belong in the OS keychain. The current config still has a legacy
  plaintext key path; removing it is a release blocker for shared machines.
- Export and delete are first-class operations. The vault is always readable as
  plain Markdown, so the user is never locked in.

### Self-improvement workshop (planned)

The workshop is a gated loop, not autonomous self-modification:

1. Threadline reads an explicit, bounded snapshot of its own source.
2. It proposes a patch or a new adapter in `.engram/workshop/<id>/`; the live
   tree is untouched.
3. A sandboxed worker runs formatting, `cargo check`, and the relevant tests
   against a copy.
4. Threadline shows the diff, test evidence, and risks.
5. The human approves; only then is the patch applied, and the old state remains
   recoverable.
6. The app never applies a patch that weakens a security test, disables a
   permission check, or changes its own approval policy without an explicit,
   separate confirmation.

Until the workshop exists, code changes go through the normal project tools and
the repository's tests.

## Tauri Migration

The desktop shell is a delivery detail, not a second architecture. The migration
keeps the tested server-side boundaries and replaces the browser tab with a
Tauri window:

- Tauri commands call the same Rust modules; the HTTP server remains available
  only as a locked-down headless fallback.
- The WebView loads Threadline first.
- Secrets move to the OS keychain.
- The workshop and capability adapters get native dialogs for consent.
- Release builds are signed, notarized where applicable, and shipped with
  checksums.

## Presentation Checklist

- [x] Threadline is the first screen.
- [x] On-device model runs in-process without a model server.
- [x] Capability digest works end to end for a local CLI and a macOS app.
- [x] Every capability run is confirmed and hash-checked.
- [x] Host/Origin/CSRF, traversal, hidden-path, and symlink protections are
      covered by tests.
- [x] JavaScript and Rust tests run in CI.
- [ ] Tauri window and native keychain.
- [ ] Workshop with isolated test execution and diff approval.
- [ ] Durable audit log and one-click export/delete review screen.
- [ ] Accessibility pass on the Threadline shell.
