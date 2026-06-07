use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;

#[derive(Clone)]
pub struct MediaTools {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HealthParams {}

#[tool_router(vis = "pub")]
impl MediaTools {
    #[tool(description = "Return media MCP health information")]
    pub async fn health(&self, Parameters(_params): Parameters<HealthParams>) -> Result<String, String> {
        Ok(serde_json::json!({
            "service": "media-mcp",
            "status": "ok"
        })
        .to_string())
    }
}
