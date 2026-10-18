use engram_core::atomic_write::{replace, TEMP_MARKER};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "engram-atomic-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn replaces_with_a_complete_payload_and_leaves_no_temp_file() {
    let dir = temp_dir();
    let path = dir.join("metadata.json");
    fs::write(&path, "old").unwrap();
    replace(&path, br#"{"new":true}"#).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"new":true}"#);
    let names: Vec<_> = fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert!(!names.iter().any(|name| {
        name.to_string_lossy().contains(TEMP_MARKER)
    }), "leftover entries: {names:?}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn failed_replacement_preserves_existing_destination_and_cleans_temp() {
    let dir = temp_dir();
    let destination = dir.join("existing-directory");
    fs::create_dir(&destination).unwrap();
    replace(&destination, b"must not replace a directory").unwrap_err();
    assert!(destination.is_dir());
    assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
        entry.unwrap().file_name().to_string_lossy().contains(TEMP_MARKER)
    }));
    let _ = fs::remove_dir_all(dir);
}
