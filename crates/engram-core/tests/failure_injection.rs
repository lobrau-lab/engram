use engram_core::backup::{self, MAGIC_V2};
use engram_core::embed::HashingEmbedder;
use engram_core::index::Index;
use engram_core::vault::{Note, Vault};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "engram-fail-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}

#[test]
fn test_corrupted_archive_byte_mutation() {
    let source = temp("corrupt-source");
    let archive = temp("corrupt.engram");
    let dest = temp("corrupt-dest");

    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("valid.md"), "# Valid Note\nSome content.").unwrap();
    backup::create(&source, &archive, "test_password").unwrap();

    let valid_bytes = fs::read(&archive).unwrap();
    assert!(valid_bytes.len() > 60);

    // Test 1: Corrupt Magic Header
    let mut bad_magic = valid_bytes.clone();
    bad_magic[0] ^= 0xFF;
    let bad_magic_file = temp("bad_magic.engram");
    fs::write(&bad_magic_file, &bad_magic).unwrap();
    let err = backup::restore(&bad_magic_file, &dest, "test_password").unwrap_err();
    assert!(err.to_string().contains("unrecognized magic signature"));
    assert!(!dest.exists());

    // Test 2: Truncate Salt Length
    let truncated_salt_len = valid_bytes[..MAGIC_V2.len() + 2].to_vec();
    let trunc_file = temp("trunc_salt.engram");
    fs::write(&trunc_file, &truncated_salt_len).unwrap();
    assert!(backup::restore(&trunc_file, &dest, "test_password").is_err());
    assert!(!dest.exists());

    // Test 3: Flip byte in ciphertext (GCM authentication tag failure)
    let mut bad_ciphertext = valid_bytes.clone();
    let last_idx = bad_ciphertext.len() - 1;
    bad_ciphertext[last_idx] ^= 0x01;
    let bad_cipher_file = temp("bad_cipher.engram");
    fs::write(&bad_cipher_file, &bad_ciphertext).unwrap();
    let err = backup::restore(&bad_cipher_file, &dest, "test_password").unwrap_err();
    assert!(err.to_string().contains("authentication failed"));
    assert!(!dest.exists());

    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_file(archive);
    let _ = fs::remove_file(bad_magic_file);
    let _ = fs::remove_file(trunc_file);
    let _ = fs::remove_file(bad_cipher_file);
}

#[test]
fn empty_vault_rebuild_clears_previous_index() {
    let dir = temp("sqlite-rollback");
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("index.sqlite");

    let index = Index::open(&db_path).unwrap();
    let embedder = HashingEmbedder::new(64);

    let initial_note = Note {
        id: "initial.md".into(),
        path: dir.join("initial.md"),
        title: "Initial".into(),
        body: "First searchable note".into(),
        tags: vec!["initial".into()],
        links: vec![],
        mtime: 100,
    };
    index.rebuild(&[initial_note], &embedder, 100).unwrap();

    let hits = index.search("searchable", false, 10).unwrap();
    assert_eq!(hits.len(), 1);

    let empty_dir = temp("empty-vault");
    let empty_vault = Vault::open(&empty_dir).unwrap();
    let count = index
        .rebuild_from_vault(&empty_vault, &embedder, 101)
        .unwrap();
    assert_eq!(count, 0);
    assert!(index.search("searchable", false, 10).unwrap().is_empty());

    drop(index);
    let _ = fs::remove_dir_all(dir);
    let _ = fs::remove_dir_all(empty_dir);
}

#[test]
fn test_concurrent_search_during_indexing() {
    let dir = temp("concurrent-vault");
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("index.sqlite");

    for i in 0..20 {
        let next_idx = (i + 1) % 20;
        fs::write(
            dir.join(format!("note_{i:02}.md")),
            format!(
                "# Topic {i}\nBody content with keyword searchable and [[note_{next_idx:02}]].\n"
            ),
        )
        .unwrap();
    }

    let vault = Vault::open(&dir).unwrap();
    let writer_index = Index::open(&db_path).unwrap();
    let embedder = HashingEmbedder::new(64);

    writer_index
        .rebuild_from_vault(&vault, &embedder, 100)
        .unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let mut search_handles = Vec::new();

    // Spawn 4 concurrent readers, each with its own connection in WAL mode
    for _ in 0..4 {
        let db_clone = db_path.clone();
        let s = Arc::clone(&stop);
        search_handles.push(thread::spawn(move || {
            let reader_index = Index::open(&db_clone).unwrap();
            let mut queries = 0usize;
            while !s.load(Ordering::Relaxed) {
                let hits = reader_index.search("searchable", false, 10).unwrap();
                assert!(!hits.is_empty());
                queries += 1;
                thread::sleep(Duration::from_millis(2));
            }
            queries
        }));
    }

    // Main thread performs concurrent full streaming rebuilds
    for iteration in 0..5 {
        let note_file = dir.join(format!("note_{:02}.md", iteration % 20));
        fs::write(
            &note_file,
            format!(
                "# Topic Updated {iteration}\nBody with searchable keyword and updated content.\n"
            ),
        )
        .unwrap();
        let _ = writer_index
            .rebuild_from_vault(&vault, &embedder, 200 + iteration as i64)
            .unwrap();
        thread::sleep(Duration::from_millis(15));
    }

    stop.store(true, Ordering::Relaxed);

    for h in search_handles {
        let queries_done = h.join().unwrap();
        assert!(queries_done > 0);
    }

    drop(writer_index);
    let _ = fs::remove_dir_all(dir);
}
