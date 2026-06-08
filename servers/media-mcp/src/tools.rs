use crate::config::MediaConfig;
use rmcp::{
    ServerHandler, handler::server::wrapper::Parameters, schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;

#[derive(Clone)]
pub struct MediaTools {
    #[allow(dead_code)]
    config: MediaConfig,
    #[allow(dead_code)]
    http: reqwest::Client,
}

impl MediaTools {
    pub fn new(config: MediaConfig, http: reqwest::Client) -> Self {
        Self { config, http }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HealthParams {}

#[tool_router(vis = "pub")]
impl MediaTools {
    #[tool(description = "Return media MCP health information")]
    pub async fn health(
        &self,
        Parameters(_params): Parameters<HealthParams>,
    ) -> Result<String, String> {
        Ok(serde_json::json!({
            "service": "media-mcp",
            "status": "ok"
        })
        .to_string())
    }
}

#[tool_handler(
    name = "media-mcp",
    version = "0.1.0",
    instructions = "Task-oriented media operator for Jellyseerr, SABnzbd, and Jellyfin. \
        Use high-level tools for media requests, download queue control, and Jellyfin library/session operations."
)]
impl ServerHandler for MediaTools {}
