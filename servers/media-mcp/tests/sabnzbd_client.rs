mod common;

use axum::{Router, extract::Query, routing::get};
use media_mcp::{clients::sabnzbd::SabnzbdClient, config::ServiceConfig};
use serde_json::json;
use std::collections::HashMap;

#[tokio::test]
async fn list_downloads_reads_queue_and_failed_history() {
    let app = Router::new().route(
        "/api",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            match params.get("mode").map(|s| s.as_str()) {
                Some("queue") => common::json_response(json!({
                    "queue": {
                        "slots": [{
                            "nzo_id": "q1",
                            "filename": "Movie",
                            "status": "Downloading",
                            "percentage": "50",
                            "size": "1 GB"
                        }]
                    }
                })),
                Some("history") => common::json_response(json!({
                    "history": {
                        "slots": [{
                            "nzo_id": "h1",
                            "name": "Failed",
                            "status": "Failed",
                            "size": "2 GB"
                        }]
                    }
                })),
                _ => common::json_response(json!({"error": "unknown mode"})),
            }
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = SabnzbdClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("sabnzbd", base_url, "key").unwrap(),
    );

    let downloads = client.list_downloads(None).await.unwrap();

    assert_eq!(downloads.len(), 2);
    assert_eq!(downloads[0].id, "q1");
    assert_eq!(downloads[1].status, "Failed");
}

#[tokio::test]
async fn pause_download_requires_id_and_returns_affected_id() {
    let app = Router::new().route(
        "/api",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            assert_eq!(params.get("mode"), Some(&"queue".to_string()));
            assert_eq!(params.get("name"), Some(&"pause".to_string()));
            let value = params.get("value").cloned().unwrap_or_default();
            common::json_response(json!({
                "status": true,
                "nzo_ids": [value]
            }))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = SabnzbdClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("sabnzbd", base_url, "key").unwrap(),
    );

    let result = client.pause_download("SABnzbd_nzo_123").await.unwrap();

    assert_eq!(result.service, "sabnzbd");
    assert_eq!(result.operation, "pause_download");
    assert_eq!(result.affected_id.as_deref(), Some("SABnzbd_nzo_123"));

    let err = client.pause_download("").await.unwrap_err();
    assert!(err.to_string().contains("a specific nzo_id is required"));
}

#[tokio::test]
async fn sends_required_query_params() {
    let app = Router::new().route(
        "/api",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            assert_eq!(params.get("output"), Some(&"json".to_string()));
            assert_eq!(params.get("apikey"), Some(&"my-key".to_string()));
            match params.get("mode").map(|s| s.as_str()) {
                Some("queue") => common::json_response(json!({ "queue": { "slots": [] } })),
                Some("history") => common::json_response(json!({ "history": { "slots": [] } })),
                _ => common::json_response(json!({ "error": "unknown mode" })),
            }
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = SabnzbdClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("sabnzbd", base_url, "my-key").unwrap(),
    );

    let _downloads = client.list_downloads(None).await.unwrap();
}

#[tokio::test]
async fn json_error_response_maps_to_upstream_error() {
    let app = Router::new().route(
        "/api",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            assert_eq!(params.get("mode"), Some(&"queue".to_string()));
            common::json_response(json!({ "error": "API Key Incorrect" }))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = SabnzbdClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("sabnzbd", base_url, "secret").unwrap(),
    );

    let err = client.list_downloads(None).await.unwrap_err();
    assert!(
        err.to_string().contains("upstream error"),
        "expected upstream error, got: {}",
        err
    );
    assert!(err.to_string().contains("API Key Incorrect"));
}

#[tokio::test]
async fn action_false_status_empty_nzo_ids_maps_to_error() {
    let app = Router::new().route(
        "/api",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            assert_eq!(params.get("mode"), Some(&"queue".to_string()));
            assert_eq!(params.get("name"), Some(&"pause".to_string()));
            common::json_response(json!({ "status": false, "nzo_ids": [] }))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = SabnzbdClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("sabnzbd", base_url, "key").unwrap(),
    );

    let err = client.pause_download("nzo_123").await.unwrap_err();
    assert!(
        err.to_string().contains("upstream error"),
        "expected upstream error, got: {}",
        err
    );
}

#[tokio::test]
async fn serialized_error_does_not_contain_apikey_on_upstream_failure() {
    let app = Router::new().route(
        "/api",
        get(|_query: Query<HashMap<String, String>>| async move {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({ "message": "server error" })),
            )
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = SabnzbdClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("sabnzbd", base_url, "leaked-secret").unwrap(),
    );

    let err = client.list_downloads(None).await.unwrap_err();
    let serialized = err.to_tool_error();
    assert!(
        !serialized.contains("leaked-secret"),
        "serialized error contained apikey value: {}",
        serialized
    );
    assert!(
        !serialized.contains("apikey"),
        "serialized error contained 'apikey': {}",
        serialized
    );
}
