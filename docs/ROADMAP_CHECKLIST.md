# Engram Roadmap Baseline

Baseline verified during Phase 0. This checklist records implementation state,
not a production-readiness claim.

## Implemented and verified

- Rust workspace builds and tests.
- Rust-core loopback inference validation tests pass.
- Crash-safe sync metadata helper and focused tests pass.
- Dump analysis, source-hash validation, create-note apply, partial-result
  reporting, idempotent retry, audit records, and create-note undo tests pass.
- Local Markdown/plain-text/HTML import extraction tests pass; unsupported PDF is
  reported as unsupported.
- Threadline privacy-policy classification tests pass.
- Existing Threadline application actions use the allowlist/argument validation/
  permission gate path.
- Remote Threadline inference is prevented from receiving vault context by the
  current browser policy helper.

## Implemented but not fully verified

- Threadline browser consent and session-key migration: browser runtime and
  CORS behavior need a real browser test suite.
- Windows atomic replacement path: compiled conditionally but not executed on a
  Windows runner in this environment.
- Threadline capability metadata and in-memory audit receipts: no browser
  integration coverage yet.

## Partially implemented

- Dump review/apply: only `create_note` applies; append/link/tag proposals remain
  visible but unsupported. Multi-operation transactions and full undo are not
  available.
- Import pipeline: Markdown/plain text/HTML are local; PDF, DOCX, images, OCR,
  structured data, audio/video, archives, and mail are unsupported.
- Capability broker: existing UI/settings/memory/conversation tools are gated;
  filesystem, web, API, and MCP adapters do not exist.

## Not implemented

- Offline memory/context replay bench.
- User-initiated local model endpoint discovery and compatibility diagnostics.
- Permission center, durable audit retention, capability revocation UI, and
  narrow leases.
- Sandboxed local filesystem tools.
- Reviewed web-search/API/MCP integrations.
- Native packaging, signing, keychain integration, and independent security
  review.

## Blocking security work

- Enforce loopback-only serving in `serve`/`open` rather than relying on CLI
  defaults.
- Make backup restore strictly non-destructive and validate malformed archive
  lengths before slicing.
- Replace one-read HTTP parsing with bounded header/body parsing and timeouts.
- Enforce canonical-root and symlink-safe reads inside `Vault::read_note`.
- Audit remaining direct writes and import collision behavior.
