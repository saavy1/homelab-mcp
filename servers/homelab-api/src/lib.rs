mod error;
mod routes;

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{
        HeaderValue, StatusCode,
        header::{CONTENT_TYPE, HeaderName},
    },
    middleware::{self, Next},
    response::Response,
    routing::{MethodFilter, on},
};
use error::{OperationMeta, failure_response};
use homelab_api_model::RiskLevel;
use homelab_core::ErrorCode;
use homelab_media::MediaService;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::limit::RequestBodyLimitLayer;

const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;
const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(crate) struct ApiState {
    media: Arc<MediaService>,
}

#[derive(Clone)]
pub(crate) struct RequestContext {
    request_id: String,
}

pub fn build_router(media: MediaService) -> Router {
    let state = ApiState {
        media: Arc::new(media),
    };
    routes::read_router()
        .merge(routes::mutation_router())
        .route("/livez", on(MethodFilter::GET, livez))
        .route("/readyz", on(MethodFilter::GET, readyz))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BODY_BYTES))
        .layer(middleware::from_fn(request_context))
}

async fn livez() -> &'static str {
    "ok"
}

async fn readyz() -> &'static str {
    "ok"
}

async fn request_context(mut request: Request, next: Next) -> Response<Body> {
    let request_id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| valid_request_id(value))
        .map(str::to_owned)
        .unwrap_or_else(generated_request_id);
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    request.extensions_mut().insert(RequestContext {
        request_id: request_id.clone(),
    });

    let mut response = next.run(request).await;
    let structured = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    if response.status() == StatusCode::PAYLOAD_TOO_LARGE && !structured {
        let (operation, risk, backend, target_id) = route_metadata(&method, &path);
        response = failure_response(
            &request_id,
            OperationMeta::new(operation, risk, backend, target_id),
            ErrorCode::Validation,
            "request body exceeds 65536 bytes",
            false,
        );
        *response.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
    } else if response.status() == StatusCode::REQUEST_TIMEOUT {
        let (operation, risk, backend, target_id) = route_metadata(&method, &path);
        response = failure_response(
            &request_id,
            OperationMeta::new(operation, risk, backend, target_id),
            ErrorCode::Timeout,
            "request did not complete in time",
            true,
        );
    }
    response.headers_mut().insert(
        REQUEST_ID_HEADER.clone(),
        HeaderValue::from_str(&request_id).expect("validated request ID is a header value"),
    );
    response
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn generated_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("req-{nanos:x}-{sequence:x}")
}

fn route_metadata<'a>(
    method: &axum::http::Method,
    path: &'a str,
) -> (&'static str, RiskLevel, &'static str, Option<&'a str>) {
    if path == "/api/v1/media/requests" && *method == axum::http::Method::POST {
        return (
            "media.requests.create",
            RiskLevel::Write,
            "jellyseerr",
            None,
        );
    }
    if let Some(id) = path
        .strip_prefix("/api/v1/media/requests/")
        .and_then(|rest| rest.strip_suffix("/approve"))
    {
        return (
            "media.requests.approve",
            RiskLevel::Write,
            "jellyseerr",
            Some(id),
        );
    }
    if let Some(id) = path
        .strip_prefix("/api/v1/media/requests/")
        .and_then(|rest| rest.strip_suffix("/decline"))
    {
        return (
            "media.requests.decline",
            RiskLevel::Write,
            "jellyseerr",
            Some(id),
        );
    }
    if path == "/api/v1/media/library/refresh" {
        return ("media.library.refresh", RiskLevel::Write, "jellyfin", None);
    }
    if let Some(rest) = path.strip_prefix("/api/v1/media/downloads/") {
        let (operation, id, risk) = if let Some(id) = rest.strip_suffix("/pause") {
            ("media.downloads.pause", id, RiskLevel::Write)
        } else if let Some(id) = rest.strip_suffix("/resume") {
            ("media.downloads.resume", id, RiskLevel::Write)
        } else if let Some(id) = rest.strip_suffix("/retry") {
            ("media.downloads.retry", id, RiskLevel::Write)
        } else {
            ("media.downloads.delete", rest, RiskLevel::Write)
        };
        return (operation, risk, "sabnzbd", Some(id));
    }
    if path == "/api/v1/media/search" {
        return ("media.search", RiskLevel::Read, "jellyseerr", None);
    }
    if let Some(id) = path.strip_prefix("/api/v1/media/items/") {
        return ("media.items.show", RiskLevel::Read, "jellyseerr", Some(id));
    }
    ("request", RiskLevel::Read, "homelab-api", None)
}
