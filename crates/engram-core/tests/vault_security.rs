use engram_core::vault::validate_relative_path;
use std::path::Path;

#[test]
fn rejects_parent_traversal() {
    assert!(validate_relative_path(Path::new("../outside.md")).is_err());
}

#[test]
fn accepts_nested_markdown_paths() {
    assert_eq!(
        validate_relative_path(Path::new("notes/today.md")).unwrap(),
        Path::new("notes/today.md")
    );
}

#[test]
fn core_read_note_rejects_parent_traversal() {
    let root = std::env::temp_dir().join(format!("engram-vault-read-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let vault = engram_core::Vault::open(&root).unwrap();
    assert!(vault.read_note("../outside.md").is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn core_read_note_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;
    let root = std::env::temp_dir().join(format!("engram-vault-link-{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!("engram-vault-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.md"), "secret").unwrap();
    symlink(outside.join("secret.md"), root.join("link.md")).unwrap();
    assert!(engram_core::Vault::open(&root)
        .unwrap()
        .read_note("link.md")
        .is_err());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}
