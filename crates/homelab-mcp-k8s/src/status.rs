use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ModelStatus {
    pub namespace: String,
    pub name: String,
    pub ready: bool,
    pub conditions: Vec<KserveCondition>,
    pub recent_events: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct KserveCondition {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ModelLogs {
    pub namespace: String,
    pub name: String,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DownloadJobRef {
    pub job_name: String,
    pub namespace: String,
    pub model_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum DownloadStatus {
    NotStarted,
    JobCreated { job_ref: DownloadJobRef },
    Running { job_ref: DownloadJobRef },
    Completed { job_ref: DownloadJobRef },
    Failed { job_ref: DownloadJobRef, reason: String },
    AlreadyCached { model_id: String, path: String },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SentinelInfo {
    pub model_id: String,
    pub revision: String,
    pub downloaded_at: String,
    pub source: String,
    pub complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_status_serializes_for_agent() {
        let status = DownloadStatus::Completed {
            job_ref: DownloadJobRef {
                job_name: "download-qwen-qwen3-8b-main".into(),
                namespace: "ai".into(),
                model_id: "Qwen/Qwen3-8B".into(),
            },
        };
        let json = serde_json::to_string(&status).expect("serializes");
        assert!(json.contains("Completed"));
    }

    #[test]
    fn already_cached_status_includes_path() {
        let status = DownloadStatus::AlreadyCached {
            model_id: "Qwen/Qwen3-8B".into(),
            path: "/tank/models/Qwen/Qwen3-8B".into(),
        };
        let json = serde_json::to_string(&status).expect("serializes");
        assert!(json.contains("/tank/models"));
    }
}
