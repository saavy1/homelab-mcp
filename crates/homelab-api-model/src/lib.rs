use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use homelab_core::{OperationEnvelope, RiskLevel};

pub const API_MAJOR: u16 = 1;
pub const API_MINOR: u16 = 1;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Capabilities {
    pub api: ApiVersion,
    pub compatible_cli_major: u16,
    pub operations: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Movie,
    Tv,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SearchMediaQuery {
    pub query: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDetailsQuery {
    pub media_type: MediaType,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateMediaRequest {
    pub media_id: i64,
    pub media_type: MediaType,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ListRequestsQuery {
    pub status: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ListDownloadsQuery {
    pub status: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DeleteDownloadQuery {
    #[serde(default)]
    pub delete_files: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct MediaSearchItem {
    pub id: String,
    pub media_type: MediaType,
    pub title: String,
    pub year: Option<i32>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct MediaRequest {
    pub id: String,
    pub media_id: String,
    pub media_type: MediaType,
    pub status: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DownloadItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub percentage: Option<String>,
    pub size: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct LibraryStatus {
    pub item_count: Option<u64>,
    pub movie_count: Option<u64>,
    pub series_count: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ActiveSession {
    pub id: String,
    pub user_name: Option<String>,
    pub item_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct MediaOperation {
    pub service: String,
    pub operation: String,
    pub affected_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct BackendHealth {
    pub backend: String,
    pub healthy: bool,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct MediaHealth {
    pub status: HealthStatus,
    pub backends: Vec<BackendHealth>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonAvailabilityQuery {
    pub media_id: i64,
    pub season: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessStatus {
    Complete,
    Incomplete,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeReleaseStatus {
    Aired,
    Future,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodePresence {
    Available,
    Missing,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct AvailabilitySeries {
    pub media_id: String,
    pub jellyfin_id: Option<String>,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CompletenessSummary {
    pub status: CompletenessStatus,
    pub expected_count: u32,
    pub available_count: u32,
    pub missing_count: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct AvailabilityEpisode {
    pub episode_id: String,
    pub episode_number: u32,
    pub title: String,
    pub air_date: Option<NaiveDate>,
    pub release_status: EpisodeReleaseStatus,
    pub presence: EpisodePresence,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SeasonAvailability {
    pub series: AvailabilitySeries,
    pub season: u32,
    pub as_of: NaiveDate,
    pub in_library: bool,
    pub aired: CompletenessSummary,
    pub announced: CompletenessSummary,
    pub unknown_air_date_count: u32,
    pub next_airing: Option<AvailabilityEpisode>,
    pub episodes: Option<Vec<AvailabilityEpisode>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_type_accepts_only_movie_or_tv() {
        assert_eq!(
            serde_json::from_str::<MediaType>(r#""movie""#).unwrap(),
            MediaType::Movie
        );
        assert_eq!(
            serde_json::from_str::<MediaType>(r#""tv""#).unwrap(),
            MediaType::Tv
        );
        assert!(serde_json::from_str::<MediaType>(r#""music""#).is_err());
    }

    #[test]
    fn item_details_query_requires_a_catalog_media_type() {
        assert_eq!(
            serde_json::from_str::<ItemDetailsQuery>(r#"{"media_type":"tv"}"#).unwrap(),
            ItemDetailsQuery {
                media_type: MediaType::Tv,
            }
        );
        assert!(serde_json::from_str::<ItemDetailsQuery>("{}").is_err());
        assert!(serde_json::from_str::<ItemDetailsQuery>(r#"{"media_type":"series"}"#).is_err());
    }

    #[test]
    fn health_status_uses_snake_case_values() {
        assert_eq!(
            serde_json::to_string(&HealthStatus::Unavailable).unwrap(),
            r#""unavailable""#
        );
        assert_eq!(
            serde_json::from_str::<HealthStatus>(r#""degraded""#).unwrap(),
            HealthStatus::Degraded
        );
    }

    #[test]
    fn search_item_has_no_raw_source_field() {
        let item = MediaSearchItem {
            id: "100".into(),
            media_type: MediaType::Movie,
            title: "Alien".into(),
            year: Some(1979),
            status: Some("available".into()),
        };

        let value = serde_json::to_value(item).unwrap();
        assert!(value.get("source").is_none());
        assert_eq!(value["media_type"], "movie");
    }

    #[test]
    fn capabilities_expose_api_and_cli_compatibility_versions() {
        let capabilities = Capabilities {
            api: ApiVersion {
                major: API_MAJOR,
                minor: API_MINOR,
            },
            compatible_cli_major: API_MAJOR,
            operations: vec!["media.search".into(), "media.library.availability".into()],
        };

        let value = serde_json::to_value(capabilities).unwrap();
        assert_eq!(value["api"]["major"], 1);
        assert_eq!(value["api"]["minor"], 1);
        assert_eq!(value["compatible_cli_major"], 1);
        assert_eq!(value["operations"][1], "media.library.availability");
    }

    #[test]
    fn mutation_request_rejects_unknown_fields() {
        let json = r#"{"media_id":100,"media_type":"movie","source":{}}"#;
        assert!(serde_json::from_str::<CreateMediaRequest>(json).is_err());
    }

    #[test]
    fn delete_download_query_defaults_to_preserving_files() {
        let query: DeleteDownloadQuery = serde_json::from_str("{}").unwrap();
        assert!(!query.delete_files);
    }

    #[test]
    fn season_availability_contract_is_normalized_snake_case() {
        let value = serde_json::to_value(SeasonAvailability {
            series: AvailabilitySeries {
                media_id: "60625".into(),
                jellyfin_id: Some("series-1".into()),
                title: "Rick and Morty".into(),
            },
            season: 3,
            as_of: NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(),
            in_library: true,
            aired: CompletenessSummary {
                status: CompletenessStatus::Incomplete,
                expected_count: 2,
                available_count: 1,
                missing_count: 1,
            },
            announced: CompletenessSummary {
                status: CompletenessStatus::Incomplete,
                expected_count: 3,
                available_count: 2,
                missing_count: 1,
            },
            unknown_air_date_count: 1,
            next_airing: Some(AvailabilityEpisode {
                episode_id: "303".into(),
                episode_number: 3,
                title: "Future".into(),
                air_date: Some(NaiveDate::from_ymd_opt(2026, 8, 21).unwrap()),
                release_status: EpisodeReleaseStatus::Future,
                presence: EpisodePresence::Available,
            }),
            episodes: Some(vec![]),
        })
        .unwrap();

        assert_eq!(value["as_of"], "2026-08-20");
        assert_eq!(value["aired"]["status"], "incomplete");
        assert_eq!(value["next_airing"]["release_status"], "future");
        assert_eq!(value["next_airing"]["presence"], "available");
        assert!(value.get("source").is_none());
    }

    #[test]
    fn season_availability_query_rejects_unknown_fields() {
        let error = serde_json::from_value::<SeasonAvailabilityQuery>(serde_json::json!({
            "media_id": 60625,
            "season": 3,
            "backend": "jellyfin"
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }
}
