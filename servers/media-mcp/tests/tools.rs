mod common;

use media_mcp::{
    config::{MediaConfig, ServiceConfig},
    tools::MediaTools,
};
use rmcp::handler::server::wrapper::Parameters;

#[tokio::test]
async fn delete_download_rejects_blank_id_before_upstream_call() {
    let tools = MediaTools::new(
        MediaConfig {
            jellyseerr: ServiceConfig::new("jellyseerr", "http://127.0.0.1:9", "key").unwrap(),
            sabnzbd: ServiceConfig::new("sabnzbd", "http://127.0.0.1:9", "key").unwrap(),
            jellyfin: ServiceConfig::new("jellyfin", "http://127.0.0.1:9", "key").unwrap(),
        },
        reqwest::Client::new(),
    );

    let params = media_mcp::tools::DeleteDownloadParams {
        nzo_id: "".to_string(),
        delete_files: Some(false),
    };
    let result = tools.delete_download(Parameters(params)).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("a specific nzo_id is required") || err.contains("validation"),
        "expected validation error for blank nzo_id, got: {}",
        err
    );
}

#[tokio::test]
async fn health_tool_serializes_tool_result_with_read_risk() {
    let tools = MediaTools::new(
        MediaConfig {
            jellyseerr: ServiceConfig::new("jellyseerr", "http://127.0.0.1:9", "key").unwrap(),
            sabnzbd: ServiceConfig::new("sabnzbd", "http://127.0.0.1:9", "key").unwrap(),
            jellyfin: ServiceConfig::new("jellyfin", "http://127.0.0.1:9", "key").unwrap(),
        },
        reqwest::Client::new(),
    );

    let result = tools
        .health(Parameters(media_mcp::tools::HealthParams {}))
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json.get("risk").and_then(|v| v.as_str()), Some("read"));
    let summary = json
        .get("summary")
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str());
    assert!(summary.is_some());
    assert!(summary.unwrap().contains("health"));
}
