use engram_core::error::{EngramError, ErrorKind};
use engram_core::events::{EngramEvent, EventStage};

#[test]
fn errors_expose_stable_user_facing_categories() {
    let error = EngramError::invalid_input("vault path escapes the selected root");
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), "vault path escapes the selected root");
}

#[test]
fn events_serialize_stage_and_progress() {
    let event = EngramEvent::Progress {
        stage: EventStage::Crawling,
        completed: 2,
        total: Some(5),
        message: "scanning notes".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("crawling"));
    assert!(json.contains("\"completed\":2"));
}
