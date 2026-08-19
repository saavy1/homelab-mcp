use crate::MediaError;
use std::{env, time::Duration};

#[derive(Clone)]
pub struct ServiceConfig {
    pub name: &'static str,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
}

impl ServiceConfig {
    pub fn new(
        name: &'static str,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, MediaError> {
        let base_url = base_url.into().trim().trim_end_matches('/').to_owned();
        let api_key = api_key.into();
        if base_url.is_empty() {
            return Err(MediaError::Config(format!(
                "{}_BASE_URL is required",
                name.to_uppercase()
            )));
        }
        let parsed = reqwest::Url::parse(&base_url).map_err(|_| {
            MediaError::Config(format!("{}_BASE_URL is invalid", name.to_uppercase()))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
            return Err(MediaError::Config(format!(
                "{}_BASE_URL must be an HTTP URL",
                name.to_uppercase()
            )));
        }
        if api_key.trim().is_empty() {
            return Err(MediaError::Config(format!(
                "{}_API_KEY is required",
                name.to_uppercase()
            )));
        }
        Ok(Self {
            name,
            base_url,
            api_key,
        })
    }
}

impl std::fmt::Debug for ServiceConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceConfig")
            .field("name", &self.name)
            .field("base_url", &redacted_url(&self.base_url))
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[derive(Clone)]
pub struct MediaConfig {
    pub jellyseerr: ServiceConfig,
    pub sabnzbd: ServiceConfig,
    pub jellyfin: ServiceConfig,
}

impl MediaConfig {
    pub fn from_env() -> Result<Self, MediaError> {
        Ok(Self {
            jellyseerr: ServiceConfig::new(
                "jellyseerr",
                env::var("JELLYSEERR_BASE_URL").unwrap_or_else(|_| {
                    "http://jellyseerr.jellyseerr.svc.cluster.local:5055".into()
                }),
                env::var("JELLYSEERR_API_KEY").unwrap_or_default(),
            )?,
            sabnzbd: ServiceConfig::new(
                "sabnzbd",
                env::var("SABNZBD_BASE_URL")
                    .unwrap_or_else(|_| "http://sabnzbd.sabnzbd.svc.cluster.local:8080".into()),
                env::var("SABNZBD_API_KEY").unwrap_or_default(),
            )?,
            jellyfin: ServiceConfig::new(
                "jellyfin",
                env::var("JELLYFIN_BASE_URL")
                    .unwrap_or_else(|_| "http://jellyfin.jellyfin.svc.cluster.local:8096".into()),
                env::var("JELLYFIN_API_KEY").unwrap_or_default(),
            )?,
        })
    }
}

impl std::fmt::Debug for MediaConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaConfig")
            .field("jellyseerr", &self.jellyseerr)
            .field("sabnzbd", &self.sabnzbd)
            .field("jellyfin", &self.jellyfin)
            .finish()
    }
}

pub fn build_http_client() -> Result<reqwest::Client, MediaError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| MediaError::Config("failed to build HTTP client".into()))
}

fn redacted_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return "<invalid-url>".into();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string().trim_end_matches('/').to_owned()
}
