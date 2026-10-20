use crate::{atomic_write, dump};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_BATCH_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedFile {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub bytes: usize,
    pub hash: String,
    pub source: String,
    pub imported_at: u64,
    pub extractor: String,
    pub status: String,
    pub extracted_bytes: usize,
    pub warnings: Vec<String>,
    pub dump_id: Option<String>,
}

pub fn import_file(
    vault: &Path,
    filename: &str,
    bytes: &[u8],
    source: &str,
) -> Result<ImportedFile> {
    if bytes.len() > MAX_FILE_BYTES {
        anyhow::bail!("file exceeds {} byte limit", MAX_FILE_BYTES);
    }
    let extension = Path::new(filename)
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let media_type = match extension.as_str() {
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    };
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash = format!("{:x}", hasher.finalize());
    let id = hash[..24].to_string();
    let dir = vault.join(".engram").join("imports");
    let originals = dir.join("originals");
    let extracted = dir.join("extracted");
    std::fs::create_dir_all(&originals)?;
    std::fs::create_dir_all(&extracted)?;
    let original_path = originals.join(format!("{id}.bin"));
    if !original_path.exists() {
        atomic_write::replace(&original_path, bytes)?;
    }
    let (text, extractor, status, warnings) = match extension.as_str() {
        "md" | "markdown" | "txt" => (
            String::from_utf8(bytes.to_vec()).context("text file is not valid UTF-8")?,
            "plain-text-v1".into(),
            "complete".into(),
            Vec::new(),
        ),
        "html" | "htm" => (
            html2text::from_read(bytes, 100),
            "html2text-0.12".into(),
            "complete".into(),
            vec!["HTML scripts and styles are omitted; layout may differ.".into()],
        ),
        _ => (
            String::new(),
            "none".into(),
            "unsupported".into(),
            vec!["No local extractor is enabled for this format.".into()],
        ),
    };
    let mut dump_id = None;
    if !text.trim().is_empty() && status == "complete" {
        atomic_write::replace(&extracted.join(format!("{id}.txt")), text.as_bytes())?;
        dump_id = Some(dump::submit(vault, filename, &format!("import:{id}"), &text)?.id);
    }
    let record = ImportedFile {
        id: id.clone(),
        filename: filename.into(),
        media_type: media_type.into(),
        bytes: bytes.len(),
        hash,
        source: source.into(),
        imported_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        extractor,
        status,
        extracted_bytes: text.len(),
        warnings,
        dump_id,
    };
    atomic_write::replace(
        &dir.join(format!("{id}.json")),
        serde_json::to_string_pretty(&record)?.as_bytes(),
    )?;
    Ok(record)
}

pub fn record_path(vault: &Path, id: &str) -> PathBuf {
    vault
        .join(".engram")
        .join("imports")
        .join(format!("{id}.json"))
}
