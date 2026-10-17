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
    assert_eq!(fs::read_to_string(restored.join("note.md")).unwrap(), "# secret\n");
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(restored);
}
