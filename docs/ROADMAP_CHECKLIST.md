# Engram Roadmap Status

Status reconciled against the current source tree on 2026-10-18. This checklist
records implementation state, not a production-readiness claim.

## Implemented and verified

- Rust workspace builds and tests.
- Rust-core loopback inference validation tests pass.
- Crash-safe sync metadata helper and focused tests pass.
- Dump analysis, source-hash validation, create-note apply, partial-result
  reporting, idempotent retry, audit records, and create-note undo tests pass.
- Local Markdown/plain-text/HTML import extraction tests pass; unsupported PDF is
  reported as unsupported.
- Threadline privacy-policy classification tests pass.
- Threadline prompt packing requires an explicit `vaultContextAllowed`
  authorization in addition to the vault preference. A focused regression test
  verifies denied packing omits a sentinel vault snippet and authorized local
  packing retains it.
- The synthetic memory/context replay bench runs and reports evidence recall,
  irrelevant selection, budget compliance, and detected conflicts. It is a
  deterministic fixture bench, not a general accuracy evaluation.
- Existing Threadline application actions use the allowlist/argument validation/
  permission gate path.
- Loopback-only `serve`/`open`, bounded HTTP headers/bodies with read/write
  timeouts, backup archive length checks with staged restore, and canonical-root
  checks in `Vault::read_note` are present in the current code.
- Threadline browser smoke check passed after adding the missing
  `evaluation-bench.js` static route: Chat/Memory/Bench/Rules/Settings navigation,
  suggestion prefill, vault toggle, and settings save worked. The default
  1,200-token output reserve is now valid in the settings form.
- A synthetic remote endpoint reached the disclosure dialog before any request;
  the request was cancelled. No remote provider was contacted.

## Implemented but not fully verified

- Threadline browser consent and session-key migration: browser runtime and
  CORS behavior need a real browser test suite.
- Browser verification was a manual local smoke check. Automated browser
  coverage for navigation, settings, consent, API key lifetime, and CORS remains
  outstanding; no real model endpoint was tested.
- Windows atomic replacement path: compiled conditionally but not executed on a
  Windows runner in this environment.
- Threadline capability metadata and in-memory audit receipts: no browser
  integration coverage yet.
- Guarded connector manifest validation and proposal→validation→authorization→
  execution→result-validation state-machine seam now have deterministic fake
  connector tests; no real connector or sandbox is implied.

## Partially implemented

- Dump review/apply: only `create_note` applies; append/link/tag proposals remain
  visible but unsupported. Multi-operation transactions and full undo are not
  available.
- Import pipeline: Markdown/plain text/HTML are local; PDF, DOCX, images, OCR,
  structured data, audio/video, archives, and mail are unsupported.
- Capability broker: existing UI/settings/memory/conversation tools are gated;
  filesystem, web, API, and MCP adapters do not exist.

## Not implemented

- User-initiated local model endpoint discovery and compatibility diagnostics.
- Permission center, durable audit retention, capability revocation UI, and
  narrow leases.
- Sandboxed local filesystem tools.
- Durable permission center, connector health UI, and cross-process sandboxing.
- Reviewed web-search/API/MCP integrations.
- Native packaging, signing, keychain integration, and independent security
  review.

## Remaining security and release work

- Enforce provenance-aware remote context isolation: the packer blocks direct
  vault-note snippets, but prior assistant turns derived from a local vault may
  still appear in conversation history, especially when source display was off.
- Audit remaining direct writes and import collision behavior.
- Review the backup restore check/rename race and guarantee no-clobber behavior
  across supported platforms.
- Add real browser coverage for consent, API key lifetime, CORS, app navigation,
  and the remote-vault privacy boundary.
- Complete cross-platform filesystem, packaging, signing, credential-storage,
  and independent security reviews before distribution.
