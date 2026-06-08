use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Debug, Serialize)]
pub struct UpstreamError {
    pub service: &'static str,
    pub operation: &'static str,
    pub status: Option<u16>,
    pub retryable: bool,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum MediaMcpError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("upstream error: {0:?}")]
    Upstream(UpstreamError),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl MediaMcpError {
    #[allow(dead_code)]
    pub fn to_tool_error(&self) -> String {
        match self {
            Self::Upstream(error) => {
                serde_json::to_string(error).unwrap_or_else(|_| error.message.clone())
            }
            Self::Validation(message) => serde_json::json!({
                "service": "media-mcp",
                "operation": "validation",
                "status": null,
                "retryable": false,
                "message": message
            })
            .to_string(),
            Self::Config(message) => serde_json::json!({
                "service": "media-mcp",
                "operation": "config",
                "status": null,
                "retryable": false,
                "message": message
            })
            .to_string(),
            Self::Http(error) => serde_json::json!({
                "service": "media-mcp",
                "operation": "http",
                "status": error.status().map(|status| status.as_u16()),
                "retryable": error.is_timeout() || error.is_connect(),
                "message": error.to_string()
            })
            .to_string(),
            Self::Serialization(error) => serde_json::json!({
                "service": "media-mcp",
                "operation": "serialization",
                "status": null,
                "retryable": false,
                "message": error.to_string()
            })
            .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_error_serializes_without_secret_url() {
        let error = MediaMcpError::Upstream(UpstreamError {
            service: "sabnzbd",
            operation: "queue",
            status: Some(401),
            retryable: false,
            message: "API Key Incorrect".into(),
        });
        let serialized = error.to_tool_error();
        assert!(serialized.contains("\"service\":\"sabnzbd\""));
        assert!(!serialized.contains("apikey"));
    }
}
