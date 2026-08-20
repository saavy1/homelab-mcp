use crate::{MediaError, config::ServiceConfig};
use homelab_api_model::{ActiveSession, LibraryStatus, MediaOperation};
use reqwest::{Client, Method};
use serde_json::Value;

pub struct JellyfinClient {
    http: Client,
    config: ServiceConfig,
}

impl JellyfinClient {
    pub fn new(http: Client, config: ServiceConfig) -> Self {
        Self { http, config }
    }

    pub async fn health(&self) -> Result<(), MediaError> {
        self.send(Method::GET, "health", "/System/Info/Public", false)
            .await
            .map(|_| ())
    }

    pub async fn get_library_status(&self) -> Result<LibraryStatus, MediaError> {
        let value = self
            .send(Method::GET, "get_library_status", "/Items/Counts", false)
            .await?;
        Ok(LibraryStatus {
            item_count: value.get("ItemCount").and_then(Value::as_u64),
            movie_count: value.get("MovieCount").and_then(Value::as_u64),
            series_count: value.get("SeriesCount").and_then(Value::as_u64),
        })
    }

    pub async fn refresh_library(&self) -> Result<MediaOperation, MediaError> {
        self.send(Method::POST, "refresh_library", "/Library/Refresh", true)
            .await?;
        Ok(MediaOperation {
            service: "jellyfin".into(),
            operation: "refresh_library".into(),
            affected_id: None,
        })
    }

    pub async fn get_active_sessions(&self) -> Result<Vec<ActiveSession>, MediaError> {
        let value = self
            .send(Method::GET, "get_active_sessions", "/Sessions", false)
            .await?;
        let sessions = value
            .as_array()
            .ok_or_else(|| MediaError::serialization("jellyfin", "get_active_sessions"))?;
        Ok(sessions
            .iter()
            .filter_map(|session| {
                Some(ActiveSession {
                    id: session.get("Id")?.as_str()?.to_owned(),
                    user_name: session
                        .get("UserName")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    item_name: session
                        .pointer("/NowPlayingItem/Name")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                })
            })
            .collect())
    }

    async fn send(
        &self,
        method: Method,
        operation: &'static str,
        path: &str,
        mutating: bool,
    ) -> Result<Value, MediaError> {
        let response = self
            .http
            .request(method, format!("{}{}", self.config.base_url, path))
            .header("X-Emby-Token", &self.config.api_key)
            .send()
            .await
            .map_err(|error| MediaError::transport("jellyfin", operation, &error, mutating))?;
        let status = response.status();
        if !status.is_success() {
            return Err(MediaError::upstream(
                "jellyfin",
                operation,
                Some(status.as_u16()),
                mutating,
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| MediaError::transport("jellyfin", operation, &error, mutating))?;
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| MediaError::serialization("jellyfin", operation))
    }
}
