use crate::{
    MediaError,
    availability::{LibraryEpisode, LibrarySeason},
    config::ServiceConfig,
};
use homelab_api_model::{ActiveSession, LibraryStatus, MediaOperation};
use reqwest::{Client, Method};
use serde_json::Value;

const PAGE_SIZE: usize = 200;
const MAX_RECORDS: usize = 10_000;

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

    pub(crate) async fn library_season(
        &self,
        media_id: &str,
        season: u32,
    ) -> Result<Option<LibrarySeason>, MediaError> {
        let series = self
            .paged_items("list_series", |start_index| {
                format!(
                    "/Items?Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&StartIndex={start_index}&Limit={PAGE_SIZE}"
                )
            })
            .await?;
        let mut matching_series = series.into_iter().filter(|item| {
            item.pointer("/ProviderIds/Tmdb").and_then(Value::as_str) == Some(media_id)
        });
        let Some(matched_series) = matching_series.next() else {
            return Ok(None);
        };
        if matching_series.next().is_some() {
            return Err(MediaError::Conflict);
        }
        let series_id = matched_series
            .get("Id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or(MediaError::Internal)?
            .to_owned();
        let encoded_series_id = percent_encode_path_segment(&series_id);
        let episodes = self
            .paged_items("list_episodes", |start_index| {
                format!(
                    "/Shows/{encoded_series_id}/Episodes?Season={season}&IsMissing=false&Fields=ProviderIds&StartIndex={start_index}&Limit={PAGE_SIZE}"
                )
            })
            .await?
            .into_iter()
            .filter_map(|item| normalize_library_episode(&item, season))
            .collect();

        Ok(Some(LibrarySeason {
            series_id,
            episodes,
        }))
    }

    async fn paged_items(
        &self,
        operation: &'static str,
        path: impl Fn(usize) -> String,
    ) -> Result<Vec<Value>, MediaError> {
        let mut items = Vec::new();
        let mut start_index = 0_usize;

        loop {
            let request_path = path(start_index);
            let mut page = self
                .send(Method::GET, operation, &request_path, false)
                .await?;
            let total = match page.get("TotalRecordCount") {
                Some(value) => {
                    let total = value
                        .as_u64()
                        .and_then(|value| usize::try_from(value).ok())
                        .ok_or(MediaError::Internal)?;
                    if total > MAX_RECORDS {
                        return Err(MediaError::Internal);
                    }
                    Some(total)
                }
                None => None,
            };
            let page_items = page
                .get_mut("Items")
                .and_then(Value::as_array_mut)
                .map(std::mem::take)
                .ok_or_else(|| MediaError::serialization("jellyfin", operation))?;
            let page_len = page_items.len();
            let accumulated_len = items
                .len()
                .checked_add(page_len)
                .ok_or(MediaError::Internal)?;
            if accumulated_len > MAX_RECORDS {
                return Err(MediaError::Internal);
            }
            items.extend(page_items);

            if page_len < PAGE_SIZE
                || total.is_some_and(|total_record_count| items.len() >= total_record_count)
            {
                return Ok(items);
            }
            start_index = start_index
                .checked_add(PAGE_SIZE)
                .ok_or(MediaError::Internal)?;
        }
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

fn normalize_library_episode(item: &Value, requested_season: u32) -> Option<LibraryEpisode> {
    let season_number = item
        .get("ParentIndexNumber")?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())?;
    if season_number != requested_season {
        return None;
    }
    let episode_number = item
        .get("IndexNumber")?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())?;
    let jellyfin_id = item
        .get("Id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let tmdb_id = item
        .pointer("/ProviderIds/Tmdb")
        .and_then(Value::as_str)
        .map(str::to_owned);

    Some(LibraryEpisode {
        jellyfin_id,
        tmdb_id,
        season_number,
        episode_number,
    })
}
fn percent_encode_path_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
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
    use axum::{Json, Router, extract::OriginalUri, http::HeaderMap, routing::get};
    use homelab_core::ErrorCode;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    async fn spawn(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}")
    }

    fn client(base_url: String, key: &str) -> JellyfinClient {
        JellyfinClient::new(
            Client::new(),
            ServiceConfig::new("jellyfin", base_url, key).unwrap(),
        )
    }

    fn record_request(
        requests: &Mutex<Vec<String>>,
        uri: &OriginalUri,
        headers: &HeaderMap,
    ) -> String {
        assert_eq!(headers["x-emby-token"], "key");
        let request = uri.to_string();
        assert!(!request.contains("AnyProviderIdEquals"));
        requests.lock().push(request.clone());
        request
    }

    #[tokio::test]
    async fn library_season_pages_series_and_real_episodes_with_exact_queries() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let series_requests = Arc::clone(&requests);
        let episode_requests = Arc::clone(&requests);
        let app = Router::new()
            .route(
                "/Items",
                get(move |uri: OriginalUri, headers: HeaderMap| {
                    let requests = Arc::clone(&series_requests);
                    async move {
                        let request = record_request(&requests, &uri, &headers);
                        let items = if request.contains("StartIndex=0") {
                            let mut items = Vec::with_capacity(200);
                            items.push(json!({
                                "Id": "series-1",
                                "Name": "Exact provider match",
                                "ProviderIds": {"Tmdb": "60625"}
                            }));
                            items.extend((1..200).map(|index| {
                                json!({
                                    "Id": format!("other-series-{index}"),
                                    "ProviderIds": {"Tmdb": format!("other-{index}")}
                                })
                            }));
                            items
                        } else {
                            vec![json!({
                                "Id": "title-only",
                                "Name": "60625",
                                "ProviderIds": {"Tmdb": "not-60625"}
                            })]
                        };
                        Json(json!({"Items": items, "TotalRecordCount": 201}))
                    }
                }),
            )
            .route(
                "/Shows/series-1/Episodes",
                get(move |uri: OriginalUri, headers: HeaderMap| {
                    let requests = Arc::clone(&episode_requests);
                    async move {
                        let request = record_request(&requests, &uri, &headers);
                        let items = if request.contains("StartIndex=0") {
                            let mut items = Vec::with_capacity(200);
                            items.push(json!({
                                "Id": "episode-1",
                                "ProviderIds": {"Tmdb": "301"},
                                "ParentIndexNumber": 3,
                                "IndexNumber": 1
                            }));
                            items.extend((1..200).map(|index| {
                                json!({
                                    "Id": format!("other-season-{index}"),
                                    "ProviderIds": {"Tmdb": format!("ignore-{index}")},
                                    "ParentIndexNumber": 2,
                                    "IndexNumber": index
                                })
                            }));
                            items
                        } else {
                            vec![
                                json!({
                                    "Id": "episode-2",
                                    "ProviderIds": {},
                                    "ParentIndexNumber": 3,
                                    "IndexNumber": 2
                                }),
                                json!({
                                    "Id": "wrong-season",
                                    "ProviderIds": {"Tmdb": "ignored"},
                                    "ParentIndexNumber": 4,
                                    "IndexNumber": 3
                                }),
                                json!({
                                    "Id": "missing-number",
                                    "ProviderIds": {"Tmdb": "ignored"},
                                    "ParentIndexNumber": 3
                                }),
                                json!({
                                    "Id": "negative-number",
                                    "ProviderIds": {"Tmdb": "ignored"},
                                    "ParentIndexNumber": 3,
                                    "IndexNumber": -1
                                }),
                                json!({
                                    "ProviderIds": {"Tmdb": "303"},
                                    "ParentIndexNumber": 3,
                                    "IndexNumber": 3
                                }),
                                json!({
                                    "Id": "   ",
                                    "ProviderIds": {},
                                    "ParentIndexNumber": 3,
                                    "IndexNumber": 4
                                }),
                                json!({
                                    "Id": "malformed-season",
                                    "ProviderIds": {"Tmdb": "ignored"},
                                    "ParentIndexNumber": "3",
                                    "IndexNumber": 5
                                }),
                            ]
                        };
                        Json(json!({"Items": items, "TotalRecordCount": 207}))
                    }
                }),
            );

        let season = client(spawn(app).await, "key")
            .library_season("60625", 3)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(season.series_id, "series-1");
        assert_eq!(
            season.episodes,
            vec![
                LibraryEpisode {
                    jellyfin_id: Some("episode-1".into()),
                    tmdb_id: Some("301".into()),
                    season_number: 3,
                    episode_number: 1,
                },
                LibraryEpisode {
                    jellyfin_id: Some("episode-2".into()),
                    tmdb_id: None,
                    season_number: 3,
                    episode_number: 2,
                },
                LibraryEpisode {
                    jellyfin_id: None,
                    tmdb_id: Some("303".into()),
                    season_number: 3,
                    episode_number: 3,
                },
                LibraryEpisode {
                    jellyfin_id: None,
                    tmdb_id: None,
                    season_number: 3,
                    episode_number: 4,
                },
            ]
        );
        assert_eq!(
            requests.lock().as_slice(),
            [
                "/Items?Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&StartIndex=0&Limit=200",
                "/Items?Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&StartIndex=200&Limit=200",
                "/Shows/series-1/Episodes?Season=3&IsMissing=false&Fields=ProviderIds&StartIndex=0&Limit=200",
                "/Shows/series-1/Episodes?Season=3&IsMissing=false&Fields=ProviderIds&StartIndex=200&Limit=200",
            ]
        );
    }

    #[tokio::test]
    async fn library_season_percent_encodes_the_opaque_series_path_segment() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let series_requests = Arc::clone(&requests);
        let episode_requests = Arc::clone(&requests);
        let app = Router::new()
            .route(
                "/Items",
                get(move |uri: OriginalUri, headers: HeaderMap| {
                    let requests = Arc::clone(&series_requests);
                    async move {
                        record_request(&requests, &uri, &headers);
                        Json(json!({
                            "Items": [{
                                "Id": "series /?#%",
                                "ProviderIds": {"Tmdb": "60625"}
                            }],
                            "TotalRecordCount": 1
                        }))
                    }
                }),
            )
            .fallback(get(move |uri: OriginalUri, headers: HeaderMap| {
                let requests = Arc::clone(&episode_requests);
                async move {
                    record_request(&requests, &uri, &headers);
                    Json(json!({"Items": [], "TotalRecordCount": 0}))
                }
            }));

        let season = client(spawn(app).await, "key")
            .library_season("60625", 3)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(season.series_id, "series /?#%");
        assert!(season.episodes.is_empty());
        assert_eq!(
            requests.lock().as_slice(),
            [
                "/Items?Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&StartIndex=0&Limit=200",
                "/Shows/series%20%2F%3F%23%25/Episodes?Season=3&IsMissing=false&Fields=ProviderIds&StartIndex=0&Limit=200",
            ]
        );
    }

    #[tokio::test]
    async fn library_season_returns_none_without_requesting_episodes_for_an_absent_series() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);
        let app = Router::new().route(
            "/Items",
            get(move |uri: OriginalUri, headers: HeaderMap| {
                let requests = Arc::clone(&observed);
                async move {
                    record_request(&requests, &uri, &headers);
                    Json(json!({"Items": [
                        {
                            "Id": "wrong-provider",
                            "Name": "999",
                            "ProviderIds": {"Tmdb": "998"}
                        },
                        {
                            "Id": "missing-provider",
                            "Name": "999"
                        }
                    ], "TotalRecordCount": 2}))
                }
            }),
        );

        assert_eq!(
            client(spawn(app).await, "key")
                .library_season("999", 3)
                .await
                .unwrap(),
            None
        );
        assert_eq!(requests.lock().len(), 1);
    }

    #[tokio::test]
    async fn library_season_rejects_duplicate_exact_series_matches() {
        let app = Router::new().route(
            "/Items",
            get(|uri: OriginalUri, headers: HeaderMap| async move {
                record_request(&Mutex::new(Vec::new()), &uri, &headers);
                Json(json!({"Items": [
                    {"Id": "opaque-series-secret-a", "ProviderIds": {"Tmdb": "60625"}},
                    {"Id": "opaque-series-secret-b", "ProviderIds": {"Tmdb": "60625"}}
                ], "TotalRecordCount": 2}))
            }),
        );

        let error = client(spawn(app).await, "key")
            .library_season("60625", 3)
            .await
            .unwrap_err();

        assert_eq!(error.error_code(), ErrorCode::Conflict);
        assert!(!error.retryable());
        assert_eq!(error.to_string(), "media records conflict");
        assert_eq!(error.public_message(), "media records conflict");
        for rendered in [
            format!("{error:?}"),
            error.to_string(),
            error.public_message(),
        ] {
            assert!(!rendered.contains("opaque-series-secret"));
            assert!(!rendered.contains("60625"));
            assert!(!rendered.contains("/Items"));
        }
    }

    #[tokio::test]
    async fn library_season_rejects_repeated_full_pages_instead_of_truncating_at_the_cap() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);
        let app = Router::new().route(
            "/Items",
            get(move |uri: OriginalUri, headers: HeaderMap| {
                let requests = Arc::clone(&observed);
                async move {
                    record_request(&requests, &uri, &headers);
                    Json(json!({
                        "Items": (0..200).map(|_| {
                            json!({"ProviderIds": {"Tmdb": "not-the-requested-id"}})
                        }).collect::<Vec<_>>()
                    }))
                }
            }),
        );

        let error = client(spawn(app).await, "key")
            .library_season("60625", 3)
            .await
            .unwrap_err();

        assert_eq!(error.error_code(), ErrorCode::Internal);
        assert!(!error.retryable());
        assert_eq!(
            error.public_message(),
            "media backend returned invalid normalized data"
        );
        assert_eq!(requests.lock().len(), 51);
        assert_eq!(
            requests.lock().last().unwrap(),
            "/Items?Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&StartIndex=10000&Limit=200"
        );
    }

    #[tokio::test]
    async fn library_season_rejects_malformed_pages_without_exposing_upstream_data() {
        let app = Router::new().route(
            "/Items",
            get(|uri: OriginalUri, headers: HeaderMap| async move {
                record_request(&Mutex::new(Vec::new()), &uri, &headers);
                Json(json!({
                    "Items": {"raw": "UPSTREAM_SECRET"},
                    "TotalRecordCount": "MALFORMED_COUNT_SECRET"
                }))
            }),
        );
        let base_url = spawn(app).await;
        let error = client(base_url.clone(), "key")
            .library_season("opaque-request-secret", 3)
            .await
            .unwrap_err();

        assert_eq!(error.error_code(), ErrorCode::Internal);
        for rendered in [
            format!("{error:?}"),
            error.to_string(),
            error.public_message(),
        ] {
            assert!(!rendered.contains("UPSTREAM_SECRET"));
            assert!(!rendered.contains("MALFORMED_COUNT_SECRET"));
            assert!(!rendered.contains("opaque-request-secret"));
            assert!(!rendered.contains("key"));
            assert!(!rendered.contains(&base_url));
            assert!(!rendered.contains("/Items"));
        }
    }

    #[test]
    fn normalized_data_errors_have_fixed_codes_and_redacted_messages() {
        let internal = MediaError::Internal;
        assert_eq!(internal.error_code(), ErrorCode::Internal);
        assert!(!internal.retryable());
        assert_eq!(internal.to_string(), "media normalized data is invalid");
        assert_eq!(
            internal.public_message(),
            "media backend returned invalid normalized data"
        );
    }
}
