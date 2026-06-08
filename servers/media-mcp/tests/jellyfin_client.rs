mod common;

use axum::{
    Router,
    extract::Path,
    routing::{get, post},
};
use media_mcp::{clients::jellyfin::JellyfinClient, config::ServiceConfig};
use serde_json::json;

#[tokio::test]
async fn get_library_status_returns_counts() {
    let app = Router::new().route(
        "/Items/Counts",
        get(|| async {
            common::json_response(json!({
                "ItemCount": 12,
                "MovieCount": 3,
                "SeriesCount": 2
            }))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyfinClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyfin", base_url, "key").unwrap(),
    );

    let status = client.get_library_status().await.unwrap();

    assert_eq!(status.item_count, Some(12));
    assert_eq!(status.movie_count, Some(3));
    assert_eq!(status.series_count, Some(2));
}

#[tokio::test]
async fn refresh_library_posts_refresh_endpoint() {
    let app = Router::new().route(
        "/Library/Refresh",
        post(|| async { common::json_response(json!({ "ok": true })) }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyfinClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyfin", base_url, "key").unwrap(),
    );

    let result = client.refresh_library().await.unwrap();

    assert_eq!(result.service, "jellyfin");
    assert_eq!(result.operation, "refresh_library");
}

#[tokio::test]
async fn refresh_library_succeeds_on_204_no_content() {
    let app = Router::new().route(
        "/Library/Refresh",
        post(|| async { axum::http::StatusCode::NO_CONTENT }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyfinClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyfin", base_url, "key").unwrap(),
    );

    let result = client.refresh_library().await.unwrap();

    assert_eq!(result.service, "jellyfin");
    assert_eq!(result.operation, "refresh_library");
    assert_eq!(result.source, json!({}));
}

#[tokio::test]
async fn refresh_library_succeeds_on_empty_success_body() {
    let app = Router::new().route(
        "/Library/Refresh",
        post(|| async { (axum::http::StatusCode::OK, "") }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyfinClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyfin", base_url, "key").unwrap(),
    );

    let result = client.refresh_library().await.unwrap();

    assert_eq!(result.service, "jellyfin");
    assert_eq!(result.operation, "refresh_library");
    assert_eq!(result.source, json!({}));
}

#[tokio::test]
async fn get_active_sessions_normalizes_fields_and_preserves_source() {
    let app = Router::new().route(
        "/Sessions",
        get(|| async {
            common::json_response(json!([
                {
                    "Id": "session-1",
                    "UserName": "Alice",
                    "NowPlayingItem": { "Name": "Movie A" },
                    "ExtraField": "ignored"
                },
                {
                    "Id": "session-2",
                    "UserName": null,
                    "NowPlayingItem": null
                }
            ]))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyfinClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyfin", base_url, "key").unwrap(),
    );

    let sessions = client.get_active_sessions().await.unwrap();
    assert_eq!(sessions.len(), 2);

    let s0 = &sessions[0];
    assert_eq!(s0.id, "session-1");
    assert_eq!(s0.user_name, Some("Alice".to_string()));
    assert_eq!(s0.item_name, Some("Movie A".to_string()));
    assert_eq!(
        s0.source,
        json!({
            "Id": "session-1",
            "UserName": "Alice",
            "NowPlayingItem": { "Name": "Movie A" },
            "ExtraField": "ignored"
        })
    );

    let s1 = &sessions[1];
    assert_eq!(s1.id, "session-2");
    assert_eq!(s1.user_name, None);
    assert_eq!(s1.item_name, None);
}

#[tokio::test]
async fn decode_error_serializes_without_token_and_service_is_jellyfin() {
    let app = Router::new().route(
        "/Items/Counts",
        get(|| async { (axum::http::StatusCode::OK, "not json") }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyfinClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyfin", base_url, "secret_token_xyz").unwrap(),
    );

    let err = client.get_library_status().await.unwrap_err();
    let serialized = err.to_tool_error();
    assert!(serialized.contains("\"service\":\"jellyfin\""));
    assert!(!serialized.contains("secret_token_xyz"));
}

#[tokio::test]
async fn get_item_details_requires_id() {
    let app = Router::new().route(
        "/Items/{id}",
        get(|Path(id): Path<String>| async move {
            common::json_response(json!({ "Id": id, "Name": "Alien" }))
        }),
    );
    let base_url = common::spawn_mock_app(app).await;
    let client = JellyfinClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyfin", base_url, "key").unwrap(),
    );

    let details = client.get_item_details("movie-123").await.unwrap();
    assert_eq!(details.get("Name").and_then(|v| v.as_str()), Some("Alien"));

    let err = client.get_item_details("").await.unwrap_err();
    assert!(err.to_string().contains("item_id is required"));
}
