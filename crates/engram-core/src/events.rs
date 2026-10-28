use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventStage {
    Indexing,
    Crawling,
    Inference,
    Backup,
    Recovery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EngramEvent {
    Progress {
        stage: EventStage,
        completed: usize,
        total: Option<usize>,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_event_stage_serialization() {
        assert_eq!(
            serde_json::to_value(EventStage::Indexing).unwrap(),
            json!("indexing")
        );
        assert_eq!(
            serde_json::to_value(EventStage::Crawling).unwrap(),
            json!("crawling")
        );
        assert_eq!(
            serde_json::to_value(EventStage::Inference).unwrap(),
            json!("inference")
        );
        assert_eq!(
            serde_json::to_value(EventStage::Backup).unwrap(),
            json!("backup")
        );
        assert_eq!(
            serde_json::to_value(EventStage::Recovery).unwrap(),
            json!("recovery")
        );
    }

    #[test]
    fn test_engram_event_serialization() {
        let event = EngramEvent::Progress {
            stage: EventStage::Indexing,
            completed: 50,
            total: Some(100),
            message: "Indexing files".to_string(),
        };

        let expected = json!({
            "type": "progress",
            "stage": "indexing",
            "completed": 50,
            "total": 100,
            "message": "Indexing files"
        });

        assert_eq!(serde_json::to_value(&event).unwrap(), expected);

        let event_no_total = EngramEvent::Progress {
            stage: EventStage::Backup,
            completed: 10,
            total: None,
            message: "Backing up".to_string(),
        };

        let expected_no_total = json!({
            "type": "progress",
            "stage": "backup",
            "completed": 10,
            "total": null,
            "message": "Backing up"
        });

        assert_eq!(serde_json::to_value(&event_no_total).unwrap(), expected_no_total);
    }
}
