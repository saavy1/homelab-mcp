use crate::{
    ApiState, RequestContext,
    error::{
        OperationMeta, conflict_response, failure_response, service_response, validation_response,
    },
};
use axum::{
    Extension, Json,
    body::to_bytes,
    extract::{FromRequest, Path, RawQuery, Request, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use homelab_api_model::{
    API_MAJOR, CreateMediaRequest, DeleteDownloadQuery, ItemDetailsQuery, ListDownloadsQuery,
    ListRequestsQuery, RiskLevel, SearchMediaQuery,
};
use homelab_core::ErrorCode;
use serde::de::DeserializeOwned;
use std::time::Duration;
use tokio::time::timeout;

const API_MAJOR_HEADER: &str = "x-homelab-api-major";
const MAX_QUERY_LENGTH: usize = 256;
const MAX_STATUS_LENGTH: usize = 64;
const MAX_ID_LENGTH: usize = 256;
const MUTATION_BODY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn health(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    let meta = OperationMeta::new("media.health", RiskLevel::Read, "media", None);
    let result = state.media.health(&context.request_id).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn search(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    RawQuery(raw): RawQuery,
) -> Response {
    let meta = OperationMeta::new("media.search", RiskLevel::Read, "jellyseerr", None);
    let query = match parse_query::<SearchMediaQuery>(raw.as_deref(), &["query"]) {
        Ok(query) if !query.query.trim().is_empty() && query.query.len() <= MAX_QUERY_LENGTH => {
            query
        }
        Ok(_) => {
            return validation_response(
                &context.request_id,
                meta,
                "query must contain between 1 and 256 characters",
            );
        }
        Err(message) => return validation_response(&context.request_id, meta, message),
    };
    let result = state.media.search(&context.request_id, &query.query).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn item_details(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Response {
    let meta = OperationMeta::new(
        "media.items.show",
        RiskLevel::Read,
        "jellyseerr",
        Some(&id),
    );
    if let Err(message) = validate_catalog_id(&id) {
        return validation_response(&context.request_id, meta, message);
    }
    let query = match parse_query::<ItemDetailsQuery>(raw.as_deref(), &["media_type"]) {
        Ok(query) => query,
        Err(message) => return validation_response(&context.request_id, meta, message),
    };
    let result = state
        .media
        .item_details(&context.request_id, &id, query.media_type)
        .await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn create_request(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    request: Request,
) -> Response {
    let meta = OperationMeta::new(
        "media.requests.create",
        RiskLevel::Write,
        "jellyseerr",
        None,
    );
    if let Err(message) = require_compatible_major(request.headers()) {
        return conflict_response(&context.request_id, meta, message);
    }
    let Json(payload) = match timeout(
        MUTATION_BODY_TIMEOUT,
        Json::<CreateMediaRequest>::from_request(request, &state),
    )
    .await
    {
        Err(_) => return body_timeout_response(&context.request_id, meta),
        Ok(Ok(payload)) => payload,
        Ok(Err(rejection)) => {
            let oversized = rejection.status() == StatusCode::PAYLOAD_TOO_LARGE;
            let mut response = validation_response(
                &context.request_id,
                meta,
                if oversized {
                    "request body exceeds 65536 bytes"
                } else {
                    "request body must be valid application/json with only documented fields"
                },
            );
            if oversized {
                *response.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
            }
            return response;
        }
    };
    let result = state
        .media
        .create_request(&context.request_id, payload)
        .await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn list_requests(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    RawQuery(raw): RawQuery,
) -> Response {
    let meta = OperationMeta::new("media.requests.list", RiskLevel::Read, "jellyseerr", None);
    let query = match parse_query::<ListRequestsQuery>(raw.as_deref(), &["status"]) {
        Ok(query) => query,
        Err(message) => return validation_response(&context.request_id, meta, message),
    };
    if query
        .status
        .as_ref()
        .is_some_and(|status| status.len() > MAX_STATUS_LENGTH)
    {
        return validation_response(
            &context.request_id,
            meta,
            "status must not exceed 64 characters",
        );
    }
    let result = state
        .media
        .list_requests(&context.request_id, query.status.as_deref())
        .await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn approve_request(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let meta = match mutation_meta(
        &context,
        request.headers(),
        &id,
        "media.requests.approve",
        "jellyseerr",
        false,
    ) {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let meta = match require_empty_body(request, &context.request_id, meta).await {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let result = state.media.approve_request(&context.request_id, &id).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn decline_request(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let meta = match mutation_meta(
        &context,
        request.headers(),
        &id,
        "media.requests.decline",
        "jellyseerr",
        false,
    ) {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let meta = match require_empty_body(request, &context.request_id, meta).await {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let result = state.media.decline_request(&context.request_id, &id).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn list_downloads(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    RawQuery(raw): RawQuery,
) -> Response {
    let meta = OperationMeta::new("media.downloads.list", RiskLevel::Read, "sabnzbd", None);
    let query = match parse_query::<ListDownloadsQuery>(raw.as_deref(), &["status"]) {
        Ok(query) => query,
        Err(message) => return validation_response(&context.request_id, meta, message),
    };
    if query
        .status
        .as_ref()
        .is_some_and(|status| status.len() > MAX_STATUS_LENGTH)
    {
        return validation_response(
            &context.request_id,
            meta,
            "status must not exceed 64 characters",
        );
    }
    let result = state
        .media
        .list_downloads(&context.request_id, query.status.as_deref())
        .await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn pause_download(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let meta = match mutation_meta(
        &context,
        request.headers(),
        &id,
        "media.downloads.pause",
        "sabnzbd",
        true,
    ) {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let meta = match require_empty_body(request, &context.request_id, meta).await {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let result = state.media.pause_download(&context.request_id, &id).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn resume_download(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let meta = match mutation_meta(
        &context,
        request.headers(),
        &id,
        "media.downloads.resume",
        "sabnzbd",
        true,
    ) {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let meta = match require_empty_body(request, &context.request_id, meta).await {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let result = state.media.resume_download(&context.request_id, &id).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn delete_download(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
    request: Request,
) -> Response {
    let query = match parse_query::<DeleteDownloadQuery>(raw.as_deref(), &["delete_files"]) {
        Ok(query) => query,
        Err(message) => {
            return validation_response(
                &context.request_id,
                OperationMeta::new(
                    "media.downloads.delete",
                    RiskLevel::Write,
                    "sabnzbd",
                    Some(&id),
                ),
                message,
            );
        }
    };
    let risk = if query.delete_files {
        RiskLevel::Destructive
    } else {
        RiskLevel::Write
    };
    let meta = OperationMeta::new("media.downloads.delete", risk, "sabnzbd", Some(&id));
    if let Err(message) = require_compatible_major(request.headers()) {
        return conflict_response(&context.request_id, meta, message);
    }
    if let Err(message) = validate_download_id(&id) {
        return validation_response(&context.request_id, meta, message);
    }
    let meta = match require_empty_body(request, &context.request_id, meta).await {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let result = state
        .media
        .delete_download(&context.request_id, &id, query.delete_files)
        .await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn retry_download(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let meta = match mutation_meta(
        &context,
        request.headers(),
        &id,
        "media.downloads.retry",
        "sabnzbd",
        true,
    ) {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let meta = match require_empty_body(request, &context.request_id, meta).await {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let result = state.media.retry_download(&context.request_id, &id).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn library_status(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    let meta = OperationMeta::new("media.library.status", RiskLevel::Read, "jellyfin", None);
    let result = state.media.library_status(&context.request_id).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn refresh_library(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    request: Request,
) -> Response {
    let meta = OperationMeta::new("media.library.refresh", RiskLevel::Write, "jellyfin", None);
    if let Err(message) = require_compatible_major(request.headers()) {
        return conflict_response(&context.request_id, meta, message);
    }
    let meta = match require_empty_body(request, &context.request_id, meta).await {
        Ok(meta) => meta,
        Err(response) => return *response,
    };
    let result = state.media.refresh_library(&context.request_id).await;
    service_response(&context.request_id, meta, result)
}

pub(crate) async fn active_sessions(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    let meta = OperationMeta::new("media.sessions.list", RiskLevel::Read, "jellyfin", None);
    let result = state.media.active_sessions(&context.request_id).await;
    service_response(&context.request_id, meta, result)
}

fn mutation_meta<'a>(
    context: &RequestContext,
    headers: &HeaderMap,
    id: &'a str,
    operation: &'static str,
    backend: &'static str,
    download_id: bool,
) -> Result<OperationMeta<'a>, Box<Response>> {
    let meta = OperationMeta::new(operation, RiskLevel::Write, backend, Some(id));
    if let Err(message) = require_compatible_major(headers) {
        return Err(Box::new(conflict_response(
            &context.request_id,
            meta,
            message,
        )));
    }
    let id_result = if download_id {
        validate_download_id(id)
    } else {
        validate_id(id)
    };
    if let Err(message) = id_result {
        return Err(Box::new(validation_response(
            &context.request_id,
            meta,
            message,
        )));
    }
    Ok(meta)
}

async fn require_empty_body<'a>(
    request: Request,
    request_id: &str,
    meta: OperationMeta<'a>,
) -> Result<OperationMeta<'a>, Box<Response>> {
    match timeout(
        MUTATION_BODY_TIMEOUT,
        to_bytes(request.into_body(), 64 * 1024),
    )
    .await
    {
        Err(_) => Err(Box::new(body_timeout_response(request_id, meta))),
        Ok(Ok(bytes)) if bytes.is_empty() => Ok(meta),
        Ok(Ok(_)) => Err(Box::new(validation_response(
            request_id,
            meta,
            "this operation does not accept a request body",
        ))),
        Ok(Err(_)) => {
            let mut response =
                validation_response(request_id, meta, "request body exceeds 65536 bytes");
            *response.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
            Err(Box::new(response))
        }
    }
}

fn body_timeout_response(request_id: &str, meta: OperationMeta<'_>) -> Response {
    failure_response(
        request_id,
        meta,
        ErrorCode::Timeout,
        "request body did not complete in time",
        true,
    )
}

fn require_compatible_major(headers: &HeaderMap) -> Result<(), &'static str> {
    let compatible = headers
        .get(API_MAJOR_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u16>().ok())
        == Some(API_MAJOR);
    compatible
        .then_some(())
        .ok_or("incompatible or missing API major; expected 1")
}

fn parse_query<T: DeserializeOwned>(raw: Option<&str>, allowed: &[&str]) -> Result<T, String> {
    let raw = raw.unwrap_or_default();
    let pairs: Vec<(String, String)> =
        serde_urlencoded::from_str(raw).map_err(|_| "query parameters are invalid".to_owned())?;
    if let Some((name, _)) = pairs
        .iter()
        .find(|(name, _)| !allowed.contains(&name.as_str()))
    {
        return Err(format!("unknown query parameter: {name}"));
    }
    serde_urlencoded::from_str(raw).map_err(|_| "query parameters are invalid".to_owned())
}

fn validate_id(id: &str) -> Result<(), &'static str> {
    let safe = !matches!(id, "." | "..")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if id.is_empty() || id.len() > MAX_ID_LENGTH || !safe {
        Err("identifier must contain 1 to 256 safe identifier characters")
    } else {
        Ok(())
    }
}

fn validate_catalog_id(id: &str) -> Result<(), &'static str> {
    if id.is_empty() || id.len() > MAX_ID_LENGTH || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        Err("item identifier must be a non-empty numeric catalog identifier")
    } else {
        Ok(())
    }
}

fn validate_download_id(id: &str) -> Result<(), &'static str> {
    validate_id(id)?;
    if id.eq_ignore_ascii_case("all") {
        Err("a specific download identifier is required")
    } else {
        Ok(())
    }
}
