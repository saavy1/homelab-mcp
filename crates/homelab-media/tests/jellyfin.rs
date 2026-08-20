mod common;

use axum::{
    Router,
    http::HeaderMap,
    routing::{get, post},
};
use homelab_media::{clients::jellyfin::JellyfinClient, config::ServiceConfig};
use serde_json::json;

fn client(base_url: String, key: &str) -> JellyfinClient {
    JellyfinClient::new(
        reqwest::Client::new(),
        ServiceConfig::new("jellyfin", base_url, key).unwrap(),
    )
}

#[tokio::test]
async fn library_status_returns_normalized_counts_and_api_key_header() {
    let app = Router::new().route(
        "/Items/Counts",
        get(|headers: HeaderMap| async move {
            assert_eq!(headers.get("x-emby-token").unwrap(), "key");
            common::json_response(json!({
                "ItemCount": 12, "MovieCount": 3, "SeriesCount": 2, "Raw": "ignored"
            }))
        }),
    );

    let status = client(common::spawn_mock_app(app).await, "key")
        .get_library_status()
        .await
        .unwrap();

    assert_eq!(status.item_count, Some(12));
    assert_eq!(status.movie_count, Some(3));
    assert_eq!(status.series_count, Some(2));
    assert!(!serde_json::to_string(&status).unwrap().contains("Raw"));
}

#[tokio::test]
async fn refresh_library_accepts_no_content_without_exposing_raw_body() {
    let app = Router::new().route(
        "/Library/Refresh",
        post(|| async { axum::http::StatusCode::NO_CONTENT }),
    );

    let result = client(common::spawn_mock_app(app).await, "key")
        .refresh_library()
        .await
        .unwrap();

    assert_eq!(result.service, "jellyfin");
    assert_eq!(result.operation, "refresh_library");
    assert_eq!(result.affected_id, None);
    assert!(!serde_json::to_string(&result).unwrap().contains("source"));
}

#[tokio::test]
async fn active_sessions_are_normalized_without_raw_source() {
    let app = Router::new().route(
        "/Sessions",
        get(|| async {
            common::json_response(json!([{
                "Id": "session-1",
                "UserName": "Alice",
                "NowPlayingItem": {"Name": "Movie A", "Secret": "raw"},
                "ExtraField": "ignored"
            }]))
        }),
    );

    let sessions = client(common::spawn_mock_app(app).await, "key")
        .get_active_sessions()
        .await
        .unwrap();

    assert_eq!(sessions[0].id, "session-1");
    assert_eq!(sessions[0].user_name.as_deref(), Some("Alice"));
    assert_eq!(sessions[0].item_name.as_deref(), Some("Movie A"));
    let json = serde_json::to_string(&sessions).unwrap();
    assert!(!json.contains("Secret"));
    assert!(!json.contains("source"));
}

#[tokio::test]
async fn authorization_body_and_token_are_redacted_from_decode_errors() {
    let app = Router::new().route(
        "/Items/Counts",
        get(|| async {
            (
                axum::http::StatusCode::OK,
                "Authorization Bearer secret_token_xyz",
            )
        }),
    );

    let error = client(common::spawn_mock_app(app).await, "secret_token_xyz")
        .get_library_status()
        .await
        .unwrap_err();
    let public = error.public_message();

    assert!(!public.contains("secret_token_xyz"));
    assert!(!public.to_ascii_lowercase().contains("authorization"));
    assert!(!error.to_string().contains("secret_token_xyz"));
}
