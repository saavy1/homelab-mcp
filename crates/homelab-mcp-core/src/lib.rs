use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Read,
    Pure,
    ClusterWrite,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Provenance {
    pub source: String,
    pub path: Option<String>,
    pub commit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
    pub allowed: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Summary {
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ToolResult<T> {
    pub summary: Summary,
    pub risk: RiskLevel,
    pub data: T,
    pub issues: Vec<ValidationIssue>,
}

impl<T> ToolResult<T> {
    pub fn read(summary: impl Into<String>, data: T) -> Self {
        Self {
            summary: Summary {
                text: summary.into(),
            },
            risk: RiskLevel::Read,
            data,
            issues: Vec::new(),
        }
    }

    pub fn pure(summary: impl Into<String>, data: T) -> Self {
        Self {
            summary: Summary {
                text: summary.into(),
            },
            risk: RiskLevel::Pure,
            data,
            issues: Vec::new(),
        }
    }

    pub fn cluster_write(summary: impl Into<String>, data: T) -> Self {
        Self {
            summary: Summary {
                text: summary.into(),
            },
            risk: RiskLevel::ClusterWrite,
            data,
            issues: Vec::new(),
        }
    }

    pub fn with_issues(mut self, issues: Vec<ValidationIssue>) -> Self {
        self.issues = issues;
        self
    }
}

#[derive(Debug, Error)]
pub enum HomelabMcpError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("sentinel missing or incomplete: {0}")]
    SentinelMissing(String),
    #[error("credential error: {0}")]
    Credential(String),
}

pub type HomelabResult<T> = Result<T, HomelabMcpError>;

pub fn compute_digest(canonical_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Sanitize a string for use as a Kubernetes label value.
/// Replaces `/` with `-` and converts to lowercase per K8s label rules.
pub fn sanitize_label_value(s: &str) -> String {
    s.replace('/', "-").replace(':', "-").to_lowercase()
}

/// Sanitize a string for use as a Kubernetes resource name (DNS subdomain label).
/// Only lowercase alphanumeric, hyphens, and dots allowed, but dots are rejected
/// by some admission webhooks (e.g. KServe), so replace dots with hyphens too.
pub fn sanitize_dns_name(s: &str) -> String {
    s.replace('.', "-")
        .replace('/', "-")
        .replace('_', "-")
        .to_lowercase()
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .with_target(true)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_result_has_read_risk_and_summary() {
        let result = ToolResult::read("listed recipes", vec!["qwen3-8b"]);
        assert_eq!(result.risk, RiskLevel::Read);
        assert_eq!(result.summary.text, "listed recipes");
    }

    #[test]
    fn cluster_write_result_carries_risk_level() {
        let result = ToolResult::cluster_write("applied InferenceService", "qwen3-8b");
        assert_eq!(result.risk, RiskLevel::ClusterWrite);
    }

    #[test]
    fn digest_is_deterministic() {
        let json = r#"{"name":"qwen3-8b","namespace":"ai"}"#;
        let d1 = compute_digest(json);
        let d2 = compute_digest(json);
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }

    #[test]
    fn digest_differs_for_different_input() {
        let d1 = compute_digest(r#"{"name":"a"}"#);
        let d2 = compute_digest(r#"{"name":"b"}"#);
        assert_ne!(d1, d2);
    }

    #[test]
    fn sanitize_label_value_replaces_slashes() {
        assert_eq!(sanitize_label_value("LiquidAI/LFM2.5-350M"), "liquidai-lfm2.5-350m");
        assert_eq!(sanitize_label_value("deepseek-ai/DeepSeek-V4-Flash"), "deepseek-ai-deepseek-v4-flash");
        assert_eq!(sanitize_label_value("no-slashes"), "no-slashes");
    }

    #[test]
    fn sanitize_dns_name_replaces_dots_and_slashes() {
        assert_eq!(sanitize_dns_name("lfm2.5-350m"), "lfm2-5-350m");
        assert_eq!(sanitize_dns_name("Qwen/Qwen3-8B"), "qwen-qwen3-8b");
        assert_eq!(sanitize_dns_name("my_model"), "my-model");
    }
}
