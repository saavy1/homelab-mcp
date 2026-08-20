use crate::{
    MediaConfig, MediaError,
    clients::{jellyfin::JellyfinClient, jellyseerr::JellyseerrClient, sabnzbd::SabnzbdClient},
};
use homelab_api_model::{
    ActiveSession, BackendHealth, CreateMediaRequest, DownloadItem, HealthStatus, LibraryStatus,
    MediaHealth, MediaOperation, MediaRequest, MediaSearchItem, MediaType, OperationEnvelope,
    RiskLevel,
};
use homelab_core::{ExecutionProvenance, ValidationIssue};

pub struct MediaService {
    jellyseerr: JellyseerrClient,
    sabnzbd: SabnzbdClient,
    jellyfin: JellyfinClient,
}

impl MediaService {
    pub fn new(config: MediaConfig, http: reqwest::Client) -> Self {
        Self {
            jellyseerr: JellyseerrClient::new(http.clone(), config.jellyseerr),
            sabnzbd: SabnzbdClient::new(http.clone(), config.sabnzbd),
            jellyfin: JellyfinClient::new(http, config.jellyfin),
        }
    }

    pub async fn health(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<MediaHealth>, MediaError> {
        let (jellyseerr, sabnzbd, jellyfin) = tokio::join!(
            self.jellyseerr.health(),
            self.sabnzbd.health(),
            self.jellyfin.health()
        );
        let backends = vec![
            backend_health("jellyseerr", jellyseerr),
            backend_health("sabnzbd", sabnzbd),
            backend_health("jellyfin", jellyfin),
        ];
        let failed = backends.iter().filter(|backend| !backend.healthy).count();
        let status = match failed {
            0 => HealthStatus::Healthy,
            3 => HealthStatus::Unavailable,
            _ => HealthStatus::Degraded,
        };
        let issues = backends
            .iter()
            .filter(|backend| !backend.healthy)
            .map(|backend| ValidationIssue {
                field: backend.backend.clone(),
                message: backend
                    .message
                    .clone()
                    .unwrap_or_else(|| "backend unavailable".into()),
                allowed: None,
            })
            .collect();
        let mut envelope = success(
            "media.health",
            request_id,
            RiskLevel::Read,
            match status {
                HealthStatus::Healthy => "all media backends are healthy",
                HealthStatus::Degraded => "one or more media backends are degraded",
                HealthStatus::Unavailable => "all media backends are unavailable",
            },
            MediaHealth { status, backends },
        );
        envelope.issues = issues;
        Ok(envelope)
    }

    pub async fn search(
        &self,
        request_id: &str,
        query: &str,
    ) -> Result<OperationEnvelope<Vec<MediaSearchItem>>, MediaError> {
        let items = self.jellyseerr.search(query).await?;
        Ok(success(
            "media.search",
            request_id,
            RiskLevel::Read,
            "media search completed",
            items,
        ))
    }

    pub async fn create_request(
        &self,
        request_id: &str,
        request: CreateMediaRequest,
    ) -> Result<OperationEnvelope<MediaRequest>, MediaError> {
        let result = self
            .jellyseerr
            .request_media(request.media_type, request.media_id)
            .await?;
        Ok(success(
            "media.requests.create",
            request_id,
            RiskLevel::Write,
            "media request created",
            result,
        ))
    }

    pub async fn list_requests(
        &self,
        request_id: &str,
        status: Option<&str>,
    ) -> Result<OperationEnvelope<Vec<MediaRequest>>, MediaError> {
        let requests = self.jellyseerr.list_requests(status).await?;
        Ok(success(
            "media.requests.list",
            request_id,
            RiskLevel::Read,
            "media requests listed",
            requests,
        ))
    }

    pub async fn approve_request(
        &self,
        request_id: &str,
        media_request_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, MediaError> {
        let operation = self.jellyseerr.approve_request(media_request_id).await?;
        Ok(success(
            "media.requests.approve",
            request_id,
            RiskLevel::Write,
            "media request approved",
            operation,
        ))
    }

    pub async fn decline_request(
        &self,
        request_id: &str,
        media_request_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, MediaError> {
        let operation = self.jellyseerr.decline_request(media_request_id).await?;
        Ok(success(
            "media.requests.decline",
            request_id,
            RiskLevel::Write,
            "media request declined",
            operation,
        ))
    }

    pub async fn list_downloads(
        &self,
        request_id: &str,
        status: Option<&str>,
    ) -> Result<OperationEnvelope<Vec<DownloadItem>>, MediaError> {
        let downloads = self.sabnzbd.list_downloads(status).await?;
        Ok(success(
            "media.downloads.list",
            request_id,
            RiskLevel::Read,
            "media downloads listed",
            downloads,
        ))
    }

    pub async fn pause_download(
        &self,
        request_id: &str,
        download_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, MediaError> {
        let operation = self.sabnzbd.pause_download(download_id).await?;
        Ok(success(
            "media.downloads.pause",
            request_id,
            RiskLevel::Write,
            "media download paused",
            operation,
        ))
    }

    pub async fn resume_download(
        &self,
        request_id: &str,
        download_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, MediaError> {
        let operation = self.sabnzbd.resume_download(download_id).await?;
        Ok(success(
            "media.downloads.resume",
            request_id,
            RiskLevel::Write,
            "media download resumed",
            operation,
        ))
    }

    pub async fn delete_download(
        &self,
        request_id: &str,
        download_id: &str,
        delete_files: bool,
    ) -> Result<OperationEnvelope<MediaOperation>, MediaError> {
        let operation = self
            .sabnzbd
            .delete_download(download_id, delete_files)
            .await?;
        Ok(success(
            "media.downloads.delete",
            request_id,
            if delete_files {
                RiskLevel::Destructive
            } else {
                RiskLevel::Write
            },
            "media download deleted",
            operation,
        ))
    }

    pub async fn retry_download(
        &self,
        request_id: &str,
        download_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, MediaError> {
        let operation = self.sabnzbd.retry_failed_download(download_id).await?;
        Ok(success(
            "media.downloads.retry",
            request_id,
            RiskLevel::Write,
            "media download retried",
            operation,
        ))
    }

    pub async fn library_status(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<LibraryStatus>, MediaError> {
        let status = self.jellyfin.get_library_status().await?;
        Ok(success(
            "media.library.status",
            request_id,
            RiskLevel::Read,
            "media library status read",
            status,
        ))
    }

    pub async fn refresh_library(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, MediaError> {
        let operation = self.jellyfin.refresh_library().await?;
        Ok(success(
            "media.library.refresh",
            request_id,
            RiskLevel::Write,
            "media library refresh started",
            operation,
        ))
    }

    pub async fn active_sessions(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<Vec<ActiveSession>>, MediaError> {
        let sessions = self.jellyfin.get_active_sessions().await?;
        Ok(success(
            "media.sessions.list",
            request_id,
            RiskLevel::Read,
            "active media sessions listed",
            sessions,
        ))
    }

    pub async fn item_details(
        &self,
        request_id: &str,
        item_id: &str,
        media_type: MediaType,
    ) -> Result<OperationEnvelope<MediaSearchItem>, MediaError> {
        let item = self.jellyseerr.item_details(media_type, item_id).await?;
        Ok(success(
            "media.items.show",
            request_id,
            RiskLevel::Read,
            "media item details read",
            item,
        ))
    }
}

fn backend_health(backend: &str, result: Result<(), MediaError>) -> BackendHealth {
    match result {
        Ok(()) => BackendHealth {
            backend: backend.into(),
            healthy: true,
            message: None,
        },
        Err(error) => BackendHealth {
            backend: backend.into(),
            healthy: false,
            message: Some(error.public_message()),
        },
    }
}

fn success<T>(
    operation: &'static str,
    request_id: &str,
    risk: RiskLevel,
    summary: &'static str,
    data: T,
) -> OperationEnvelope<T> {
    OperationEnvelope::success(
        operation,
        request_id,
        risk,
        summary,
        data,
        ExecutionProvenance::service("homelab-media"),
    )
}
