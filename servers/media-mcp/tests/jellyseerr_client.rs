mod common;

use axum::{
    Router,
    extract::Path,
    http::Uri,
    routing::{get, post},
};
use media_mcp::{clients::jellyseerr::JellyseerrClient, config::ServiceConfig};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn search_media_normalizes_results() {
    let app = Router::new().route(
        "/api/v1/search",
        get(|| async {
            common::json_response(json!({
                "results": [{
                    "id": 100,
                    "mediaType": "movie",
                    "title": "Alien",
                    "releaseDate": "1979-05-25",
                    "mediaInfo": {"status": 5}
                }]
            }))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyseerrClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyseerr", base_url, "key").unwrap(),
    );

    let results = client.search("alien").await.unwrap();

    assert_eq!(results[0].id, "100");
    assert_eq!(results[0].media_type, "movie");
    assert_eq!(results[0].title, "Alien");
    assert_eq!(results[0].year, Some(1979));
}

#[tokio::test]
async fn search_media_percent_encodes_reserved_query_characters() {
    let app = Router::new().route(
        "/api/v1/search",
        get(|uri: Uri| async move {
            assert_eq!(uri.query(), Some("query=Witch%20Hat%20Atelier"));
            common::json_response(json!({"results": []}))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyseerrClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyseerr", base_url, "key").unwrap(),
    );

    let results = client.search("Witch Hat Atelier").await.unwrap();

    assert!(results.is_empty());
}

#[tokio::test]
async fn request_media_tv_includes_available_seasons() {
    let request_body = Arc::new(Mutex::new(None));
    let captured_body = request_body.clone();
    let app = Router::new()
        .route(
            "/api/v1/tv/{id}",
            get(|Path(id): Path<i64>| async move {
                assert_eq!(id, 196950);
                common::json_response(json!({
                    "id": 196950,
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
                let captured_body = captured_body.clone();
                async move {
                    *captured_body.lock().unwrap() = Some(body);
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
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyseerrClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyseerr", base_url, "key").unwrap(),
    );

    let result = client.request_media("tv", 196950).await.unwrap();

    assert_eq!(result.media_id, "196950");
    assert_eq!(result.media_type, "tv");
    assert_eq!(
        request_body.lock().unwrap().as_ref().unwrap(),
        &json!({
            "mediaType": "tv",
            "mediaId": 196950,
            "seasons": [1, 2]
        })
    );
}

#[tokio::test]
async fn list_requests_normalizes_paginated_results() {
    let app = Router::new().route(
        "/api/v1/request",
        get(|| async {
            common::json_response(json!({
                "pageInfo": {"pages": 1, "pageSize": 20},
                "results": [{
                    "id": 42,
                    "mediaId": 101,
                    "status": 1,
                    "title": "Inception",
                    "media": {
                        "mediaType": "movie",
                        "tmdbId": 101
                    }
                }]
            }))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyseerrClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyseerr", base_url, "key").unwrap(),
    );

    let results = client.list_requests(None).await.unwrap();

    assert!(!results.is_empty());
    assert_eq!(results[0].id, "42");
    assert_eq!(results[0].media_id, "101");
    assert_eq!(results[0].media_type, "movie");
    assert_eq!(results[0].status, "1");
    assert_eq!(results[0].title.as_deref(), Some("Inception"));
}

#[tokio::test]
async fn approve_request_returns_affected_request_id() {
    let app = Router::new().route(
        "/api/v1/request/{id}/approve",
        post(|Path(id): Path<String>| async move {
            common::json_response(json!({"id": id, "status": "approved"}))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyseerrClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyseerr", base_url, "key").unwrap(),
    );

    let result = client.approve_request("42").await.unwrap();

    assert_eq!(result.service, "jellyseerr");
    assert_eq!(result.operation, "approve_request");
    assert_eq!(result.affected_id.as_deref(), Some("42"));
}
