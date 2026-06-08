use crate::{
    config::ServiceConfig,
    error::{MediaMcpError, UpstreamError},
    models::{DownloadItem, OperationResult},
};
use reqwest::Client;
use serde_json::Value;
use tracing::{Instrument, info_span};

pub struct SabnzbdClient {
    http: Client,
    config: ServiceConfig,
}

impl SabnzbdClient {
    pub fn new(http: Client, config: ServiceConfig) -> Self {
        Self { http, config }
    }

    pub async fn list_downloads(
        &self,
        state: Option<&str>,
    ) -> Result<Vec<DownloadItem>, MediaMcpError> {
        let mut downloads = Vec::new();

        if state != Some("history") {
            let queue = self
                .api("list_queue", &[("mode", "queue"), ("limit", "1000")])
                .await?;
            if let Some(slots) = queue
                .get("queue")
                .and_then(|q| q.get("slots"))
                .and_then(|s| s.as_array())
            {
                downloads.extend(slots.iter().map(normalize_queue_item));
            }
        }

        if state != Some("queue") {
            let history = self
                .api(
                    "list_history",
                    &[("mode", "history"), ("failed_only", "0"), ("limit", "1000")],
                )
                .await?;
            if let Some(slots) = history
                .get("history")
                .and_then(|h| h.get("slots"))
                .and_then(|s| s.as_array())
            {
                downloads.extend(slots.iter().map(normalize_history_item));
            }
        }

        Ok(downloads)
    }

    pub async fn pause_download(&self, nzo_id: &str) -> Result<OperationResult, MediaMcpError> {
        self.require_id(nzo_id)?;
        let body = self
            .api(
                "pause_download",
                &[("mode", "queue"), ("name", "pause"), ("value", nzo_id)],
            )
            .await?;
        Ok(self.operation_result("pause_download", nzo_id, body))
    }

    pub async fn resume_download(&self, nzo_id: &str) -> Result<OperationResult, MediaMcpError> {
        self.require_id(nzo_id)?;
        let body = self
            .api(
                "resume_download",
                &[("mode", "queue"), ("name", "resume"), ("value", nzo_id)],
            )
            .await?;
        Ok(self.operation_result("resume_download", nzo_id, body))
    }

    pub async fn delete_download(
        &self,
        nzo_id: &str,
        delete_files: bool,
    ) -> Result<OperationResult, MediaMcpError> {
        self.require_id(nzo_id)?;
        let del_files = if delete_files { "1" } else { "0" };
        let body = self
            .api(
                "delete_download",
                &[
                    ("mode", "queue"),
                    ("name", "delete"),
                    ("value", nzo_id),
                    ("del_files", del_files),
                ],
            )
            .await?;
        Ok(self.operation_result("delete_download", nzo_id, body))
    }

    pub async fn retry_failed_download(
        &self,
        nzo_id: &str,
    ) -> Result<OperationResult, MediaMcpError> {
        self.require_id(nzo_id)?;
        let body = self
            .api(
                "retry_failed_download",
                &[("mode", "retry"), ("value", nzo_id)],
            )
            .await?;
        Ok(self.operation_result("retry_failed_download", nzo_id, body))
    }

    fn require_id(&self, nzo_id: &str) -> Result<(), MediaMcpError> {
        if nzo_id.trim().is_empty() || nzo_id.trim().eq_ignore_ascii_case("all") {
            return Err(MediaMcpError::Validation(
                "a specific nzo_id is required".to_string(),
            ));
        }
        Ok(())
    }

    async fn api(
        &self,
        operation: &'static str,
        params: &[(&str, &str)],
    ) -> Result<Value, MediaMcpError> {
        let span = info_span!("upstream_http", service = "sabnzbd", operation);
        async {
            let url = format!("{}/api", self.config.base_url);
            let mut query: Vec<(&str, &str)> =
                vec![("output", "json"), ("apikey", &self.config.api_key)];
            query.extend_from_slice(params);

            let response = self.http.get(&url).query(&query).send().await?;
            let status = response.status();
            let body: Value = response.json().await?;

            let error_message = body
                .get("error")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string());

            if !status.is_success() || error_message.is_some() {
                let retryable = status.is_server_error() || status.as_u16() == 429;
                let message = error_message.unwrap_or_else(|| {
                    status
                        .canonical_reason()
                        .unwrap_or("upstream request failed")
                        .to_string()
                });
                return Err(MediaMcpError::Upstream(UpstreamError {
                    service: "sabnzbd",
                    operation,
                    status: if status.is_success() {
                        None
                    } else {
                        Some(status.as_u16())
                    },
                    retryable,
                    message,
                }));
            }

            Ok(body)
        }
        .instrument(span)
        .await
    }

    fn operation_result(&self, operation: &str, nzo_id: &str, source: Value) -> OperationResult {
        OperationResult {
            service: "sabnzbd".to_string(),
            operation: operation.to_string(),
            affected_id: Some(nzo_id.to_string()),
            source,
        }
    }
}

fn normalize_queue_item(value: &Value) -> DownloadItem {
    let id = value
        .get("nzo_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let name = value
        .get("filename")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("name").and_then(|v| v.as_str()))
        .unwrap_or("Unknown")
        .to_string();

    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let percentage = value
        .get("percentage")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let size = value
        .get("size")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    DownloadItem {
        id,
        name,
        status,
        percentage,
        size,
        source: value.clone(),
    }
}

fn normalize_history_item(value: &Value) -> DownloadItem {
    let id = value
        .get("nzo_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("filename").and_then(|v| v.as_str()))
        .unwrap_or("Unknown")
        .to_string();

    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let size = value
        .get("size")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    DownloadItem {
        id,
        name,
        status,
        percentage: None,
        size,
        source: value.clone(),
    }
}
