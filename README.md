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
- **Armed macOS takeover** through Accessibility and CoreGraphics, with a timed
  session, a persistent stop control, and a local audit log.
- **Folder-based** last-write-wins sync with conflict files.

The local workspace runs on macOS, Linux, and Windows. It binds to localhost
only by default and can be started with `engram open`.

## Threadline

Threadline is the front door. `engram open` opens the Threadline workspace at
`/`; the Markdown vault remains available at `/vault`. Threadline is a
local-first chat and agent workspace whose source snapshot lives under
`crates/engram-cli/web/`. Engram embeds those assets in the binary, so there is
no separate frontend build or model server. The running app is the local Rust
process plus the browser UI; there is no Tauri shell in the current path. A
native wrapper can be added later without changing the server-side boundaries.

On macOS 26+ with Apple Intelligence enabled, Threadline uses the operating
system's Foundation Models model **inside the Engram process**. There is no
API key, no model download, and no loopback model server. The app detects the
built-in model on first launch and selects it automatically. **Connections &
settings** shows its readiness and lets you switch back to it at any time.
The built-in provider currently supports text chat, not function-tool calls.

Threadline conversations, settings, and derived conversation memory remain in
browser `localStorage`; Engram notes and the SQLite index remain in the vault.
The **Vault** composer toggle queries Engram's same-origin read-only search API
and packs selected note snippets into the request alongside conversation
context. The context inspector shows which notes were packed and links back to
their source. Edits made in the web vault update the index immediately; run
`engram index` after changing Markdown files outside the web UI. Context token
counts remain estimates.

The Vault view supports creating and editing notes, renaming them, and moving
them to a recoverable trash at `.engram/trash/`. Restore is available from the
Trash view. Renames leave wiki links untouched and show broken links on the note
instead of silently rewriting them.

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
inspected again. This is the safe intake path; it does not grant control by
itself.

### Takeover (macOS)

**Takeover** is Threadline's broader GUI-control surface. A small Swift helper,
compiled at build time with `xcrun swiftc`, bridges macOS Accessibility and
CoreGraphics. Threadline can inspect the accessibility tree, list windows,
focus an application, press keys, type, click, scroll, drag, click a named
control, set a field value, open an app, run AppleScript, and capture the
screen.

Control is off by default. Open **Takeover**, grant Accessibility (and Screen
Recording if you want captures), choose a session length from 1 to 120 minutes,
and press **Arm takeover**. While the session is armed, a local
OpenAI-compatible endpoint that supports function tools may invoke the
`computer_use` tool without a prompt for every step. The built-in Apple model is
currently text-only, so use a compatible local endpoint for model-driven
takeover. The top bar shows a live countdown and **Stop** button; pressing
`Escape` also stops the session. Bounded request workers let Stop terminate an
in-flight helper action or AppleScript. Session arm, stop, expiry, and every
action are appended to `.engram/takeover/audit.jsonl`.

The `computer_use` tool is only offered to a local inference endpoint that
supports function tools. A remote endpoint never receives it. The helper is
extracted under `~/Library/Caches/Engram/bin`; macOS may ask for Accessibility
permission again after a rebuild. The rest of Engram keeps working if the helper
cannot be built.

This is real control: AppleScript can automate applications and can run shell
commands. The session boundary, audit log, and kill switch are the safety model,
not a claim that arbitrary automation is harmless. Only arm a session while you
are present and intend to let the agent act.

### Workshop

**Workshop** drafts changes to Threadline's own source without applying them.
Start the server with `--project-root .`, name up to three source files, and
describe a goal. The built-in model writes a proposal (intent, files, suggested
diff, tests, risks) into `.engram/workshop/`. Hidden and sensitive paths are
refused, the selected files are bounded, and the model cannot run commands or
apply a patch. A human reviews the proposal and makes the change through the
normal repository tests.

This is the first, deliberately narrow slice of self-improvement. Sandboxed
test execution and an approved apply step are not enabled yet.

## Layout

```text
engram/
├── Cargo.toml
├── .github/workflows/ci.yml
└── crates/
    ├── engram-core/   # config, vault, index, embed, ai, sync, embedded inference
    └── engram-cli/    # the `engram` binary
        ├── native/control.swift  # macOS Accessibility / CoreGraphics helper
        ├── src/serve.rs          # local-only HTTP API and Threadline routes
        ├── src/takeover.rs       # armed sessions, audit, helper extraction
        └── web/                  # Threadline UI embedded in the binary
```

On macOS, the takeover helper is compiled during `cargo build` with
`xcrun swiftc`. Install Xcode Command Line Tools if `xcrun` is missing; the rest
of Engram still builds and reports takeover as unavailable.

## Build

```sh
cargo check --workspace --all-targets
cargo build --workspace --release

# Initialize a fresh vault (config + Welcome.md)
./target/release/engram --vault /path/to/vault init

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

`open` initializes an empty vault automatically (config + `Welcome.md`).
`serve` does not. If the vault already contains notes but has no config, run
`init` once first; otherwise Threadline chat still works, but the vault has no
search index.

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

- GUI takeover is macOS-only. Linux and Windows report the feature unavailable.
- Takeover needs Accessibility permission, and screen capture needs Screen
  Recording permission. The helper is unsigned and may need to be re-approved
  after a rebuild.
- An armed takeover session lives in the server process. Closing or reloading
  the tab sends a best-effort disarm; if that request is lost, the session
  remains until it expires.
- AppleScript is intentionally unrestricted inside an armed session. There is no
  per-application AppleScript allow-list yet.
- No deletion propagation (no tombstones yet).
- Lamport counter is global, not per-entry/per-device.
- Shared manifest can race if two devices export simultaneously.
- Conflict files are `.md`, so `engram index` indexes them as real notes.
- Stopword list is English-only.
- The LLM endpoint and model are configurable; the client sends only portable
  OpenAI Chat Completions fields.
- Device id is derived from host + vault path, so mounting a vault at a new
  path yields a new identity.
