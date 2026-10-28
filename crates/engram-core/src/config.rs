//! Configuration and device identity.

use crate::atomic_write;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// OpenAI-compatible inference endpoint.
///
/// The endpoint and model remain configurable, so the same client works with
/// MLX-Serve, oMLX, vLLM, llama.cpp, hosted OpenAI-compatible APIs, and other
/// servers without provider-specific request fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    #[serde(alias = "ollama", alias = "openai", alias = "mlx", alias = "omlx")]
    OpenAiCompatible,
}

fn default_llm_base() -> String {
    "http://127.0.0.1:8080/v1".to_string()
}

fn default_llm_model() -> String {
    String::new()
}

fn default_embed_dim() -> usize {
    512
}

pub fn default_provider() -> Provider {
    Provider::OpenAiCompatible
}

pub fn default_context_window() -> usize {
    2048
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub vault: PathBuf,
    pub db: PathBuf,
    #[serde(default = "default_llm_base")]
    pub llm_base: String,
    #[serde(default = "default_llm_model")]
    pub llm_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_key: Option<String>,
    #[serde(default = "default_embed_dim")]
    pub embed_dim: usize,
    #[serde(default = "default_provider")]
    pub provider: Provider,
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    // Intentionally NO `#[serde(default)]`: if a config file is missing a
    // device identity, loading it must fail loudly rather than silently mint a
    // fresh id (identity rotation corrupts a sync system on day two).
    pub device_id: String,
}

impl Config {
    pub fn new(vault: impl Into<PathBuf>) -> Self {
        let vault = vault.into();
        let db = vault.join(".engram").join("index.sqlite");
        let device_id = derive_device_id(&vault);
        Self {
            vault,
            db,
            llm_base: default_llm_base(),
            llm_model: default_llm_model(),
            llm_key: None,
            embed_dim: default_embed_dim(),
            provider: default_provider(),
            context_window: default_context_window(),
            device_id,
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serializing config")?;
        atomic_write::replace(path, text.as_bytes())
            .with_context(|| format!("writing config {}", path.display()))?;
        Ok(())
    }
}

/// Deterministic, path-scoped device identity.
///
/// No clock, no PID: the same `(host, vault path)` always yields the same id,
/// so repeated exports attribute writes to one stable device.
pub fn derive_device_id(vault: &Path) -> String {
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "node".to_string());
    let mut hasher = Sha256::new();
    Digest::update(&mut hasher, host.as_bytes());
    Digest::update(&mut hasher, vault.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    format!(
        "{host}-{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_device_id_is_deterministic() {
        let path = Path::new("/test/vault/path");
        let id1 = derive_device_id(path);
        let id2 = derive_device_id(path);
        assert_eq!(id1, id2);
        assert!(!id1.is_empty());
    }

    #[test]
    fn config_new_initializes_with_defaults() {
        let vault = PathBuf::from("/my/vault");
        let config = Config::new(&vault);

        assert_eq!(config.vault, vault);
        assert_eq!(config.db, vault.join(".engram").join("index.sqlite"));
        assert_eq!(config.llm_base, default_llm_base());
        assert_eq!(config.llm_model, default_llm_model());
        assert_eq!(config.llm_key, None);
        assert_eq!(config.embed_dim, default_embed_dim());
        assert_eq!(config.provider, default_provider());
        assert_eq!(config.context_window, default_context_window());
        assert_eq!(config.device_id, derive_device_id(&vault));
    }

    #[test]
    fn config_round_trips_through_save_and_load() {
        let config_path = std::env::temp_dir().join("config_round_trip.toml");

        let vault = PathBuf::from("/my/vault");
        let mut original = Config::new(&vault);
        original.llm_key = Some("test-key".to_string());
        original.llm_model = "test-model".to_string();

        original.save(&config_path).unwrap();

        let loaded = Config::load(&config_path).unwrap();

        assert_eq!(original.vault, loaded.vault);
        assert_eq!(original.db, loaded.db);
        assert_eq!(original.llm_base, loaded.llm_base);
        assert_eq!(original.llm_model, loaded.llm_model);
        assert_eq!(original.llm_key, loaded.llm_key);
        assert_eq!(original.embed_dim, loaded.embed_dim);
        assert_eq!(original.provider, loaded.provider);
        assert_eq!(original.context_window, loaded.context_window);
        assert_eq!(original.device_id, loaded.device_id);

        let _ = std::fs::remove_file(config_path);
    }

    #[test]
    fn missing_device_id_fails_loudly() {
        let config_path = std::env::temp_dir().join("config_missing_device_id.toml");

        let toml_str = r#"
            vault = "/tmp"
            db = "/tmp/.engram/index.sqlite"
        "#;

        std::fs::write(&config_path, toml_str).unwrap();

        let result = Config::load(&config_path);
        assert!(result.is_err(), "Loading should fail if device_id is missing");

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("parsing config"), "Error should mention parsing config");

        let _ = std::fs::remove_file(config_path);
    }

    #[test]
    fn missing_optional_fields_use_defaults() {
        let config_path = std::env::temp_dir().join("config_missing_optional.toml");

        let toml_str = r#"
            vault = "/tmp"
            db = "/tmp/.engram/index.sqlite"
            device_id = "test-device-id"
        "#;

        std::fs::write(&config_path, toml_str).unwrap();

        let loaded = Config::load(&config_path).unwrap();

        assert_eq!(loaded.llm_base, default_llm_base());
        assert_eq!(loaded.llm_model, default_llm_model());
        assert_eq!(loaded.llm_key, None);
        assert_eq!(loaded.embed_dim, default_embed_dim());
        assert_eq!(loaded.provider, default_provider());
        assert_eq!(loaded.context_window, default_context_window());
        assert_eq!(loaded.device_id, "test-device-id");

        let _ = std::fs::remove_file(config_path);
    }
}
