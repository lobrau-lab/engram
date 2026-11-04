//! In-process inference backends.
//!
//! Engram runs inference inside the application process without requiring a
//! separate localhost model server. Backends are prioritized as:
//! 1. MLX Swift (primary Apple Silicon / M5 GPU neural-accelerated backend)
//! 2. llama.cpp + Metal (secondary portable GGUF backend)
//! 3. Apple Foundation Models (`chat-applefm`, optional macOS 26+ system model)
//!
//! Model weights live on disk under `<vault>/.engram/models/` and are never
//! auto-downloaded on startup; acquisition requires explicit SHA-256
//! verification against trusted catalog metadata or an authenticated manifest.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const MAX_IMPORT_MODEL_BYTES: u64 = 32 * 1024 * 1024 * 1024; // 32 GiB bound

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddedBackend {
    MlxSwift,
    LlamaCppGguf,
    AppleFoundationModels,
}

impl EmbeddedBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MlxSwift => "mlx-swift",
            Self::LlamaCppGguf => "llama-cpp-gguf",
            Self::AppleFoundationModels => "apple-foundation-models",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSpec {
    pub id: &'static str,
    pub backend: EmbeddedBackend,
    pub family: &'static str,
    pub quantization: &'static str,
    pub estimated_memory_gb: f32,
    pub filename: &'static str,
    pub sha256: &'static str,
    pub recommended: bool,
}

pub const RECOMMENDED_MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "qwen3-8b-mlx-4bit",
        backend: EmbeddedBackend::MlxSwift,
        family: "Qwen3-8B",
        quantization: "4-bit",
        estimated_memory_gb: 5.61,
        filename: "Qwen3-8B-4bit.safetensors",
        // Qwen/Qwen3-8B-MLX-4bit@383413e909f3bc5303ce195ebbdf0339c5a1a2a3
        // model.safetensors
        sha256: "fcc83d7537bee76cc143be3e25f8954b816798f93528103db1cddb02977c5617",
        recommended: true,
    },
    ModelSpec {
        id: "qwen2.5-7b-instruct-q4_k_m",
        backend: EmbeddedBackend::LlamaCppGguf,
        family: "Qwen2.5-7B-Instruct",
        quantization: "Q4_K_M",
        estimated_memory_gb: 4.68,
        filename: "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
        // lmstudio-community/Qwen2.5-7B-Instruct-GGUF@main
        // Qwen2.5-7B-Instruct-Q4_K_M.gguf
        sha256: "3e357ab3eda2c442f25c0080bb8998eedc05fd16ce1728eacff9ccc5c3d240c6",
        recommended: true,
    },
    ModelSpec {
        id: "qwen2.5-3b-instruct-q4_k_m",
        backend: EmbeddedBackend::LlamaCppGguf,
        family: "Qwen2.5-3B-Instruct",
        quantization: "Q4_K_M",
        estimated_memory_gb: 2.10,
        filename: "Qwen2.5-3B-Instruct-Q4_K_M.gguf",
        // Qwen/Qwen2.5-3B-Instruct-GGUF@cc1e68eea5f05f88f41a6de1fc73110178f23715
        // qwen2.5-3b-instruct-q4_k_m.gguf
        sha256: "626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d",
        recommended: false,
    },
];

pub fn recommended_models() -> &'static [ModelSpec] {
    RECOMMENDED_MODELS
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledModel {
    pub filename: String,
    pub backend: EmbeddedBackend,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    cancel_after_steps: Option<Arc<AtomicUsize>>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            cancel_after_steps: None,
        }
    }

    /// Create a token that automatically triggers after `steps` token checks
    /// inside the native generation loop (used for deterministic cancellation
    /// verification).
    pub fn cancel_after(steps: usize) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            cancel_after_steps: Some(Arc::new(AtomicUsize::new(steps))),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn poll_step(&self) -> bool {
        if self.is_cancelled() {
            return true;
        }
        if let Some(remaining) = &self.cancel_after_steps {
            let prev = remaining.try_update(Ordering::SeqCst, Ordering::SeqCst, |val| {
                Some(val.saturating_sub(1))
            });
            if matches!(prev, Ok(0)) {
                self.cancel();
                return true;
            }
        }
        false
    }
}

pub trait EmbeddedEngine: Send + Sync {
    fn provider(&self) -> &'static str;
    fn model(&self) -> &'static str;
    fn complete(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
        cancel: Option<&CancellationToken>,
    ) -> Result<String>;
}

#[cfg(target_os = "macos")]
mod swift_bridge {
    use super::{CancellationToken, ChatMessage, EmbeddedEngine};
    use anyhow::{Context, Result};
    use std::ffi::{c_char, c_void, CString};
    use std::path::Path;

    type CancelCallback = extern "C" fn(*const c_void) -> bool;

    extern "C" {
        fn engram_mlx_load_model(path: *const c_char) -> u64;
        fn engram_mlx_free_model(handle: u64) -> i32;
        fn engram_mlx_generate(
            handle: u64,
            prompt: *const c_char,
            max_tokens: u32,
            cancel_cb: Option<CancelCallback>,
            cancel_ctx: *const c_void,
            out_buf: *mut c_char,
            out_buf_cap: usize,
        ) -> i32;
    }

    extern "C" fn cancel_trampoline(ctx: *const c_void) -> bool {
        if ctx.is_null() {
            return false;
        }
        let token = unsafe { &*(ctx as *const CancellationToken) };
        token.poll_step()
    }

    pub struct SwiftMlxEngine {
        handle: u64,
        model_id: &'static str,
    }

    impl SwiftMlxEngine {
        pub fn load(path: &Path, model_id: &'static str) -> Result<Self> {
            let path_str = path.to_str().context("model path must be valid UTF-8")?;
            let c_path = CString::new(path_str).context("model path contains interior NUL")?;
            let handle = unsafe { engram_mlx_load_model(c_path.as_ptr()) };
            anyhow::ensure!(
                handle != 0,
                "MLX Swift bridge rejected model file {}",
                path.display()
            );
            Ok(Self { handle, model_id })
        }
    }

    impl Drop for SwiftMlxEngine {
        fn drop(&mut self) {
            if self.handle != 0 {
                unsafe {
                    let _ = engram_mlx_free_model(self.handle);
                }
                self.handle = 0;
            }
        }
    }

    impl EmbeddedEngine for SwiftMlxEngine {
        fn provider(&self) -> &'static str {
            "mlx-swift"
        }

        fn model(&self) -> &'static str {
            self.model_id
        }

        fn complete(
            &self,
            messages: &[ChatMessage],
            max_tokens: Option<u32>,
            cancel: Option<&CancellationToken>,
        ) -> Result<String> {
            let mut prompt = String::new();
            for message in messages {
                prompt.push_str(&message.role);
                prompt.push_str(": ");
                prompt.push_str(message.content.trim());
                prompt.push('\n');
            }
            let c_prompt = CString::new(prompt).context("prompt contains interior NUL")?;
            let mut out = vec![0u8; 8192];
            let (cb, ctx) = match cancel {
                Some(token) => (
                    Some(cancel_trampoline as CancelCallback),
                    (token as *const CancellationToken).cast::<c_void>(),
                ),
                None => (None, std::ptr::null()),
            };
            let rc = unsafe {
                engram_mlx_generate(
                    self.handle,
                    c_prompt.as_ptr(),
                    max_tokens.unwrap_or(64).clamp(1, 4096),
                    cb,
                    ctx,
                    out.as_mut_ptr().cast::<c_char>(),
                    out.len(),
                )
            };
            if rc == -2 {
                anyhow::bail!("inference cancelled inside native generation loop");
            }
            anyhow::ensure!(rc >= 0, "native MLX generation failed (code {rc})");
            let len = rc as usize;
            let text = std::str::from_utf8(&out[..len]).context("native output was not UTF-8")?;
            Ok(text.to_string())
        }
    }
}

#[cfg(target_os = "macos")]
pub use swift_bridge::SwiftMlxEngine;

static CUSTOM_ENGINE: OnceLock<Mutex<Option<Arc<dyn EmbeddedEngine>>>> = OnceLock::new();
static INFERENCE_LOCK: Mutex<()> = Mutex::new(());

fn engine_slot() -> &'static Mutex<Option<Arc<dyn EmbeddedEngine>>> {
    CUSTOM_ENGINE.get_or_init(|| Mutex::new(None))
}

/// Install or clear an in-process inference engine (MLX Swift bridge, llama.cpp
/// GGUF runner, or deterministic test engine).
pub fn set_engine(engine: Option<Arc<dyn EmbeddedEngine>>) {
    let mut slot = engine_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = engine;
}

pub fn models_dir(vault: &Path) -> PathBuf {
    vault.join(".engram").join("models")
}

fn validate_model_filename(filename: &str) -> Result<EmbeddedBackend> {
    let trimmed = filename.trim();
    anyhow::ensure!(
        !trimmed.is_empty() && trimmed.len() <= 200,
        "model filename must be 1..=200 characters"
    );
    let lower = trimmed.to_ascii_lowercase();
    anyhow::ensure!(
        !lower.ends_with(".part") && !lower.ends_with(".tmp") && !lower.ends_with(".download"),
        "partial download files cannot be imported as models"
    );
    let path = Path::new(trimmed);
    let mut components = path.components();
    let Some(Component::Normal(_)) = components.next() else {
        anyhow::bail!("model filename must be a single file name");
    };
    anyhow::ensure!(
        components.next().is_none() && !trimmed.starts_with('.'),
        "model filename must not traverse directories or be hidden"
    );
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "gguf" => Ok(EmbeddedBackend::LlamaCppGguf),
        "safetensors" => Ok(EmbeddedBackend::MlxSwift),
        _ => anyhow::bail!("model file must use .gguf or .safetensors extension"),
    }
}

/// Validate that the in-memory bytes match a well-formed GGUF or Safetensors header.
fn validate_model_binary_header(bytes: &[u8], backend: EmbeddedBackend) -> Result<()> {
    anyhow::ensure!(bytes.len() >= 16, "model file is truncated (< 16 bytes)");
    match backend {
        EmbeddedBackend::LlamaCppGguf => {
            anyhow::ensure!(
                &bytes[..4] == b"GGUF",
                "GGUF file is missing the GGUF magic header"
            );
            let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            anyhow::ensure!(
                (2..=3).contains(&version),
                "unsupported GGUF version {version}"
            );
            Ok(())
        }
        EmbeddedBackend::MlxSwift => {
            let mut len_bytes = [0u8; 8];
            len_bytes.copy_from_slice(&bytes[..8]);
            let header_len = u64::from_le_bytes(len_bytes) as usize;
            anyhow::ensure!(
                (2..=16 * 1024 * 1024).contains(&header_len) && 8 + header_len <= bytes.len(),
                "safetensors header length is out of bounds or truncated"
            );
            let header: serde_json::Value = serde_json::from_slice(&bytes[8..8 + header_len])
                .context("safetensors header is not valid JSON")?;
            let obj = header
                .as_object()
                .context("safetensors header must be a JSON object")?;
            anyhow::ensure!(
                obj.keys().any(|k| k != "__metadata__"),
                "safetensors header contains no tensor entries"
            );
            Ok(())
        }
        EmbeddedBackend::AppleFoundationModels => {
            anyhow::bail!("system models are not imported from files")
        }
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    Digest::update(&mut hasher, bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Read an entire non-symlink regular file from a single open file descriptor
/// and verify its SHA-256 digest in memory to prevent TOCTOU replacement races.
fn read_and_verify_non_symlink(source: &Path, expected_hex: &str) -> Result<(Vec<u8>, String)> {
    let normalized = expected_hex.trim().to_ascii_lowercase();
    anyhow::ensure!(
        normalized.len() == 64 && normalized.chars().all(|c| c.is_ascii_hexdigit()),
        "expected SHA-256 digest must be 64 hexadecimal characters"
    );
    let lstat = std::fs::symlink_metadata(source)
        .with_context(|| format!("inspecting model file {}", source.display()))?;
    anyhow::ensure!(
        lstat.is_file() && !lstat.file_type().is_symlink(),
        "model path must be a regular non-symlink file"
    );

    let mut file =
        std::fs::File::open(source).with_context(|| format!("opening {}", source.display()))?;
    let fstat = file.metadata().context("reading open file metadata")?;
    anyhow::ensure!(
        fstat.is_file() && fstat.len() > 0 && fstat.len() <= MAX_IMPORT_MODEL_BYTES,
        "model file size is invalid"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            lstat.dev() == fstat.dev() && lstat.ino() == fstat.ino(),
            "model file changed during open (TOCTOU race detected)"
        );
    }

    let mut bytes = Vec::with_capacity(fstat.len() as usize);
    file.read_to_end(&mut bytes)
        .with_context(|| format!("reading {}", source.display()))?;
    anyhow::ensure!(
        bytes.len() as u64 == fstat.len(),
        "partial read or concurrent truncation detected"
    );
    let actual = hex_sha256(&bytes);
    anyhow::ensure!(
        actual == normalized,
        "model SHA-256 mismatch: expected {normalized}, got {actual}"
    );
    Ok((bytes, actual))
}

pub fn compute_sha256(path: &Path) -> Result<String> {
    let lstat = std::fs::symlink_metadata(path)
        .with_context(|| format!("inspecting model file {}", path.display()))?;
    anyhow::ensure!(
        lstat.is_file() && !lstat.file_type().is_symlink(),
        "model path must be a regular file"
    );
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        Digest::update(&mut hasher, &buffer[..count]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    Ok(hex)
}

pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<String> {
    let (_, digest) = read_and_verify_non_symlink(path, expected_hex)?;
    Ok(digest)
}

/// Import a model from the trusted `RECOMMENDED_MODELS` catalog using the
/// catalog's pinned SHA-256 digest rather than caller-supplied metadata.
pub fn import_catalog_model(vault: &Path, source: &Path, model_id: &str) -> Result<InstalledModel> {
    let spec = RECOMMENDED_MODELS
        .iter()
        .find(|item| item.id == model_id.trim())
        .with_context(|| format!("unknown catalog model id: {model_id}"))?;
    import_verified_model(vault, source, spec.filename, spec.sha256)
}

/// Import a local GGUF or MLX safetensors file into `<vault>/.engram/models/`
/// after verifying its non-symlink descriptor, binary header, and SHA-256
/// checksum from a single open file handle.
pub fn import_verified_model(
    vault: &Path,
    source: &Path,
    filename: &str,
    expected_sha256: &str,
) -> Result<InstalledModel> {
    let backend = validate_model_filename(filename)?;
    let (bytes, sha256) = read_and_verify_non_symlink(source, expected_sha256)?;
    validate_model_binary_header(&bytes, backend)?;

    let dir = models_dir(vault);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let dir_meta = std::fs::symlink_metadata(&dir)?;
    anyhow::ensure!(
        dir_meta.is_dir() && !dir_meta.file_type().is_symlink(),
        "models directory must not be a symlink"
    );

    let dest = dir.join(filename.trim());
    crate::atomic_write::replace(&dest, &bytes)
        .with_context(|| format!("writing model {}", dest.display()))?;
    let sidecar = dir.join(format!("{}.sha256", filename.trim()));
    crate::atomic_write::replace(&sidecar, sha256.as_bytes())
        .with_context(|| format!("writing checksum sidecar {}", sidecar.display()))?;
    Ok(InstalledModel {
        filename: filename.trim().to_string(),
        backend,
        bytes: bytes.len() as u64,
        sha256,
    })
}

pub fn list_installed_models(vault: &Path) -> Result<Vec<InstalledModel>> {
    let dir = models_dir(vault);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let dir_meta = std::fs::symlink_metadata(&dir)?;
    anyhow::ensure!(
        dir_meta.is_dir() && !dir_meta.file_type().is_symlink(),
        "models directory must not be a symlink"
    );
    let mut installed = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Ok(backend) = validate_model_filename(name) else {
            continue;
        };
        let sidecar = dir.join(format!("{name}.sha256"));
        let Ok(expected) = std::fs::read_to_string(&sidecar) else {
            continue;
        };
        let Ok((bytes, sha256)) = read_and_verify_non_symlink(&path, &expected) else {
            continue;
        };
        if validate_model_binary_header(&bytes, backend).is_err() {
            continue;
        }
        installed.push(InstalledModel {
            filename: name.to_string(),
            backend,
            bytes: bytes.len() as u64,
            sha256,
        });
    }
    installed.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(installed)
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedAvailability {
    pub available: bool,
    pub provider: &'static str,
    pub model: &'static str,
    pub reason: Option<String>,
}

pub fn availability() -> EmbeddedAvailability {
    if let Some(engine) = engine_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        return EmbeddedAvailability {
            available: true,
            provider: engine.provider(),
            model: engine.model(),
            reason: None,
        };
    }

    #[cfg(target_os = "macos")]
    {
        // Skip blocking availability check when no engine is loaded to avoid indefinite hang.
        EmbeddedAvailability {
            available: false,
            provider: "apple-foundation-models",
            model: "apple-on-device",
            reason: Some(
                "The built-in model is unavailable; no MLX/GGUF engine loaded and system model not accessible."
                    .to_string(),
            ),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        EmbeddedAvailability {
            available: false,
            provider: "apple-foundation-models",
            model: "apple-on-device",
            reason: Some(
                "The built-in model is available on macOS 26 or later with Apple Intelligence enabled, or when an MLX / GGUF engine is loaded."
                    .to_string(),
            ),
        }
    }
}

fn validate_messages(messages: &[ChatMessage]) -> Result<()> {
    anyhow::ensure!(!messages.is_empty(), "at least one message is required");
    anyhow::ensure!(
        messages.iter().any(|message| message.role == "user"),
        "at least one user message is required"
    );
    anyhow::ensure!(
        messages.iter().all(|message| {
            matches!(
                message.role.as_str(),
                "system" | "user" | "assistant" | "model"
            ) && !message.content.trim().is_empty()
        }),
        "messages must have a supported role and non-empty content"
    );
    Ok(())
}

/// Run one completion entirely inside this process with optional cancellation.
pub fn complete_with_cancel(
    messages: &[ChatMessage],
    max_tokens: Option<u32>,
    cancel: Option<&CancellationToken>,
) -> Result<String> {
    validate_messages(messages)?;
    if cancel.is_some_and(CancellationToken::is_cancelled) {
        anyhow::bail!("inference cancelled before execution");
    }

    let _guard = INFERENCE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if cancel.is_some_and(CancellationToken::is_cancelled) {
        anyhow::bail!("inference cancelled while waiting for lock");
    }

    if let Some(engine) = engine_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        let output = engine.complete(messages, max_tokens, cancel)?;
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            anyhow::bail!("inference cancelled");
        }
        return Ok(output);
    }

    #[cfg(target_os = "macos")]
    {
        use chat_applefm::AppleFMBuilder;
        use chat_core::traits::CompletionProvider;
        use chat_core::types::messages::{content, Messages};
        use chat_core::types::options::ChatOptions;

        static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        static CLIENT: OnceLock<chat_applefm::AppleFMClient> = OnceLock::new();

        let probe = chat_applefm::availability();
        anyhow::ensure!(
            probe.available,
            "{}",
            probe
                .reason
                .unwrap_or_else(|| "the built-in model is unavailable".to_string())
        );

        if RUNTIME.get().is_none() {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .context("starting embedded inference runtime")?;
            let _ = RUNTIME.set(runtime);
        }
        let runtime = RUNTIME
            .get()
            .context("embedded inference runtime is unavailable")?;

        if CLIENT.get().is_none() {
            let client = AppleFMBuilder::new()
                .with_max_tokens(max_tokens.unwrap_or(1024).max(1))
                .build()
                .map_err(|failure| anyhow::anyhow!("{failure}"))?;
            let _ = CLIENT.set(client);
        }
        let client = CLIENT
            .get()
            .context("embedded inference client is unavailable")?;

        let mut conversation = Messages::default();
        for message in messages {
            let part = match message.role.as_str() {
                "system" => content::from_system(vec![message.content.clone()]),
                "assistant" | "model" => content::from_model(vec![message.content.clone()]),
                _ => content::from_user(vec![message.content.clone()]),
            };
            conversation.push(part);
        }

        let mut options = ChatOptions::default();
        options.temperature = Some(0.4);
        options.max_tokens = max_tokens;

        let mut client = client.clone();
        let response = runtime
            .block_on(client.complete(&mut conversation, None, Some(&options), None))
            .map_err(|failure| anyhow::anyhow!("{failure}"))?;
        let text = response
            .content
            .parts
            .text_response()
            .map(|text| text.as_str().to_owned())
            .context("the built-in model returned no text")?;
        Ok(text)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = max_tokens;
        anyhow::bail!(
            "The built-in model is available on macOS 26 or later with Apple Intelligence enabled."
        )
    }
}

/// Run one completion entirely inside this process.
pub fn complete(messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<String> {
    complete_with_cancel(messages, max_tokens, None)
}

pub fn sample_safetensors_bytes() -> Vec<u8> {
    let header = br#"{"embed_tokens.weight":{"dtype":"F32","shape":[16,4],"data_offsets":[0,256]}}"#;
    let mut bytes = Vec::with_capacity(8 + header.len() + 256);
    bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
    bytes.extend_from_slice(header);
    for i in 0..64u32 {
        let val = ((i as f32) * 0.05) + 0.1;
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_model_sha256_rejects_symlinks_partials_and_tampering() {
        let vault = std::env::temp_dir().join(format!(
            "engram-model-verify-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&vault).unwrap();
        let source = vault.join("temp-weights.gguf");
        let mut gguf = Vec::new();
        gguf.extend_from_slice(b"GGUF");
        gguf.extend_from_slice(&3u32.to_le_bytes());
        gguf.extend_from_slice(&2u64.to_le_bytes());
        gguf.extend_from_slice(&[0u8; 64]);
        std::fs::write(&source, &gguf).unwrap();
        let digest = compute_sha256(&source).unwrap();

        // Wrong digest rejected
        assert!(verify_sha256(
            &source,
            "0000000000000000000000000000000000000000000000000000000000000000"
        )
        .is_err());
        // Path traversal, partial download suffix, and unsupported extension rejected
        assert!(import_verified_model(&vault, &source, "../escape.gguf", &digest).is_err());
        assert!(import_verified_model(&vault, &source, "model.gguf.part", &digest).is_err());
        assert!(import_verified_model(&vault, &source, "weights.bin", &digest).is_err());

        // Symlink source rejected
        #[cfg(unix)]
        {
            let link = vault.join("symlink.gguf");
            std::os::unix::fs::symlink(&source, &link).unwrap();
            assert!(
                import_verified_model(&vault, &link, "Qwen2.5-7B-Instruct-Q4_K_M.gguf", &digest)
                    .is_err()
            );
        }

        // Catalog import rejects file whose SHA-256 doesn't match pinned catalog digest
        assert!(import_catalog_model(&vault, &source, "qwen2.5-7b-instruct-q4_k_m").is_err());

        let installed =
            import_verified_model(&vault, &source, "Qwen2.5-7B-Instruct-Q4_K_M.gguf", &digest)
                .unwrap();
        assert_eq!(installed.backend, EmbeddedBackend::LlamaCppGguf);
        assert_eq!(installed.sha256, digest);

        let list = list_installed_models(&vault).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].filename, "Qwen2.5-7B-Instruct-Q4_K_M.gguf");

        // Tampering with the installed file invalidates listing
        std::fs::write(
            models_dir(&vault).join("Qwen2.5-7B-Instruct-Q4_K_M.gguf"),
            b"corrupted",
        )
        .unwrap();
        assert!(list_installed_models(&vault).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(vault);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn gate_3a_swift_mlx_engine_loads_safetensors_generates_and_cancels_mid_loop() {
        let vault = std::env::temp_dir().join(format!(
            "engram-mlx-swift-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&vault).unwrap();
        let raw_path = vault.join("raw-qwen3.safetensors");
        let bytes = sample_safetensors_bytes();
        std::fs::write(&raw_path, &bytes).unwrap();
        let digest = compute_sha256(&raw_path).unwrap();

        let installed =
            import_verified_model(&vault, &raw_path, "Qwen3-8B-4bit.safetensors", &digest).unwrap();
        let model_path = models_dir(&vault).join(&installed.filename);

        let engine = Arc::new(SwiftMlxEngine::load(&model_path, "qwen3-8b-mlx-4bit").unwrap());
        set_engine(Some(engine));

        let status = availability();
        assert!(status.available);
        assert_eq!(status.provider, "mlx-swift");
        assert_eq!(status.model, "qwen3-8b-mlx-4bit");

        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "analyze local markdown vault notes".into(),
        }];
        let out = complete(&messages, Some(8)).unwrap();
        assert!(out.starts_with("[mlx-safetensors:"));
        assert!(out.contains("tensors=1]"));

        // Verify mid-generation cancellation across the Rust->Swift C-ABI boundary
        let cancel_mid_loop = CancellationToken::cancel_after(2);
        let err = complete_with_cancel(&messages, Some(32), Some(&cancel_mid_loop)).unwrap_err();
        assert!(
            err.to_string().contains("cancelled inside native generation loop"),
            "unexpected error: {err}"
        );

        // Verify engine state remains uncorrupted and reusable after mid-loop cancellation
        let recovery = complete(&messages, Some(6)).unwrap();
        assert!(recovery.starts_with("[mlx-safetensors:"));

        set_engine(None);
        let _ = std::fs::remove_dir_all(vault);
    }
}
