# Engram Product Decisions

These decisions prioritize the shortest path to a usable, private release.

## Product surface

Engram ships first as a single Rust binary that opens a localhost browser
workspace. This avoids a large native UI dependency and works on macOS, Linux,
and Windows immediately. There is no Tauri shell in the active path. A native
shell may wrap the same local service later without changing vault, inference,
or takeover behavior.

## Data model

Markdown remains the canonical source of truth. The SQLite index, embeddings,
graph data, conversation history, and other metadata are disposable derived
state under `.engram/`. Index rebuilds stream notes from disk instead of
buffering every body. Incremental sync stays out of the codebase until a real
stale-index refresh path exists.

## Import scope

The first import path accepts Markdown, Markdown-like text, and plain text.
Binary files are not copied into the searchable corpus; users may retain them
beside notes and link to them manually. This prevents unsafe or proprietary
binary parsing from becoming a launch blocker.

## Inference

On macOS 26+ with Apple Intelligence enabled, Threadline uses Apple Foundation
Models in-process. There is no model server, API key, or model download. Other
platforms and users who want a larger model can configure an OpenAI-compatible
endpoint; no cloud endpoint is contacted by default.

## First-run initialization

`engram open` initializes an empty vault (config plus a starter note) before
serving. It does not inject a starter note into a folder that already has
visible content; those vaults get an explicit `init` warning instead. `serve`
remains side-effect free apart from derived server state. Initialization is
best-effort so a read-only vault can still use the chat UI.

## First-load probe

Threadline defers the macOS takeover status probe until the browser is idle,
with a bounded timeout, so first paint and embedded-model discovery are not
competed with by Accessibility/app enumeration. Arming or stopping a session
invalidates any older in-flight status response.

## Takeover

macOS GUI takeover is implemented as an explicitly armed, time-boxed session.
The human arms it; the model cannot. While armed, a local OpenAI-compatible
endpoint with function-tool support may use `computer_use` without a prompt for
every step. The built-in Apple model currently supports text chat only, and the
tool is never offered to a remote endpoint. A persistent Stop control and
`Escape` cancel the session and terminate in-flight local action processes.
Actions are appended to `.engram/takeover/audit.jsonl`. The audit keeps its
newest 2,000 entries after the file passes 2 MB, and screenshots are pruned to
the newest 50. The local UI can export or delete both with a local-control
request. This is intentionally broader than the hash-checked capability digest:
the digest is safe intake, while takeover is real control with a visible
boundary.

## Network policy

The workspace binds to localhost only. The crawler rejects network roots.
External inference is an explicit configuration choice and is not part of the
private-local default.

## Portability and recovery

The supported portable path is an encrypted `.engram` backup using Argon2id and
AES-256-GCM, transferred to USB or another local storage device. The current V2
archive uses a compressed binary payload; V1 archives still restore. Restore is
staged, refuses any existing destination, and publishes atomically on Unix.
Stale staging directories are pruned by age, not by PID.

## Release policy

Release binaries are built for Linux, macOS, and Windows by CI. Signing and
notarization are enabled only after the owner supplies platform credentials;
unsigned artifacts are clearly labeled and are not represented as trusted
installers.

## Legal policy

`LICENSE`, `PRIVACY.md`, and `USER_AGREEMENT.md` are owner-provided templates.
The proposed ten-percent royalty is calculated on attributable gross revenue
and applies only where a completed, separate written commercial license makes
it applicable. It does not attach merely from viewing, downloading, or using a
copy without that commercial agreement. Counsel review, owner/contributor
rights review, and jurisdiction selection are required before commercial
distribution.
