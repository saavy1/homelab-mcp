use crate::error::MediaMcpError;
use std::env;

#[derive(Clone, Debug)]
pub struct ServiceConfig {
    pub name: &'static str,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Clone, Debug)]
pub struct MediaConfig {
    pub jellyseerr: ServiceConfig,
    pub sabnzbd: ServiceConfig,
    pub jellyfin: ServiceConfig,
}

impl ServiceConfig {
    pub fn new(
        name: &'static str,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, MediaMcpError> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let api_key = api_key.into();
        if base_url.trim().is_empty() {
            return Err(MediaMcpError::Config(format!(
                "{}_BASE_URL is required",
                name.to_uppercase()
            )));
        }
        if api_key.trim().is_empty() {
            return Err(MediaMcpError::Config(format!(
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

impl MediaConfig {
    pub fn from_env() -> Result<Self, MediaMcpError> {
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

pub fn redacted_url(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_url_removes_query_values() {
        assert_eq!(
            redacted_url("http://sabnzbd.local/api?apikey=secret&mode=queue"),
            "http://sabnzbd.local/api?<redacted>"
        );
    }

    #[test]
    fn service_config_rejects_blank_api_key() {
        let error = ServiceConfig::new("jellyfin", "http://jellyfin.local", " ").unwrap_err();
        assert!(error.to_string().contains("JELLYFIN_API_KEY is required"));
    }
}
