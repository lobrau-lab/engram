use engram_core::ai::{truncate, validate_local_endpoint};

#[test]
fn test_truncate() {
    assert_eq!(truncate("hello", 10), "hello");
    assert_eq!(truncate("hello world", 5), "hello…");
    assert_eq!(truncate("🦀🦀🦀", 1), "…");
    assert_eq!(truncate("🦀🦀🦀", 4), "🦀…");
    assert_eq!(truncate("🦀🦀🦀", 8), "🦀🦀…");
    assert_eq!(truncate("🦀🦀🦀", 12), "🦀🦀🦀");
    assert_eq!(truncate("🦀🦀🦀", 15), "🦀🦀🦀");
}

#[test]
fn test_validate_local_endpoint() {
    assert!(validate_local_endpoint("http://localhost:8080").is_ok());
    assert!(validate_local_endpoint("http://127.0.0.1:11434").is_ok());
    assert!(validate_local_endpoint("http://[::1]:8080").is_ok());
    assert!(validate_local_endpoint("http://localhost:8080/v1/chat/completions").is_ok());
    assert!(validate_local_endpoint("https://localhost:8080").is_err(), "https not allowed");
    assert!(validate_local_endpoint("http://user:pass@localhost:8080").is_err(), "credentials not allowed");
    assert!(validate_local_endpoint("http://example.com").is_err(), "external IP not allowed");
    assert!(validate_local_endpoint("http://192.168.1.1").is_err(), "external IP not allowed");
    assert!(validate_local_endpoint("not a url").is_err());
}
