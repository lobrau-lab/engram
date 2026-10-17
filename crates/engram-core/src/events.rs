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
