use crate::{
    config::ServiceConfig,
    error::{MediaMcpError, UpstreamError},
    models::{ActiveSession, LibraryStatus, OperationResult},
};
use reqwest::Client;
use serde_json::Value;
use tracing::{Instrument, info_span};

pub struct JellyfinClient {
    http: Client,
    config: ServiceConfig,
}

impl JellyfinClient {
    pub fn new(http: Client, config: ServiceConfig) -> Self {
        Self { http, config }
    }

    pub async fn get_library_status(&self) -> Result<LibraryStatus, MediaMcpError> {
        let operation = "get_library_status";
        let span = info_span!("upstream_http", service = "jellyfin", operation);
        async {
            let body = self.get(operation, "/Items/Counts").await?;
            Ok(LibraryStatus {
                item_count: body.get("ItemCount").and_then(|v| v.as_u64()),
                movie_count: body.get("MovieCount").and_then(|v| v.as_u64()),
                series_count: body.get("SeriesCount").and_then(|v| v.as_u64()),
                source: body,
            })
        }
        .instrument(span)
        .await
    }

    pub async fn refresh_library(&self) -> Result<OperationResult, MediaMcpError> {
        let operation = "refresh_library";
        let span = info_span!("upstream_http", service = "jellyfin", operation);
        async {
            let body = self.post(operation, "/Library/Refresh").await?;
            Ok(OperationResult {
                service: "jellyfin".to_string(),
                operation: operation.to_string(),
                affected_id: None,
                source: body,
            })
        }
        .instrument(span)
        .await
    }

    pub async fn get_active_sessions(&self) -> Result<Vec<ActiveSession>, MediaMcpError> {
        let operation = "get_active_sessions";
        let span = info_span!("upstream_http", service = "jellyfin", operation);
        async {
            let body = self.get(operation, "/Sessions").await?;
            let sessions = body
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|value| ActiveSession {
                            id: value
                                .get("Id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            user_name: value
                                .get("UserName")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            item_name: value
                                .get("NowPlayingItem")
                                .and_then(|v| v.get("Name"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            source: value.clone(),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(sessions)
        }
        .instrument(span)
        .await
    }

    pub async fn get_item_details(&self, item_id: &str) -> Result<Value, MediaMcpError> {
        if item_id.trim().is_empty() {
            return Err(MediaMcpError::Validation("item_id is required".to_string()));
        }
        let operation = "get_item_details";
        let span = info_span!("upstream_http", service = "jellyfin", operation);
        async {
            let body = self.get(operation, &format!("/Items/{}", item_id)).await?;
            Ok(body)
        }
        .instrument(span)
        .await
    }

    async fn get(&self, operation: &'static str, path: &str) -> Result<Value, MediaMcpError> {
        let url = format!("{}{}", self.config.base_url, path);
        let response = self
            .http
            .get(&url)
            .header("X-Emby-Token", &self.config.api_key)
            .send()
            .await
            .map_err(|e| self.map_transport_error(operation, e))?;
        self.handle_response(operation, response).await
    }

    async fn post(&self, operation: &'static str, path: &str) -> Result<Value, MediaMcpError> {
        let url = format!("{}{}", self.config.base_url, path);
        let response = self
            .http
            .post(&url)
            .header("X-Emby-Token", &self.config.api_key)
            .send()
            .await
            .map_err(|e| self.map_transport_error(operation, e))?;
        self.handle_response(operation, response).await
    }

    async fn handle_response(
        &self,
        operation: &'static str,
        response: reqwest::Response,
    ) -> Result<Value, MediaMcpError> {
        let status = response.status();
        if !status.is_success() {
            let retryable = status.is_server_error() || status.as_u16() == 429;
            let message = response.text().await.unwrap_or_default();
            return Err(MediaMcpError::Upstream(UpstreamError {
                service: "jellyfin",
                operation,
                status: Some(status.as_u16()),
                retryable,
                message,
            }));
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Object(Default::default()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| self.map_transport_error(operation, e))?;
        if bytes.is_empty() {
            return Ok(Value::Object(Default::default()));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            MediaMcpError::Upstream(UpstreamError {
                service: "jellyfin",
                operation,
                status: Some(status.as_u16()),
                retryable: false,
                message: format!("decode failed: {}", e),
            })
        })
    }

    fn map_transport_error(&self, operation: &'static str, error: reqwest::Error) -> MediaMcpError {
        MediaMcpError::Upstream(UpstreamError {
            service: "jellyfin",
            operation,
            status: error.status().map(|s| s.as_u16()),
            retryable: error.is_timeout() || error.is_connect(),
            message: format!("request failed: {}", error.without_url()),
        })
    }
}
