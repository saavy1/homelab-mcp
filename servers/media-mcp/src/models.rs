use homelab_mcp_core::ToolResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct MediaSearchResult {
    pub id: String,
    pub media_type: String,
    pub title: String,
    pub year: Option<i32>,
    pub status: Option<String>,
    pub source: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct MediaRequest {
    pub id: String,
    pub media_id: String,
    pub media_type: String,
    pub status: String,
    pub title: Option<String>,
    pub source: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct DownloadItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub percentage: Option<String>,
    pub size: Option<String>,
    pub source: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct LibraryStatus {
    pub item_count: Option<u64>,
    pub movie_count: Option<u64>,
    pub series_count: Option<u64>,
    pub source: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ActiveSession {
    pub id: String,
    pub user_name: Option<String>,
    pub item_name: Option<String>,
    pub source: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct OperationResult {
    pub service: String,
    pub operation: String,
    pub affected_id: Option<String>,
    pub source: Value,
}

pub type ReadResult<T> = ToolResult<T>;
pub type WriteResult<T> = ToolResult<T>;
