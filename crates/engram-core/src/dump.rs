use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const MAX_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingItem {
    pub id: String,
    pub filename: String,
    pub source: String,
    pub content: String,
    pub hash: String,
    pub created_at: u64,
    pub status: String,
}

pub fn submit(vault: &Path, filename: &str, source: &str, content: &str) -> Result<PendingItem> {
    if content.len() > MAX_BYTES { anyhow::bail!("dump item exceeds {} bytes", MAX_BYTES); }
    if filename.is_empty() || filename.len() > 255 { anyhow::bail!("filename must be 1–255 bytes"); }
    let ext = Path::new(filename).extension().and_then(|x| x.to_str()).unwrap_or("");
    if !matches!(ext, "md" | "markdown" | "txt" | "") { anyhow::bail!("unsupported format; use pasted text, .md, .markdown, or .txt"); }
    let mut h = Sha256::new(); h.update(content.as_bytes());
    let hash = format!("{:x}", h.finalize());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let item = PendingItem { id: format!("{}-{}", &hash[..16], now), filename: filename.into(), source: source.into(), content: content.into(), hash, created_at: now, status: "pending".into() };
    let dir = vault.join(".engram").join("dump"); std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{}.json", item.id)), serde_json::to_vec_pretty(&item)?)?;
    Ok(item)
}

pub fn list(vault: &Path) -> Result<Vec<PendingItem>> {
    let dir = vault.join(".engram").join("dump"); if !dir.exists() { return Ok(Vec::new()); }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? { let path = entry?.path(); if path.extension().and_then(|x| x.to_str()) == Some("json") { out.push(serde_json::from_slice(&std::fs::read(path)?)?); } }
    out.sort_by_key(|item: &PendingItem| item.created_at); Ok(out)
}

pub fn get(vault: &Path, id: &str) -> Result<PendingItem> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") { anyhow::bail!("invalid dump id"); }
    let path = vault.join(".engram").join("dump").join(format!("{id}.json"));
    serde_json::from_slice(&std::fs::read(&path).with_context(|| format!("reading dump item {id}"))?).context("parsing dump item")
}
