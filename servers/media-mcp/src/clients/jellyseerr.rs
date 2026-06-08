use crate::{
    config::ServiceConfig,
    error::{MediaMcpError, UpstreamError},
    models::{MediaRequest, MediaSearchResult, OperationResult},
};
use reqwest::Client;
use serde_json::Value;
use tracing::{Instrument, info_span};

pub struct JellyseerrClient {
    http: Client,
    config: ServiceConfig,
}

impl JellyseerrClient {
    pub fn new(http: Client, config: ServiceConfig) -> Self {
        Self { http, config }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<MediaSearchResult>, MediaMcpError> {
        let operation = "search";
        let span = info_span!("upstream_http", service = "jellyseerr", operation);
        async {
            let url = format!(
                "{}/api/v1/search?query={}",
                self.config.base_url,
                percent_encode_query(query)
            );
            let response = self
                .http
                .get(&url)
                .header("X-Api-Key", &self.config.api_key)
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                let retryable = status.is_server_error() || status.as_u16() == 429;
                let message = response.text().await.unwrap_or_default();
                return Err(MediaMcpError::Upstream(UpstreamError {
                    service: "jellyseerr",
                    operation,
                    status: Some(status.as_u16()),
                    retryable,
                    message,
                }));
            }

            let body: Value = response.json().await?;
            let results = body
                .get("results")
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().map(normalize_search_result).collect::<Vec<_>>())
                .unwrap_or_default();

            Ok(results)
        }
        .instrument(span)
        .await
    }

    pub async fn request_media(
        &self,
        media_type: &str,
        media_id: i64,
    ) -> Result<MediaRequest, MediaMcpError> {
        let operation = "request_media";
        let span = info_span!("upstream_http", service = "jellyseerr", operation);
        async {
            let url = format!("{}/api/v1/request", self.config.base_url);
            let mut payload = serde_json::json!({
                "mediaType": media_type,
                "mediaId": media_id,
            });
            if media_type.eq_ignore_ascii_case("tv") {
                payload["seasons"] = serde_json::json!(self.tv_seasons(media_id).await?);
            }

            let response = self
                .http
                .post(&url)
                .header("X-Api-Key", &self.config.api_key)
                .json(&payload)
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                let retryable = status.is_server_error() || status.as_u16() == 429;
                let message = response.text().await.unwrap_or_default();
                return Err(MediaMcpError::Upstream(UpstreamError {
                    service: "jellyseerr",
                    operation,
                    status: Some(status.as_u16()),
                    retryable,
                    message,
                }));
            }

            let body: Value = response.json().await?;
            Ok(normalize_request(&body))
        }
        .instrument(span)
        .await
    }

    async fn tv_seasons(&self, media_id: i64) -> Result<Vec<i64>, MediaMcpError> {
        let operation = "request_media_tv_details";
        let url = format!("{}/api/v1/tv/{}", self.config.base_url, media_id);
        let response = self
            .http
            .get(&url)
            .header("X-Api-Key", &self.config.api_key)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let retryable = status.is_server_error() || status.as_u16() == 429;
            let message = response.text().await.unwrap_or_default();
            return Err(MediaMcpError::Upstream(UpstreamError {
                service: "jellyseerr",
                operation,
                status: Some(status.as_u16()),
                retryable,
                message,
            }));
        }

        let body: Value = response.json().await?;
        let seasons = body
            .get("seasons")
            .and_then(|value| value.as_array())
            .map(|seasons| {
                seasons
                    .iter()
                    .filter_map(|season| {
                        season.get("seasonNumber").and_then(|value| value.as_i64())
                    })
                    .filter(|season_number| *season_number > 0)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if seasons.is_empty() {
            return Err(MediaMcpError::Validation(format!(
                "no requestable seasons found for tv media id {media_id}"
            )));
        }

        Ok(seasons)
    }

    pub async fn list_requests(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<MediaRequest>, MediaMcpError> {
        let operation = "list_requests";
        let span = info_span!("upstream_http", service = "jellyseerr", operation);
        async {
            let url = format!("{}/api/v1/request", self.config.base_url);
            let mut request = self
                .http
                .get(&url)
                .header("X-Api-Key", &self.config.api_key);

            if let Some(filter) = status {
                request = request.query(&[("filter", filter)]);
            }

            let response = request.send().await?;

            let response_status = response.status();
            if !response_status.is_success() {
                let retryable =
                    response_status.is_server_error() || response_status.as_u16() == 429;
                let message = response.text().await.unwrap_or_default();
                return Err(MediaMcpError::Upstream(UpstreamError {
                    service: "jellyseerr",
                    operation,
                    status: Some(response_status.as_u16()),
                    retryable,
                    message,
                }));
            }

            let body: Value = response.json().await?;
            let results = body
                .get("results")
                .and_then(|r| r.as_array())
                .or_else(|| body.as_array())
                .map(|arr| arr.iter().map(normalize_request).collect::<Vec<_>>())
                .unwrap_or_default();

            Ok(results)
        }
        .instrument(span)
        .await
    }

    pub async fn approve_request(
        &self,
        request_id: &str,
    ) -> Result<OperationResult, MediaMcpError> {
        let operation = "approve_request";
        let span = info_span!("upstream_http", service = "jellyseerr", operation);
        async {
            if request_id.trim().is_empty() {
                return Err(MediaMcpError::Validation(
                    "request_id is required".to_string(),
                ));
            }

            let url = format!(
                "{}/api/v1/request/{}/approve",
                self.config.base_url, request_id
            );
            let response = self
                .http
                .post(&url)
                .header("X-Api-Key", &self.config.api_key)
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                let retryable = status.is_server_error() || status.as_u16() == 429;
                let message = response.text().await.unwrap_or_default();
                return Err(MediaMcpError::Upstream(UpstreamError {
                    service: "jellyseerr",
                    operation,
                    status: Some(status.as_u16()),
                    retryable,
                    message,
                }));
            }

            let body: Value = response.json().await?;
            Ok(OperationResult {
                service: "jellyseerr".to_string(),
                operation: operation.to_string(),
                affected_id: Some(request_id.to_string()),
                source: body,
            })
        }
        .instrument(span)
        .await
    }

    pub async fn decline_request(
        &self,
        request_id: &str,
    ) -> Result<OperationResult, MediaMcpError> {
        let operation = "decline_request";
        let span = info_span!("upstream_http", service = "jellyseerr", operation);
        async {
            if request_id.trim().is_empty() {
                return Err(MediaMcpError::Validation(
                    "request_id is required".to_string(),
                ));
            }

            let url = format!(
                "{}/api/v1/request/{}/decline",
                self.config.base_url, request_id
            );
            let response = self
                .http
                .post(&url)
                .header("X-Api-Key", &self.config.api_key)
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                let retryable = status.is_server_error() || status.as_u16() == 429;
                let message = response.text().await.unwrap_or_default();
                return Err(MediaMcpError::Upstream(UpstreamError {
                    service: "jellyseerr",
                    operation,
                    status: Some(status.as_u16()),
                    retryable,
                    message,
                }));
            }

            let body: Value = response.json().await?;
            Ok(OperationResult {
                service: "jellyseerr".to_string(),
                operation: operation.to_string(),
                affected_id: Some(request_id.to_string()),
                source: body,
            })
        }
        .instrument(span)
        .await
    }
}

fn percent_encode_query(query: &str) -> String {
    query
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn normalize_search_result(value: &Value) -> MediaSearchResult {
    let id = value
        .get("id")
        .and_then(|v| v.as_i64())
        .map(|i| i.to_string())
        .or_else(|| {
            value
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let media_type = value
        .get("mediaType")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("name").and_then(|v| v.as_str()))
        .unwrap_or("Unknown")
        .to_string();

    let year = value
        .get("releaseDate")
        .and_then(|v| v.as_str())
        .and_then(|s| s.get(..4).and_then(|yr| yr.parse::<i32>().ok()))
        .or_else(|| {
            value
                .get("firstAirDate")
                .and_then(|v| v.as_str())
                .and_then(|s| s.get(..4).and_then(|yr| yr.parse::<i32>().ok()))
        });

    let status = value
        .get("mediaInfo")
        .and_then(|m| m.get("status"))
        .and_then(|v| v.as_i64())
        .map(|i| i.to_string())
        .or_else(|| {
            value
                .get("mediaInfo")
                .and_then(|m| m.get("status"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    MediaSearchResult {
        id,
        media_type,
        title,
        year,
        status,
        source: value.clone(),
    }
}

fn normalize_request(value: &Value) -> MediaRequest {
    let id = value
        .get("id")
        .and_then(|v| v.as_i64())
        .map(|i| i.to_string())
        .or_else(|| {
            value
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let media_id = value
        .get("mediaId")
        .and_then(|v| v.as_i64())
        .map(|i| i.to_string())
        .or_else(|| {
            value
                .get("media")
                .and_then(|m| m.get("tmdbId"))
                .and_then(|v| v.as_i64())
                .map(|i| i.to_string())
        })
        .or_else(|| {
            value
                .get("mediaId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let media_type = value
        .get("mediaType")
        .and_then(|v| v.as_str())
        .or_else(|| {
            value
                .get("media")
                .and_then(|m| m.get("mediaType"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| value.get("type").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string();

    let status = value
        .get("status")
        .and_then(|v| v.as_i64())
        .map(|i| i.to_string())
        .or_else(|| {
            value
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("name").and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    MediaRequest {
        id,
        media_id,
        media_type,
        status,
        title,
        source: value.clone(),
    }
}
