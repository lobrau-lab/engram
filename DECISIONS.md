# Engram Product Decisions

These decisions prioritize the shortest path to a usable, private release.

## Product surface

Engram ships first as a single Rust binary that opens a localhost browser
workspace. This avoids a large native UI dependency and works on macOS, Linux,
and Windows immediately. A native shell may wrap the same local service later
without changing vault or inference behavior.

## Data model

Markdown remains the canonical source of truth. The SQLite index, embeddings,
graph data, conversation history, and other metadata are disposable derived
state under `.engram/`.

## Import scope

The first import path accepts Markdown, Markdown-like text, and plain text.
Binary files are not copied into the searchable corpus; users may retain them
beside notes and link to them manually. This prevents unsafe or proprietary
binary parsing from becoming a launch blocker.

## Inference

Engram uses standard OpenAI-compatible local endpoints. It discovers the first
model advertised by `/v1/models` when no model is configured. No cloud endpoint
is contacted by default, and no model is bundled with the application.

## Network policy

The workspace binds to localhost only. The crawler rejects network roots.
External inference is an explicit configuration choice and is not part of the
private-local default.

## Portability and recovery

The supported portable path is an encrypted `.engram` backup using Argon2id and
AES-256-GCM, transferred to USB or another local storage device. Restore is
staged and refuses to overwrite a non-empty destination.

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
