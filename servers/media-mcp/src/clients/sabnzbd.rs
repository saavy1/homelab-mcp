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
        self.validate_action_response("pause_download", nzo_id, &body)?;
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
        self.validate_action_response("resume_download", nzo_id, &body)?;
        Ok(self.operation_result("resume_download", nzo_id, body))
    }

    pub async fn delete_download(
        &self,
        nzo_id: &str,
        delete_files: bool,
    ) -> Result<OperationResult, MediaMcpError> {
        self.require_id(nzo_id)?;
        let del_files = if delete_files { "1" } else { "0" };

        // Try queue delete first.
        let queue_body = self
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

        if self
            .validate_action_response("delete_download", nzo_id, &queue_body)
            .is_ok()
        {
            return Ok(self.operation_result("delete_download", nzo_id, queue_body));
        }

        // Only fall back to history when the queue response is a valid
        // action response shape that does not contain the requested id.
        if !self.is_valid_empty_action_response(&queue_body, nzo_id) {
            return match self.validate_action_response("delete_download", nzo_id, &queue_body) {
                Ok(()) => unreachable!(),
                Err(e) => Err(e),
            };
        }

        // Queue did not affect the requested id; try history delete.
        let history_body = self
            .api(
                "delete_download",
                &[
                    ("mode", "history"),
                    ("name", "delete"),
                    ("value", nzo_id),
                    ("del_files", del_files),
                ],
            )
            .await?;

        if history_body.get("status").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(self.operation_result("delete_download", nzo_id, history_body));
        }

        self.validate_action_response("delete_download", nzo_id, &history_body)?;
        Ok(self.operation_result("delete_download", nzo_id, history_body))
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

        // SABnzbd history retry may return a status-only success response.
        if body.get("status").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(self.operation_result("retry_failed_download", nzo_id, body));
        }

        self.validate_action_response("retry_failed_download", nzo_id, &body)?;
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

            let response = self
                .http
                .get(&url)
                .query(&query)
                .send()
                .await
                .map_err(|e| map_reqwest_error(e, operation))?;
            let status = response.status();

            if !status.is_success() {
                let retryable = status.is_server_error() || status.as_u16() == 429;
                return Err(MediaMcpError::Upstream(UpstreamError {
                    service: "sabnzbd",
                    operation,
                    status: Some(status.as_u16()),
                    retryable,
                    message: status
                        .canonical_reason()
                        .unwrap_or("upstream request failed")
                        .to_string(),
                }));
            }

            let body_text = response
                .text()
                .await
                .map_err(|e| map_reqwest_error(e, operation))?;
            let body: Value =
                serde_json::from_str(&body_text).map_err(|e| map_serde_error(e, operation))?;

            if let Some(error_message) = body
                .get("error")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string())
            {
                return Err(MediaMcpError::Upstream(UpstreamError {
                    service: "sabnzbd",
                    operation,
                    status: None,
                    retryable: false,
                    message: error_message,
                }));
            }

            Ok(body)
        }
        .instrument(span)
        .await
    }

    fn validate_action_response(
        &self,
        operation: &'static str,
        nzo_id: &str,
        body: &Value,
    ) -> Result<(), MediaMcpError> {
        if body.get("status").and_then(|v| v.as_bool()) == Some(false) {
            return Err(MediaMcpError::Upstream(UpstreamError {
                service: "sabnzbd",
                operation,
                status: None,
                retryable: false,
                message: "operation returned status false".to_string(),
            }));
        }

        let nzo_ids = body
            .get("nzo_ids")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                MediaMcpError::Upstream(UpstreamError {
                    service: "sabnzbd",
                    operation,
                    status: None,
                    retryable: false,
                    message: "nzo_ids missing or not an array in action response".to_string(),
                })
            })?;

        let found = nzo_ids
            .iter()
            .any(|v| v.as_str().map(|s| s == nzo_id).unwrap_or(false));
        if !found {
            return Err(MediaMcpError::Upstream(UpstreamError {
                service: "sabnzbd",
                operation,
                status: None,
                retryable: false,
                message: format!("nzo_id {} not found in response", nzo_id),
            }));
        }

        Ok(())
    }

    /// Returns true when the response is a valid action response shape
    /// that does not contain the requested id (status is not false,
    /// nzo_ids is present as an array, and the array does not contain nzo_id).
    fn is_valid_empty_action_response(&self, body: &Value, nzo_id: &str) -> bool {
        if body.get("status").and_then(|v| v.as_bool()) == Some(false) {
            return false;
        }

        let Some(nzo_ids) = body.get("nzo_ids").and_then(|v| v.as_array()) else {
            return false;
        };

        !nzo_ids
            .iter()
            .any(|v| v.as_str().map(|s| s == nzo_id).unwrap_or(false))
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

fn map_reqwest_error(err: reqwest::Error, operation: &'static str) -> MediaMcpError {
    let status = err.status().map(|s| s.as_u16());
    let retryable =
        err.is_timeout() || err.is_connect() || status.is_some_and(|s| s >= 500 || s == 429);
    let message = if err.is_timeout() {
        "request timed out".to_string()
    } else if err.is_connect() {
        "connection failed".to_string()
    } else if err.is_body() || err.is_decode() {
        "failed to read response body".to_string()
    } else if let Some(s) = status {
        format!("upstream returned HTTP {}", s)
    } else {
        "request failed".to_string()
    };
    MediaMcpError::Upstream(UpstreamError {
        service: "sabnzbd",
        operation,
        status,
        retryable,
        message,
    })
}

fn map_serde_error(err: serde_json::Error, operation: &'static str) -> MediaMcpError {
    MediaMcpError::Upstream(UpstreamError {
        service: "sabnzbd",
        operation,
        status: None,
        retryable: false,
        message: format!("invalid JSON response: {}", err),
    })
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
