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
- macOS Accessibility tree, screen captures, and takeover audit logs
- Takeover session tokens (held in the browser tab and server memory)

## Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| Vault path traversal | Relative-path validation and canonical-root checks inside `Vault::read_note` | Symlink behavior on unusual filesystems requires platform testing |
| Malicious backup archive | Authenticated AES-GCM payload, Argon2id password derivation, staged restore | Password strength remains the user’s responsibility |
| Corrupt or wrong-password backup | GCM authentication failure before restore | No recovery is possible without the password |
| Accidental overwrite | Restore refuses any existing destination and stages extracted files before rename | Destination check and rename are separate operations; no-clobber race behavior needs cross-platform review |
| Network data leakage | Rust `serve`/`open` validate loopback binds; Rust inference validates local endpoints; direct Threadline vault snippets require explicit local-context authorization at prompt packing; remote requests require disclosure consent | Prior assistant turns derived from vault content may still be included in remote conversation history; browser integration needs automated coverage |
| Prompt injection in notes | Notes are treated as untrusted context and cited by ID | Models may still produce misleading output |
| Unauthorized GUI control | Takeover is off by default; arming requires a human UI action, a local-control header, and a random session token; the model cannot arm a session; Stop increments an epoch and terminates in-flight action processes | Any process with the user’s local privileges can invoke the extracted helper directly; the helper is not a security boundary against a compromised user account |
| Model-driven destructive GUI actions | `computer_use` is offered only to a local OpenAI-compatible endpoint with function-tool support and only executes inside an armed, time-boxed session; every action is audited | A local model can still take harmful actions while the user has armed a session |
| Screen capture leakage | Captures stay under `.engram/takeover/screenshots`; the UI serves them same-origin with random filenames; Screen Recording permission is required | Local filesystem readers and anyone with the user’s session can access the captures; no encryption at rest |
| Takeover audit tampering | Audit entries are appended locally and shown in the UI | The file is user-writable; there is no cryptographic integrity or remote attestation |
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

The 2026-10-18 manual browser smoke check verified Threadline navigation,
settings save, vault toggle, and a remote disclosure dialog using a synthetic
endpoint that was cancelled before transmission. Automated browser, CORS, and
real-provider integration coverage remain release gates.
