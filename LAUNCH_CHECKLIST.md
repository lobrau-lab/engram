# Public Launch Checklist

This checklist is intentionally incomplete until each item has evidence.

## Build and artifacts

- [ ] Linux release artifact built from a clean checkout
- [ ] macOS release artifact built, signed, and notarized
- [ ] Windows release artifact built and signed
- [ ] SHA-256 checksums published for every artifact
- [ ] Reproducible version and release notes recorded

## Functional QA

- [ ] Create, edit, rename, and delete notes
- [ ] Search, backlinks, graph, crawler, and local inference
- [ ] Conversation history and citation verification
- [ ] Encrypted backup and restore with wrong-password failure
- [ ] USB removal and interrupted-write recovery
- [ ] Empty, large, malformed, and non-UTF-8 vault fixtures

## Security QA

- [ ] Independent threat-model review
- [ ] Dependency audit and secret scan
- [ ] Symlink, traversal, archive, and XSS testing
- [ ] Local-only network behavior verified
- [ ] API keys excluded from logs and release artifacts

## Legal and operations

- [ ] Counsel reviews proprietary license and royalty agreement
- [ ] Review ownership/provenance of AI-assisted work, contributor code, and UI assets; obtain any needed written permissions or assignments
- [ ] Add and verify a real user acceptance flow before presenting the draft as binding terms
- [ ] Privacy statement adapted to actual jurisdiction and data flows
- [ ] Support and vulnerability-reporting contact published
- [ ] Rollback and compromised-release procedure documented
