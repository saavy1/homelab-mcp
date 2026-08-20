use homelab_core::ErrorCode;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct UpstreamError {
    pub service: &'static str,
    pub operation: &'static str,
    pub status: Option<u16>,
    pub retryable: bool,
}

#[derive(Clone, Debug)]
pub struct TransportError {
    pub service: &'static str,
    pub operation: &'static str,
    pub timeout: bool,
    pub retryable: bool,
    pub unknown_outcome: bool,
}

#[derive(Clone, Debug)]
pub struct SerializationError {
    pub service: &'static str,
    pub operation: &'static str,
}

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("media configuration is invalid")]
    Config(String),
    #[error("media request validation failed: {0}")]
    Validation(String),
    #[error("{service} upstream request failed", service = .0.service)]
    Upstream(UpstreamError),
    #[error("{service} transport failed", service = .0.service)]
    Transport(TransportError),
    #[error("{service} response could not be decoded", service = .0.service)]
    Serialization(SerializationError),
    #[error("media records conflict")]
    Conflict,
    #[error("media normalized data is invalid")]
    Internal,
}

impl MediaError {
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::Config(_) | Self::Serialization(_) | Self::Internal => ErrorCode::Internal,
            Self::Validation(_) => ErrorCode::Validation,
            Self::Upstream(error) => match error.status {
                Some(401 | 403) => ErrorCode::Forbidden,
                Some(404) => ErrorCode::NotFound,
                Some(409) => ErrorCode::Conflict,
                Some(408) => ErrorCode::Timeout,
                Some(500..=599) | None => ErrorCode::Unavailable,
                Some(_) => ErrorCode::Unavailable,
            },
            Self::Conflict => ErrorCode::Conflict,
            Self::Transport(error) if error.unknown_outcome => ErrorCode::UnknownOutcome,
            Self::Transport(error) if error.timeout => ErrorCode::Timeout,
            Self::Transport(_) => ErrorCode::Unavailable,
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Upstream(error) => error.retryable,
            Self::Transport(error) => error.retryable,
            Self::Config(_)
            | Self::Validation(_)
            | Self::Serialization(_)
            | Self::Conflict
            | Self::Internal => false,
        }
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::Config(_) => "media service configuration is invalid".into(),
            Self::Validation(message) => message.clone(),
            Self::Conflict => "media records conflict".into(),
            Self::Internal => "media backend returned invalid normalized data".into(),
            Self::Upstream(error) => match error.status {
                Some(401 | 403) => format!("{} rejected the request", error.service),
                Some(404) => format!("{} resource was not found", error.service),
                _ => format!("{} is unavailable", error.service),
            },
            Self::Transport(error) if error.unknown_outcome => {
                format!("{} operation outcome is unknown", error.service)
            }
            Self::Transport(error) if error.timeout => {
                format!("{} did not respond in time", error.service)
            }
            Self::Transport(error) => format!("{} is unavailable", error.service),
            Self::Serialization(error) => {
                format!("{} returned an invalid response", error.service)
            }
        }
    }

    pub(crate) fn upstream(
        service: &'static str,
        operation: &'static str,
        status: Option<u16>,
        mutating: bool,
    ) -> Self {
        let retryable =
            !mutating && status.is_none_or(|value| value >= 500 || matches!(value, 408 | 429));
        Self::Upstream(UpstreamError {
            service,
            operation,
            status,
            retryable,
        })
    }

    pub(crate) fn transport(
        service: &'static str,
        operation: &'static str,
        error: &reqwest::Error,
        mutating: bool,
    ) -> Self {
        Self::Transport(TransportError {
            service,
            operation,
            timeout: error.is_timeout(),
            retryable: !mutating && (error.is_timeout() || error.is_connect()),
            unknown_outcome: mutating && error.is_timeout(),
        })
    }

    pub(crate) fn serialization(service: &'static str, operation: &'static str) -> Self {
        Self::Serialization(SerializationError { service, operation })
    }
}
