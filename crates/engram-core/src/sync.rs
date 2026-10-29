//! Folder-based last-write-wins sync with a manifest + Lamport state sidecar.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::atomic_write;
use crate::vault::{content_hash, validate_relative_path, Vault};

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
    let text = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
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
    let text = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
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

fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|part| match part {
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

/// Reject symlinks in every existing component so a sync manifest cannot
/// escape the vault through a link placed inside it.
fn ensure_no_symlink_components(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "symbolic links are not allowed in sync paths"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Validate a manifest entry before it is allowed to touch the filesystem.
/// The sync folder is treated as untrusted input: entries must be relative
/// Markdown paths with no hidden components, traversal, or symlink escapes.
fn safe_manifest_target(vault: &Path, raw: &str) -> Result<(PathBuf, PathBuf)> {
    let relative = validate_relative_path(Path::new(raw))
        .with_context(|| format!("invalid sync path {raw:?}"))?;
    anyhow::ensure!(
        relative
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md")),
        "sync entries must be Markdown files"
    );
    anyhow::ensure!(
        !has_hidden_component(&relative),
        "sync entries may not use hidden paths"
    );

    let root = vault
        .canonicalize()
        .with_context(|| format!("resolving vault {}", vault.display()))?;
    ensure_no_symlink_components(&root, &relative)?;
    let target = root.join(&relative);
    if target.exists() {
        let resolved = target.canonicalize()?;
        anyhow::ensure!(resolved.starts_with(&root), "sync target escapes the vault");
    }
    Ok((relative, target))
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
            let known = state.entries.get(&note.id).cloned();
            if let Some(prev) = known.as_ref().filter(|e| e.mtime == note.mtime) {
                stats.skipped += 1;
                let size = std::fs::metadata(&note.path).map(|m| m.len()).unwrap_or(0);
                entries.push(Entry {
                    path: note.id.clone(),
                    hash: prev.hash.clone(),
                    mtime: note.mtime,
                    size,
                    device: self.device_id.clone(),
                    lamport: prev.lamport,
                });
                continue;
            }

            let content = std::fs::read_to_string(&note.path)
                .with_context(|| format!("reading {}", note.path.display()))?;
            let hash = content_hash(&content);
            let size = content.len() as u64;

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
        anyhow::ensure!(manifest.version == 1, "unsupported sync manifest version");
        let mut state = load_local_state(vault)?;
        if state.device_id.is_empty() {
            state.device_id = self.device_id.clone();
        }
        let src_root = src
            .canonicalize()
            .with_context(|| format!("resolving sync folder {}", src.display()))?;

        let mut stats = SyncStats::default();
        let mut max_lamport = state.lamport.max(manifest.lamport);

        for entry in &manifest.entries {
            let (relative, dst) = safe_manifest_target(vault, &entry.path)?;
            let source = src_root.join(&relative);
            if !source.exists() {
                continue;
            }
            let src_file = source
                .canonicalize()
                .with_context(|| format!("resolving sync entry {}", relative.display()))?;
            anyhow::ensure!(
                src_file.starts_with(&src_root),
                "sync entry escapes the sync folder"
            );

            let local = state.entries.get(&entry.path).cloned();
            if let Some(ref local_entry) = local {
                if local_entry.hash == entry.hash {
                    stats.skipped += 1;
                    max_lamport = max_lamport.max(entry.lamport);
                    continue;
                }
            }

            let incoming = std::fs::read_to_string(&src_file)
                .with_context(|| format!("reading {}", src_file.display()))?;
            let incoming_hash = content_hash(&incoming);

            match local {
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
                            let cpath =
                                conflict_path(vault, &relative.to_string_lossy(), &entry.device);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_save_and_load_local_state() {
        let dir = std::env::temp_dir().join(format!(
            "engram-sync-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let mut entries = BTreeMap::new();
        entries.insert(
            "test.md".to_string(),
            LocalEntry {
                hash: "hash123".to_string(),
                mtime: 123456789,
                lamport: 42,
            },
        );

        let state = LocalState {
            device_id: "device-1".to_string(),
            lamport: 43,
            entries,
        };

        // Save local state
        save_local_state(&dir, &state).unwrap();

        // Verify file exists
        let state_file = dir.join(STATE_REL);
        assert!(state_file.exists());

        // Load local state
        let loaded_state = load_local_state(&dir).unwrap();

        assert_eq!(loaded_state.device_id, state.device_id);
        assert_eq!(loaded_state.lamport, state.lamport);
        assert_eq!(loaded_state.entries.len(), 1);

        let loaded_entry = loaded_state.entries.get("test.md").unwrap();
        assert_eq!(loaded_entry.hash, "hash123");
        assert_eq!(loaded_entry.mtime, 123456789);
        assert_eq!(loaded_entry.lamport, 42);

        // Clean up
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
