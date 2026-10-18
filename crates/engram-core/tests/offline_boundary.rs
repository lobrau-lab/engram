use engram_core::ai::{validate_local_endpoint, ChatOpts, LlmClient, OpenAiCompat};
use engram_core::Provider;

#[test]
fn allows_plain_http_loopback_forms() {
    for endpoint in [
        "http://localhost:8080/v1",
        "http://127.0.0.1:1234/v1",
        "http://[::1]:8080/v1",
    ] {
        assert!(validate_local_endpoint(endpoint).is_ok(), "{endpoint}");
    }
}

#[test]
fn rejects_remote_https_domain_userinfo_and_malformed_endpoints() {
    for endpoint in [
        "https://localhost:8080/v1",
        "http://example.com/v1",
        "http://127.0.0.2:8080/v1",
        "http://localhost.evil/v1",
        "http://user:password@localhost:8080/v1",
        "not a url",
    ] {
        assert!(validate_local_endpoint(endpoint).is_err(), "{endpoint}");
    }
}

#[test]
fn invalid_inference_endpoint_fails_before_transport() {
    let client = OpenAiCompat::new(
        "https://example.com/v1",
        "model",
        None,
        Provider::OpenAiCompatible,
    );
    let error = client.complete("test", &ChatOpts::default()).unwrap_err();
    assert!(error.to_string().contains("plain HTTP"));
}
