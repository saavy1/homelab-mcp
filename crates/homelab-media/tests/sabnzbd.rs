mod common;

use axum::{Router, extract::Query, routing::get};
use homelab_media::{clients::sabnzbd::SabnzbdClient, config::ServiceConfig};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use std::sync::atomic::{AtomicUsize, Ordering};

fn client(base_url: String, key: &str) -> SabnzbdClient {
    SabnzbdClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("sabnzbd", base_url, key).unwrap(),
    )
}

#[tokio::test]
async fn list_downloads_normalizes_queue_and_failed_history_without_raw_source() {
    let app = Router::new().route(
        "/api",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            assert_eq!(params.get("output").map(String::as_str), Some("json"));
            assert_eq!(params.get("apikey").map(String::as_str), Some("key"));
            match params.get("mode").map(String::as_str) {
                Some("queue") => common::json_response(json!({
                    "queue": {"slots": [{
                        "nzo_id": "q1", "filename": "Movie", "status": "Downloading",
                        "percentage": "50", "size": "1 GB", "secret": "raw"
                    }]}
                })),
                Some("history") => common::json_response(json!({
                    "history": {"slots": [{
                        "nzo_id": "h1", "name": "Failed", "status": "Failed", "size": "2 GB"
                    }]}
                })),
                _ => panic!("unexpected mode"),
            }
        }),
    );

    let downloads = client(common::spawn_mock_app(app).await, "key")
        .list_downloads(None)
        .await
        .unwrap();

    assert_eq!(downloads.len(), 2);
    assert_eq!(downloads[0].id, "q1");
    assert_eq!(downloads[0].percentage.as_deref(), Some("50"));
    assert_eq!(downloads[1].status, "Failed");
    let json = serde_json::to_string(&downloads).unwrap();
    assert!(!json.contains("secret"));
    assert!(!json.contains("source"));
}

#[tokio::test]
async fn pause_resume_delete_and_retry_validate_action_responses() {
    let app = Router::new().route(
        "/api",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            let mode = params.get("mode").map(String::as_str).unwrap();
            if mode == "retry" {
                assert_eq!(params.get("value").map(String::as_str), Some("h1"));
                return common::json_response(json!({"status": true}));
            }
            let action = params.get("name").map(String::as_str).unwrap();
            let id = params.get("value").cloned().unwrap();
            assert!(matches!(action, "pause" | "resume" | "delete"));
            if action == "delete" {
                assert_eq!(params.get("del_files").map(String::as_str), Some("1"));
            }
            common::json_response(json!({"status": true, "nzo_ids": [id]}))
        }),
    );
    let client = client(common::spawn_mock_app(app).await, "key");

    assert_eq!(client.pause_download("q1").await.unwrap().affected_id.as_deref(), Some("q1"));
    assert_eq!(client.resume_download("q2").await.unwrap().affected_id.as_deref(), Some("q2"));
    assert_eq!(client.delete_download("q3", true).await.unwrap().affected_id.as_deref(), Some("q3"));
    assert_eq!(client.retry_failed_download("h1").await.unwrap().affected_id.as_deref(), Some("h1"));
}

#[tokio::test]
async fn false_or_malformed_action_response_is_an_upstream_error() {
    let requests = Arc::new(AtomicUsize::new(0));
    let request_count = Arc::clone(&requests);
    let app = Router::new().route(
        "/api",
        get(move |Query(_): Query<HashMap<String, String>>| {
            request_count.fetch_add(1, Ordering::SeqCst);
            async { common::json_response(json!({"status": false, "nzo_ids": []})) }
        }),
    );
    let client = client(common::spawn_mock_app(app).await, "key");

    let error = client.pause_download("q1").await.unwrap_err();

    assert!(matches!(error, homelab_media::MediaError::Upstream(_)));
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn blank_download_ids_fail_before_an_upstream_request() {
    let requests = Arc::new(AtomicUsize::new(0));
    let request_count = Arc::clone(&requests);
    let app = Router::new().route(
        "/api",
        get(move || {
            request_count.fetch_add(1, Ordering::SeqCst);
            async { common::json_response(json!({"status": true})) }
        }),
    );
    let client = client(common::spawn_mock_app(app).await, "key");

    assert!(client.pause_download("  ").await.is_err());
    assert!(client.resume_download("").await.is_err());
    assert!(client.delete_download("\t", false).await.is_err());
    assert!(client.retry_failed_download("\n").await.is_err());
    assert_eq!(requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn upstream_body_and_api_key_are_redacted_from_errors() {
    let app = Router::new().route(
        "/api",
        get(|| async {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                "apikey=leaked-secret Authorization: Bearer leaked-secret",
            )
        }),
    );

    let error = client(common::spawn_mock_app(app).await, "leaked-secret")
        .list_downloads(None)
        .await
        .unwrap_err();
    let public = error.public_message();

    assert!(!public.contains("leaked-secret"));
    assert!(!public.to_ascii_lowercase().contains("apikey"));
    assert!(!error.to_string().contains("leaked-secret"));
}
