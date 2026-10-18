# Engram Offline Inference Boundary

## Intended boundary

Engram’s Rust core is local-first. Vault Markdown, the SQLite index, hash
embeddings, Dump inbox records, crawler inputs, backups, prompts, model
responses, and credentials are intended to remain on the user’s device.
Engram does not provide telemetry, remote model discovery, update checks, or
cloud synchronization.

The Rust inference client uses HTTP only for a configured local inference
sidecar. The current architecture therefore permits loopback sockets; it does
not claim literal zero network or socket activity.

## Enforced by this change

`engram-core::ai::validate_local_endpoint` is the shared policy for the Rust
OpenAI-compatible client. It runs before both inference and `/models` discovery
requests.

Allowed endpoint forms:

- `http://localhost:8080/v1`
- `http://127.0.0.1:1234/v1`
- `http://[::1]:8080/v1`

The port and path prefix are configurable. The validator rejects HTTPS,
non-loopback IP addresses, arbitrary DNS names, malformed URLs, and userinfo.
It does not resolve DNS names to decide whether they happen to map to loopback.
Invalid configuration fails locally before a ureq request is created or sent.

The crawler separately rejects `http://` and `https://` roots. Localhost
workspace serving is inbound browser traffic and is distinct from outbound
inference traffic.

## Not covered by this boundary

- Threadline’s browser-side provider can call its configured endpoint directly;
  this change does not secure that path or make it offline-only.
- Operating-system tools, browser extensions, shell commands, GitHub Actions,
  Cargo, package downloads, and other build/runtime tooling may use networks.
- A user may explicitly configure a non-Engram browser provider in Threadline;
  no vault-content isolation broker exists yet for that capability.
- A local sidecar may itself make network requests; Engram cannot control the
  sidecar’s implementation.

## Literal zero-network operation

Literal zero-network operation would require replacing loopback HTTP with an
embedded inference runtime or a local IPC mechanism whose transport and
permissions are separately controlled. That is a different architecture and
is not silently substituted here.

## Threadline online tools

Allowing Threadline to use online models or tools safely requires an explicit
capability broker. The broker would need per-request consent, source/data
classification, destination disclosure, redaction, audit records, and a hard
separation between vault retrieval and online tool payloads. Until that exists,
this Rust-core guard must not be described as securing the browser path.

## Data handling rule

Errors and diagnostics must not include note contents, prompts, model output,
API keys, or other user data. Model output remains untrusted and does not gain
filesystem, shell, email, or application-control permissions.
