use crate::{MediaError, config::ServiceConfig};
use homelab_api_model::{MediaOperation, MediaRequest, MediaSearchItem, MediaType};
use reqwest::{Client, Method};
use serde_json::{Value, json};

pub struct JellyseerrClient {
    http: Client,
    config: ServiceConfig,
}

impl JellyseerrClient {
    pub fn new(http: Client, config: ServiceConfig) -> Self {
        Self { http, config }
    }

    pub async fn health(&self) -> Result<(), MediaError> {
        self.send(Method::GET, "health", "/api/v1/status", None, false)
            .await
            .map(|_| ())
    }

    pub async fn search(&self, query: &str) -> Result<Vec<MediaSearchItem>, MediaError> {
        if query.trim().is_empty() {
            return Err(MediaError::Validation("query is required".into()));
        }
        let path = format!("/api/v1/search?query={}", percent_encode_query(query));
        let value = self.send(Method::GET, "search", &path, None, false).await?;
        Ok(value
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(normalize_search_result)
            .collect())
    }

    pub async fn request_media(
        &self,
        media_type: MediaType,
        media_id: i64,
    ) -> Result<MediaRequest, MediaError> {
        let media_type_text = media_type_text(media_type);
        let mut body = json!({"mediaType": media_type_text, "mediaId": media_id});
        if media_type == MediaType::Tv {
            let seasons = self.tv_seasons(media_id).await?;
            body["seasons"] = json!(seasons);
        }
        let value = self
            .send(Method::POST, "request_media", "/api/v1/request", Some(body), true)
            .await?;
        normalize_request(&value)
            .ok_or_else(|| MediaError::serialization("jellyseerr", "request_media"))
    }

    pub async fn list_requests(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<MediaRequest>, MediaError> {
        let path = status
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("/api/v1/request?filter={}", percent_encode_query(value)))
            .unwrap_or_else(|| "/api/v1/request".into());
        let value = self
            .send(Method::GET, "list_requests", &path, None, false)
            .await?;
        Ok(value
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(normalize_request)
            .collect())
    }

    pub async fn approve_request(&self, request_id: &str) -> Result<MediaOperation, MediaError> {
        self.request_action(request_id, "approve_request", "approve").await
    }

    pub async fn decline_request(&self, request_id: &str) -> Result<MediaOperation, MediaError> {
        self.request_action(request_id, "decline_request", "decline").await
    }

    async fn tv_seasons(&self, media_id: i64) -> Result<Vec<i64>, MediaError> {
        let value = self
            .send(
                Method::GET,
                "tv_seasons",
                &format!("/api/v1/tv/{media_id}"),
                None,
                false,
            )
            .await?;
        Ok(value
            .get("seasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|season| season.get("seasonNumber").and_then(Value::as_i64))
            .filter(|season| *season > 0)
            .collect())
    }

    async fn request_action(
        &self,
        request_id: &str,
        operation: &'static str,
        action: &str,
    ) -> Result<MediaOperation, MediaError> {
        require_id(request_id, "request_id")?;
        self.send(
            Method::POST,
            operation,
            &format!("/api/v1/request/{request_id}/{action}"),
            None,
            true,
        )
        .await?;
        Ok(MediaOperation {
            service: "jellyseerr".into(),
            operation: operation.into(),
            affected_id: Some(request_id.into()),
        })
    }

    async fn send(
        &self,
        method: Method,
        operation: &'static str,
        path: &str,
        body: Option<Value>,
        mutating: bool,
    ) -> Result<Value, MediaError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.config.base_url, path))
            .header("X-Api-Key", &self.config.api_key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| MediaError::transport("jellyseerr", operation, &error, mutating))?;
        let status = response.status();
        if !status.is_success() {
            return Err(MediaError::upstream(
                "jellyseerr",
                operation,
                Some(status.as_u16()),
                mutating,
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| MediaError::transport("jellyseerr", operation, &error, mutating))?;
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| MediaError::serialization("jellyseerr", operation))
    }
}

fn require_id(value: &str, field: &str) -> Result<(), MediaError> {
    if value.trim().is_empty() {
        Err(MediaError::Validation(format!("{field} is required")))
    } else {
        Ok(())
    }
}

fn media_type_text(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Movie => "movie",
        MediaType::Tv => "tv",
    }
}

fn parse_media_type(value: &str) -> Option<MediaType> {
    match value.to_ascii_lowercase().as_str() {
        "movie" => Some(MediaType::Movie),
        "tv" | "series" => Some(MediaType::Tv),
        _ => None,
    }
}

fn scalar_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

fn normalize_search_result(value: &Value) -> Option<MediaSearchItem> {
    let id = value.get("id").and_then(scalar_string)?;
    let media_type = value
        .get("mediaType")
        .and_then(Value::as_str)
        .and_then(parse_media_type)?;
    let title = value
        .get("title")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)?
        .to_owned();
    let year = value
        .get("releaseDate")
        .or_else(|| value.get("firstAirDate"))
        .and_then(Value::as_str)
        .and_then(|date| date.get(..4))
        .and_then(|year| year.parse().ok());
    let status = value
        .get("mediaInfo")
        .and_then(|media| media.get("status"))
        .and_then(scalar_string);
    Some(MediaSearchItem {
        id,
        media_type,
        title,
        year,
        status,
    })
}

fn normalize_request(value: &Value) -> Option<MediaRequest> {
    let media = value.get("media");
    let media_id = value
        .get("mediaId")
        .and_then(scalar_string)
        .or_else(|| media.and_then(|item| item.get("tmdbId")).and_then(scalar_string))?;
    let media_type = value
        .get("mediaType")
        .and_then(Value::as_str)
        .or_else(|| media.and_then(|item| item.get("mediaType")).and_then(Value::as_str))
        .and_then(parse_media_type)?;
    Some(MediaRequest {
        id: value.get("id").and_then(scalar_string)?,
        media_id,
        media_type,
        status: value.get("status").and_then(scalar_string).unwrap_or_default(),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                media
                    .and_then(|item| item.get("title"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
    })
}

fn percent_encode_query(query: &str) -> String {
    let mut encoded = String::with_capacity(query.len());
    for byte in query.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    encoded
}
