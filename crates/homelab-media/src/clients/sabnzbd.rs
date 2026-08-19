use crate::{MediaError, config::ServiceConfig};
use homelab_api_model::{DownloadItem, MediaOperation};
use reqwest::Client;
use serde_json::Value;

pub struct SabnzbdClient {
    http: Client,
    config: ServiceConfig,
}

impl SabnzbdClient {
    pub fn new(http: Client, config: ServiceConfig) -> Self {
        Self { http, config }
    }

    pub async fn health(&self) -> Result<(), MediaError> {
        self.api("health", &[("mode", "version")], false)
            .await
            .map(|_| ())
    }

    pub async fn list_downloads(
        &self,
        state: Option<&str>,
    ) -> Result<Vec<DownloadItem>, MediaError> {
        let mut downloads = Vec::new();
        if state != Some("history") {
            let queue = self
                .api(
                    "list_queue",
                    &[("mode", "queue"), ("limit", "1000")],
                    false,
                )
                .await?;
            downloads.extend(
                queue
                    .pointer("/queue/slots")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(normalize_queue_item),
            );
        }
        if state != Some("queue") {
            let history = self
                .api(
                    "list_history",
                    &[("mode", "history"), ("failed_only", "0"), ("limit", "1000")],
                    false,
                )
                .await?;
            downloads.extend(
                history
                    .pointer("/history/slots")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(normalize_history_item),
            );
        }
        Ok(downloads)
    }

    pub async fn pause_download(&self, nzo_id: &str) -> Result<MediaOperation, MediaError> {
        self.queue_action("pause_download", "pause", nzo_id).await
    }

    pub async fn resume_download(&self, nzo_id: &str) -> Result<MediaOperation, MediaError> {
        self.queue_action("resume_download", "resume", nzo_id).await
    }

    pub async fn delete_download(
        &self,
        nzo_id: &str,
        delete_files: bool,
    ) -> Result<MediaOperation, MediaError> {
        require_id(nzo_id)?;
        let del_files = if delete_files { "1" } else { "0" };
        let queue = self
            .api(
                "delete_download",
                &[
                    ("mode", "queue"),
                    ("name", "delete"),
                    ("value", nzo_id),
                    ("del_files", del_files),
                ],
                true,
            )
            .await?;
        if action_contains_id(&queue, nzo_id) {
            return Ok(operation("delete_download", nzo_id));
        }
        if !valid_empty_action(&queue, nzo_id) {
            return Err(MediaError::upstream(
                "sabnzbd",
                "delete_download",
                None,
                true,
            ));
        }
        let history = self
            .api(
                "delete_download",
                &[
                    ("mode", "history"),
                    ("name", "delete"),
                    ("value", nzo_id),
                    ("del_files", del_files),
                ],
                true,
            )
            .await?;
        if history.get("status").and_then(Value::as_bool) == Some(true)
            || action_contains_id(&history, nzo_id)
        {
            return Ok(operation("delete_download", nzo_id));
        }
        Err(MediaError::upstream(
            "sabnzbd",
            "delete_download",
            None,
            true,
        ))
    }

    pub async fn retry_failed_download(
        &self,
        nzo_id: &str,
    ) -> Result<MediaOperation, MediaError> {
        require_id(nzo_id)?;
        let value = self
            .api(
                "retry_failed_download",
                &[("mode", "retry"), ("value", nzo_id)],
                true,
            )
            .await?;
        if value.get("status").and_then(Value::as_bool) == Some(true)
            || action_contains_id(&value, nzo_id)
        {
            Ok(operation("retry_failed_download", nzo_id))
        } else {
            Err(MediaError::upstream(
                "sabnzbd",
                "retry_failed_download",
                None,
                true,
            ))
        }
    }

    async fn queue_action(
        &self,
        operation_name: &'static str,
        action: &str,
        nzo_id: &str,
    ) -> Result<MediaOperation, MediaError> {
        require_id(nzo_id)?;
        let value = self
            .api(
                operation_name,
                &[("mode", "queue"), ("name", action), ("value", nzo_id)],
                true,
            )
            .await?;
        if action_contains_id(&value, nzo_id) {
            Ok(operation(operation_name, nzo_id))
        } else {
            Err(MediaError::upstream(
                "sabnzbd",
                operation_name,
                None,
                true,
            ))
        }
    }

    async fn api(
        &self,
        operation: &'static str,
        params: &[(&str, &str)],
        mutating: bool,
    ) -> Result<Value, MediaError> {
        let mut query = Vec::with_capacity(params.len() + 2);
        query.push(("output", "json"));
        query.push(("apikey", self.config.api_key.as_str()));
        query.extend_from_slice(params);
        let response = self
            .http
            .get(format!("{}/api", self.config.base_url))
            .query(&query)
            .send()
            .await
            .map_err(|error| MediaError::transport("sabnzbd", operation, &error, mutating))?;
        let status = response.status();
        if !status.is_success() {
            return Err(MediaError::upstream(
                "sabnzbd",
                operation,
                Some(status.as_u16()),
                mutating,
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| MediaError::transport("sabnzbd", operation, &error, mutating))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| MediaError::serialization("sabnzbd", operation))?;
        if value.get("error").is_some_and(|error| !error.is_null()) {
            return Err(MediaError::upstream("sabnzbd", operation, None, mutating));
        }
        Ok(value)
    }
}

fn require_id(nzo_id: &str) -> Result<(), MediaError> {
    if nzo_id.trim().is_empty() || nzo_id.trim().eq_ignore_ascii_case("all") {
        Err(MediaError::Validation(
            "a specific nzo_id is required".into(),
        ))
    } else {
        Ok(())
    }
}

fn action_contains_id(value: &Value, nzo_id: &str) -> bool {
    value.get("status").and_then(Value::as_bool) == Some(true)
        && value
            .get("nzo_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(nzo_id)))
}

fn valid_empty_action(value: &Value, nzo_id: &str) -> bool {
    value.get("status").and_then(Value::as_bool) != Some(false)
        && value
            .get("nzo_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().all(|id| id.as_str() != Some(nzo_id)))
}

fn operation(operation_name: &str, nzo_id: &str) -> MediaOperation {
    MediaOperation {
        service: "sabnzbd".into(),
        operation: operation_name.into(),
        affected_id: Some(nzo_id.into()),
    }
}

fn scalar_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
        .or_else(|| value.as_f64().map(|number| number.to_string()))
}

fn normalize_queue_item(value: &Value) -> Option<DownloadItem> {
    Some(DownloadItem {
        id: value.get("nzo_id").and_then(scalar_string)?,
        name: value
            .get("filename")
            .or_else(|| value.get("name"))
            .and_then(scalar_string)
            .unwrap_or_default(),
        status: value
            .get("status")
            .and_then(scalar_string)
            .unwrap_or_default(),
        percentage: value.get("percentage").and_then(scalar_string),
        size: value
            .get("size")
            .or_else(|| value.get("mb"))
            .and_then(scalar_string),
    })
}

fn normalize_history_item(value: &Value) -> Option<DownloadItem> {
    Some(DownloadItem {
        id: value.get("nzo_id").and_then(scalar_string)?,
        name: value
            .get("name")
            .or_else(|| value.get("filename"))
            .and_then(scalar_string)
            .unwrap_or_default(),
        status: value
            .get("status")
            .and_then(scalar_string)
            .unwrap_or_default(),
        percentage: value.get("percentage").and_then(scalar_string),
        size: value
            .get("size")
            .or_else(|| value.get("bytes"))
            .and_then(scalar_string),
    })
}
