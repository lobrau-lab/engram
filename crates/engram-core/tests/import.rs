use engram_core::import::import_file;

#[test]
fn html_is_extracted_locally_and_submitted_to_dump() {
    let root = std::env::temp_dir().join(format!("engram-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let record = import_file(
        &root,
        "page.html",
        b"<html><script>bad()</script><h1>Hello</h1><p>World</p></html>",
        "file-picker",
    )
    .unwrap();
    assert_eq!(record.status, "complete");
    assert!(record.dump_id.is_some());
    let dump = engram_core::dump::list(&root).unwrap();
    assert!(dump[0].content.contains("Hello"));
    assert!(!dump[0].content.contains("bad"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unsupported_pdf_is_preserved_without_fake_extraction() {
    let root = std::env::temp_dir().join(format!("engram-import-pdf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let record = import_file(&root, "scan.pdf", b"not a pdf", "file-picker").unwrap();
    assert_eq!(record.status, "unsupported");
    assert!(record.dump_id.is_none());
    assert_eq!(record.bytes, 9);
    assert!(root
        .join(".engram/imports/originals")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let _ = std::fs::remove_dir_all(root);
}
