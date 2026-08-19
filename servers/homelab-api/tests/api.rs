use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{Method, Response, StatusCode},
    response::IntoResponse,
};
use homelab_api::build_router;
use homelab_media::{MediaConfig, MediaService, ServiceConfig};
use http_body_util::BodyExt;
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::{io::Write, process::Command, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tower::ServiceExt;

const API_MAJOR_HEADER: &str = "x-homelab-api-major";
const REQUEST_ID_HEADER: &str = "x-request-id";
const SECRET: &str = "super-secret-api-key";

#[derive(Clone, Default)]
struct BackendState {
    calls: Arc<Mutex<Vec<String>>>,
    delay_path: Option<&'static str>,
}

#[derive(Clone)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

async fn spawn_backend(state: BackendState) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, Router::new().fallback(backend).with_state(state))
            .await
            .unwrap();
    });
    format!("http://{address}")
}

async fn backend(State(state): State<BackendState>, request: Request) -> Response<Body> {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().to_owned();
    state.calls.lock().push(format!("{method} {uri}"));

    if state.delay_path == Some(path.as_str()) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if path == "/api/v1/request" && method == Method::POST {
        return (
            StatusCode::OK,
            axum::Json(json!({
                "id": 42,
                "mediaId": 100,
                "mediaType": "movie",
                "status": 1,
                "title": "Alien"
            })),
        )
            .into_response();
    }
    if path == "/api/v1/request/secret/approve" {
        return (StatusCode::UNAUTHORIZED, format!("upstream body {SECRET}")).into_response();
    }
    if path.starts_with("/api/v1/request/") && method == Method::POST {
        return axum::Json(json!({})).into_response();
    }
    if path == "/api/v1/request" {
        return axum::Json(json!({"results": [{
            "id": 42,
            "mediaId": 100,
            "mediaType": "movie",
            "status": 1,
            "title": "Alien"
        }]}))
        .into_response();
    }
    if path == "/api/v1/search" {
        return axum::Json(json!({"results": [{
            "id": 100,
            "mediaType": "movie",
            "title": "Alien",
            "releaseDate": "1979-05-25"
        }]}))
        .into_response();
    }
    if path == "/api/v1/status" || path == "/System/Info/Public" {
        return axum::Json(json!({})).into_response();
    }
    if path == "/Items/Counts" {
        return axum::Json(json!({"ItemCount": 9, "MovieCount": 4, "SeriesCount": 5}))
            .into_response();
    }
    if path == "/Library/Refresh" {
        return StatusCode::NO_CONTENT.into_response();
    }
    if path == "/Sessions" {
        return axum::Json(json!([{"Id": "session-1", "UserName": "saavy"}])).into_response();
    }
    if let Some(id) = path.strip_prefix("/Items/") {
        return match id {
            "missing" => (StatusCode::NOT_FOUND, "not found").into_response(),
            "unavailable" => {
                (StatusCode::INTERNAL_SERVER_ERROR, "private backend failure").into_response()
            }
            "invalid" => axum::Json(json!({})).into_response(),
            _ => axum::Json(json!({
                "Id": id,
                "Type": "Movie",
                "Name": "Alien",
                "ProductionYear": 1979
            }))
            .into_response(),
        };
    }
    if path == "/api" {
        let query = uri.query().unwrap_or_default();
        if query.contains("mode=queue") && query.contains("name=") {
            let id = query_value(query, "value").unwrap_or("download-1");
            return axum::Json(json!({"status": true, "nzo_ids": [id]})).into_response();
        }
        if query.contains("mode=history") && query.contains("name=delete") {
            return axum::Json(json!({"status": true})).into_response();
        }
        if query.contains("mode=retry") {
            let id = query_value(query, "value").unwrap_or("download-1");
            return axum::Json(json!({"status": true, "nzo_ids": [id]})).into_response();
        }
        if query.contains("mode=queue") {
            return axum::Json(json!({"queue": {"slots": []}})).into_response();
        }
        if query.contains("mode=history") {
            return axum::Json(json!({"history": {"slots": []}})).into_response();
        }
        return axum::Json(json!({"version": "4.0"})).into_response();
    }

    StatusCode::NOT_FOUND.into_response()
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then_some(value)
    })
}

fn service(base_url: &str, timeout: Duration) -> MediaService {
    let config = MediaConfig {
        jellyseerr: ServiceConfig::new("jellyseerr", base_url, SECRET).unwrap(),
        sabnzbd: ServiceConfig::new("sabnzbd", base_url, SECRET).unwrap(),
        jellyfin: ServiceConfig::new("jellyfin", base_url, SECRET).unwrap(),
    };
    let http = reqwest::Client::builder().timeout(timeout).build().unwrap();
    MediaService::new(config, http)
}

async fn app() -> (Router, BackendState) {
    let state = BackendState::default();
    let base_url = spawn_backend(state.clone()).await;
    (
        build_router(service(&base_url, Duration::from_secs(2))),
        state,
    )
}

fn request(method: Method, uri: &str) -> axum::http::request::Builder {
    axum::http::Request::builder().method(method).uri(uri)
}

fn mutation(method: Method, uri: &str) -> axum::http::request::Builder {
    request(method, uri).header(API_MAJOR_HEADER, "1")
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn exact_curated_routes_are_mounted_and_mcp_is_not() {
    let (app, _) = app().await;
    let cases = [
        (Method::GET, "/api/v1/capabilities", None),
        (Method::GET, "/api/v1/health", None),
        (Method::GET, "/api/v1/media/search?query=Alien", None),
        (Method::GET, "/api/v1/media/items/item-1", None),
        (
            Method::POST,
            "/api/v1/media/requests",
            Some(r#"{"media_id":100,"media_type":"movie"}"#),
        ),
        (Method::GET, "/api/v1/media/requests", None),
        (Method::POST, "/api/v1/media/requests/42/approve", None),
        (Method::POST, "/api/v1/media/requests/42/decline", None),
        (Method::GET, "/api/v1/media/downloads", None),
        (
            Method::POST,
            "/api/v1/media/downloads/download-1/pause",
            None,
        ),
        (
            Method::POST,
            "/api/v1/media/downloads/download-1/resume",
            None,
        ),
        (Method::DELETE, "/api/v1/media/downloads/download-1", None),
        (
            Method::POST,
            "/api/v1/media/downloads/download-1/retry",
            None,
        ),
        (Method::GET, "/api/v1/media/library/status", None),
        (Method::POST, "/api/v1/media/library/refresh", None),
        (Method::GET, "/api/v1/media/sessions", None),
        (Method::GET, "/livez", None),
        (Method::GET, "/readyz", None),
    ];

    for (method, uri, body) in cases {
        let mut builder = request(method.clone(), uri);
        if method != Method::GET {
            builder = builder.header(API_MAJOR_HEADER, "1");
        }
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{method} {uri}");
    }

    for uri in [
        "/mcp",
        "/api/v1/mcp",
        "/api/v1/media/downloads/download-1/cancel",
        "/api/v1/proxy",
    ] {
        let response = app
            .clone()
            .oneshot(request(Method::GET, uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }

    for (method, uri) in [
        (Method::POST, "/api/v1/media/search"),
        (Method::GET, "/api/v1/media/library/refresh"),
    ] {
        let response = app
            .clone()
            .oneshot(request(method, uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{uri}");
    }
}

#[tokio::test]
async fn incompatible_or_missing_client_major_is_rejected_before_mutation() {
    let (app, backend) = app().await;
    for header in [Some("2"), None] {
        let mut builder = request(Method::POST, "/api/v1/media/requests/42/approve");
        if let Some(header) = header {
            builder = builder.header(API_MAJOR_HEADER, header);
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = json_body(response).await;
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "conflict");
    }
    assert!(backend.calls.lock().is_empty());
}

#[tokio::test]
async fn create_request_rejects_unknown_fields_and_wrong_content_type() {
    let (app, backend) = app().await;
    let body = r#"{"media_id":100,"media_type":"movie","backend_url":"http://attacker"}"#;
    let response = app
        .clone()
        .oneshot(
            mutation(Method::POST, "/api/v1/media/requests")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(response).await["error"]["code"], "validation");

    let response = app
        .oneshot(
            mutation(Method::POST, "/api/v1/media/requests")
                .body(Body::from(r#"{"media_id":100,"media_type":"movie"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(backend.calls.lock().is_empty());
}

#[tokio::test]
async fn bodyless_mutations_reject_caller_supplied_fields() {
    let (app, backend) = app().await;
    let response = app
        .oneshot(
            mutation(Method::POST, "/api/v1/media/requests/42/approve")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"backend_url":"http://attacker"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(response).await["error"]["code"], "validation");
    assert!(backend.calls.lock().is_empty());
}

#[tokio::test]
async fn query_routes_reject_unknown_fields_and_oversized_values() {
    let (app, backend) = app().await;
    for uri in [
        "/api/v1/media/search?query=Alien&backend_url=http%3A%2F%2Fattacker",
        "/api/v1/media/requests?host=attacker",
        "/api/v1/media/downloads?backend_url=attacker",
    ] {
        let response = app
            .clone()
            .oneshot(request(Method::GET, uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
    }
    let long_query = "x".repeat(257);
    let response = app
        .oneshot(
            request(
                Method::GET,
                &format!("/api/v1/media/search?query={long_query}"),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(backend.calls.lock().is_empty());
}

#[tokio::test]
async fn identifiers_reject_path_syntax_before_backend_calls() {
    let (app, backend) = app().await;
    for (method, uri) in [
        (Method::GET, "/api/v1/media/items/%2E%2E"),
        (
            Method::POST,
            "/api/v1/media/requests/http%3A%2F%2Fattacker/approve",
        ),
        (Method::POST, "/api/v1/media/downloads/%2Fapi/pause"),
    ] {
        let response = app
            .clone()
            .oneshot(mutation(method, uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
    }
    assert!(backend.calls.lock().is_empty());
}

#[tokio::test]
async fn delete_download_defaults_delete_files_false() {
    let (app, backend) = app().await;
    let response = app
        .oneshot(
            mutation(Method::DELETE, "/api/v1/media/downloads/download-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let calls = backend.calls.lock();
    assert!(calls.iter().any(|call| call.contains("del_files=0")));
}

#[tokio::test]
async fn upstream_secret_body_and_credentials_are_not_returned() {
    let (app, _) = app().await;
    let response = app
        .oneshot(
            mutation(Method::POST, "/api/v1/media/requests/secret/approve")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains(SECRET));
    assert!(!text.contains("upstream body"));
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn stable_error_codes_map_to_documented_http_statuses() {
    let (app, _) = app().await;
    for (id, status, code) in [
        ("missing", StatusCode::NOT_FOUND, "not_found"),
        (
            "unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
        ),
        ("invalid", StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    ] {
        let response = app
            .clone()
            .oneshot(
                request(Method::GET, &format!("/api/v1/media/items/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let body = json_body(response).await;
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], code);
    }
}

#[tokio::test]
async fn timeouts_are_503_and_mutations_preserve_unknown_outcome() {
    for (path, backend_path, expected_code, retryable) in [
        ("/api/v1/media/items/slow", "/Items/slow", "timeout", true),
        (
            "/api/v1/media/library/refresh",
            "/Library/Refresh",
            "unknown_outcome",
            false,
        ),
    ] {
        let state = BackendState {
            delay_path: Some(backend_path),
            ..BackendState::default()
        };
        let base_url = spawn_backend(state).await;
        let app = build_router(service(&base_url, Duration::from_millis(30)));
        let method = if path.ends_with("refresh") {
            Method::POST
        } else {
            Method::GET
        };
        let mut builder = request(method.clone(), path);
        if method == Method::POST {
            builder = builder.header(API_MAJOR_HEADER, "1");
        }
        let response = app
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = json_body(response).await;
        assert_eq!(body["error"]["code"], expected_code);
        assert_eq!(body["error"]["retryable"], retryable);
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn mutation_body_ingestion_times_out_before_backend_dispatch() {
    let (app, backend) = app().await;
    for (path, content_type) in [
        ("/api/v1/media/requests", Some("application/json")),
        ("/api/v1/media/requests/42/approve", None),
    ] {
        let mut builder = mutation(Method::POST, path);
        if let Some(content_type) = content_type {
            builder = builder.header("content-type", content_type);
        }
        let body =
            Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>());
        let response = tokio::time::timeout(
            Duration::from_secs(6),
            app.clone().oneshot(builder.body(body).unwrap()),
        )
        .await
        .expect("mutation body ingestion must have its own timeout")
        .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
        let body = json_body(response).await;
        assert_eq!(body["error"]["code"], "timeout");
        assert_eq!(body["error"]["retryable"], true);
    }
    assert!(backend.calls.lock().is_empty());
}

#[tokio::test]
async fn request_ids_are_accepted_or_generated_and_propagated() {
    let (app, _) = app().await;
    for supplied in [Some("request-from-client"), None] {
        let mut builder = request(Method::GET, "/api/v1/capabilities");
        if let Some(id) = supplied {
            builder = builder.header(REQUEST_ID_HEADER, id);
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let response_id = response
            .headers()
            .get(REQUEST_ID_HEADER)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(!response_id.is_empty());
        if let Some(id) = supplied {
            assert_eq!(response_id, id);
        }
        assert_eq!(json_body(response).await["request_id"], response_id);
    }
}

#[tokio::test]
async fn request_body_limit_returns_a_structured_error() {
    let (app, backend) = app().await;
    let body = Bytes::from(vec![b'x'; 70 * 1024]);
    let response = app
        .oneshot(
            mutation(Method::POST, "/api/v1/media/requests")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = json_body(response).await;
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"]["code"], "validation");
    assert!(backend.calls.lock().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_request_emits_one_redacted_completion_event() {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_writer(LogWriter(logs.clone()))
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let (app, _) = app().await;
    let response = app
        .oneshot(
            mutation(Method::POST, "/api/v1/media/requests")
                .header(REQUEST_ID_HEADER, "audit-1")
                .header("content-type", "application/json")
                .body(Body::from(Bytes::from(vec![b'x'; 70 * 1024])))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let text = String::from_utf8(logs.lock().clone()).unwrap();
    let events: Vec<Value> = text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| {
            event["fields"]["event"] == "operation_completed"
                && event["fields"]["request_id"] == "audit-1"
        })
        .collect();
    assert_eq!(events.len(), 1, "{text}");
    let fields = &events[0]["fields"];
    assert_eq!(fields["request_id"], "audit-1");
    assert_eq!(fields["operation"], "media.requests.create");
    assert_eq!(fields["risk"], "write");
    assert_eq!(fields["result_class"], "validation");
    assert_eq!(fields["backend"], "jellyseerr");
    assert_eq!(fields["retryable"], false);
    assert!(fields["duration_ms"].is_number());
    assert!(!text.contains(SECRET));
}

#[test]
fn binary_exits_nonzero_when_required_configuration_is_missing() {
    let binary = std::env::var("CARGO_BIN_EXE_homelab-api").unwrap();
    let output = Command::new(binary)
        .env_remove("JELLYSEERR_API_KEY")
        .env_remove("SABNZBD_API_KEY")
        .env_remove("JELLYFIN_API_KEY")
        .env("PORT", "0")
        .output()
        .unwrap();
    assert!(!output.status.success());
}
