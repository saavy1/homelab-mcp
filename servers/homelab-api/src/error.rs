use axum::{Json, http::StatusCode, response::IntoResponse};
use homelab_api_model::{OperationEnvelope, RiskLevel};
use homelab_core::{ErrorCode, ExecutionProvenance, OperationError};
use homelab_media::MediaError;
use serde::Serialize;
use serde_json::Value;
use std::time::Instant;

pub(crate) struct OperationMeta<'a> {
    pub operation: &'static str,
    pub risk: RiskLevel,
    pub backend: &'static str,
    pub target_id: Option<&'a str>,
    pub started: Instant,
}

impl<'a> OperationMeta<'a> {
    pub fn new(
        operation: &'static str,
        risk: RiskLevel,
        backend: &'static str,
        target_id: Option<&'a str>,
    ) -> Self {
        Self {
            operation,
            risk,
            backend,
            target_id,
            started: Instant::now(),
        }
    }
}

pub(crate) fn service_response<T: Serialize>(
    request_id: &str,
    meta: OperationMeta<'_>,
    result: Result<OperationEnvelope<T>, MediaError>,
) -> axum::response::Response {
    match result {
        Ok(envelope) => {
            completion(&meta, request_id, "success", false);
            Json(envelope).into_response()
        }
        Err(error) => {
            let code = error.error_code();
            let retryable = error.retryable();
            let message = error.public_message();
            failure_response(request_id, meta, code, message, retryable)
        }
    }
}

pub(crate) fn success_response<T: Serialize>(
    request_id: &str,
    meta: OperationMeta<'_>,
    summary: &'static str,
    data: T,
) -> axum::response::Response {
    let envelope = OperationEnvelope::success(
        meta.operation,
        request_id,
        meta.risk.clone(),
        summary,
        data,
        ExecutionProvenance::service("homelab-api"),
    );
    completion(&meta, request_id, "success", false);
    Json(envelope).into_response()
}

pub(crate) fn validation_response(
    request_id: &str,
    meta: OperationMeta<'_>,
    message: impl Into<String>,
) -> axum::response::Response {
    failure_response(request_id, meta, ErrorCode::Validation, message, false)
}

pub(crate) fn conflict_response(
    request_id: &str,
    meta: OperationMeta<'_>,
    message: impl Into<String>,
) -> axum::response::Response {
    failure_response(request_id, meta, ErrorCode::Conflict, message, false)
}

pub(crate) fn failure_response(
    request_id: &str,
    meta: OperationMeta<'_>,
    code: ErrorCode,
    message: impl Into<String>,
    retryable: bool,
) -> axum::response::Response {
    let status = status_for(&code);
    let result_class = error_code_name(&code);
    let envelope = OperationEnvelope::<Value>::failure(
        meta.operation,
        request_id,
        meta.risk.clone(),
        OperationError::new(code, message, retryable),
        ExecutionProvenance::service("homelab-api"),
    );
    completion(&meta, request_id, result_class, retryable);
    (status, Json(envelope)).into_response()
}

pub(crate) fn status_for(code: &ErrorCode) -> StatusCode {
    match code {
        ErrorCode::Validation => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::Forbidden => StatusCode::FORBIDDEN,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::Conflict => StatusCode::CONFLICT,
        ErrorCode::Unavailable | ErrorCode::Timeout | ErrorCode::UnknownOutcome => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn completion(meta: &OperationMeta<'_>, request_id: &str, result_class: &str, retryable: bool) {
    tracing::info!(
        event = "operation_completed",
        request_id,
        operation = meta.operation,
        risk = risk_name(&meta.risk),
        result_class,
        duration_ms = meta.started.elapsed().as_millis() as u64,
        backend = meta.backend,
        target_id = meta.target_id.unwrap_or(""),
        retryable,
        "homelab API operation completed"
    );
}

fn risk_name(risk: &RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Read => "read",
        RiskLevel::Pure => "pure",
        RiskLevel::Write => "write",
        RiskLevel::Destructive => "destructive",
        RiskLevel::ClusterWrite => "cluster_write",
    }
}

fn error_code_name(code: &ErrorCode) -> &'static str {
    match code {
        ErrorCode::Validation => "validation",
        ErrorCode::Forbidden => "forbidden",
        ErrorCode::NotFound => "not_found",
        ErrorCode::Conflict => "conflict",
        ErrorCode::Unavailable => "unavailable",
        ErrorCode::Timeout => "timeout",
        ErrorCode::UnknownOutcome => "unknown_outcome",
        ErrorCode::Internal => "internal",
    }
}
