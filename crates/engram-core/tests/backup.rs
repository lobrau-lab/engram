use engram_core::backup;
use std::fs;

fn temp(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "engram-{name}-{}-{}",
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
fn encrypted_backup_round_trip_and_wrong_password_fails() {
    let source = temp("backup-source");
    let restored = temp("backup-restored");
    let file = source.join("note.md");
    fs::create_dir_all(&source).unwrap();
    fs::write(&file, "# secret\n").unwrap();
    let archive = source.with_extension("engram");
    let manifest = backup::create(&source, &archive, "correct horse").unwrap();
    assert_eq!(manifest.files, 1);
    assert_eq!(manifest.version, 2);
    assert!(backup::restore(&archive, &restored, "wrong horse").is_err());
    backup::restore(&archive, &restored, "correct horse").unwrap();
    assert_eq!(
        fs::read_to_string(restored.join("note.md")).unwrap(),
        "# secret\n"
    );
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(restored);
    let _ = fs::remove_file(archive);
}

#[test]
fn v1_legacy_backup_compatibility() {
    let source = temp("v1-legacy-source");
    let archive = temp("v1-legacy.engram");
    let restored = temp("v1-legacy-restored");

    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("legacy_note.md"),
        "# Legacy Note V1\nBody content.",
    )
    .unwrap();
    fs::write(source.join("nested.md"), "# Nested Note\nMore text.").unwrap();

    // Create using the explicit legacy v1 test helper
    let manifest = backup::create_v1_legacy_for_test(&source, &archive, "v1password").unwrap();
    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.files, 2);

    // Verify file starts with MAGIC_V1
    let archive_bytes = fs::read(&archive).unwrap();
    assert!(archive_bytes.starts_with(backup::MAGIC_V1));

    // Restore using the unified restore function
    let restored_manifest = backup::restore(&archive, &restored, "v1password").unwrap();
    assert_eq!(restored_manifest.version, 1);
    assert_eq!(restored_manifest.files, 2);
    assert_eq!(
        fs::read_to_string(restored.join("legacy_note.md")).unwrap(),
        "# Legacy Note V1\nBody content."
    );
    assert_eq!(
        fs::read_to_string(restored.join("nested.md")).unwrap(),
        "# Nested Note\nMore text."
    );

    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(restored);
    let _ = fs::remove_file(archive);
}

#[test]
fn v2_archive_size_is_dramatically_smaller_than_v1() {
    let source = temp("bench-compare-source");
    let archive_v1 = temp("compare-v1.engram");
    let archive_v2 = temp("compare-v2.engram");

    fs::create_dir_all(&source).unwrap();
    for i in 0..50 {
        let content = format!(
            "# Note {i}\n\nLorem ipsum dolor sit amet, consectetur adipiscing elit.\n\
             Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n\
             Section {i} detailing local-first distributed graph processing.\n"
        );
        fs::write(source.join(format!("note_{i:03}.md")), content).unwrap();
    }

    let manifest_v1 =
        backup::create_v1_legacy_for_test(&source, &archive_v1, "compare_pass").unwrap();
    let manifest_v2 = backup::create(&source, &archive_v2, "compare_pass").unwrap();

    assert_eq!(manifest_v1.files, 50);
    assert_eq!(manifest_v2.files, 50);

    let v1_size = fs::metadata(&archive_v1).unwrap().len();
    let v2_size = fs::metadata(&archive_v2).unwrap().len();

    // V2 binary compressed archive should be at least 4x smaller than V1 JSON byte-array
    println!("V1 archive size: {v1_size} bytes, V2 archive size: {v2_size} bytes");
    assert!(
        v2_size < v1_size / 4,
        "Expected V2 ({v2_size} bytes) to be < 25% of V1 ({v1_size} bytes)"
    );

    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_file(archive_v1);
    let _ = fs::remove_file(archive_v2);
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
    fs::write(&truncated, b"ENGRAM-BACKUP-2\n\x01").unwrap();
    let fresh = source.with_extension("fresh");
    assert!(backup::restore(&truncated, &fresh, "password").is_err());
    assert!(!fresh.exists());
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(destination);
    let _ = fs::remove_file(archive);
    let _ = fs::remove_file(truncated);
}

#[test]
fn backup_and_restore_reject_empty_passwords() {
    let source = temp("empty-pw-source");
    let archive = temp("empty-pw.engram");
    let dest = temp("empty-pw-dest");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("test.md"), "content").unwrap();

    let create_err = backup::create(&source, &archive, "   ").unwrap_err();
    assert!(create_err.to_string().contains("password cannot be empty"));

    backup::create(&source, &archive, "valid-pass").unwrap();
    let restore_err = backup::restore(&archive, &dest, "").unwrap_err();
    assert!(restore_err.to_string().contains("password cannot be empty"));

    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_file(archive);
}

#[test]
fn restore_rejects_broken_symlinks_at_destination() {
    let source = temp("symlink-source");
    let archive = temp("symlink.engram");
    let parent = temp("symlink-parent");
    let broken_dest = parent.join("broken_target");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&parent).unwrap();
    fs::write(source.join("note.md"), "data").unwrap();
    backup::create(&source, &archive, "password").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(parent.join("nonexistent_file"), &broken_dest).unwrap();
        let err = backup::restore(&archive, &broken_dest, "password").unwrap_err();
        assert!(err.to_string().contains("destination already exists"));
    }

    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(parent);
    let _ = fs::remove_file(archive);
}

#[test]
fn stale_staging_directories_are_pruned_before_restore() {
    let source = temp("stale-source");
    let archive = temp("stale.engram");
    let parent = temp("stale-parent");
    let dest = parent.join("target_vault");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&parent).unwrap();
    fs::write(source.join("doc.md"), "# Document").unwrap();
    backup::create(&source, &archive, "password").unwrap();

    let stale_dir = parent.join(".target_vault.engram-restore-999999-0");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(stale_dir.join("stale.txt"), "leftover").unwrap();
    let stale_file = fs::File::open(&stale_dir).unwrap();
    stale_file
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60))
        .unwrap();
    assert!(stale_dir.exists());

    let manifest = backup::restore(&archive, &dest, "password").unwrap();
    assert_eq!(manifest.files, 1);
    assert!(dest.join("doc.md").exists());
    assert!(!stale_dir.exists());

    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(parent);
    let _ = fs::remove_file(archive);
}

#[test]
fn fresh_staging_from_another_process_is_not_deleted() {
    let source = temp("live-staging-source");
    let archive = temp("live-staging.engram");
    let parent = temp("live-staging-parent");
    let dest = parent.join("target_vault");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&parent).unwrap();
    fs::write(source.join("doc.md"), "# Document").unwrap();
    backup::create(&source, &archive, "password").unwrap();

    // A different PID with a fresh mtime is a live restore, not a leftover.
    let live_dir = parent.join(".target_vault.engram-restore-999999-0");
    fs::create_dir_all(&live_dir).unwrap();
    fs::write(live_dir.join("in-progress.txt"), "live").unwrap();

    let manifest = backup::restore(&archive, &dest, "password").unwrap();
    assert_eq!(manifest.files, 1);
    assert!(live_dir.join("in-progress.txt").exists());

    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(parent);
    let _ = fs::remove_file(archive);
}
