use engram_core::dump::{self, MAX_BYTES};
use engram_core::{ChatOpts, LlmClient, Provider};
use anyhow::Result;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_VAULT: AtomicU64 = AtomicU64::new(0);

struct FakeClient { response: String }
impl LlmClient for FakeClient {
    fn provider(&self) -> Provider { Provider::OpenAiCompatible }
    fn complete(&self, _prompt: &str, _opts: &ChatOpts) -> Result<String> { Ok(self.response.clone()) }
}

fn vault() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("engram-dump-test-{}-{}", std::process::id(), NEXT_VAULT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn analysis_is_separate_and_raw_item_remains_unchanged() {
    let root = vault();
    let item = dump::submit(&root, "note.txt", "file", "Keep this exact source.").unwrap();
    let before = dump::get(&root, &item.id).unwrap();
    let fake = FakeClient { response: r#"{"version":1,"suggestions":[{"id":"one","type":"create_note","path":"notes/source.md","title":"Source","body":"Keep this exact source.","tags":["source"],"rationale":"The source is a note.","evidence":"Keep this exact source."}]}"#.into() };
    let analysis = dump::analyze(&root, &item.id, &fake).unwrap();
    assert_eq!(analysis.source_id, item.id);
    assert!(analysis.suggestions[0].eligible);
    let after = dump::get(&root, &item.id).unwrap();
    assert_eq!(before.content, after.content);
    assert_eq!(before.hash, after.hash);
    assert!(dump::analysis_path(&root, &item.id).exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unsupported_or_unsafe_model_suggestions_are_rejected() {
    let root = vault();
    let item = dump::submit(&root, "note.txt", "file", "Evidence").unwrap();
    let fake = FakeClient { response: r#"{"version":1,"suggestions":[{"id":"bad","type":"create_note","path":"../escape.md","title":"Bad","body":"x","rationale":"bad","evidence":"Evidence"}]}"#.into() };
    assert!(dump::analyze(&root, &item.id, &fake).is_err());
    assert_eq!(dump::get(&root, &item.id).unwrap().content, "Evidence");
    assert!(!dump::analysis_path(&root, &item.id).exists());
    assert!(dump::submit(&root, "bad.pdf", "file", "x").is_err());
    assert!(dump::submit(&root, "big.txt", "file", &"x".repeat(MAX_BYTES + 1)).is_err());
    let _ = std::fs::remove_dir_all(root);
}
