> **🔒 PRIVATE & PROPRIETARY**
>
> **Copyright owner:** Tk (GitHub: lobrau-lab)<br>
> **Contact:** lordlobrau@gmail.com
>
> This repository contains private and proprietary software owned by Tk. No
> license, right, title, or interest is granted by access to, possession of,
> or inspection of this repository. No person may view, fork, clone, modify,
> distribute, or use the Work absent the Owner’s prior express written
> authorization and satisfaction of any applicable payment or royalty terms.
> Unauthorized access or use may give rise to all remedies available at law.

The draft commercial terms are in [`USER_AGREEMENT.md`](USER_AGREEMENT.md).
The proposed 10% share is based on attributable gross revenue and applies only
when a completed separate commercial license makes it applicable. The draft
is not an executed agreement, and the app does not yet implement a terms
acceptance or commercial licensing flow. The current draft defines Gross
Revenue as receipts attributable to the licensed use, less refunds and sales
taxes only—not net profit—unless the signed schedule says otherwise.

Release requirements are tracked in [`LAUNCH_CHECKLIST.md`](LAUNCH_CHECKLIST.md)
and the security boundary is documented in [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md).
Product defaults and low-friction implementation decisions are documented in
[`DECISIONS.md`](DECISIONS.md).

# Engram

Local-first, AI-native "second brain" MVP.

- **Markdown** files are the source of truth.
- **Rust** CLI (single `engram` binary).
- **SQLite FTS5** full-text index.
- **Hash embeddings** + brute-force cosine (no ML at build time).
- **Built-in on-device inference** on macOS 26+ through Apple Foundation Models:
  no separate model server, API key, or model download.
- **Optional OpenAI-compatible** endpoints (MLX-Serve / oMLX / vLLM / llama.cpp)
  for other platforms or larger models.
- **Folder-based** last-write-wins sync with conflict files.

The local workspace runs on macOS, Linux, and Windows. It binds to localhost
only by default and can be started with `engram open`.

## Threadline

Threadline is the front door. `engram open` opens the Threadline workspace at
`/`; the Markdown vault remains available at `/vault`. Threadline is a
local-first chat and agent workspace whose source snapshot lives under
`crates/engram-cli/web/`. Engram embeds those assets in the binary, so there is
no separate frontend build or model server.

On macOS 26+ with Apple Intelligence enabled, Threadline uses the operating
system's Foundation Models model **inside the Engram process**. There is no
API key, no model download, and no loopback model server. The app detects the
built-in model on first launch and selects it automatically. **Connections &
settings** shows its readiness and lets you switch back to it at any time.

Threadline conversations, settings, and derived conversation memory remain in
browser `localStorage`; Engram notes and the SQLite index remain in the vault.
The **Vault** composer toggle queries Engram's same-origin read-only search API
and packs selected note snippets into the request alongside conversation
context. The context inspector shows which notes were packed and links back to
their source. Run `engram index` after changing notes. Context token counts
remain estimates.

Optional OpenAI-compatible endpoints remain supported. The browser calls the
configured endpoint directly, so a loopback endpoint is preferred; remote
endpoints require HTTPS and an explicit disclosure before any context is sent.
On macOS, **Connections & settings** also has an optional oMLX service manager
for fixed `start`, `stop`, and `restart` actions. It does not install software
or download models, and it is not required for the built-in path.

### Capability digest

**Applications** teaches Threadline about a local program. Give it an absolute
path to an executable or a macOS `.app` bundle. Threadline records the
program's version, usage line, and flags in
`.engram/capabilities/<id>.json`, along with a SHA-256 hash of the executable.

The digest can run only the actions reviewed during inspection (`--help`,
`--version`, or opening an app bundle). It never uses a shell, accepts no
arbitrary arguments from the model, re-checks the hash before every run, and
asks for confirmation each time. A replaced binary fails closed until it is
inspected again. This is the safe foundation for teaching Threadline new
applications; broader GUI takeover is intentionally not enabled.

## Layout

```text
engram/
├── Cargo.toml
├── .github/workflows/ci.yml
└── crates/
    ├── engram-core/   # config, vault, index, embed, ai, sync
    └── engram-cli/    # the `engram` binary
```

## Build

```sh
cargo check --workspace --all-targets
cargo build --workspace --release

# Open the local browser workspace
./target/release/engram --vault /path/to/vault serve

# Start and open the workspace automatically
./target/release/engram --vault /path/to/vault open

# Encrypted portable backup (password is never passed on the command line)
export ENGRAM_BACKUP_PASSWORD='use a strong password'
./target/release/engram --vault /path/to/vault backup /path/to/backup.engram
./target/release/engram --vault /path/to/restored restore /path/to/backup.engram

# Import local Markdown/text files into the vault
./target/release/engram --vault /path/to/vault import-files /path/to/files
```

## Smoke test

```sh
BIN=./target/release/engram
rm -rf /tmp/brain /tmp/sync
$BIN --vault /tmp/brain init
$BIN --vault /tmp/brain index
$BIN --vault /tmp/brain search "welcome"
$BIN --vault /tmp/brain search "who is who"
$BIN --vault /tmp/brain export /tmp/sync
```

## Known gaps (v0.1)

- No deletion propagation (no tombstones yet).
- Lamport counter is global, not per-entry/per-device.
- Shared manifest can race if two devices export simultaneously.
- Conflict files are `.md`, so `engram index` indexes them as real notes.
- Stopword list is English-only.
- The LLM endpoint and model are configurable; the client sends only portable
  OpenAI Chat Completions fields.
- Device id is derived from host + vault path, so mounting a vault at a new
  path yields a new identity.
