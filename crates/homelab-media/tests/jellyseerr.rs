mod common;

use axum::{
    Router,
    extract::Path,
    http::{HeaderMap, Uri},
    routing::{get, post},
};
use homelab_api_model::MediaType;
use homelab_media::{clients::jellyseerr::JellyseerrClient, config::ServiceConfig};
use serde_json::json;
use parking_lot::Mutex;
use std::sync::Arc;

fn client(base_url: String, key: &str) -> JellyseerrClient {
    JellyseerrClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyseerr", base_url, key).unwrap(),
    )
}

#[tokio::test]
async fn search_normalizes_without_raw_source_and_encodes_query() {
    let app = Router::new().route(
        "/api/v1/search",
        get(|uri: Uri, headers: HeaderMap| async move {
            assert_eq!(uri.query(), Some("query=Witch%20Hat%20Atelier"));
            assert_eq!(headers.get("x-api-key").unwrap(), "key");
            common::json_response(json!({
                "results": [{
                    "id": 100,
                    "mediaType": "movie",
                    "title": "Alien",
                    "releaseDate": "1979-05-25",
                    "mediaInfo": {"status": 5},
                    "rawSecret": "must-not-escape"
                }]
            }))
        }),
    );

    let results = client(common::spawn_mock_app(app).await, "key")
        .search("Witch Hat Atelier")
        .await
        .unwrap();

    assert_eq!(results[0].id, "100");
    assert_eq!(results[0].media_type, MediaType::Movie);
    assert_eq!(results[0].title, "Alien");
    assert_eq!(results[0].year, Some(1979));
    assert_eq!(results[0].status.as_deref(), Some("5"));
    assert!(!serde_json::to_string(&results).unwrap().contains("rawSecret"));
    assert!(!serde_json::to_string(&results).unwrap().contains("source"));
}

#[tokio::test]
async fn tv_request_excludes_season_zero_and_includes_available_seasons() {
    let request_body = Arc::new(Mutex::new(None));
    let captured_body = Arc::clone(&request_body);
    let app = Router::new()
        .route(
            "/api/v1/tv/{id}",
            get(|Path(id): Path<i64>| async move {
                assert_eq!(id, 196950);
                common::json_response(json!({
                    "seasons": [
                        {"seasonNumber": 0},
                        {"seasonNumber": 1},
                        {"seasonNumber": 2}
                    ]
                }))
            }),
        )
        .route(
            "/api/v1/request",
            post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let captured_body = Arc::clone(&captured_body);
                async move {
                    *captured_body.lock() = Some(body);
                    common::json_response(json!({
                        "id": 42,
                        "mediaId": 196950,
                        "mediaType": "tv",
                        "status": 1,
                        "title": "Witch Hat Atelier"
                    }))
                }
            }),
        );

    let result = client(common::spawn_mock_app(app).await, "key")
        .request_media(MediaType::Tv, 196950)
        .await
        .unwrap();
    assert_eq!(result.media_id, "196950");
    assert_eq!(result.media_type, MediaType::Tv);
    assert_eq!(
        request_body.lock().as_ref().unwrap(),
        &json!({"mediaType": "tv", "mediaId": 196950, "seasons": [1, 2]})
    );
}

#[tokio::test]
async fn list_requests_normalizes_status_and_nested_media_type() {
    let app = Router::new().route(
        "/api/v1/request",
        get(|| async {
            common::json_response(json!({
                "results": [{
                    "id": 42,
                    "mediaId": 101,
                    "status": 1,
                    "title": "Inception",
                    "media": {"mediaType": "movie", "tmdbId": 101},
                    "unknown": {"token": "must-not-escape"}
                }]
            }))
        }),
    );

    let results = client(common::spawn_mock_app(app).await, "key")
        .list_requests(None)
        .await
        .unwrap();

    assert_eq!(results[0].id, "42");
    assert_eq!(results[0].media_id, "101");
    assert_eq!(results[0].media_type, MediaType::Movie);
    assert_eq!(results[0].status, "1");
    assert_eq!(results[0].title.as_deref(), Some("Inception"));
    assert!(!serde_json::to_string(&results).unwrap().contains("token"));
}

#[tokio::test]
async fn approve_and_decline_use_the_exact_request_id() {
    let app = Router::new()
        .route(
            "/api/v1/request/{id}/approve",
            post(|Path(id): Path<String>| async move {
                assert_eq!(id, "request-42");
                common::json_response(json!({"id": id, "status": "approved"}))
            }),
        )
        .route(
            "/api/v1/request/{id}/decline",
            post(|Path(id): Path<String>| async move {
                assert_eq!(id, "request-43");
                common::json_response(json!({"id": id, "status": "declined"}))
            }),
        );
    let client = client(common::spawn_mock_app(app).await, "key");

    let approved = client.approve_request("request-42").await.unwrap();
    let declined = client.decline_request("request-43").await.unwrap();

    assert_eq!(approved.affected_id.as_deref(), Some("request-42"));
    assert_eq!(declined.affected_id.as_deref(), Some("request-43"));
}

#[tokio::test]
async fn upstream_authorization_body_and_key_are_redacted() {
    let app = Router::new().route(
        "/api/v1/search",
        get(|| async {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                "Authorization: Bearer super-secret; x-api-key=super-secret",
            )
        }),
    );

    let error = client(common::spawn_mock_app(app).await, "super-secret")
        .search("alien")
        .await
        .unwrap_err();
    let public = error.public_message();

    assert!(!public.contains("super-secret"));
    assert!(!public.to_ascii_lowercase().contains("authorization"));
    assert!(!error.to_string().contains("super-secret"));
}
