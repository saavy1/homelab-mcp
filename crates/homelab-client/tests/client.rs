use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use homelab_api_model::{
    API_MAJOR, ActiveSession, ApiVersion, Capabilities, CreateMediaRequest, DeleteDownloadQuery,
    DownloadItem, LibraryStatus, ListDownloadsQuery, ListRequestsQuery, MediaHealth,
    MediaOperation, MediaRequest, MediaSearchItem, MediaType, OperationEnvelope, RiskLevel,
    SearchMediaQuery,
};
use homelab_client::{ClientError, HomelabClient};
use homelab_core::ExecutionProvenance;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::net::TcpListener;
use url::Url;

const REQUEST_ID_HEADER: &str = "x-request-id";
const API_MAJOR_HEADER: &str = "x-homelab-api-major";

fn success<T: Serialize>(operation: &str, request_id: &str, data: T) -> Json<OperationEnvelope<T>> {
    Json(OperationEnvelope::success(
        operation,
        request_id,
        RiskLevel::Read,
        "test response",
        data,
        ExecutionProvenance::service("test-api"),
    ))
}

fn compatible_capabilities() -> Capabilities {
    Capabilities {
        api: ApiVersion {
            major: API_MAJOR,
            minor: 0,
        },
        compatible_cli_major: API_MAJOR,
        operations: Vec::new(),
    }
}

async fn spawn(app: Router) -> Url {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Url::parse(&format!("http://{address}")).unwrap()
}

#[tokio::test]
async fn base_url_keeps_api_prefix_and_protocol_ids_round_trip() {
    let app = Router::new().route(
        "/proxy/api/v1/capabilities",
        get(|headers: HeaderMap| async move {
            assert_eq!(headers[REQUEST_ID_HEADER], "caller-request");
            assert_eq!(headers[API_MAJOR_HEADER], "1");
            let mut response = success(
                "capabilities.show",
                "server-response-request",
                compatible_capabilities(),
            )
            .into_response();
            response.headers_mut().insert(
                REQUEST_ID_HEADER,
                "server-response-request".parse().unwrap(),
            );
            response
        }),
    );
    let mut base_url = spawn(app).await;
    base_url.set_path("/proxy/api/v1");
    let client = HomelabClient::new(base_url, reqwest::Client::new()).unwrap();

    let envelope = client.capabilities("caller-request").await.unwrap();

    assert_eq!(envelope.request_id, "server-response-request");
    assert_eq!(envelope.data.unwrap().api.major, API_MAJOR);
}

#[tokio::test]
async fn incompatible_capabilities_stop_mutation_before_it_is_sent() {
    let mutation_count = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api/v1/capabilities",
            get(|| async {
                success(
                    "capabilities.show",
                    "incompatible",
                    Capabilities {
                        api: ApiVersion {
                            major: API_MAJOR + 1,
                            minor: 0,
                        },
                        compatible_cli_major: API_MAJOR,
                        operations: Vec::new(),
                    },
                )
            }),
        )
        .route(
            "/api/v1/media/requests",
            post({
                let mutation_count = Arc::clone(&mutation_count);
                move || {
                    mutation_count.fetch_add(1, Ordering::SeqCst);
                    async { StatusCode::NO_CONTENT }
                }
            }),
        );
    let mut base_url = spawn(app).await;
    base_url.set_path("/api/v1");
    let client = HomelabClient::new(base_url, reqwest::Client::new()).unwrap();

    let error = client
        .media()
        .create_request(
            "incompatible",
            &CreateMediaRequest {
                media_id: 100,
                media_type: MediaType::Movie,
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        ClientError::IncompatibleApi { expected: API_MAJOR, actual } if actual == API_MAJOR + 1
    ));
    assert_eq!(mutation_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn incompatible_cli_major_stops_mutation_before_it_is_sent() {
    let mutation_count = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api/v1/capabilities",
            get(|| async {
                success(
                    "capabilities.show",
                    "incompatible-cli",
                    Capabilities {
                        api: ApiVersion {
                            major: API_MAJOR,
                            minor: 0,
                        },
                        compatible_cli_major: API_MAJOR + 1,
                        operations: Vec::new(),
                    },
                )
            }),
        )
        .route(
            "/api/v1/media/requests",
            post({
                let mutation_count = Arc::clone(&mutation_count);
                move || {
                    mutation_count.fetch_add(1, Ordering::SeqCst);
                    async { StatusCode::NO_CONTENT }
                }
            }),
        );
    let mut base_url = spawn(app).await;
    base_url.set_path("/api/v1");
    let client = HomelabClient::new(base_url, reqwest::Client::new()).unwrap();

    let error = client
        .media()
        .create_request(
            "incompatible-cli",
            &CreateMediaRequest {
                media_id: 100,
                media_type: MediaType::Movie,
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        ClientError::IncompatibleApi { expected: API_MAJOR, actual } if actual == API_MAJOR + 1
    ));
    assert_eq!(mutation_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn timed_out_mutation_is_sent_exactly_once() {
    let mutation_count = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api/v1/capabilities",
            get(|| async { success("capabilities.show", "timeout", compatible_capabilities()) }),
        )
        .route(
            "/api/v1/media/requests",
            post({
                let mutation_count = Arc::clone(&mutation_count);
                move || {
                    let mutation_count = Arc::clone(&mutation_count);
                    async move {
                        mutation_count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        success(
                            "media.requests.create",
                            "timeout",
                            MediaRequest {
                                id: "1".into(),
                                media_id: "100".into(),
                                media_type: MediaType::Movie,
                                status: "pending".into(),
                                title: None,
                            },
                        )
                    }
                }
            }),
        );
    let mut base_url = spawn(app).await;
    base_url.set_path("/api/v1");
    let http = reqwest::Client::builder()
        .timeout(Duration::from_millis(40))
        .build()
        .unwrap();
    let client = HomelabClient::new(base_url, http).unwrap();

    let error = client
        .media()
        .create_request(
            "timeout",
            &CreateMediaRequest {
                media_id: 100,
                media_type: MediaType::Movie,
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ClientError::Transport(_)));
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(mutation_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn non_json_error_body_is_redacted_from_decode_error() {
    const SECRET_BODY: &str = "upstream password=hunter2";
    let app = Router::new().route(
        "/api/v1/media/search",
        get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, SECRET_BODY) }),
    );
    let mut base_url = spawn(app).await;
    base_url.set_path("/api/v1");
    let client = HomelabClient::new(base_url, reqwest::Client::new()).unwrap();

    let error = client
        .media()
        .search(
            "redacted",
            &SearchMediaQuery {
                query: "Alien".into(),
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        ClientError::Decode {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            ..
        }
    ));
    assert!(!format!("{error}").contains(SECRET_BODY));
    assert!(!format!("{error:?}").contains(SECRET_BODY));
}

#[derive(Clone, Debug)]
struct SeenRequest {
    method: Method,
    uri: String,
    body: Option<Value>,
}

type Seen = Arc<Mutex<Vec<SeenRequest>>>;

async fn fixed_api(State(seen): State<Seen>, request: Request) -> Response {
    assert_eq!(request.headers()[REQUEST_ID_HEADER], "typed-contract");
    assert_eq!(request.headers()[API_MAJOR_HEADER], "1");
    let method = request.method().clone();
    let uri = request.uri().to_string();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 65_536).await.unwrap();
    let body = (!bytes.is_empty()).then(|| serde_json::from_slice(&bytes).unwrap());
    seen.lock().push(SeenRequest {
        method: method.clone(),
        uri,
        body,
    });

    let (operation, data) = match (method, path.as_str()) {
        (Method::GET, "/api/v1/capabilities") => (
            "capabilities.show",
            serde_json::to_value(compatible_capabilities()).unwrap(),
        ),
        (Method::GET, "/api/v1/health") => {
            ("media.health", json!({"status": "healthy", "backends": []}))
        }
        (Method::GET, "/api/v1/media/search") => ("media.search", json!([])),
        (Method::GET, "/api/v1/media/items/item%2Fone") => (
            "media.items.show",
            json!({"id": "item/one", "media_type": "movie", "title": "Alien", "year": 1979, "status": null}),
        ),
        (Method::GET, "/api/v1/media/requests") => ("media.requests.list", json!([])),
        (Method::POST, "/api/v1/media/requests") => (
            "media.requests.create",
            json!({"id": "r1", "media_id": "100", "media_type": "movie", "status": "pending", "title": null}),
        ),
        (Method::POST, "/api/v1/media/requests/r%2F1/approve") => (
            "media.requests.approve",
            json!({"service": "jellyseerr", "operation": "approve", "affected_id": "r/1"}),
        ),
        (Method::POST, "/api/v1/media/requests/r%2F1/decline") => (
            "media.requests.decline",
            json!({"service": "jellyseerr", "operation": "decline", "affected_id": "r/1"}),
        ),
        (Method::GET, "/api/v1/media/downloads") => ("media.downloads.list", json!([])),
        (Method::POST, "/api/v1/media/downloads/d%2F1/pause") => (
            "media.downloads.pause",
            json!({"service": "sabnzbd", "operation": "pause", "affected_id": "d/1"}),
        ),
        (Method::POST, "/api/v1/media/downloads/d%2F1/resume") => (
            "media.downloads.resume",
            json!({"service": "sabnzbd", "operation": "resume", "affected_id": "d/1"}),
        ),
        (Method::DELETE, "/api/v1/media/downloads/d%2F1") => (
            "media.downloads.delete",
            json!({"service": "sabnzbd", "operation": "delete", "affected_id": "d/1"}),
        ),
        (Method::POST, "/api/v1/media/downloads/d%2F1/retry") => (
            "media.downloads.retry",
            json!({"service": "sabnzbd", "operation": "retry", "affected_id": "d/1"}),
        ),
        (Method::GET, "/api/v1/media/library/status") => (
            "media.library.status",
            json!({"item_count": 10, "movie_count": 6, "series_count": 4}),
        ),
        (Method::POST, "/api/v1/media/library/refresh") => (
            "media.library.refresh",
            json!({"service": "jellyfin", "operation": "refresh", "affected_id": null}),
        ),
        (Method::GET, "/api/v1/media/sessions") => ("media.sessions.list", json!([])),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Json(json!({
        "ok": true,
        "operation": operation,
        "request_id": "typed-contract",
        "risk": "read",
        "summary": {"text": "test response"},
        "data": data,
        "issues": [],
        "provenance": {"service": "test-api", "timestamp": "2026-08-19T00:00:00Z"}
    }))
    .into_response()
}

#[tokio::test]
async fn every_operation_uses_a_typed_fixed_route() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .fallback(fixed_api)
        .with_state(Arc::clone(&seen));
    let mut base_url = spawn(app).await;
    base_url.set_path("/api/v1");
    let client = HomelabClient::new(base_url, reqwest::Client::new()).unwrap();
    let media = client.media();

    let _: OperationEnvelope<MediaHealth> = media.health("typed-contract").await.unwrap();
    let _: OperationEnvelope<Vec<MediaSearchItem>> = media
        .search(
            "typed-contract",
            &SearchMediaQuery {
                query: "Witch Hat".into(),
            },
        )
        .await
        .unwrap();
    let _: OperationEnvelope<MediaSearchItem> = media
        .item_details("typed-contract", "item/one")
        .await
        .unwrap();
    let _: OperationEnvelope<Vec<MediaRequest>> = media
        .list_requests(
            "typed-contract",
            &ListRequestsQuery {
                status: Some("pending review".into()),
            },
        )
        .await
        .unwrap();
    let _: OperationEnvelope<Vec<DownloadItem>> = media
        .list_downloads(
            "typed-contract",
            &ListDownloadsQuery {
                status: Some("downloading".into()),
            },
        )
        .await
        .unwrap();
    let _: OperationEnvelope<LibraryStatus> = media.library_status("typed-contract").await.unwrap();
    let _: OperationEnvelope<Vec<ActiveSession>> =
        media.active_sessions("typed-contract").await.unwrap();
    let _: OperationEnvelope<MediaRequest> = media
        .create_request(
            "typed-contract",
            &CreateMediaRequest {
                media_id: 100,
                media_type: MediaType::Movie,
            },
        )
        .await
        .unwrap();
    let _: OperationEnvelope<MediaOperation> = media
        .approve_request("typed-contract", "r/1")
        .await
        .unwrap();
    let _: OperationEnvelope<MediaOperation> = media
        .decline_request("typed-contract", "r/1")
        .await
        .unwrap();
    let _: OperationEnvelope<MediaOperation> =
        media.pause_download("typed-contract", "d/1").await.unwrap();
    let _: OperationEnvelope<MediaOperation> = media
        .resume_download("typed-contract", "d/1")
        .await
        .unwrap();
    let _: OperationEnvelope<MediaOperation> = media
        .delete_download(
            "typed-contract",
            "d/1",
            &DeleteDownloadQuery { delete_files: true },
        )
        .await
        .unwrap();
    let _: OperationEnvelope<MediaOperation> =
        media.retry_download("typed-contract", "d/1").await.unwrap();
    let _: OperationEnvelope<MediaOperation> =
        media.refresh_library("typed-contract").await.unwrap();

    let seen = seen.lock();
    let routes: Vec<_> = seen
        .iter()
        .map(|request| (request.method.clone(), request.uri.as_str()))
        .collect();
    assert_eq!(
        routes,
        vec![
            (Method::GET, "/api/v1/health"),
            (Method::GET, "/api/v1/media/search?query=Witch+Hat"),
            (Method::GET, "/api/v1/media/items/item%2Fone"),
            (Method::GET, "/api/v1/media/requests?status=pending+review"),
            (Method::GET, "/api/v1/media/downloads?status=downloading"),
            (Method::GET, "/api/v1/media/library/status"),
            (Method::GET, "/api/v1/media/sessions"),
            (Method::GET, "/api/v1/capabilities"),
            (Method::POST, "/api/v1/media/requests"),
            (Method::POST, "/api/v1/media/requests/r%2F1/approve"),
            (Method::POST, "/api/v1/media/requests/r%2F1/decline"),
            (Method::POST, "/api/v1/media/downloads/d%2F1/pause"),
            (Method::POST, "/api/v1/media/downloads/d%2F1/resume"),
            (
                Method::DELETE,
                "/api/v1/media/downloads/d%2F1?delete_files=true"
            ),
            (Method::POST, "/api/v1/media/downloads/d%2F1/retry"),
            (Method::POST, "/api/v1/media/library/refresh"),
        ]
    );
    assert_eq!(
        seen[8].body,
        Some(json!({"media_id": 100, "media_type": "movie"}))
    );
    assert!(
        seen.iter()
            .enumerate()
            .all(|(index, request)| index == 8 || request.body.is_none())
    );
}
