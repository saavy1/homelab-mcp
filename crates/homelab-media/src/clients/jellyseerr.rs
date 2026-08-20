use crate::{
    MediaError,
    availability::{ExpectedEpisode, ExpectedSeason},
    config::ServiceConfig,
};
use chrono::NaiveDate;
use homelab_api_model::{MediaOperation, MediaRequest, MediaSearchItem, MediaType};
use reqwest::{Client, Method};
use serde_json::{Value, json};
use std::collections::HashSet;

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

    pub async fn item_details(
        &self,
        media_type: MediaType,
        item_id: &str,
    ) -> Result<MediaSearchItem, MediaError> {
        require_catalog_id(item_id)?;
        let value = self
            .send(
                Method::GET,
                "item_details",
                &format!("/api/v1/{}/{item_id}", media_type_text(media_type)),
                None,
                false,
            )
            .await?;
        normalize_catalog_item(&value, media_type)
            .ok_or_else(|| MediaError::serialization("jellyseerr", "item_details"))
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
            .send(
                Method::POST,
                "request_media",
                "/api/v1/request",
                Some(body),
                true,
            )
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
            .or_else(|| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(normalize_request)
            .collect())
    }

    pub async fn approve_request(&self, request_id: &str) -> Result<MediaOperation, MediaError> {
        self.request_action(request_id, "approve_request", "approve")
            .await
    }

    pub async fn decline_request(&self, request_id: &str) -> Result<MediaOperation, MediaError> {
        self.request_action(request_id, "decline_request", "decline")
            .await
    }

    pub(crate) async fn expected_season(
        &self,
        media_id: i64,
        season: u32,
    ) -> Result<ExpectedSeason, MediaError> {
        let details_path = format!("/api/v1/tv/{media_id}");
        let season_path = format!("/api/v1/tv/{media_id}/season/{season}");
        let (details, season_value) = tokio::try_join!(
            self.send(
                Method::GET,
                "get_tv_details",
                &details_path,
                None,
                false
            ),
            self.send(
                Method::GET,
                "get_tv_season",
                &season_path,
                None,
                false
            ),
        )?;
        normalize_expected_season(media_id, season, &details, &season_value)
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
        let seasons = value
            .get("seasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|season| season.get("seasonNumber").and_then(Value::as_i64))
            .filter(|season| *season > 0)
            .collect::<Vec<_>>();
        if seasons.is_empty() {
            return Err(MediaError::Validation(format!(
                "no requestable seasons found for tv media id {media_id}"
            )));
        }
        Ok(seasons)
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

fn require_catalog_id(value: &str) -> Result<(), MediaError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        Err(MediaError::Validation(
            "item_id must be a non-empty numeric catalog identifier".into(),
        ))
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

fn normalize_expected_season(
    media_id: i64,
    requested_season: u32,
    details: &Value,
    season_value: &Value,
) -> Result<ExpectedSeason, MediaError> {
    let details_error = || MediaError::serialization("jellyseerr", "get_tv_details");
    let season_error = || MediaError::serialization("jellyseerr", "get_tv_season");

    let title = details
        .get("name")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(details_error)?
        .to_owned();
    let normalized_season = season_value
        .get("seasonNumber")
        .and_then(Value::as_i64)
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| *number == requested_season)
        .ok_or_else(season_error)?;
    let source_episodes = season_value
        .get("episodes")
        .and_then(Value::as_array)
        .ok_or_else(season_error)?;

    let mut episode_ids = HashSet::with_capacity(source_episodes.len());
    let mut episode_numbers = HashSet::with_capacity(source_episodes.len());
    let mut episodes = Vec::with_capacity(source_episodes.len());
    for episode in source_episodes {
        let tmdb_id = episode
            .get("id")
            .and_then(scalar_string)
            .filter(|id| !id.is_empty())
            .ok_or_else(season_error)?;
        let episode_number = episode
            .get("episodeNumber")
            .and_then(Value::as_i64)
            .and_then(|number| u32::try_from(number).ok())
            .ok_or_else(season_error)?;
        let title = episode
            .get("name")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .ok_or_else(season_error)?
            .to_owned();
        if !episode_ids.insert(tmdb_id.clone()) || !episode_numbers.insert(episode_number) {
            return Err(season_error());
        }
        let air_date = episode
            .get("airDate")
            .and_then(Value::as_str)
            .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok());
        episodes.push(ExpectedEpisode {
            tmdb_id,
            episode_number,
            title,
            air_date,
        });
    }

    Ok(ExpectedSeason {
        media_id: media_id.to_string(),
        title,
        season: normalized_season,
        episodes,
    })
}

fn normalize_search_result(value: &Value) -> Option<MediaSearchItem> {
    let media_type = value
        .get("mediaType")
        .and_then(Value::as_str)
        .and_then(parse_media_type)?;
    normalize_catalog_item(value, media_type)
}

fn normalize_catalog_item(value: &Value, media_type: MediaType) -> Option<MediaSearchItem> {
    let id = value.get("id").and_then(scalar_string)?;
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
    let media_id = value.get("mediaId").and_then(scalar_string).or_else(|| {
        media
            .and_then(|item| item.get("tmdbId"))
            .and_then(scalar_string)
    })?;
    let media_type = value
        .get("mediaType")
        .and_then(Value::as_str)
        .or_else(|| {
            media
                .and_then(|item| item.get("mediaType"))
                .and_then(Value::as_str)
        })
        .and_then(parse_media_type)?;
    Some(MediaRequest {
        id: value.get("id").and_then(scalar_string)?,
        media_id,
        media_type,
        status: value
            .get("status")
            .and_then(scalar_string)
            .unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::OriginalUri,
        http::{HeaderMap, StatusCode},
        routing::get,
    };
    use homelab_core::ErrorCode;
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Barrier};

    async fn spawn(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}")
    }

    fn client(base_url: String, key: &str) -> JellyseerrClient {
        JellyseerrClient::new(
            Client::new(),
            ServiceConfig::new("jellyseerr", base_url, key).unwrap(),
        )
    }

    fn valid_details() -> Value {
        json!({"id": 60625, "name": "Rick and Morty"})
    }

    fn valid_season(season: u32) -> Value {
        json!({
            "seasonNumber": season,
            "episodes": [
                {"id": 301, "episodeNumber": 1, "name": "A", "airDate": "2017-04-01"}
            ]
        })
    }

    fn expected_season_app(details: Value, season: Value, season_number: u32) -> Router {
        Router::new()
            .route(
                "/api/v1/tv/60625",
                get(move || {
                    let details = details.clone();
                    async move { Json(details) }
                }),
            )
            .route(
                &format!("/api/v1/tv/60625/season/{season_number}"),
                get(move || {
                    let season = season.clone();
                    async move { Json(season) }
                }),
            )
    }

    async fn normalization_error(details: Value, season: Value) -> MediaError {
        let app = expected_season_app(details, season, 3);
        client(spawn(app).await, "key")
            .expected_season(60625, 3)
            .await
            .unwrap_err()
    }

    #[tokio::test]
    async fn expected_season_reads_tv_and_season_concurrently_and_normalizes_dates() {
        let barrier = Arc::new(Barrier::new(2));
        let details_barrier = Arc::clone(&barrier);
        let season_barrier = Arc::clone(&barrier);
        let app = Router::new()
            .route(
                "/api/v1/tv/60625",
                get(move |uri: OriginalUri, headers: HeaderMap| {
                    let barrier = Arc::clone(&details_barrier);
                    async move {
                        barrier.wait().await;
                        assert_eq!(uri.path(), "/api/v1/tv/60625");
                        assert_eq!(headers["x-api-key"], "key");
                        Json(json!({"id": 60625, "name": "Rick and Morty"}))
                    }
                }),
            )
            .route(
                "/api/v1/tv/60625/season/3",
                get(move |uri: OriginalUri, headers: HeaderMap| {
                    let barrier = Arc::clone(&season_barrier);
                    async move {
                        barrier.wait().await;
                        assert_eq!(uri.path(), "/api/v1/tv/60625/season/3");
                        assert_eq!(headers["x-api-key"], "key");
                        Json(json!({"seasonNumber": 3, "episodes": [
                            {"id": 303, "episodeNumber": 3, "name": "C", "airDate": null},
                            {"id": 301, "episodeNumber": 1, "name": "A", "airDate": "2017-04-01"},
                            {"id": 302, "episodeNumber": 2, "name": "B", "airDate": "not-a-date"}
                        ]}))
                    }
                }),
            );

        let season = client(spawn(app).await, "key")
            .expected_season(60625, 3)
            .await
            .unwrap();

        assert_eq!(season.media_id, "60625");
        assert_eq!(season.title, "Rick and Morty");
        assert_eq!(season.season, 3);
        assert_eq!(
            season
                .episodes
                .iter()
                .map(|episode| (
                    episode.tmdb_id.as_str(),
                    episode.episode_number,
                    episode.title.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![("303", 3, "C"), ("301", 1, "A"), ("302", 2, "B")]
        );
        assert_eq!(season.episodes[0].air_date, None);
        assert_eq!(
            season.episodes[1].air_date.unwrap().to_string(),
            "2017-04-01"
        );
        assert_eq!(season.episodes[2].air_date, None);
    }

    #[tokio::test]
    async fn expected_season_requires_all_normalized_fields() {
        let cases = [
            (
                "tv title",
                json!({"id": 60625}),
                valid_season(3),
            ),
            (
                "episode id",
                valid_details(),
                json!({"seasonNumber": 3, "episodes": [
                    {"episodeNumber": 1, "name": "A"}
                ]}),
            ),
            (
                "episode number",
                valid_details(),
                json!({"seasonNumber": 3, "episodes": [
                    {"id": 301, "name": "A"}
                ]}),
            ),
            (
                "episode title",
                valid_details(),
                json!({"seasonNumber": 3, "episodes": [
                    {"id": 301, "episodeNumber": 1}
                ]}),
            ),
        ];

        for (field, details, season) in cases {
            let error = normalization_error(details, season).await;
            assert_eq!(error.error_code(), ErrorCode::Internal, "{field}");
        }
    }

    #[tokio::test]
    async fn expected_season_rejects_missing_or_malformed_episode_lists() {
        for season in [
            json!({"seasonNumber": 3}),
            json!({"seasonNumber": 3, "episodes": {"raw": "UPSTREAM_SECRET"}}),
        ] {
            let error = normalization_error(valid_details(), season).await;
            assert_eq!(error.error_code(), ErrorCode::Internal);
            assert!(!format!("{error:?}").contains("UPSTREAM_SECRET"));
            assert!(!error.to_string().contains("UPSTREAM_SECRET"));
            assert!(!error.public_message().contains("UPSTREAM_SECRET"));
        }
    }

    #[tokio::test]
    async fn expected_season_rejects_contradictory_season_number() {
        let error = normalization_error(valid_details(), valid_season(4)).await;

        assert_eq!(error.error_code(), ErrorCode::Internal);
    }

    #[tokio::test]
    async fn expected_season_rejects_duplicate_episode_identities_or_numbers() {
        let duplicate_id = json!({"seasonNumber": 3, "episodes": [
            {"id": 301, "episodeNumber": 1, "name": "A"},
            {"id": 301, "episodeNumber": 2, "name": "B"}
        ]});
        let duplicate_number = json!({"seasonNumber": 3, "episodes": [
            {"id": 301, "episodeNumber": 1, "name": "A"},
            {"id": 302, "episodeNumber": 1, "name": "B"}
        ]});

        for season in [duplicate_id, duplicate_number] {
            let error = normalization_error(valid_details(), season).await;
            assert_eq!(error.error_code(), ErrorCode::Internal);
        }
    }

    #[tokio::test]
    async fn expected_season_checks_episode_number_conversion() {
        for episode_number in [json!(-1), json!(u64::from(u32::MAX) + 1)] {
            let error = normalization_error(
                valid_details(),
                json!({"seasonNumber": 3, "episodes": [
                    {"id": 301, "episodeNumber": episode_number, "name": "A"}
                ]}),
            )
            .await;
            assert_eq!(error.error_code(), ErrorCode::Internal);
        }
    }

    #[tokio::test]
    async fn expected_season_maps_tv_details_404_without_raw_body() {
        let app = Router::new()
            .route(
                "/api/v1/tv/60625",
                get(|| async { (StatusCode::NOT_FOUND, "UPSTREAM_SECRET") }),
            )
            .route(
                "/api/v1/tv/60625/season/3",
                get(|| async { Json(valid_season(3)) }),
            );

        let error = client(spawn(app).await, "key")
            .expected_season(60625, 3)
            .await
            .unwrap_err();

        assert_eq!(error.error_code(), ErrorCode::NotFound);
        assert!(!format!("{error:?}").contains("UPSTREAM_SECRET"));
        assert!(!error.to_string().contains("UPSTREAM_SECRET"));
        assert!(!error.public_message().contains("UPSTREAM_SECRET"));
    }

    #[tokio::test]
    async fn expected_season_maps_tv_season_404_without_raw_body() {
        let app = Router::new()
            .route(
                "/api/v1/tv/60625",
                get(|| async { Json(valid_details()) }),
            )
            .route(
                "/api/v1/tv/60625/season/3",
                get(|| async { (StatusCode::NOT_FOUND, "UPSTREAM_SECRET") }),
            );

        let error = client(spawn(app).await, "key")
            .expected_season(60625, 3)
            .await
            .unwrap_err();

        assert_eq!(error.error_code(), ErrorCode::NotFound);
        assert!(!format!("{error:?}").contains("UPSTREAM_SECRET"));
        assert!(!error.to_string().contains("UPSTREAM_SECRET"));
        assert!(!error.public_message().contains("UPSTREAM_SECRET"));
    }

    #[tokio::test]
    async fn expected_season_sends_season_zero_unchanged() {
        let app = expected_season_app(valid_details(), valid_season(0), 0);

        let season = client(spawn(app).await, "key")
            .expected_season(60625, 0)
            .await
            .unwrap();

        assert_eq!(season.season, 0);
    }
}
