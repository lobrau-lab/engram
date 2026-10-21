# Threadline frontend snapshot

This directory contains the Threadline browser UI and its modular chat,
conversation-memory, context-packing, application-action, and OpenAI-compatible
provider code. Engram serves these source assets from `/threadline/` and embeds
them into the CLI binary with `include_str!`; keep the relative module paths
intact when editing them.

Conversation state is saved through the same-origin workspace API to
`.engram/threadline/workspace.json`; the browser keeps `localStorage` only as a
fallback and migration source. Engram's Markdown vault and SQLite index remain
on disk. When the Vault toggle is enabled,
Threadline retrieves matching snippets from Engram's same-origin
`/api/v1/vault/search` endpoint and packs selected snippets under its existing
estimated context budget. The inspector and assistant source list link each
selected note to its vault page. Conversation memory and vault retrieval are
independent toggles. The inference provider is called by the browser. Endpoint
URLs are classified with the browser URL API: loopback HTTP is local, remote
endpoints must use HTTPS, and malformed or userinfo endpoints are rejected.
Remote chat requests show a per-request disclosure of destination, key
presence, and assembled data categories before sending. API keys are held in
page-session memory only and removed from legacy local-storage state; reloads
require re-entry. This is not OS keychain protection.

The original design prototype is in the Codex outputs folder. This checked-in
copy is the source used by the Engram binary; changes to it do not automatically
update the prototype copy.

Brand artwork lives in `assets/`. The Engram wordmark is used in the sidebar,
the symbol-only mark is used for the favicon and assistant avatar, and the
provided color variants are available from Connections & settings. PNG files
are embedded in the CLI binary and served from `/threadline/assets/`.
