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

#[test]
fn review_accept_apply_is_idempotent_and_preserves_source() {
    let root = vault();
    let item = dump::submit(&root, "note.txt", "file", "Evidence for a note.").unwrap();
    let fake = FakeClient { response: r#"{"version":1,"suggestions":[{"id":"one","type":"create_note","path":"notes/source.md","title":"Source","body":"Evidence for a note.","rationale":"create","evidence":"Evidence for a note."}]}"#.into() };
    dump::analyze(&root, &item.id, &fake).unwrap();
    dump::review(&root, &item.id, "s0-one", "accepted", Some("notes/edited.md".into()), Some("Edited".into()), Some("Edited body".into())).unwrap();
    let report = dump::apply(&root, &item.id, &["s0-one".into()]).unwrap();
    assert_eq!(report.applied, 1);
    assert!(root.join("notes/edited.md").exists());
    assert_eq!(dump::get(&root, &item.id).unwrap().content, "Evidence for a note.");
    let second = dump::apply(&root, &item.id, &["s0-one".into()]).unwrap();
    assert_eq!(second.skipped, 1);
    let audit = std::fs::read_to_string(root.join(".engram/dump-audit.jsonl")).unwrap();
    assert!(audit.contains("s0-one"));
    dump::undo(&root, &item.id, "s0-one").unwrap();
    assert!(!root.join("notes/edited.md").exists());
    assert_eq!(dump::get(&root, &item.id).unwrap().content, "Evidence for a note.");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_rejects_existing_destination_and_stale_source() {
    let root = vault();
    let item = dump::submit(&root, "note.txt", "file", "Evidence").unwrap();
    let fake = FakeClient { response: r#"{"version":1,"suggestions":[{"id":"one","type":"create_note","path":"note.md","title":"N","body":"Evidence","rationale":"create","evidence":"Evidence"}]}"#.into() };
    dump::analyze(&root, &item.id, &fake).unwrap();
    dump::review(&root, &item.id, "s0-one", "accepted", None, None, None).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("note.md"), "existing").unwrap();
    let conflict_report = dump::apply(&root, &item.id, &["s0-one".into()]).unwrap();
    assert_eq!(conflict_report.errors.len(), 1);
    let item_path = root.join(".engram/dump").join(format!("{}.json", item.id));
    let mut raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&item_path).unwrap()).unwrap();
    raw["content"] = serde_json::Value::String("changed".into());
    raw["hash"] = serde_json::Value::String("changed-hash".into());
    std::fs::write(item_path, serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(dump::apply(&root, &item.id, &["s0-one".into()]).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn partial_apply_reports_success_and_failure_without_claiming_batch_success() {
    let root = vault();
    let item = dump::submit(&root, "note.txt", "file", "Evidence one and two").unwrap();
    let fake = FakeClient { response: r#"{"version":1,"suggestions":[{"id":"one","type":"create_note","path":"one.md","title":"One","body":"Evidence one","rationale":"one","evidence":"Evidence one and two"},{"id":"two","type":"create_note","path":"two.md","title":"Two","body":"Evidence two","rationale":"two","evidence":"Evidence one and two"}]}"#.into() };
    dump::analyze(&root, &item.id, &fake).unwrap();
    dump::review(&root, &item.id, "s0-one", "accepted", None, None, None).unwrap();
    dump::review(&root, &item.id, "s1-two", "accepted", None, None, None).unwrap();
    std::fs::write(root.join("two.md"), "conflict").unwrap();
    let report = dump::apply(&root, &item.id, &["s0-one".into(), "s1-two".into()]).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(report.errors.len(), 1);
    assert!(root.join("one.md").exists());
    assert_eq!(std::fs::read_to_string(root.join("two.md")).unwrap(), "conflict");
    let _ = std::fs::remove_dir_all(root);
}
