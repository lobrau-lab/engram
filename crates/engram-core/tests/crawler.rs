use engram_core::crawler::Crawler;
use std::fs;

#[test]
fn crawls_only_supported_local_text_files() {
    let root = std::env::temp_dir().join(format!("engram-crawler-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("note.md"), "# Note\n\n[[Other]] #tag").unwrap();
    fs::write(root.join("image.bin"), [0, 1, 2]).unwrap();

    let report = Crawler.scan_roots(std::slice::from_ref(&root)).unwrap();
    assert_eq!(report.documents, 1);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.links, 1);
    assert_eq!(report.tags, 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_network_roots() {
    let error = Crawler.scan_roots(&["https://example.com".into()]);
    assert!(error.is_err());
}
