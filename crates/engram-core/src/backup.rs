use crate::atomic_write;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result};
use argon2::{password_hash::SaltString, Argon2};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

pub const MAGIC_V1: &[u8] = b"ENGRAM-BACKUP-1\n";
pub const MAGIC_V2: &[u8] = b"ENGRAM-BACKUP-2\n";

/// Maximum allowed individual file size in a backup archive (50 MiB).
pub const MAX_ARCHIVE_FILE_BYTES: usize = 50 * 1024 * 1024;

/// Maximum number of files allowed in a single backup archive.
pub const MAX_ARCHIVE_FILES: usize = 200_000;

/// Maximum total uncompressed archive size (500 MiB) to prevent decompression bombs.
pub const MAX_TOTAL_DECOMPRESSED_BYTES: usize = 500 * 1024 * 1024;

const FLAG_COMPRESSED_DEFLATE: u32 = 1;

/// A staging directory older than this is treated as abandoned. The age check
/// avoids deleting a live restore's staging directory when a PID is reused.
const STALE_STAGING_AGE: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupManifest {
    pub version: u32,
    pub files: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LegacyBackupPayloadV1 {
    pub manifest: BackupManifest,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// RAII guard ensuring a staging directory is purged if an operation is
/// interrupted, panics, or fails before explicit publication.
pub struct StagingGuard {
    path: PathBuf,
    active: bool,
}

impl StagingGuard {
    pub fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    /// Disarm the guard once staging has been atomically published.
    pub fn disarm(mut self) {
        self.active = false;
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Creates a versioned (V2) compressed, authenticated binary backup archive.
pub fn create(source: &Path, destination: &Path, password: &str) -> Result<BackupManifest> {
    if password.trim().is_empty() {
        anyhow::bail!("backup password cannot be empty");
    }

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_uncompressed_bytes: usize = 0;

    for entry in WalkDir::new(source).into_iter().filter_entry(|e| {
        e.path() == source || !e.path().components().any(|c| c.as_os_str() == ".engram")
    }) {
        let entry = entry?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|x| x.to_str()) != Some("md")
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(source)?
            .to_string_lossy()
            .to_string();
        let bytes = std::fs::read(entry.path())
            .with_context(|| format!("reading note {}", entry.path().display()))?;
        if bytes.len() > MAX_ARCHIVE_FILE_BYTES {
            anyhow::bail!(
                "file {} exceeds maximum archive limit of {} bytes",
                relative,
                MAX_ARCHIVE_FILE_BYTES
            );
        }
        total_uncompressed_bytes = total_uncompressed_bytes
            .checked_add(bytes.len())
            .context("archive total size overflow")?;
        if total_uncompressed_bytes > MAX_TOTAL_DECOMPRESSED_BYTES {
            anyhow::bail!(
                "vault exceeds maximum archive uncompressed limit of {} bytes",
                MAX_TOTAL_DECOMPRESSED_BYTES
            );
        }
        files.push((relative, bytes));
        if files.len() > MAX_ARCHIVE_FILES {
            anyhow::bail!(
                "vault exceeds maximum archive file count of {}",
                MAX_ARCHIVE_FILES
            );
        }
    }

    write_archive(files, destination, password)
}

fn write_archive(
    mut files: Vec<(String, Vec<u8>)>,
    destination: &Path,
    password: &str,
) -> Result<BackupManifest> {
    // Sort files deterministically by relative path
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let total_uncompressed_bytes = files.iter().try_fold(0usize, |total, (_, content)| {
        total
            .checked_add(content.len())
            .context("archive total size overflow")
    })?;

    let manifest = BackupManifest {
        version: 2,
        files: files.len(),
    };

    // Serialize to binary payload
    let mut raw_payload = Vec::with_capacity(total_uncompressed_bytes + (files.len() * 32));
    // Header: Format version (u32), flags (u32), file count (u32)
    raw_payload.extend_from_slice(&2u32.to_le_bytes());
    raw_payload.extend_from_slice(&FLAG_COMPRESSED_DEFLATE.to_le_bytes());
    raw_payload.extend_from_slice(&(files.len() as u32).to_le_bytes());

    for (rel_path, content) in &files {
        let path_bytes = rel_path.as_bytes();
        if path_bytes.len() > u16::MAX as usize {
            anyhow::bail!("relative path too long for archive: {rel_path}");
        }
        raw_payload.extend_from_slice(&(path_bytes.len() as u16).to_le_bytes());
        raw_payload.extend_from_slice(path_bytes);
        raw_payload.extend_from_slice(&(content.len() as u32).to_le_bytes());
        raw_payload.extend_from_slice(content);
    }

    // Compress raw payload using miniz_oxide (DEFLATE level 6)
    let compressed_payload = miniz_oxide::deflate::compress_to_vec(&raw_payload, 6);

    // Envelope with uncompressed size (u64) + compressed data
    let mut payload_to_encrypt = Vec::with_capacity(8 + compressed_payload.len());
    payload_to_encrypt.extend_from_slice(&(raw_payload.len() as u64).to_le_bytes());
    payload_to_encrypt.extend_from_slice(&compressed_payload);

    // Cryptographic key derivation via Argon2id
    let salt = SaltString::generate(&mut OsRng);
    let key = derive_key(password, salt.as_str().as_bytes())?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce_bytes: [u8; 12] = rand::random();
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), payload_to_encrypt.as_ref())
        .map_err(|_| anyhow::anyhow!("backup encryption failed"))?;

    let mut output = MAGIC_V2.to_vec();
    output.extend_from_slice(&(salt.as_str().len() as u32).to_le_bytes());
    output.extend_from_slice(salt.as_str().as_bytes());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    atomic_write::replace(destination, &output)?;
    Ok(manifest)
}

/// Restores a backup archive, with full backward-compatibility for V1 (JSON-array)
/// and support for V2 (compressed binary).
pub fn restore(backup: &Path, destination: &Path, password: &str) -> Result<BackupManifest> {
    if password.trim().is_empty() {
        anyhow::bail!("backup password cannot be empty");
    }
    if destination.symlink_metadata().is_ok() {
        anyhow::bail!("restore destination already exists; refusing to overwrite it");
    }

    let bytes =
        std::fs::read(backup).with_context(|| format!("reading backup {}", backup.display()))?;
    if bytes.len() < 16 {
        anyhow::bail!("invalid Engram backup: file too small");
    }

    let is_v2 = bytes.starts_with(MAGIC_V2);
    let is_v1 = bytes.starts_with(MAGIC_V1);
    if !is_v1 && !is_v2 {
        anyhow::bail!("invalid Engram backup header: unrecognized magic signature");
    }

    let magic_len = if is_v2 {
        MAGIC_V2.len()
    } else {
        MAGIC_V1.len()
    };
    let mut cursor = magic_len;

    if bytes.len() < cursor + 4 {
        anyhow::bail!("truncated backup salt length");
    }
    let salt_len = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into()?) as usize;
    cursor += 4;
    if salt_len == 0 || salt_len > 4096 || bytes.len() < cursor + salt_len + 12 {
        anyhow::bail!("invalid or truncated backup salt");
    }
    let salt = &bytes[cursor..cursor + salt_len];
    cursor += salt_len;
    let nonce = &bytes[cursor..cursor + 12];
    cursor += 12;
    if bytes.len() <= cursor {
        anyhow::bail!("truncated backup payload");
    }

    let key = derive_key(password, salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), &bytes[cursor..])
        .map_err(|_| {
            anyhow::anyhow!(
                "backup authentication failed: incorrect password or corrupted ciphertext"
            )
        })?;

    let files: Vec<(String, Vec<u8>)> = if is_v2 {
        parse_v2_payload(&plaintext)?
    } else {
        parse_v1_payload(&plaintext)?
    };

    if files.len() > MAX_ARCHIVE_FILES {
        anyhow::bail!(
            "archive exceeds maximum allowed file count of {}",
            MAX_ARCHIVE_FILES
        );
    }

    let manifest = BackupManifest {
        version: if is_v2 { 2 } else { 1 },
        files: files.len(),
    };

    let parent = destination
        .parent()
        .context("restore destination has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating parent directory {}", parent.display()))?;

    cleanup_stale_staging(parent, destination);

    let staging_path = unique_staging(parent, destination)?;
    let staging_guard = StagingGuard::new(staging_path);

    for (relative, content) in &files {
        if content.len() > MAX_ARCHIVE_FILE_BYTES {
            anyhow::bail!(
                "file {} in archive exceeds maximum limit of {} bytes",
                relative,
                MAX_ARCHIVE_FILE_BYTES
            );
        }
        let rel_path = Path::new(relative);
        let validated = crate::vault::validate_relative_path(rel_path)
            .with_context(|| format!("unsafe path in archive: {relative}"))?;

        if validated
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        {
            anyhow::bail!("archive contains forbidden hidden entry: {relative}");
        }

        let target = staging_guard.path().join(validated);
        if let Some(target_parent) = target.parent() {
            std::fs::create_dir_all(target_parent)?;
        }
        std::fs::write(&target, content)
            .with_context(|| format!("writing restored file {}", target.display()))?;
    }

    publish_staging(staging_guard.path(), destination)?;

    staging_guard.disarm();
    Ok(manifest)
}

fn parse_v1_payload(plaintext: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let legacy: LegacyBackupPayloadV1 =
        serde_json::from_slice(plaintext).context("invalid v1 backup payload structure")?;
    Ok(legacy.files.into_iter().collect())
}

fn parse_v2_payload(plaintext: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    if plaintext.len() < 8 {
        anyhow::bail!("truncated v2 backup envelope");
    }
    let uncompressed_len = u64::from_le_bytes(plaintext[..8].try_into()?) as usize;
    if uncompressed_len > MAX_TOTAL_DECOMPRESSED_BYTES {
        anyhow::bail!(
            "v2 archive uncompressed size ({uncompressed_len} bytes) exceeds safety limit ({MAX_TOTAL_DECOMPRESSED_BYTES} bytes)"
        );
    }

    let compressed_data = &plaintext[8..];
    let decompressed = miniz_oxide::inflate::decompress_to_vec_with_limit(
        compressed_data,
        MAX_TOTAL_DECOMPRESSED_BYTES,
    )
    .map_err(|e| anyhow::anyhow!("v2 archive decompression failed: {e:?}"))?;

    if decompressed.len() != uncompressed_len {
        anyhow::bail!(
            "decompressed size mismatch: expected {uncompressed_len}, got {}",
            decompressed.len()
        );
    }

    if decompressed.len() < 12 {
        anyhow::bail!("truncated v2 decompressed header");
    }

    let format_version = u32::from_le_bytes(decompressed[0..4].try_into()?);
    if format_version != 2 {
        anyhow::bail!("unsupported v2 inner format version: {format_version}");
    }
    let flags = u32::from_le_bytes(decompressed[4..8].try_into()?);
    if flags != FLAG_COMPRESSED_DEFLATE {
        anyhow::bail!("unsupported v2 archive flags: {flags}");
    }
    let file_count = u32::from_le_bytes(decompressed[8..12].try_into()?) as usize;
    if file_count > MAX_ARCHIVE_FILES {
        anyhow::bail!("v2 archive exceeds maximum allowed file count of {MAX_ARCHIVE_FILES}");
    }

    let mut cursor = 12usize;
    let mut files = Vec::with_capacity(file_count);

    for _ in 0..file_count {
        if cursor + 2 > decompressed.len() {
            anyhow::bail!("truncated v2 file entry path length");
        }
        let path_len = u16::from_le_bytes(decompressed[cursor..cursor + 2].try_into()?) as usize;
        cursor += 2;

        if cursor + path_len > decompressed.len() {
            anyhow::bail!("truncated v2 file entry path string");
        }
        let rel_path = std::str::from_utf8(&decompressed[cursor..cursor + path_len])
            .context("v2 archive file path is not valid UTF-8")?
            .to_string();
        cursor += path_len;

        if cursor + 4 > decompressed.len() {
            anyhow::bail!("truncated v2 file entry content length");
        }
        let content_len = u32::from_le_bytes(decompressed[cursor..cursor + 4].try_into()?) as usize;
        cursor += 4;

        if cursor + content_len > decompressed.len() {
            anyhow::bail!("truncated v2 file entry content bytes");
        }
        let content = decompressed[cursor..cursor + content_len].to_vec();
        cursor += content_len;

        files.push((rel_path, content));
    }

    Ok(files)
}

fn unique_staging(parent: &Path, destination: &Path) -> Result<PathBuf> {
    let name = destination
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("vault");
    for attempt in 0..128u32 {
        let candidate = parent.join(format!(
            ".{name}.engram-restore-{}-{attempt}",
            std::process::id()
        ));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("could not allocate a unique restore staging directory")
}

/// Publish a fully written staging directory. On Unix the destination name is
/// reserved with an atomic `create_dir` before the rename, so a rename can
/// never silently replace a file or directory that appeared during the restore.
fn publish_staging(staging: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        match std::fs::create_dir(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                anyhow::bail!("restore destination already exists; refusing to overwrite it")
            }
            Err(error) => {
                return Err(error).with_context(|| format!("reserving {}", destination.display()))
            }
        }
        if let Err(error) = std::fs::rename(staging, destination) {
            // Remove only the empty placeholder we just created.
            let _ = std::fs::remove_dir(destination);
            return Err(error).with_context(|| {
                format!("publishing restored vault to {}", destination.display())
            });
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        // Windows rename refuses to replace an existing directory, so the
        // destination-exists check remains the guard there.
        if destination.symlink_metadata().is_ok() {
            anyhow::bail!("restore destination already exists; refusing to overwrite it");
        }
        std::fs::rename(staging, destination)
            .with_context(|| format!("publishing restored vault to {}", destination.display()))
    }
}

fn cleanup_stale_staging(parent: &Path, destination: &Path) {
    let name = destination
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("vault");
    let prefix = format!(".{name}.engram-restore-");

    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if !file_name.starts_with(&prefix) {
            continue;
        }
        let stale = std::fs::metadata(entry.path())
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > STALE_STAGING_AGE);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}

/// Helper for tests to synthesize legacy V1 archives to verify backward compatibility.
pub fn create_v1_legacy_for_test(
    source: &Path,
    destination: &Path,
    password: &str,
) -> Result<BackupManifest> {
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(source).into_iter().filter_entry(|e| {
        e.path() == source || !e.path().components().any(|c| c.as_os_str() == ".engram")
    }) {
        let entry = entry?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|x| x.to_str()) != Some("md")
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(source)?
            .to_string_lossy()
            .to_string();
        files.insert(relative, std::fs::read(entry.path())?);
    }

    let manifest = BackupManifest {
        version: 1,
        files: files.len(),
    };
    let payload = serde_json::to_vec(&LegacyBackupPayloadV1 {
        manifest: manifest.clone(),
        files,
    })?;
    let salt = SaltString::generate(&mut OsRng);
    let key = derive_key(password, salt.as_str().as_bytes())?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce_bytes: [u8; 12] = rand::random();
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), payload.as_ref())
        .map_err(|_| anyhow::anyhow!("backup encryption failed"))?;
    let mut output = MAGIC_V1.to_vec();
    output.extend_from_slice(&(salt.as_str().len() as u32).to_le_bytes());
    output.extend_from_slice(salt.as_str().as_bytes());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    atomic_write::replace(destination, &output)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "engram-backup-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp");
        path
    }

    #[test]
    fn publish_refuses_an_existing_destination() {
        let parent = temp("publish-existing");
        let staging = parent.join("staging");
        let destination = parent.join("vault");
        std::fs::create_dir_all(&staging).expect("create staging");
        std::fs::write(staging.join("note.md"), "new").expect("write staged note");
        std::fs::create_dir_all(&destination).expect("create destination");
        std::fs::write(destination.join("keep.md"), "keep").expect("write destination note");

        let error = publish_staging(&staging, &destination).expect_err("must refuse");
        assert!(error.to_string().contains("destination already exists"));
        assert_eq!(
            std::fs::read_to_string(destination.join("keep.md")).expect("destination intact"),
            "keep"
        );
        assert!(staging.join("note.md").exists());
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn publish_removes_its_placeholder_when_the_rename_fails() {
        let parent = temp("publish-failure");
        let missing_staging = parent.join("missing-staging");
        let destination = parent.join("vault");

        let error = publish_staging(&missing_staging, &destination).expect_err("must fail");
        assert!(error.to_string().contains("publishing restored vault"));
        assert!(!destination.exists());
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn restore_rejects_parent_traversal_in_an_archive() {
        let parent = temp("restore-traversal");
        let archive = parent.join("evil.engram");
        let destination = parent.join("restored");

        write_archive(
            vec![("../escape.md".into(), b"evil".to_vec())],
            &archive,
            "password",
        )
        .expect("write archive");

        let error = restore(&archive, &destination, "password").expect_err("must reject");
        assert!(error.to_string().contains("unsafe path in archive"));
        assert!(!destination.exists());
        assert!(!parent.join("escape.md").exists());
        let _ = std::fs::remove_dir_all(parent);
    }
}
