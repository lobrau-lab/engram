use engram_core::backup;
use std::fs;

fn temp(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("engram-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    path
}

#[test]
fn encrypted_backup_round_trip_and_wrong_password_fails() {
    let source = temp("backup-source");
    let restored = temp("backup-restored");
    let file = source.join("note.md");
    fs::create_dir_all(&source).unwrap();
    fs::write(&file, "# secret\n").unwrap();
    let archive = source.with_extension("engram");
    let manifest = backup::create(&source, &archive, "correct horse").unwrap();
    assert_eq!(manifest.files, 1);
    assert!(backup::restore(&archive, &restored, "wrong horse").is_err());
    backup::restore(&archive, &restored, "correct horse").unwrap();
    assert_eq!(
        fs::read_to_string(restored.join("note.md")).unwrap(),
        "# secret\n"
    );
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(restored);
}

#[test]
fn restore_refuses_existing_destination_and_truncated_headers() {
    let source = temp("backup-existing-source");
    let destination = temp("backup-existing-destination");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("note.md"), "note").unwrap();
    let archive = source.with_extension("engram");
    backup::create(&source, &archive, "password").unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("keep.md"), "keep").unwrap();
    assert!(backup::restore(&archive, &destination, "password").is_err());
    assert_eq!(
        fs::read_to_string(destination.join("keep.md")).unwrap(),
        "keep"
    );
    let truncated = source.with_extension("truncated.engram");
    fs::write(&truncated, b"ENGRAM-BACKUP-1\n\x01").unwrap();
    let fresh = source.with_extension("fresh");
    assert!(backup::restore(&truncated, &fresh, "password").is_err());
    assert!(!fresh.exists());
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(destination);
}
