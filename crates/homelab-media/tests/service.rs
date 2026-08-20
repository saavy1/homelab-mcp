mod common;

use axum::{
    Router,
    extract::{Path, Query},
    http::StatusCode,
    routing::{get, post},
};
use homelab_api_model::{CreateMediaRequest, HealthStatus, MediaType, RiskLevel};
use homelab_core::ErrorCode;
use homelab_media::{MediaConfig, MediaService, ServiceConfig};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

fn config(jellyseerr: String, sabnzbd: String, jellyfin: String) -> MediaConfig {
    MediaConfig {
        jellyseerr: ServiceConfig::new("jellyseerr", jellyseerr, "seerr-key").unwrap(),
        sabnzbd: ServiceConfig::new("sabnzbd", sabnzbd, "sab-key").unwrap(),
        jellyfin: ServiceConfig::new("jellyfin", jellyfin, "fin-key").unwrap(),
    }
}

fn service(config: MediaConfig) -> MediaService {
    MediaService::new(config, reqwest::Client::new())
}

#[tokio::test]
async fn health_probes_all_backends_concurrently_and_reports_degraded() {
    let probes = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api/v1/status",
            get({
                let probes = Arc::clone(&probes);
                move || {
                    probes.fetch_add(1, Ordering::SeqCst);
                    async {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        common::json_response(json!({"version": "1"}))
                    }
                }
            }),
        )
        .route(
            "/api",
            get({
                let probes = Arc::clone(&probes);
                move || {
                    probes.fetch_add(1, Ordering::SeqCst);
                    async {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                }
            }),
        )
        .route(
            "/System/Info/Public",
            get({
                let probes = Arc::clone(&probes);
                move || {
                    probes.fetch_add(1, Ordering::SeqCst);
                    async {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        common::json_response(json!({"Version": "1"}))
                    }
                }
            }),
        );
    let base_url = common::spawn_mock_app(app).await;
    let service = service(config(base_url.clone(), base_url.clone(), base_url));

    let started = Instant::now();
    let result = service.health("req-health").await.unwrap();
    let elapsed = started.elapsed();

    assert!(result.ok);
    assert_eq!(result.operation, "media.health");
    assert_eq!(result.request_id, "req-health");
    assert_eq!(result.risk, RiskLevel::Read);
    let health = result.data.unwrap();
    assert_eq!(health.status, HealthStatus::Degraded);
    assert_eq!(
        health
            .backends
            .iter()
            .filter(|backend| !backend.healthy)
            .count(),
        1
    );
    assert_eq!(result.issues.len(), 1);
    assert_eq!(probes.load(Ordering::SeqCst), 3);
    assert!(
        elapsed < Duration::from_millis(220),
        "health probes were not concurrent: {elapsed:?}"
    );
}

#[tokio::test]
async fn health_reports_unavailable_data_when_every_backend_fails() {
    let app = Router::new()
        .route(
            "/api/v1/status",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        )
        .route("/api", get(|| async { StatusCode::SERVICE_UNAVAILABLE }))
        .route(
            "/System/Info/Public",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        );
    let base_url = common::spawn_mock_app(app).await;
    let result = service(config(base_url.clone(), base_url.clone(), base_url))
        .health("req-health")
        .await
        .unwrap();

    assert!(result.ok);
    assert_eq!(result.data.unwrap().status, HealthStatus::Unavailable);
    assert_eq!(result.issues.len(), 3);
}

#[tokio::test]
async fn timeout_after_mutation_is_unknown_outcome_not_retryable_and_not_retried() {
    let requests = Arc::new(AtomicUsize::new(0));
    let request_count = Arc::clone(&requests);
    let app = Router::new().route(
        "/api",
        get(move || {
            request_count.fetch_add(1, Ordering::SeqCst);
            async {
                tokio::time::sleep(Duration::from_millis(150)).await;
                common::json_response(json!({"status": true, "nzo_ids": ["nzo-1"]}))
            }
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_millis(25))
        .build()
        .unwrap();
    let service = MediaService::new(config(base_url.clone(), base_url.clone(), base_url), http);

    let error = service
        .pause_download("req-pause", "nzo-1")
        .await
        .unwrap_err();

    assert_eq!(error.error_code(), ErrorCode::UnknownOutcome);
    assert!(!error.retryable());
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn read_rate_limit_is_retryable() {
    let app = Router::new().route(
        "/api/v1/search",
        get(|| async { StatusCode::TOO_MANY_REQUESTS }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let service = service(config(base_url.clone(), base_url.clone(), base_url));

    let error = service.search("req-search", "alien").await.unwrap_err();

    assert!(error.retryable());
}

#[tokio::test]
async fn mutation_rate_limit_is_not_retryable() {
    let app = Router::new().route(
        "/api/v1/request",
        post(|| async { StatusCode::TOO_MANY_REQUESTS }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let service = service(config(base_url.clone(), base_url.clone(), base_url));

    let error = service
        .create_request(
            "req-create",
            CreateMediaRequest {
                media_id: 100,
                media_type: MediaType::Movie,
            },
        )
        .await
        .unwrap_err();

    assert!(!error.retryable());
}

#[tokio::test]
async fn item_details_uses_jellyseerr_catalog_and_never_jellyfin() {
    let jellyseerr = Router::new().route(
        "/api/v1/tv/{id}",
        get(|Path(id): Path<String>| async move {
            assert_eq!(id, "60625");
            common::json_response(json!({
                "id": 60625,
                "mediaType": "tv",
                "name": "Rick and Morty",
                "firstAirDate": "2013-12-02"
            }))
        }),
    );
    let jellyfin_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&jellyfin_calls);
    let jellyfin = Router::new().fallback(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        async { StatusCode::INTERNAL_SERVER_ERROR }
    });
    let jellyseerr_url = common::spawn_mock_app(jellyseerr).await;
    let jellyfin_url = common::spawn_mock_app(jellyfin).await;
    let service = service(config(
        jellyseerr_url.clone(),
        jellyseerr_url,
        jellyfin_url,
    ));

    let result = service
        .item_details("req-item", "60625", MediaType::Tv)
        .await
        .unwrap();

    assert_eq!(result.data.unwrap().id, "60625");
    assert_eq!(jellyfin_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn blank_ids_fail_locally_at_the_service_boundary() {
    let requests = Arc::new(AtomicUsize::new(0));
    let request_count = Arc::clone(&requests);
    let app = Router::new().fallback(move || {
        request_count.fetch_add(1, Ordering::SeqCst);
        async { StatusCode::OK }
    });
    let base_url = common::spawn_mock_app(app).await;
    let service = service(config(base_url.clone(), base_url.clone(), base_url));

    assert_eq!(
        service
            .approve_request("r", " ")
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::Validation
    );
    assert_eq!(
        service
            .decline_request("r", "")
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::Validation
    );
    assert_eq!(
        service
            .pause_download("r", "\t")
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::Validation
    );
    assert_eq!(
        service
            .item_details("r", "\n", MediaType::Tv)
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::Validation
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
}

fn operations_app() -> Router {
    Router::new()
        .route("/api/v1/search", get(|| async { common::json_response(json!({"results": []})) }))
        .route(
            "/api/v1/request",
            get(|| async { common::json_response(json!({"results": []})) })
                .post(|| async { common::json_response(json!({"id": 1, "mediaId": 2, "mediaType": "movie", "status": 1})) }),
        )
        .route(
            "/api/v1/request/{id}/approve",
            post(|Path(id): Path<String>| async move { common::json_response(json!({"id": id})) }),
        )
        .route(
            "/api/v1/request/{id}/decline",
            post(|Path(id): Path<String>| async move { common::json_response(json!({"id": id})) }),
        )
        .route(
            "/api",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                match (params.get("mode").map(String::as_str), params.get("name").map(String::as_str)) {
                    (Some("queue"), None) => common::json_response(json!({"queue": {"slots": []}})),
                    (Some("history"), None) => common::json_response(json!({"history": {"slots": []}})),
                    (Some("retry"), _) => common::json_response(json!({"status": true})),
                    _ => common::json_response(json!({"status": true, "nzo_ids": [params.get("value").cloned().unwrap_or_default()]})),
                }
            }),
        )
        .route("/Items/Counts", get(|| async { common::json_response(json!({"ItemCount": 1})) }))
        .route("/Library/Refresh", post(|| async { StatusCode::NO_CONTENT }))
        .route("/Sessions", get(|| async { common::json_response(json!([])) }))
        .route(
            "/api/v1/movie/{id}",
            get(|Path(id): Path<String>| async move {
                common::json_response(json!({
                    "id": id,
                    "mediaType": "movie",
                    "title": "Alien"
                }))
            }),
        )
}

#[tokio::test]
async fn service_uses_exact_operation_names_and_risk_levels() {
    let base_url = common::spawn_mock_app(operations_app()).await;
    let service = service(config(base_url.clone(), base_url.clone(), base_url));
    let request = CreateMediaRequest {
        media_id: 2,
        media_type: MediaType::Movie,
    };

    let search = service.search("r1", "alien").await.unwrap();
    assert_eq!(
        (search.operation.as_str(), search.risk),
        ("media.search", RiskLevel::Read)
    );
    let create = service.create_request("r2", request).await.unwrap();
    assert_eq!(
        (create.operation.as_str(), create.risk),
        ("media.requests.create", RiskLevel::Write)
    );
    let list_requests = service.list_requests("r3", None).await.unwrap();
    assert_eq!(
        (list_requests.operation.as_str(), list_requests.risk),
        ("media.requests.list", RiskLevel::Read)
    );
    let approve = service.approve_request("r4", "1").await.unwrap();
    assert_eq!(
        (approve.operation.as_str(), approve.risk),
        ("media.requests.approve", RiskLevel::Write)
    );
    let decline = service.decline_request("r5", "1").await.unwrap();
    assert_eq!(
        (decline.operation.as_str(), decline.risk),
        ("media.requests.decline", RiskLevel::Write)
    );
    let downloads = service.list_downloads("r6", None).await.unwrap();
    assert_eq!(
        (downloads.operation.as_str(), downloads.risk),
        ("media.downloads.list", RiskLevel::Read)
    );
    let pause = service.pause_download("r7", "q1").await.unwrap();
    assert_eq!(
        (pause.operation.as_str(), pause.risk),
        ("media.downloads.pause", RiskLevel::Write)
    );
    let resume = service.resume_download("r8", "q1").await.unwrap();
    assert_eq!(
        (resume.operation.as_str(), resume.risk),
        ("media.downloads.resume", RiskLevel::Write)
    );
    let delete_safe = service.delete_download("r9", "q1", false).await.unwrap();
    assert_eq!(
        (delete_safe.operation.as_str(), delete_safe.risk),
        ("media.downloads.delete", RiskLevel::Write)
    );
    let delete_files = service.delete_download("r10", "q1", true).await.unwrap();
    assert_eq!(
        (delete_files.operation.as_str(), delete_files.risk),
        ("media.downloads.delete", RiskLevel::Destructive)
    );
    let retry = service.retry_download("r11", "h1").await.unwrap();
    assert_eq!(
        (retry.operation.as_str(), retry.risk),
        ("media.downloads.retry", RiskLevel::Write)
    );
    let library = service.library_status("r12").await.unwrap();
    assert_eq!(
        (library.operation.as_str(), library.risk),
        ("media.library.status", RiskLevel::Read)
    );
    let refresh = service.refresh_library("r13").await.unwrap();
    assert_eq!(
        (refresh.operation.as_str(), refresh.risk),
        ("media.library.refresh", RiskLevel::Write)
    );
    let sessions = service.active_sessions("r14").await.unwrap();
    assert_eq!(
        (sessions.operation.as_str(), sessions.risk),
        ("media.sessions.list", RiskLevel::Read)
    );
    let item = service
        .item_details("r15", "1", MediaType::Movie)
        .await
        .unwrap();
    assert_eq!(
        (item.operation.as_str(), item.risk),
        ("media.items.show", RiskLevel::Read)
    );
}
