//! Markdown vault: scanning, note loading, and frontmatter/body parsing.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct Note {
    /// Vault-relative path, using forward slashes (portable id).
    pub id: String,
    pub path: PathBuf,
    pub title: String,
    /// Body with frontmatter stripped.
    pub body: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub mtime: i64,
}

#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
}

/// Validate a vault-relative path before joining it to a trusted root.
pub fn validate_relative_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        anyhow::bail!("absolute paths are not allowed in a vault")
    }
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            anyhow::bail!("parent traversal is not allowed in a vault path")
        }
    }
    if path.as_os_str().is_empty() {
        anyhow::bail!("vault path cannot be empty")
    }
    Ok(path.to_path_buf())
}

impl Vault {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        // Canonicalize the root so ids stay vault-relative on platforms where
        // the configured path and the filesystem path differ (e.g. /var on macOS).
        let root = root.canonicalize().unwrap_or(root);
        Ok(Self { root })
    }

    /// Load all notes into a vector. Retained for compatibility.
    pub fn scan(&self) -> Result<Vec<Note>> {
        let mut notes = Vec::new();
        self.scan_each(|note| {
            notes.push(note);
            Ok(())
        })?;
        Ok(notes)
    }

    /// Stream notes one by one through a callback without retaining all note
    /// bodies simultaneously in memory.
    pub fn scan_each<F>(&self, mut callback: F) -> Result<usize>
    where
        F: FnMut(Note) -> Result<()>,
    {
        if !self.root.exists() {
            return Ok(0);
        }
        let walker = WalkDir::new(&self.root)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                !e.file_name().to_string_lossy().starts_with('.')
            });
        let mut count = 0usize;
        for entry in walker {
            let entry = entry.with_context(|| format!("walking {}", self.root.display()))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if is_hidden_child(&self.root, path) {
                continue;
            }
            let note = self.load_note(path)?;
            callback(note)?;
            count += 1;
        }
        Ok(count)
    }

    pub fn read_note(&self, id: &str) -> Result<Note> {
        let path = self.resolve_read_path(id)?;
        self.load_note(&path)
    }

    /// Resolve a vault-relative note without following a path outside the
    /// canonical vault root. Core callers get the same protection as routes.
    pub fn resolve_read_path(&self, id: &str) -> Result<PathBuf> {
        let relative = validate_relative_path(Path::new(id))?;
        let root = self.root.canonicalize()?;
        let path = root.join(relative).canonicalize()?;
        if !path.starts_with(&root) {
            anyhow::bail!("note path escapes vault")
        }
        Ok(path)
    }

    fn load_note(&self, path: &Path) -> Result<Note> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let id = rel_id(&self.root, path);
        let (fm, body) = split_frontmatter(&content);
        let fm = fm.unwrap_or("");
        let tags = extract_tags(&content);
        let links = extract_links(&content);
        let title = derive_title(fm, body, &id);
        let mtime = mtime_secs(path)?;
        Ok(Note {
            id,
            path: path.to_path_buf(),
            title,
            body: body.to_string(),
            tags,
            links,
            mtime,
        })
    }
}

/// True if any path component *below* `root` is hidden (starts with `.`).
///
/// This never inspects `root` itself, so a vault rooted at a dot-directory
/// (e.g. `/home/u/.notes`) is still scanned. This is the root-prefix fix.
pub fn is_hidden_child(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s.starts_with('.') && s != "." && s != ".."
    })
}

pub fn rel_id(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

pub fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

pub fn content_hash(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex(&h.finalize())
}

pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Split optional `---` YAML frontmatter from the body.
pub fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let first_line_end = match content.find('\n') {
        Some(i) => i + 1,
        None => return (None, content),
    };
    if content[..first_line_end].trim() != "---" {
        return (None, content);
    }
    let rest = &content[first_line_end..];
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']).trim() == "---" {
            let fm = &content[first_line_end..first_line_end + offset];
            let body = &rest[offset + line.len()..];
            return (Some(fm), body);
        }
        offset += line.len();
    }
    (None, content)
}

pub fn extract_tags(content: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    if let Some(fm) = split_frontmatter(content).0 {
        let mut in_tags = false;
        for line in fm.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("tags:") {
                in_tags = true;
                let rest = rest.trim();
                if !rest.is_empty() {
                    let inner = rest.trim_start_matches('[').trim_end_matches(']');
                    for part in inner.split(',') {
                        push_tag(&mut tags, part);
                    }
                }
            } else if in_tags && l.starts_with('-') {
                push_tag(&mut tags, l.trim_start_matches('-'));
            } else if !l.is_empty() {
                in_tags = false;
            }
        }
    }
    for raw in split_frontmatter(content).1.split_whitespace() {
        if let Some(t) = raw.strip_prefix('#') {
            let t: String = t
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '/')
                .collect();
            if !t.is_empty() {
                tags.push(t);
            }
        }
    }
    tags.sort();
    tags.dedup();
    tags
}

fn push_tag(tags: &mut Vec<String>, raw: &str) {
    let t = raw.trim().trim_matches(|c| c == '"' || c == '\'');
    if !t.is_empty() {
        tags.push(t.to_string());
    }
}

pub fn extract_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();

    // [[wiki links]]
    let mut i = 0usize;
    while let Some(pos) = content[i..].find("[[") {
        let start = i + pos + 2;
        if let Some(end) = content[start..].find("]]") {
            let target = &content[start..start + end];
            let target = target.split('|').next().unwrap_or(target).trim();
            if !target.is_empty() {
                links.push(target.to_string());
            }
            i = start + end + 2;
        } else {
            break;
        }
    }

    // [text](target)
    let mut i = 0usize;
    while let Some(pos) = content[i..].find("](") {
        let start = i + pos + 2;
        if let Some(end) = content[start..].find(')') {
            let target = content[start..start + end].trim();
            let target = target.split_whitespace().next().unwrap_or(target);
            if !target.is_empty() && !target.starts_with("http") {
                links.push(target.to_string());
            }
            i = start + end + 1;
        } else {
            break;
        }
    }

    links.sort();
    links.dedup();
    links
}

pub fn derive_title(fm: &str, body: &str, id: &str) -> String {
    for line in fm.lines() {
        let l = line.trim();
        if let Some(t) = l.strip_prefix("title:") {
            let t = t.trim().trim_matches(|c| c == '"' || c == '\'');
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    for line in body.lines() {
        let l = line.trim();
        if let Some(h) = l.strip_prefix('#') {
            let h = h.trim_start_matches('#').trim();
            if !h.is_empty() {
                return h.to_string();
            }
        }
    }
    Path::new(id)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(id)
        .to_string()
}

fn mtime_secs(path: &Path) -> Result<i64> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let modified = meta
        .modified()
        .with_context(|| format!("mtime {}", path.display()))?;
    let d = modified
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Ok(d.as_secs() as i64)
}
