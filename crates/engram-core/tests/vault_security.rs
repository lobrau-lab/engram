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
