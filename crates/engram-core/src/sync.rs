//! Folder-based last-write-wins sync with a manifest + Lamport state sidecar.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::vault::{content_hash, Vault};
use crate::atomic_write;

pub const MANIFEST_NAME: &str = ".engram-manifest.json";
pub const STATE_REL: &str = ".engram/state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub hash: String,
    pub mtime: i64,
    pub size: u64,
    pub device: String,
    pub lamport: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub generated_by: String,
    pub lamport: u64,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalEntry {
    pub hash: String,
    pub mtime: i64,
    pub lamport: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalState {
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub lamport: u64,
    #[serde(default)]
    pub entries: BTreeMap<String, LocalEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    pub copied: usize,
    pub skipped: usize,
    pub overwrites: usize,
    pub conflicts: usize,
}

pub trait SyncBackend {
    fn export(&self, vault: &Path, dest: &Path) -> Result<SyncStats>;
    fn import(&self, vault: &Path, src: &Path) -> Result<SyncStats>;
}

pub struct FolderSync {
    device_id: String,
    dry_run: bool,
}

impl FolderSync {
    pub fn new(device_id: impl Into<String>, dry_run: bool) -> Self {
        Self {
            device_id: device_id.into(),
            dry_run,
        }
    }
}

pub fn manifest_path(dir: &Path) -> PathBuf {
    dir.join(MANIFEST_NAME)
}

pub fn state_path(vault: &Path) -> PathBuf {
    vault.join(STATE_REL)
}

pub fn load_manifest(dir: &Path) -> Result<Option<Manifest>> {
    let p = manifest_path(dir);
    if !p.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
    let m: Manifest =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
    Ok(Some(m))
}

pub fn save_manifest(dir: &Path, m: &Manifest) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let p = manifest_path(dir);
    let text = serde_json::to_string_pretty(m).context("serializing manifest")?;
    atomic_write::replace(&p, text.as_bytes())
        .with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

pub fn load_local_state(vault: &Path) -> Result<LocalState> {
    let p = state_path(vault);
    if !p.exists() {
        return Ok(LocalState::default());
    }
    let text =
        std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
    let s: LocalState =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
    Ok(s)
}

pub fn save_local_state(vault: &Path, s: &LocalState) -> Result<()> {
    let p = state_path(vault);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(s).context("serializing local state")?;
    atomic_write::replace(&p, text.as_bytes())
        .with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

pub fn append_overwrite_log(vault: &Path, line: &str) -> Result<()> {
    use std::io::Write;
    let dir = vault.join(".engram");
    std::fs::create_dir_all(&dir)?;
    let p = dir.join("overwrites.log");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .with_context(|| format!("opening {}", p.display()))?;
    writeln!(f, "{line}").with_context(|| format!("appending {}", p.display()))?;
    Ok(())
}

pub fn conflict_path(vault: &Path, rel: &str, device: &str) -> PathBuf {
    let p = Path::new(rel);
    let parent = p.parent().filter(|x| !x.as_os_str().is_empty());
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("note");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("md");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!("{stem}.conflict-{device}-{ts}.{ext}");
    match parent {
        Some(dir) => vault.join(dir).join(name),
        None => vault.join(name),
    }
}

fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::copy(src, dst)
        .with_context(|| format!("copying {} -> {}", src.display(), dst.display()))?;
    Ok(())
}

impl SyncBackend for FolderSync {
    fn export(&self, vault: &Path, dest: &Path) -> Result<SyncStats> {
        let v = Vault::open(vault.to_path_buf())?;
        let notes = v.scan()?;
        let mut state = load_local_state(vault)?;
        if state.device_id.is_empty() {
            state.device_id = self.device_id.clone();
        }

        let mut stats = SyncStats::default();
        let mut entries = Vec::new();
        let mut lamport = state.lamport;

        for note in &notes {
            let content = std::fs::read_to_string(&note.path)
                .with_context(|| format!("reading {}", note.path.display()))?;
            let hash = content_hash(&content);
            let size = content.len() as u64;

            let known = state.entries.get(&note.id).cloned();
            if let Some(prev) = known.filter(|e| e.hash == hash) {
                stats.skipped += 1;
                entries.push(Entry {
                    path: note.id.clone(),
                    hash,
                    mtime: note.mtime,
                    size,
                    device: self.device_id.clone(),
                    lamport: prev.lamport,
                });
                continue;
            }

            lamport += 1;
            if !self.dry_run {
                copy_file(&note.path, &dest.join(&note.id))?;
            }
            stats.copied += 1;
            state.entries.insert(
                note.id.clone(),
                LocalEntry {
                    hash: hash.clone(),
                    mtime: note.mtime,
                    lamport,
                },
            );
            entries.push(Entry {
                path: note.id.clone(),
                hash,
                mtime: note.mtime,
                size,
                device: self.device_id.clone(),
                lamport,
            });
        }

        state.lamport = lamport;
        let manifest = Manifest {
            version: 1,
            generated_by: self.device_id.clone(),
            lamport,
            entries,
        };
        if !self.dry_run {
            save_manifest(dest, &manifest)?;
            save_local_state(vault, &state)?;
        }
        Ok(stats)
    }

    fn import(&self, vault: &Path, src: &Path) -> Result<SyncStats> {
        let manifest = match load_manifest(src)? {
            Some(m) => m,
            None => anyhow::bail!("no manifest found in {}", src.display()),
        };
        let mut state = load_local_state(vault)?;
        if state.device_id.is_empty() {
            state.device_id = self.device_id.clone();
        }

        let mut stats = SyncStats::default();
        let mut max_lamport = state.lamport.max(manifest.lamport);

        for entry in &manifest.entries {
            let src_file = src.join(&entry.path);
            if !src_file.exists() {
                continue;
            }
            let dst = vault.join(&entry.path);

            let incoming = std::fs::read_to_string(&src_file)
                .with_context(|| format!("reading {}", src_file.display()))?;
            let incoming_hash = content_hash(&incoming);

            match state.entries.get(&entry.path).cloned() {
                None => {
                    // New to this device.
                    if !self.dry_run {
                        copy_file(&src_file, &dst)?;
                    }
                    stats.copied += 1;
                    state.entries.insert(
                        entry.path.clone(),
                        LocalEntry {
                            hash: incoming_hash,
                            mtime: entry.mtime,
                            lamport: entry.lamport,
                        },
                    );
                    max_lamport = max_lamport.max(entry.lamport);
                }
                Some(local) => {
                    if local.hash == incoming_hash {
                        // Identical content: upsert state, advance max_lamport.
                        state.entries.insert(
                            entry.path.clone(),
                            LocalEntry {
                                hash: incoming_hash,
                                mtime: entry.mtime,
                                lamport: local.lamport.max(entry.lamport),
                            },
                        );
                        max_lamport = max_lamport.max(entry.lamport);
                        stats.skipped += 1;
                    } else if entry.lamport > local.lamport {
                        // Remote strictly newer: overwrite, and log it.
                        if !self.dry_run {
                            append_overwrite_log(
                                vault,
                                &format!(
                                    "overwrite {} local_lamport={} remote_lamport={} remote_device={}",
                                    entry.path, local.lamport, entry.lamport, entry.device
                                ),
                            )?;
                            copy_file(&src_file, &dst)?;
                        }
                        stats.overwrites += 1;
                        state.entries.insert(
                            entry.path.clone(),
                            LocalEntry {
                                hash: incoming_hash,
                                mtime: entry.mtime,
                                lamport: entry.lamport,
                            },
                        );
                        max_lamport = max_lamport.max(entry.lamport);
                    } else if entry.lamport < local.lamport {
                        // Ours is newer: keep it.
                        stats.skipped += 1;
                    } else {
                        // Concurrent (equal lamport, divergent content): fork.
                        if !self.dry_run {
                            let cpath = conflict_path(vault, &entry.path, &entry.device);
                            copy_file(&src_file, &cpath)?;
                            // Evict the stale entry so the next export re-adopts
                            // the file with a fresh, higher lamport.
                            state.entries.remove(&entry.path);
                        }
                        stats.conflicts += 1;
                    }
                }
            }
        }

        state.lamport = max_lamport;
        if !self.dry_run {
            save_local_state(vault, &state)?;
        }
        Ok(stats)
    }
}
