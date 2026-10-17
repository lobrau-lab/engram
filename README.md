> **🔒 PRIVATE & PROPRIETARY**
>
> **Owner:** Tk (lobrau-lab)  
> **Contact:** lordlobrau@gmail.com
>
> This repository contains private and proprietary software owned by Tk. No
> license, right, title, or interest is granted by access to, possession of,
> or inspection of this repository. No person may view, fork, clone, modify,
> distribute, or use the Work absent the Owner’s prior express written
> authorization and satisfaction of any applicable payment or royalty terms.
> Unauthorized access or use may give rise to all remedies available at law.

# Engram

Local-first, AI-native "second brain" MVP.

- **Markdown** files are the source of truth.
- **Rust** CLI (single `engram` binary).
- **SQLite FTS5** full-text index.
- **Hash embeddings** + brute-force cosine (no ML at build time).
- **OpenAI-compatible** LLM sidecar (MLX-Serve / oMLX / vLLM / llama.cpp / hosted APIs).
  If `llm_model` is empty, Engram discovers the first model from `/v1/models`.
- **Folder-based** last-write-wins sync with conflict files.

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

# Encrypted portable backup (password is never passed on the command line)
export ENGRAM_BACKUP_PASSWORD='use a strong password'
./target/release/engram --vault /path/to/vault backup /path/to/backup.engram
./target/release/engram --vault /path/to/restored restore /path/to/backup.engram
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
