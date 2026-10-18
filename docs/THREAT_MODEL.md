# Engram Threat Model

## Security boundary

Engram is local-first. The vault, index, crawler, backups, and inference
prompts are intended to remain on the user’s device. The `serve` and `open`
commands bind to `127.0.0.1` by default; exposing them on another interface
is not supported as a secure deployment mode.

## Assets

- Markdown source files and imported documents
- SQLite index and relationship metadata
- Encrypted backup archives
- Inference prompts and model responses
- Backup passwords and LLM API keys

## Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| Vault path traversal | Relative-path validation and canonical-root checks | Symlink behavior on unusual filesystems requires platform testing |
| Malicious backup archive | Authenticated AES-GCM payload, Argon2id password derivation, staged restore | Password strength remains the user’s responsibility |
| Corrupt or wrong-password backup | GCM authentication failure before restore | No recovery is possible without the password |
| Accidental overwrite | Restore refuses non-empty destinations | Users can still delete files manually |
| Network data leakage | Localhost-only default and local crawler rejection of URLs | Users can explicitly configure external inference endpoints |
| Prompt injection in notes | Notes are treated as untrusted context and cited by ID | Models may still produce misleading output |
| XSS in notes | Browser preview escapes Markdown text | Future rich-rendering features must preserve this invariant |
| Unauthorized repository access | Proprietary license and access controls outside the binary | Repository hosting configuration is operational responsibility |

## Release gates

Before public distribution, maintainers must complete:

1. Independent review of cryptographic and restore code.
2. macOS, Linux, and Windows filesystem and browser tests.
3. Secret scanning and dependency vulnerability review.
4. Verification that release artifacts contain no vault, database, API key, or
   development files.
5. Legal review of `LICENSE`, `PRIVACY.md`, and `USER_AGREEMENT.md`.
