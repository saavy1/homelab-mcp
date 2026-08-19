pub(crate) mod media;

use crate::{
    ApiState, RequestContext,
    error::{OperationMeta, success_response},
};
use axum::{
    Extension, Router,
    extract::State,
    http::StatusCode,
    response::Response,
    routing::{MethodFilter, on},
};
use homelab_api_model::{API_MAJOR, API_MINOR, ApiVersion, Capabilities, RiskLevel};
use std::time::Duration;
use tower_http::timeout::TimeoutLayer;

const OPERATIONS: &[&str] = &[
    "media.health",
    "media.search",
    "media.items.show",
    "media.requests.create",
    "media.requests.list",
    "media.requests.approve",
    "media.requests.decline",
    "media.downloads.list",
    "media.downloads.pause",
    "media.downloads.resume",
    "media.downloads.delete",
    "media.downloads.retry",
    "media.library.status",
    "media.library.refresh",
    "media.sessions.list",
];

pub(crate) fn read_router() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/capabilities", on(MethodFilter::GET, capabilities))
        .route("/api/v1/health", on(MethodFilter::GET, media::health))
        .route("/api/v1/media/search", on(MethodFilter::GET, media::search))
        .route(
            "/api/v1/media/items/{id}",
            on(MethodFilter::GET, media::item_details),
        )
        .route(
            "/api/v1/media/requests",
            on(MethodFilter::GET, media::list_requests),
        )
        .route(
            "/api/v1/media/downloads",
            on(MethodFilter::GET, media::list_downloads),
        )
        .route(
            "/api/v1/media/library/status",
            on(MethodFilter::GET, media::library_status),
        )
        .route(
            "/api/v1/media/sessions",
            on(MethodFilter::GET, media::active_sessions),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(35),
        ))
}

pub(crate) fn mutation_router() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/media/requests",
            axum::routing::post(media::create_request),
        )
        .route(
            "/api/v1/media/requests/{id}/approve",
            axum::routing::post(media::approve_request),
        )
        .route(
            "/api/v1/media/requests/{id}/decline",
            axum::routing::post(media::decline_request),
        )
        .route(
            "/api/v1/media/downloads/{id}/pause",
            axum::routing::post(media::pause_download),
        )
        .route(
            "/api/v1/media/downloads/{id}/resume",
            axum::routing::post(media::resume_download),
        )
        .route(
            "/api/v1/media/downloads/{id}",
            axum::routing::delete(media::delete_download),
        )
        .route(
            "/api/v1/media/downloads/{id}/retry",
            axum::routing::post(media::retry_download),
        )
        .route(
            "/api/v1/media/library/refresh",
            axum::routing::post(media::refresh_library),
        )
}

async fn capabilities(
    State(_state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    success_response(
        &context.request_id,
        OperationMeta::new("capabilities.show", RiskLevel::Pure, "homelab-api", None),
        "homelab API capabilities listed",
        Capabilities {
            api: ApiVersion {
                major: API_MAJOR,
                minor: API_MINOR,
            },
            compatible_cli_major: API_MAJOR,
            operations: OPERATIONS
                .iter()
                .map(|operation| (*operation).into())
                .collect(),
        },
    )
}
