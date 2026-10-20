use engram_core::sync::{save_manifest, Entry, FolderSync, Manifest, SyncBackend};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "engram-sync-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn entry(path: &str) -> Entry {
    Entry {
        path: path.into(),
        hash: "unused".into(),
        mtime: 1,
        size: 1,
        device: "remote".into(),
        lamport: 2,
    }
}

fn manifest(entries: Vec<Entry>) -> Manifest {
    Manifest {
        version: 1,
        generated_by: "remote".into(),
        lamport: 2,
        entries,
    }
}

#[test]
fn export_import_round_trip() {
    let root = temp_dir();
    let vault = root.join("vault");
    let sync = root.join("sync");
    let restored = root.join("restored");
    fs::create_dir_all(&vault).unwrap();
    fs::create_dir_all(&sync).unwrap();
    fs::create_dir_all(&restored).unwrap();
    fs::write(vault.join("note.md"), "# Note\n\nPortable body.\n").unwrap();

    FolderSync::new("local", false)
        .export(&vault, &sync)
        .unwrap();
    assert!(sync.join("note.md").is_file());

    FolderSync::new("restored", false)
        .import(&restored, &sync)
        .unwrap();
    assert_eq!(
        fs::read_to_string(restored.join("note.md")).unwrap(),
        "# Note\n\nPortable body.\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_traversal_entry_without_writing_outside_the_vault() {
    let root = temp_dir();
    let vault = root.join("a/b/vault");
    let sync = root.join("a/sync");
    fs::create_dir_all(&vault).unwrap();
    fs::create_dir_all(&sync).unwrap();
    fs::write(root.join("owned.md"), "outside").unwrap();
    save_manifest(&sync, &manifest(vec![entry("../../owned.md")])).unwrap();

    let result = FolderSync::new("local", false).import(&vault, &sync);
    assert!(result.is_err(), "traversal entry must be rejected");
    assert!(!root.join("a/owned.md").exists(), "wrote outside the vault");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_hidden_and_non_markdown_entries() {
    let root = temp_dir();
    let vault = root.join("vault");
    let sync = root.join("sync");
    fs::create_dir_all(&vault).unwrap();
    fs::create_dir_all(&sync).unwrap();

    for path in [".engram/evil.md", "note.txt", "notes/.hidden.md"] {
        save_manifest(&sync, &manifest(vec![entry(path)])).unwrap();
        assert!(
            FolderSync::new("local", false)
                .import(&vault, &sync)
                .is_err(),
            "accepted unsafe manifest entry: {path}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape_inside_the_vault() {
    use std::os::unix::fs::symlink;

    let root = temp_dir();
    let vault = root.join("vault");
    let outside = root.join("outside");
    let sync = root.join("sync");
    fs::create_dir_all(&vault).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(&sync).unwrap();
    symlink(&outside, vault.join("link")).unwrap();
    fs::create_dir_all(sync.join("link")).unwrap();
    fs::write(sync.join("link/note.md"), "escaped").unwrap();
    save_manifest(&sync, &manifest(vec![entry("link/note.md")])).unwrap();

    let result = FolderSync::new("local", false).import(&vault, &sync);
    assert!(result.is_err(), "symlink escape must be rejected");
    assert!(!outside.join("note.md").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_source_symlink_outside_the_sync_folder() {
    #[cfg(not(unix))]
    return;
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let root = temp_dir();
        let vault = root.join("vault");
        let sync = root.join("sync");
        fs::create_dir_all(&vault).unwrap();
        fs::create_dir_all(&sync).unwrap();
        let secret = root.join("secret.md");
        fs::write(&secret, "secret").unwrap();
        symlink(&secret, sync.join("note.md")).unwrap();
        save_manifest(&sync, &manifest(vec![entry("note.md")])).unwrap();

        assert!(FolderSync::new("local", false)
            .import(&vault, &sync)
            .is_err());
        assert!(!vault.join("note.md").exists());
        let _ = fs::remove_dir_all(root);
    }
}
