use engram_core::embed::HashingEmbedder;
use engram_core::index::Index;
use engram_core::vault::Note;
use std::path::PathBuf;

fn create_temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "engram-index-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_note(id: &str, title: &str, body: &str, tags: Vec<&str>) -> Note {
    Note {
        id: id.to_string(),
        path: PathBuf::from(id),
        title: title.to_string(),
        body: body.to_string(),
        tags: tags.into_iter().map(|s| s.to_string()).collect(),
        links: vec![],
        mtime: 0,
    }
}

#[test]
fn test_index_rebuild_and_search() {
    let dir = create_temp_dir();
    let index = Index::open(&dir.join("index.sqlite")).unwrap();
    let embedder = HashingEmbedder::new(32);

    let notes = vec![
        create_note("1", "Hello World", "This is a test note.", vec!["test"]),
        create_note("2", "Rust Programming", "Rust is awesome.", vec!["rust", "programming"]),
    ];

    index.rebuild(&notes, &embedder, 12345).unwrap();

    assert!(index.has_state_notes().unwrap());

    let last_indexed = index.get_last_indexed().unwrap();
    assert_eq!(last_indexed, Some(12345));

    let search_results = index.search("test", false, 10).unwrap();
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0].id, "1");

    let search_results = index.search("rust", false, 10).unwrap();
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0].id, "2");

    // Test that searching for a non-existent word returns nothing
    let search_results = index.search("python", false, 10).unwrap();
    assert_eq!(search_results.len(), 0);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_index_upsert() {
    let dir = create_temp_dir();
    let index = Index::open(&dir.join("index.sqlite")).unwrap();
    let embedder = HashingEmbedder::new(32);

    let note1 = create_note("1", "Initial Note", "Initial body.", vec![]);
    index.upsert(&note1, &embedder).unwrap();

    let search_results = index.search("Initial", false, 10).unwrap();
    assert_eq!(search_results.len(), 1);

    // Update the note
    let note1_updated = create_note("1", "Updated Note", "Updated body.", vec![]);
    index.upsert(&note1_updated, &embedder).unwrap();

    let search_results = index.search("Initial", false, 10).unwrap();
    assert_eq!(search_results.len(), 0);

    let search_results = index.search("Updated", false, 10).unwrap();
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0].title, "Updated Note");

    // Check note body fetching
    let body = index.get_note_body("1").unwrap();
    assert_eq!(body, Some("Updated body.".to_string()));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_index_clear() {
    let dir = create_temp_dir();
    let index = Index::open(&dir.join("index.sqlite")).unwrap();
    let embedder = HashingEmbedder::new(32);

    let note = create_note("1", "To be cleared", "Will be removed soon.", vec![]);
    index.upsert(&note, &embedder).unwrap();

    assert!(index.has_state_notes().unwrap());

    index.clear().unwrap();

    assert!(!index.has_state_notes().unwrap());

    let search_results = index.search("cleared", false, 10).unwrap();
    assert_eq!(search_results.len(), 0);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_index_all_notes_lite_and_embeddings() {
    let dir = create_temp_dir();
    let index = Index::open(&dir.join("index.sqlite")).unwrap();
    let embedder = HashingEmbedder::new(32);

    let note = create_note("1", "Lite Test", "Body.", vec!["tag1"]);
    index.upsert(&note, &embedder).unwrap();

    let lite_notes = index.all_notes_lite().unwrap();
    assert_eq!(lite_notes.len(), 1);
    assert_eq!(lite_notes[0].id, "1");
    assert_eq!(lite_notes[0].title, "Lite Test");
    assert_eq!(lite_notes[0].tags, vec!["tag1"]);

    let notes_with_emb = index.notes_with_embeddings().unwrap();
    assert_eq!(notes_with_emb.len(), 1);
    assert_eq!(notes_with_emb[0].0.id, "1");
    assert_eq!(notes_with_emb[0].1.len(), 32);

    let _ = std::fs::remove_dir_all(dir);
}
