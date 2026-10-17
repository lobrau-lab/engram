# Engram Desktop Design

## Intent

Engram is a private, local-first replacement for a Markdown knowledge
workspace. Markdown files remain the source of truth; the application adds
navigation, indexing, local inference, relationship discovery, encrypted
portable backups, and safe recovery around them.

## Goals

- Desktop support for macOS and Linux first, with Windows compatibility.
- A usable Markdown editor, vault navigation, search, backlinks, tags, graph
  view, command palette, and keyboard-first workflows.
- On-device-only operation by default, with no telemetry, cloud account, or
  required network service.
- Automatic discovery of local OpenAI-compatible inference servers and a
  provider-neutral adapter boundary for future native runtimes.
- A local crawler for Markdown and explicitly permitted local document roots,
  producing searchable text, relationships, provenance, and citations.
- Encrypted vault backups and portable USB operation with integrity checks,
  recovery, and conflict-safe import/export.
- AI answers grounded in local sources, with citations and explicit handling of
  unsupported claims.

## Non-goals for the first desktop release

- No mandatory cloud synchronization or hosted inference.
- No unrestricted web crawler; network access must be an explicit opt-in.
- No silent deletion, silent conflict resolution, or opaque background upload.
- No plugin marketplace or arbitrary third-party code execution.

## Security and privacy requirements

- Network access is disabled by default and surfaced visibly when enabled.
- Vault paths and indexed content never leave the device unless the user
  explicitly exports them.
- Secrets are redacted from logs and stored using platform secure storage where
  available.
- Encrypted backups use an authenticated encryption design with a documented
  key-derivation policy; password material is never persisted in plaintext.
- Imports validate paths, reject traversal, verify hashes, and write through a
  temporary staging area before replacement.
- AI context includes source identifiers and never treats model output as a
  source of truth.

## Architecture

The existing `engram-core` crate remains UI-independent and owns vault access,
indexing, embeddings, inference protocols, crawling, encryption, and sync. A
new desktop application crate provides the cross-platform shell and exposes a
narrow command/query API to a web-based UI. The CLI remains a supported
headless interface and shares the same core services.

The desktop shell should use a Rust-native cross-platform WebView approach
(Tauri or an equivalent reviewed option), but the first implementation task
must validate packaging and local-only behavior before committing to a large UI
framework dependency set.

## Phases

1. Core contracts and security boundaries.
2. Desktop shell and note workspace.
3. Local crawler, relationship extraction, and citations.
4. Universal on-device inference discovery and grounded assistant UI.
5. Encrypted USB backup, portable mode, recovery, and conflict UX.
6. Cross-platform packaging, threat-model review, and release hardening.
