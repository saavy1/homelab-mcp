use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode},
    response::Response,
    routing::any,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::{
    process::{Command, Output, Stdio},
    sync::Arc,
};
use tokio::net::TcpListener;

#[derive(Clone)]
enum Mode {
    Normal,
    Error {
        status: StatusCode,
        code: &'static str,
    },
    Health(&'static str),
    Incompatible,
    ManySearch,
}

#[derive(Clone, Debug)]
struct SeenRequest {
    method: Method,
    uri: String,
    request_id: Option<String>,
    body: Vec<u8>,
}

#[derive(Clone)]
struct MockState {
    mode: Mode,
    seen: Arc<Mutex<Vec<SeenRequest>>>,
}

struct MockApi {
    base_url: String,
    seen: Arc<Mutex<Vec<SeenRequest>>>,
}

impl MockApi {
    async fn spawn(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let state = MockState {
            mode,
            seen: seen.clone(),
        };
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/{*path}", any(mock_handler))
                    .with_state(state),
            )
            .await
            .unwrap();
        });
        Self {
            base_url: format!("http://{address}/api/v1"),
            seen,
        }
    }

    fn requests(&self) -> Vec<SeenRequest> {
        self.seen.lock().clone()
    }
}

async fn mock_handler(State(state): State<MockState>, request: Request) -> Response {
    let method = request.method().clone();
    let uri = request.uri().to_string();
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(request.into_body(), 64 * 1024)
        .await
        .unwrap()
        .to_vec();
    state.seen.lock().push(SeenRequest {
        method: method.clone(),
        uri: uri.clone(),
        request_id: request_id.clone(),
        body: body.clone(),
    });

    let path = uri.split('?').next().unwrap();
    let (status, payload) = if path == "/api/v1/capabilities" {
        let compatible_cli_major = if matches!(state.mode, Mode::Incompatible) {
            2
        } else {
            1
        };
        (
            StatusCode::OK,
            envelope(
                "capabilities",
                request_id.as_deref().unwrap_or("server-request"),
                json!({
                    "api": {"major": 1, "minor": 0},
                    "compatible_cli_major": compatible_cli_major,
                    "operations": ["media.search", "media.downloads.delete"]
                }),
            ),
        )
    } else if let Mode::Error { status, code } = state.mode {
        (
            status,
            json!({
                "ok": false,
                "operation": operation_for(&method, path),
                "request_id": request_id.as_deref().unwrap_or("server-request"),
                "risk": "read",
                "summary": {"text": "safe public failure"},
                "error": {"code": code, "message": "safe public failure", "retryable": status == StatusCode::SERVICE_UNAVAILABLE},
                "provenance": {"service": "mock", "timestamp": "2026-08-19T00:00:00Z"}
            }),
        )
    } else {
        normal_response(
            &state.mode,
            &method,
            path,
            request_id.as_deref().unwrap_or("server-request"),
        )
    };

    let mut response = Response::new(Body::from(serde_json::to_vec(&payload).unwrap()));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    if let Some(request_id) = request_id.and_then(|id| HeaderValue::from_str(&id).ok()) {
        response.headers_mut().insert("x-request-id", request_id);
    }
    response
}

fn envelope(operation: &str, request_id: &str, data: Value) -> Value {
    json!({
        "ok": true,
        "operation": operation,
        "request_id": request_id,
        "risk": "read",
        "summary": {"text": "mock operation completed"},
        "data": data,
        "provenance": {"service": "mock", "timestamp": "2026-08-19T00:00:00Z"}
    })
}

fn operation_for(method: &Method, path: &str) -> &'static str {
    match (method, path) {
        (&Method::GET, "/api/v1/health") => "media.health",
        (&Method::GET, "/api/v1/media/search") => "media.search",
        (&Method::POST, "/api/v1/media/requests") => "media.requests.create",
        (&Method::GET, "/api/v1/media/requests") => "media.requests.list",
        (&Method::GET, "/api/v1/media/downloads") => "media.downloads.list",
        (&Method::GET, "/api/v1/media/library/status") => "media.library.status",
        (&Method::POST, "/api/v1/media/library/refresh") => "media.library.refresh",
        (&Method::GET, "/api/v1/media/sessions") => "media.sessions.list",
        _ if path.ends_with("/approve") => "media.requests.approve",
        _ if path.ends_with("/decline") => "media.requests.decline",
        _ if path.ends_with("/pause") => "media.downloads.pause",
        _ if path.ends_with("/resume") => "media.downloads.resume",
        _ if path.ends_with("/retry") => "media.downloads.retry",
        (&Method::DELETE, _) => "media.downloads.delete",
        (&Method::GET, _) => "media.items.show",
        _ => "unknown",
    }
}

fn normal_response(
    mode: &Mode,
    method: &Method,
    path: &str,
    request_id: &str,
) -> (StatusCode, Value) {
    let operation = operation_for(method, path);
    let data = match (method, path) {
        (&Method::GET, "/api/v1/health") => json!({
            "status": match mode { Mode::Health(status) => *status, _ => "healthy" },
            "backends": [{"backend": "jellyfin", "healthy": true, "message": null}]
        }),
        (&Method::GET, "/api/v1/media/search") => {
            if matches!(mode, Mode::ManySearch) {
                Value::Array((0..25).map(|index| json!({
                    "id": index.to_string(), "media_type": "movie", "title": format!("Title {index}"),
                    "year": 2000 + index, "status": "available", "api_key": "do-not-print"
                })).collect())
            } else {
                json!([{"id": "60625", "media_type": "tv", "title": "Rick and Morty", "year": 2013, "status": "available"}])
            }
        }
        (&Method::POST, "/api/v1/media/requests") => json!({
            "id": "req-100", "media_id": "100", "media_type": "movie", "status": "pending", "title": "Alien"
        }),
        (&Method::GET, "/api/v1/media/requests") => json!([{
            "id": "req-100", "media_id": "100", "media_type": "movie", "status": "pending", "title": "Alien"
        }]),
        (&Method::GET, "/api/v1/media/downloads") => json!([{
            "id": "nzo-1", "name": "Alien", "status": "downloading", "percentage": "20", "size": "1 GB"
        }]),
        (&Method::GET, "/api/v1/media/library/status") => {
            json!({"item_count": 3, "movie_count": 2, "series_count": 1})
        }
        (&Method::GET, "/api/v1/media/sessions") => {
            json!([{"id": "session-1", "user_name": "viewer", "item_name": "Alien"}])
        }
        (&Method::GET, _) if path.starts_with("/api/v1/media/items/") => json!({
            "id": "60625", "media_type": "tv", "title": "Rick and Morty", "year": 2013, "status": "available"
        }),
        _ => json!({"service": "mock", "operation": operation, "affected_id": "target-1"}),
    };
    (StatusCode::OK, envelope(operation, request_id, data))
}

fn run(api: &MockApi, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_homelab"))
        .args(args)
        .env("HOMELAB_API_URL", &api.base_url)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn assert_json_success(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn representative_commands_emit_one_json_document_and_exact_requests() {
    let api = MockApi::spawn(Mode::Normal).await;
    for args in [
        vec!["capabilities"],
        vec!["media", "search", "--query", "Alien"],
        vec![
            "media",
            "request",
            "create",
            "--media-id",
            "100",
            "--media-type",
            "movie",
        ],
        vec![
            "media",
            "downloads",
            "delete",
            "--download-id",
            "nzo-1",
            "--delete-files",
        ],
        vec!["media", "library", "status"],
        vec!["media", "sessions", "list"],
    ] {
        assert_json_success(&run(&api, &args));
    }

    let requests = api.requests();
    assert!(requests.iter().any(|request| request.method == Method::GET
        && request.uri == "/api/v1/media/search?query=Alien"));
    let create = requests
        .iter()
        .find(|request| request.method == Method::POST && request.uri == "/api/v1/media/requests")
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&create.body).unwrap(),
        json!({"media_id": 100, "media_type": "movie"})
    );
    assert!(
        requests
            .iter()
            .any(|request| request.method == Method::DELETE
                && request.uri == "/api/v1/media/downloads/nzo-1?delete_files=true")
    );
    assert!(
        requests
            .iter()
            .any(|request| request.uri == "/api/v1/media/library/status")
    );
    assert!(
        requests
            .iter()
            .any(|request| request.uri == "/api/v1/media/sessions")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_complete_curated_command_tree_is_available() {
    let api = MockApi::spawn(Mode::Normal).await;
    for args in [
        vec!["media", "health"],
        vec![
            "media",
            "item",
            "show",
            "--item-id",
            "60625",
            "--media-type",
            "tv",
        ],
        vec!["media", "requests", "list", "--status", "pending"],
        vec!["media", "requests", "approve", "--request-id", "media-1"],
        vec!["media", "requests", "decline", "--request-id", "media-1"],
        vec!["media", "downloads", "list", "--status", "failed"],
        vec!["media", "downloads", "pause", "--download-id", "nzo-1"],
        vec!["media", "downloads", "resume", "--download-id", "nzo-1"],
        vec!["media", "downloads", "retry", "--download-id", "nzo-1"],
        vec!["media", "library", "refresh"],
    ] {
        assert_json_success(&run(&api, &args));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn search_result_id_and_type_are_accepted_by_item_show_with_exact_query() {
    let api = MockApi::spawn(Mode::Normal).await;
    let search = assert_json_success(&run(
        &api,
        &["media", "search", "--query", "Rick and Morty"],
    ));
    let item = &search["data"][0];
    let item_id = item["id"].as_str().unwrap();
    let media_type = item["media_type"].as_str().unwrap();

    let shown = assert_json_success(&run(
        &api,
        &[
            "media",
            "item",
            "show",
            "--item-id",
            item_id,
            "--media-type",
            media_type,
        ],
    ));

    assert_eq!(shown["data"]["id"], "60625");
    assert_eq!(shown["data"]["media_type"], "tv");
    let requests = api.requests();
    assert_eq!(requests[0].uri, "/api/v1/media/search?query=Rick+and+Morty");
    assert_eq!(
        requests[1].uri,
        "/api/v1/media/items/60625?media_type=tv"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_arguments_exit_two_without_http_and_missing_config_is_structured() {
    let api = MockApi::spawn(Mode::Normal).await;
    for args in [
        vec!["media", "search"],
        vec!["media", "item", "show", "--item-id", "60625"],
        vec![
            "media",
            "item",
            "show",
            "--item-id",
            "60625",
            "--media-type",
            "series",
        ],
        vec![
            "media",
            "item",
            "show",
            "--item-id",
            "not-numeric",
            "--media-type",
            "tv",
        ],
    ] {
        let invalid = run(&api, &args);
        assert_eq!(invalid.status.code(), Some(2), "{args:?}");
        let invalid_json: Value = serde_json::from_slice(&invalid.stdout).unwrap();
        assert_eq!(invalid_json["ok"], false);
    }
    assert!(api.requests().is_empty());

    let missing = Command::new(env!("CARGO_BIN_EXE_homelab"))
        .arg("capabilities")
        .env_remove("HOMELAB_API_URL")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let missing_json: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(missing_json["error"]["code"], "validation");
    assert!(!String::from_utf8_lossy(&missing.stdout).contains("HOME="));
}

#[tokio::test(flavor = "multi_thread")]
async fn generated_and_accepted_request_ids_are_sent_without_reading_stdin() {
    let generated_api = MockApi::spawn(Mode::Normal).await;
    assert_json_success(&run(&generated_api, &["capabilities"]));
    let generated = generated_api.requests()[0].request_id.clone().unwrap();
    assert!(uuid::Uuid::parse_str(&generated).is_ok());

    let accepted_api = MockApi::spawn(Mode::Normal).await;
    let output = run(
        &accepted_api,
        &["--request-id", "agent-correlation-1", "capabilities"],
    );
    let value = assert_json_success(&output);
    assert_eq!(value["request_id"], "agent-correlation-1");
    assert_eq!(
        accepted_api.requests()[0].request_id.as_deref(),
        Some("agent-correlation-1")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn request_ids_that_are_not_http_header_values_fail_before_http() {
    let api = MockApi::spawn(Mode::Normal).await;
    let output = run(
        &api,
        &["--request-id", "agent\ninjected-header", "capabilities"],
    );

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "validation");
    assert!(api.requests().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn api_errors_and_partial_health_use_stable_exit_classes() {
    for (status, code, expected) in [
        (StatusCode::FORBIDDEN, "forbidden", 3),
        (StatusCode::NOT_FOUND, "not_found", 4),
        (StatusCode::CONFLICT, "conflict", 4),
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable", 5),
        (StatusCode::SERVICE_UNAVAILABLE, "timeout", 5),
    ] {
        let api = MockApi::spawn(Mode::Error { status, code }).await;
        let output = run(&api, &["media", "search", "--query", "Alien"]);
        assert_eq!(output.status.code(), Some(expected));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["code"], code);
    }

    let degraded = MockApi::spawn(Mode::Health("degraded")).await;
    let output = run(&degraded, &["media", "health"]);
    assert_eq!(output.status.code(), Some(6));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["status"], "degraded");

    let unavailable = MockApi::spawn(Mode::Health("unavailable")).await;
    assert_eq!(
        run(&unavailable, &["media", "health"]).status.code(),
        Some(5)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn table_output_is_bounded_and_cannot_render_unknown_credentials() {
    let api = MockApi::spawn(Mode::ManySearch).await;
    let output = run(
        &api,
        &["--output", "table", "media", "search", "--query", "Alien"],
    );
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Title 0"));
    assert!(text.lines().count() <= 28, "table was not bounded: {text}");
    assert!(!text.contains("api_key"));
    assert!(!text.contains("do-not-print"));
    assert!(serde_json::from_str::<Value>(&text).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn destructive_commands_stop_on_incompatibility_and_are_never_retried() {
    let incompatible = MockApi::spawn(Mode::Incompatible).await;
    let output = run(
        &incompatible,
        &["media", "downloads", "delete", "--download-id", "nzo-1"],
    );
    assert_eq!(output.status.code(), Some(4));
    let requests = incompatible.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].uri, "/api/v1/capabilities");

    let unavailable = MockApi::spawn(Mode::Error {
        status: StatusCode::SERVICE_UNAVAILABLE,
        code: "unavailable",
    })
    .await;
    let output = run(
        &unavailable,
        &["media", "downloads", "delete", "--download-id", "nzo-1"],
    );
    assert_eq!(output.status.code(), Some(5));
    let requests = unavailable.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == Method::DELETE)
            .count(),
        1
    );
}
