//! In-process inference backends.
//!
//! The default desktop path does not require a separate model server. On
//! macOS 26+ the operating system ships a Foundation Models model; this
//! module talks to it through the `chat-applefm` bridge. Other platforms
//! report the backend as unavailable instead of silently reaching the
//! network.

#[allow(unused_imports)]
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedAvailability {
    pub available: bool,
    pub provider: &'static str,
    pub model: &'static str,
    pub reason: Option<String>,
}

pub fn availability() -> EmbeddedAvailability {
    #[cfg(target_os = "macos")]
    {
        let probe = chat_applefm::availability();
        EmbeddedAvailability {
            available: probe.available,
            provider: "apple-foundation-models",
            model: "apple-on-device",
            reason: probe.reason,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        EmbeddedAvailability {
            available: false,
            provider: "apple-foundation-models",
            model: "apple-on-device",
            reason: Some(
                "The built-in model is available on macOS 26 or later with Apple Intelligence enabled."
                    .to_string(),
            ),
        }
    }
}

/// Run one completion entirely inside this process.
///
/// `messages` uses the same role names as the OpenAI chat format. The
/// caller is responsible for keeping the total prompt within the model's
/// context window; the web client already packs context against a budget.
pub fn complete(messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<String> {
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

    #[cfg(target_os = "macos")]
    {
        use chat_applefm::AppleFMBuilder;
        use chat_core::traits::CompletionProvider;
        use chat_core::types::messages::{content, Messages};
        use chat_core::types::options::ChatOptions;
        use std::sync::OnceLock;

        // The on-device provider is a shared system resource. Serialize
        // completions while still allowing status, takeover, and file
        // requests to run on other connection threads.
        static INFERENCE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = INFERENCE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

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

        // The client's session is shared across clones and serialized
        // internally; cloning avoids holding a lock across the bridge call.
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
