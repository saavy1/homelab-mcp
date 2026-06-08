use crate::{
    clients::{jellyfin::JellyfinClient, jellyseerr::JellyseerrClient, sabnzbd::SabnzbdClient},
    config::MediaConfig,
    error::MediaMcpError,
    observability::ToolCall,
};
use homelab_mcp_core::ToolResult;
use rmcp::{
    ServerHandler, handler::server::wrapper::Parameters, schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;

#[derive(Clone)]
pub struct MediaTools {
    config: MediaConfig,
    http: reqwest::Client,
}

impl MediaTools {
    pub fn new(config: MediaConfig, http: reqwest::Client) -> Self {
        Self { config, http }
    }

    fn jellyseerr_client(&self) -> JellyseerrClient {
        JellyseerrClient::new(self.http.clone(), self.config.jellyseerr.clone())
    }

    fn sabnzbd_client(&self) -> SabnzbdClient {
        SabnzbdClient::new(self.http.clone(), self.config.sabnzbd.clone())
    }

    fn jellyfin_client(&self) -> JellyfinClient {
        JellyfinClient::new(self.http.clone(), self.config.jellyfin.clone())
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchMediaParams {
    pub query: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RequestMediaParams {
    pub media_type: String,
    pub media_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListRequestsParams {
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RequestActionParams {
    pub request_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListDownloadsParams {
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DownloadActionParams {
    pub nzo_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteDownloadParams {
    pub nzo_id: String,
    pub delete_files: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EmptyParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ItemDetailsParams {
    pub item_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HealthParams {}

fn request_id() -> String {
    chrono::Utc::now().timestamp_millis().to_string()
}

fn error_status_retryable(err: &MediaMcpError) -> (Option<u16>, bool) {
    match err {
        MediaMcpError::Upstream(e) => (e.status, e.retryable),
        MediaMcpError::Http(e) => (
            e.status().map(|s| s.as_u16()),
            e.is_timeout() || e.is_connect(),
        ),
        _ => (None, false),
    }
}

#[tool_router(vis = "pub")]
impl MediaTools {
    #[tool(description = "Return media MCP health information")]
    pub async fn health(
        &self,
        Parameters(_params): Parameters<HealthParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("health", request_id());
        let result = ToolResult::read(
            "health check ok",
            serde_json::json!({
                "service": "media-mcp",
                "status": "ok"
            }),
        );
        call.complete("media-mcp", "health", None);
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }

    #[tool(description = "Search for media in Jellyseerr")]
    pub async fn search_media(
        &self,
        Parameters(params): Parameters<SearchMediaParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("search_media", request_id());
        let client = self.jellyseerr_client();
        match client.search(&params.query).await {
            Ok(data) => {
                let result = ToolResult::read("searched media", data);
                call.complete("jellyseerr", "search", None);
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyseerr", "search", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Request media in Jellyseerr")]
    pub async fn request_media(
        &self,
        Parameters(params): Parameters<RequestMediaParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("request_media", request_id());
        let client = self.jellyseerr_client();
        match client
            .request_media(&params.media_type, params.media_id)
            .await
        {
            Ok(data) => {
                let affected_id = data.id.clone();
                let result = ToolResult::cluster_write("requested media", data);
                call.complete("jellyseerr", "request_media", Some(&affected_id));
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyseerr", "request_media", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "List media requests in Jellyseerr")]
    pub async fn list_requests(
        &self,
        Parameters(params): Parameters<ListRequestsParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("list_requests", request_id());
        let client = self.jellyseerr_client();
        match client.list_requests(params.status.as_deref()).await {
            Ok(data) => {
                let result = ToolResult::read("listed media requests", data);
                call.complete("jellyseerr", "list_requests", None);
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyseerr", "list_requests", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Approve a media request in Jellyseerr")]
    pub async fn approve_request(
        &self,
        Parameters(params): Parameters<RequestActionParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("approve_request", request_id());
        let client = self.jellyseerr_client();
        match client.approve_request(&params.request_id).await {
            Ok(data) => {
                let affected_id = data.affected_id.clone();
                let result = ToolResult::cluster_write("approved request", data);
                call.complete("jellyseerr", "approve_request", affected_id.as_deref());
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyseerr", "approve_request", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Decline a media request in Jellyseerr")]
    pub async fn decline_request(
        &self,
        Parameters(params): Parameters<RequestActionParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("decline_request", request_id());
        let client = self.jellyseerr_client();
        match client.decline_request(&params.request_id).await {
            Ok(data) => {
                let affected_id = data.affected_id.clone();
                let result = ToolResult::cluster_write("declined request", data);
                call.complete("jellyseerr", "decline_request", affected_id.as_deref());
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyseerr", "decline_request", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "List downloads in SABnzbd")]
    pub async fn list_downloads(
        &self,
        Parameters(params): Parameters<ListDownloadsParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("list_downloads", request_id());
        let client = self.sabnzbd_client();
        match client.list_downloads(params.state.as_deref()).await {
            Ok(data) => {
                let result = ToolResult::read("listed downloads", data);
                call.complete("sabnzbd", "list_downloads", None);
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("sabnzbd", "list_downloads", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Pause a download in SABnzbd")]
    pub async fn pause_download(
        &self,
        Parameters(params): Parameters<DownloadActionParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("pause_download", request_id());
        let client = self.sabnzbd_client();
        match client.pause_download(&params.nzo_id).await {
            Ok(data) => {
                let affected_id = data.affected_id.clone();
                let result = ToolResult::cluster_write("paused download", data);
                call.complete("sabnzbd", "pause_download", affected_id.as_deref());
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("sabnzbd", "pause_download", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Resume a download in SABnzbd")]
    pub async fn resume_download(
        &self,
        Parameters(params): Parameters<DownloadActionParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("resume_download", request_id());
        let client = self.sabnzbd_client();
        match client.resume_download(&params.nzo_id).await {
            Ok(data) => {
                let affected_id = data.affected_id.clone();
                let result = ToolResult::cluster_write("resumed download", data);
                call.complete("sabnzbd", "resume_download", affected_id.as_deref());
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("sabnzbd", "resume_download", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Delete a download in SABnzbd")]
    pub async fn delete_download(
        &self,
        Parameters(params): Parameters<DeleteDownloadParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("delete_download", request_id());
        let client = self.sabnzbd_client();
        let delete_files = params.delete_files.unwrap_or(false);
        match client.delete_download(&params.nzo_id, delete_files).await {
            Ok(data) => {
                let affected_id = data.affected_id.clone();
                let result = ToolResult::cluster_write("deleted download", data);
                call.complete("sabnzbd", "delete_download", affected_id.as_deref());
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("sabnzbd", "delete_download", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Retry a failed download in SABnzbd")]
    pub async fn retry_failed_download(
        &self,
        Parameters(params): Parameters<DownloadActionParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("retry_failed_download", request_id());
        let client = self.sabnzbd_client();
        match client.retry_failed_download(&params.nzo_id).await {
            Ok(data) => {
                let affected_id = data.affected_id.clone();
                let result = ToolResult::cluster_write("retried failed download", data);
                call.complete("sabnzbd", "retry_failed_download", affected_id.as_deref());
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("sabnzbd", "retry_failed_download", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Get Jellyfin library status")]
    pub async fn get_library_status(
        &self,
        Parameters(_params): Parameters<EmptyParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("get_library_status", request_id());
        let client = self.jellyfin_client();
        match client.get_library_status().await {
            Ok(data) => {
                let result = ToolResult::read("read library status", data);
                call.complete("jellyfin", "get_library_status", None);
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyfin", "get_library_status", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Refresh Jellyfin library")]
    pub async fn refresh_library(
        &self,
        Parameters(_params): Parameters<EmptyParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("refresh_library", request_id());
        let client = self.jellyfin_client();
        match client.refresh_library().await {
            Ok(data) => {
                let result = ToolResult::cluster_write("refreshed library", data);
                call.complete("jellyfin", "refresh_library", None);
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyfin", "refresh_library", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Get active sessions in Jellyfin")]
    pub async fn get_active_sessions(
        &self,
        Parameters(_params): Parameters<EmptyParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("get_active_sessions", request_id());
        let client = self.jellyfin_client();
        match client.get_active_sessions().await {
            Ok(data) => {
                let result = ToolResult::read("listed active sessions", data);
                call.complete("jellyfin", "get_active_sessions", None);
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyfin", "get_active_sessions", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }

    #[tool(description = "Get item details from Jellyfin")]
    pub async fn get_item_details(
        &self,
        Parameters(params): Parameters<ItemDetailsParams>,
    ) -> Result<String, String> {
        let call = ToolCall::start("get_item_details", request_id());
        let client = self.jellyfin_client();
        match client.get_item_details(&params.item_id).await {
            Ok(data) => {
                let result = ToolResult::read("read item details", data);
                call.complete("jellyfin", "get_item_details", Some(&params.item_id));
                serde_json::to_string(&result).map_err(|e| e.to_string())
            }
            Err(e) => {
                let (status, retryable) = error_status_retryable(&e);
                call.fail("jellyfin", "get_item_details", status, retryable);
                Err(e.to_tool_error())
            }
        }
    }
}

#[tool_handler(
    name = "media-mcp",
    version = "0.1.0",
    instructions = "Task-oriented media operator for Jellyseerr, SABnzbd, and Jellyfin. \
        Use high-level tools for media requests, download queue control, and Jellyfin library/session operations."
)]
impl ServerHandler for MediaTools {}
