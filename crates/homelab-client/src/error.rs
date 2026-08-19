use homelab_api_model::OperationEnvelope;
use reqwest::StatusCode;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("invalid homelab API base URL: {reason}")]
    InvalidBaseUrl { reason: &'static str },

    #[error("incompatible homelab API major version: expected {expected}, received {actual}")]
    IncompatibleApi { expected: u16, actual: u16 },

    #[error("homelab API transport failed")]
    Transport(#[source] reqwest::Error),

    #[error("homelab API returned status {status}")]
    Api {
        status: StatusCode,
        envelope: Box<OperationEnvelope<Value>>,
    },

    #[error("homelab API response could not be decoded (status {status})")]
    Decode {
        status: StatusCode,
        request_id: Option<String>,
    },
}
