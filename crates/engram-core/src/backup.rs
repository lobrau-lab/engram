use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Key, Nonce};
use anyhow::{Context, Result};
use argon2::{Argon2, password_hash::SaltString};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use walkdir::WalkDir;

const MAGIC: &[u8] = b"ENGRAM-BACKUP-1\n";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub version: u32,
    pub files: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupPayload {
    manifest: BackupManifest,
    files: BTreeMap<String, Vec<u8>>,
}

pub fn create(source: &Path, destination: &Path, password: &str) -> Result<BackupManifest> {
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(source).into_iter().filter_entry(|e| {
        e.path() == source || !e.path().components().any(|c| c.as_os_str() == ".engram")
    }) {
        let entry = entry?;
        if !entry.file_type().is_file() || entry.path().extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let relative = entry.path().strip_prefix(source)?.to_string_lossy().to_string();
        files.insert(relative, std::fs::read(entry.path())?);
    }
    let manifest = BackupManifest { version: 1, files: files.len() };
    let payload = serde_json::to_vec(&BackupPayload { manifest: manifest.clone(), files })?;
    let salt = SaltString::generate(&mut OsRng);
    let key = derive_key(password, salt.as_str().as_bytes())?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce_bytes: [u8; 12] = rand::random();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce_bytes), payload.as_ref())
        .map_err(|_| anyhow::anyhow!("backup encryption failed"))?;
    let mut output = MAGIC.to_vec();
    output.extend_from_slice(&(salt.as_str().len() as u32).to_le_bytes());
    output.extend_from_slice(salt.as_str().as_bytes());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    let temporary = destination.with_extension("tmp");
    std::fs::write(&temporary, output).with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, destination)?;
    Ok(manifest)
}

pub fn restore(backup: &Path, destination: &Path, password: &str) -> Result<BackupManifest> {
    if destination.exists()
        && std::fs::read_dir(destination)?.next().transpose()?.is_some()
    {
        anyhow::bail!("restore destination is not empty; refusing to overwrite it")
    }
    let bytes = std::fs::read(backup)?;
    if !bytes.starts_with(MAGIC) { anyhow::bail!("invalid Engram backup header") }
    let mut cursor = MAGIC.len();
    let salt_len = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into()?) as usize;
    cursor += 4;
    let salt = &bytes[cursor..cursor + salt_len]; cursor += salt_len;
    let nonce = &bytes[cursor..cursor + 12]; cursor += 12;
    let key = derive_key(password, salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let plaintext = cipher.decrypt(Nonce::from_slice(nonce), &bytes[cursor..])
        .map_err(|_| anyhow::anyhow!("backup authentication failed"))?;
    let payload: BackupPayload = serde_json::from_slice(&plaintext).context("invalid backup payload")?;
    let staging = destination.with_extension("engram-restore-tmp");
    if staging.exists() { std::fs::remove_dir_all(&staging)?; }
    std::fs::create_dir_all(&staging)?;
    for (relative, content) in &payload.files {
        let relative = crate::vault::validate_relative_path(Path::new(relative))?;
        let target = staging.join(relative);
        if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::write(target, content)?;
    }
    if destination.exists() { std::fs::remove_dir_all(destination)?; }
    std::fs::rename(staging, destination)?;
    Ok(payload.manifest)
}

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default().hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}
