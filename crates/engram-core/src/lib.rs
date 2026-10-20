//! Engram core: local-first second brain.
//!
//! Trait boundaries (`Embedder`, `LlmClient`, `SyncBackend`) are the extension
//! points: v0.2/v0.3 upgrades are additive rather than rewrites.

pub mod ai;
pub mod atomic_write;
pub mod backup;
pub mod capability;
pub mod config;
pub mod crawler;
pub mod dump;
pub mod embed;
pub mod embedded;
pub mod error;
pub mod events;
pub mod import;
pub mod index;
pub mod sync;
pub mod vault;

pub use ai::{build_context_prompt, ChatOpts, LlmClient, OpenAiCompat};
pub use config::{Config, Provider};
pub use embed::{cosine, Embedder, HashingEmbedder};
pub use index::{Hit, Index, NoteLite, QueryFilterResult};
pub use sync::{FolderSync, LocalState, SyncBackend, SyncStats};
pub use vault::{is_hidden_child, Note, Vault};
