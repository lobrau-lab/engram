//! Vault-backed Threadline workspace: one versioned JSON document under
//! `.engram/threadline/workspace.json`.
//!
//! The browser keeps a localStorage copy only as a fallback. This file is the
//! durable workspace and is never included in `.engram`-excluding backups, so
//! the UI exposes an explicit export/import path.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_WORKSPACE_BYTES: usize = 1024 * 1024;
const SCHEMA_VERSION: u64 = 1;
const MAX_DEPTH: usize = 20;
const MAX_STRING_BYTES: usize = 200_000;
const MAX_ARRAY_ITEMS: usize = 20_000;
const MAX_OBJECT_KEYS: usize = 500;
const MAX_KEY_BYTES: usize = 128;

static WORKSPACE_LOCK: Mutex<()> = Mutex::new(());

pub struct LoadedWorkspace {
    pub workspace: Value,
    pub revision: String,
}

#[derive(Debug)]
pub enum SaveError {
    Conflict(Option<String>),
    BadRequest(String),
    Internal(String),
}

pub fn load(vault: &Path) -> Result<Option<LoadedWorkspace>> {
    let path = workspace_path(vault);
    if let Some(parent) = path.parent() {
        reject_symlink(parent)?;
    }
    reject_symlink(&path)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    if bytes.len() > MAX_WORKSPACE_BYTES {
        bail!(
            "workspace file exceeds the {} byte limit",
            MAX_WORKSPACE_BYTES
        );
    }
    let workspace: Value =
        serde_json::from_slice(&bytes).context("workspace file is not valid JSON")?;
    validate_workspace(&workspace)?;
    Ok(Some(LoadedWorkspace {
        workspace,
        revision: revision_of(&bytes),
    }))
}

pub fn save(vault: &Path, body: &str) -> std::result::Result<String, SaveError> {
    let request: Value = serde_json::from_str(body)
        .map_err(|error| SaveError::BadRequest(format!("invalid workspace request: {error}")))?;
    let workspace = request
        .get("workspace")
        .cloned()
        .ok_or_else(|| SaveError::BadRequest("missing workspace".into()))?;
    let base_revision = request
        .get("baseRevision")
        .and_then(Value::as_str)
        .map(str::to_owned);
    validate_workspace(&workspace).map_err(|error| SaveError::BadRequest(error.to_string()))?;

    let bytes = serde_json::to_vec(&workspace)
        .map_err(|error| SaveError::Internal(format!("serializing workspace: {error}")))?;
    if bytes.len() > MAX_WORKSPACE_BYTES {
        return Err(SaveError::BadRequest(format!(
            "workspace exceeds the {} byte limit",
            MAX_WORKSPACE_BYTES
        )));
    }

    // Serialize reads and writes in this process so two requests cannot both
    // pass the revision check and then overwrite each other.
    let _guard = WORKSPACE_LOCK
        .lock()
        .map_err(|_| SaveError::Internal("workspace lock is poisoned".into()))?;
    let current = load(vault).map_err(|error| SaveError::Internal(error.to_string()))?;
    let current_revision = current.as_ref().map(|loaded| loaded.revision.as_str());
    match (current_revision, base_revision.as_deref()) {
        (Some(current), Some(base)) if current == base => {}
        (None, None) => {}
        (Some(current), _) => return Err(SaveError::Conflict(Some(current.to_owned()))),
        (None, Some(_)) => return Err(SaveError::Conflict(None)),
    }

    write_workspace(&workspace_path(vault), &bytes)
        .map_err(|error| SaveError::Internal(error.to_string()))?;
    Ok(revision_of(&bytes))
}

fn workspace_path(vault: &Path) -> PathBuf {
    vault
        .join(".engram")
        .join("threadline")
        .join("workspace.json")
}

fn write_workspace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("workspace destination has no parent directory")?;
    reject_symlink(parent)?;
    reject_symlink(path)?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }

    // `atomic_write::replace` preserves the destination mode. Pre-create a
    // missing file as 0600 so the first save never appears world-readable.
    let mut created = false;
    if !path.exists() {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(_) => created = true,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).with_context(|| format!("creating {}", path.display()))
            }
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
    }

    if let Err(error) = engram_core::atomic_write::replace(path, bytes) {
        if created {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            bail!("workspace path must not be a symlink: {}", path.display());
        }
    }
    Ok(())
}

fn revision_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn validate_workspace(workspace: &Value) -> Result<()> {
    let object = workspace
        .as_object()
        .context("workspace must be a JSON object")?;
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(SCHEMA_VERSION) {
        bail!("workspace schemaVersion must be {SCHEMA_VERSION}");
    }
    validate_value(workspace, 0)
}

fn validate_value(value: &Value, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        bail!("workspace nesting exceeds {MAX_DEPTH} levels");
    }
    match value {
        Value::String(text) if text.len() > MAX_STRING_BYTES => {
            bail!("workspace string exceeds {MAX_STRING_BYTES} bytes")
        }
        Value::Array(items) => {
            if items.len() > MAX_ARRAY_ITEMS {
                bail!("workspace array exceeds {MAX_ARRAY_ITEMS} items");
            }
            for item in items {
                validate_value(item, depth + 1)?;
            }
        }
        Value::Object(map) => {
            if map.len() > MAX_OBJECT_KEYS {
                bail!("workspace object exceeds {MAX_OBJECT_KEYS} keys");
            }
            for (key, item) in map {
                if key.len() > MAX_KEY_BYTES {
                    bail!("workspace key exceeds {MAX_KEY_BYTES} bytes");
                }
                if is_secret_key(key) {
                    bail!("workspace must not contain credentials ({key})");
                }
                validate_value(item, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_secret_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect();
    matches!(
        normalized.as_str(),
        "apikey"
            | "authorization"
            | "password"
            | "secret"
            | "token"
            | "sessiontoken"
            | "accesstoken"
            | "refreshtoken"
            | "credential"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "engram-workspace-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn request(workspace: Value, base: Option<&str>) -> String {
        serde_json::json!({ "workspace": workspace, "baseRevision": base }).to_string()
    }

    fn workspace() -> Value {
        serde_json::json!({
            "schemaVersion": 1,
            "settings": { "model": "local" },
            "conversations": [],
            "activeId": null
        })
    }

    #[test]
    fn round_trip_and_revision_conflict() {
        let vault = temp("round-trip");
        assert!(load(&vault).unwrap().is_none());

        let first = save(&vault, &request(workspace(), None)).unwrap();
        let loaded = load(&vault).unwrap().unwrap();
        assert_eq!(loaded.revision, first);
        assert_eq!(loaded.workspace["settings"]["model"], "local");

        let mut changed = workspace();
        changed["settings"]["model"] = Value::String("changed".into());
        let second = save(&vault, &request(changed, Some(&first))).unwrap();
        assert_ne!(first, second);

        let error = save(&vault, &request(workspace(), Some(&first))).unwrap_err();
        assert!(matches!(error, SaveError::Conflict(Some(revision)) if revision == second));
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn rejects_credentials_and_oversized_strings() {
        let vault = temp("bounds");
        let mut with_key = workspace();
        with_key["settings"]["apiKey"] = Value::String("secret".into());
        assert!(matches!(
            save(&vault, &request(with_key, None)),
            Err(SaveError::BadRequest(_))
        ));

        let mut huge = workspace();
        huge["settings"]["systemPrompt"] = Value::String("x".repeat(MAX_STRING_BYTES + 1));
        assert!(matches!(
            save(&vault, &request(huge, None)),
            Err(SaveError::BadRequest(_))
        ));
        assert!(!workspace_path(&vault).exists());
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn corrupt_file_is_reported_not_overwritten() {
        let vault = temp("corrupt");
        let path = workspace_path(&vault);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not json").unwrap();
        assert!(load(&vault).is_err());
        assert!(matches!(
            save(&vault, &request(workspace(), None)),
            Err(SaveError::Internal(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");
        let _ = fs::remove_dir_all(vault);
    }

    #[cfg(unix)]
    #[test]
    fn workspace_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let vault = temp("permissions");
        save(&vault, &request(workspace(), None)).unwrap();
        let mode = fs::metadata(workspace_path(&vault))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = fs::metadata(workspace_path(&vault).parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        let _ = fs::remove_dir_all(vault);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_workspace_directory() {
        let vault = temp("symlink");
        let outside = temp("symlink-outside");
        fs::create_dir_all(vault.join(".engram")).unwrap();
        std::os::unix::fs::symlink(&outside, vault.join(".engram").join("threadline")).unwrap();

        assert!(matches!(
            save(&vault, &request(workspace(), None)),
            Err(SaveError::Internal(_))
        ));
        assert!(!outside.join("workspace.json").exists());
        let _ = fs::remove_dir_all(vault);
        let _ = fs::remove_dir_all(outside);
    }
}
