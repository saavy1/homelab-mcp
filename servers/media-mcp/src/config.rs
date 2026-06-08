use crate::error::MediaMcpError;
use std::env;

#[allow(dead_code)]
#[derive(Clone)]
pub struct ServiceConfig {
    pub name: &'static str,
    pub base_url: String,
    pub api_key: String,
}

impl std::fmt::Debug for ServiceConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceConfig")
            .field("name", &self.name)
            .field("base_url", &redacted_url(&self.base_url))
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct MediaConfig {
    pub jellyseerr: ServiceConfig,
    pub sabnzbd: ServiceConfig,
    pub jellyfin: ServiceConfig,
}

impl std::fmt::Debug for MediaConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaConfig")
            .field("jellyseerr", &self.jellyseerr)
            .field("sabnzbd", &self.sabnzbd)
            .field("jellyfin", &self.jellyfin)
            .finish()
    }
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

#[allow(dead_code)]
pub fn redacted_url(url: &str) -> String {
    let mut redacted = url.to_string();

    // Redact userinfo credentials such as https://user:pass@host/path
    if let Some(scheme_end) = redacted.find("://") {
        let after_scheme = scheme_end + 3;
        let rest = &redacted[after_scheme..];
        let authority_end = rest.find(&['/', '?', '#'][..]).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        if let Some(at_pos) = authority.find('@') {
            redacted.replace_range(after_scheme..after_scheme + at_pos, "<redacted>");
        }
    }

    // Redact query string
    if let Some(pos) = redacted.find('?') {
        format!("{}?<redacted>", &redacted[..pos])
    } else {
        redacted
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
    fn redacted_url_removes_userinfo() {
        assert_eq!(
            redacted_url("https://user:pass@example.test/path"),
            "https://<redacted>@example.test/path"
        );
    }

    #[test]
    fn redacted_url_removes_userinfo_and_query_string() {
        assert_eq!(
            redacted_url("https://user:pass@example.test/path?query=secret"),
            "https://<redacted>@example.test/path?<redacted>"
        );
    }

    #[test]
    fn service_config_rejects_blank_api_key() {
        let error = ServiceConfig::new("jellyfin", "http://jellyfin.local", " ").unwrap_err();
        assert!(error.to_string().contains("JELLYFIN_API_KEY is required"));
    }

    #[test]
    fn service_config_debug_redacts_api_key() {
        let config = ServiceConfig::new("sabnzbd", "http://sabnzbd.local", "super-secret").unwrap();
        let debug = format!("{config:?}");
        assert!(debug.contains("http://sabnzbd.local"));
        assert!(debug.contains("name: \"sabnzbd\""));
        assert!(debug.contains("api_key: \"<redacted>\""));
        assert!(!debug.contains("super-secret"));
    }

    #[test]
    fn service_config_debug_redacts_userinfo() {
        let config = ServiceConfig::new(
            "sabnzbd",
            "https://user:pass@example.test/path",
            "super-secret",
        )
        .unwrap();
        let debug = format!("{config:?}");
        assert!(!debug.contains("user:pass"));
        assert!(debug.contains("base_url: \"https://<redacted>@example.test/path\""));
    }

    #[test]
    fn media_config_debug_redacts_all_api_keys() {
        let config = MediaConfig {
            jellyseerr: ServiceConfig::new("jellyseerr", "http://jellyseerr.local", "secret1")
                .unwrap(),
            sabnzbd: ServiceConfig::new("sabnzbd", "http://sabnzbd.local", "secret2").unwrap(),
            jellyfin: ServiceConfig::new("jellyfin", "http://jellyfin.local", "secret3").unwrap(),
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret1"));
        assert!(!debug.contains("secret2"));
        assert!(!debug.contains("secret3"));
        assert!(debug.contains("api_key: \"<redacted>\""));
    }
}
