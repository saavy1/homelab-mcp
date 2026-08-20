use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use homelab_core::{OperationEnvelope, RiskLevel};

pub const API_MAJOR: u16 = 1;
pub const API_MINOR: u16 = 0;

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
        assert!(
            serde_json::from_str::<ItemDetailsQuery>(r#"{"media_type":"series"}"#).is_err()
        );
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
            operations: vec!["media.search".into()],
        };

        let value = serde_json::to_value(capabilities).unwrap();
        assert_eq!(value["api"]["major"], 1);
        assert_eq!(value["api"]["minor"], 0);
        assert_eq!(value["compatible_cli_major"], 1);
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
}
