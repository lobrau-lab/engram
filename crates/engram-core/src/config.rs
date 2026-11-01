//! Configuration and device identity.

use crate::atomic_write;
use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
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
pub struct RenderStationConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub user: String,
    #[serde(default = "default_station_port")]
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<PathBuf>,
    #[serde(default = "default_remote_workspace")]
    pub remote_workspace: String,
    #[serde(default)]
    pub prefer_pinokio: bool,
}

fn default_station_port() -> u16 {
    22
}

fn default_remote_workspace() -> String {
    "~/.engram-station".to_string()
}

impl Default for RenderStationConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            user: String::new(),
            port: default_station_port(),
            key_path: None,
            remote_workspace: default_remote_workspace(),
            prefer_pinokio: false,
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default)]
    pub station: RenderStationConfig,
}

impl Config {
    pub fn new(vault: impl Into<PathBuf>) -> Self {
        let vault = vault.into();
        let db = vault.join(".engram").join("index.sqlite");
        let device_id = Some(generate_device_id());
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
            station: RenderStationConfig::default(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        if cfg.device_id.as_deref().unwrap_or_default().is_empty() {
            cfg.device_id = Some(generate_device_id());
            cfg.save(path)?;
        }
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

fn generate_device_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn loading_config_without_device_id_persists_one() {
        let dir = std::env::temp_dir().join(format!(
            "engram-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut config = Config::new(dir.join("vault"));
        config.device_id = None;
        config.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();
        let id = loaded.device_id.clone().unwrap();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|character| character.is_ascii_hexdigit()));
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains(&format!("device_id = \"{id}\"")));
        assert_eq!(Config::load(&path).unwrap().device_id.as_deref(), Some(id.as_str()));

        let _ = std::fs::remove_dir_all(dir);
    }
}
