use crate::{ClientError, HomelabClient};
use homelab_api_model::{
    ActiveSession, CreateMediaRequest, DeleteDownloadQuery, DownloadItem, ItemDetailsQuery,
    LibraryStatus, ListDownloadsQuery, ListRequestsQuery, MediaHealth, MediaOperation, MediaRequest,
    MediaSearchItem, MediaType, OperationEnvelope, SearchMediaQuery,
};
use reqwest::Method;

pub struct MediaClient<'a> {
    client: &'a HomelabClient,
}

impl<'a> MediaClient<'a> {
    pub(crate) fn new(client: &'a HomelabClient) -> Self {
        Self { client }
    }

    pub async fn health(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<MediaHealth>, ClientError> {
        let url = self.client.route(&["health"])?;
        self.client
            .execute(self.client.http.request(Method::GET, url), request_id)
            .await
    }

    pub async fn search(
        &self,
        request_id: &str,
        query: &SearchMediaQuery,
    ) -> Result<OperationEnvelope<Vec<MediaSearchItem>>, ClientError> {
        let mut url = self.client.route(&["media", "search"])?;
        url.query_pairs_mut().append_pair("query", &query.query);
        self.client
            .execute(self.client.http.request(Method::GET, url), request_id)
            .await
    }

    pub async fn item_details(
        &self,
        request_id: &str,
        item_id: &str,
        query: &ItemDetailsQuery,
    ) -> Result<OperationEnvelope<MediaSearchItem>, ClientError> {
        let mut url = self.client.route(&["media", "items", item_id])?;
        url.query_pairs_mut().append_pair(
            "media_type",
            match query.media_type {
                MediaType::Movie => "movie",
                MediaType::Tv => "tv",
            },
        );
        self.client
            .execute(self.client.http.request(Method::GET, url), request_id)
            .await
    }

    pub async fn create_request(
        &self,
        request_id: &str,
        request: &CreateMediaRequest,
    ) -> Result<OperationEnvelope<MediaRequest>, ClientError> {
        self.client.ensure_compatible(request_id).await?;
        let url = self.client.route(&["media", "requests"])?;
        self.client
            .execute(
                self.client.http.request(Method::POST, url).json(request),
                request_id,
            )
            .await
    }

    pub async fn list_requests(
        &self,
        request_id: &str,
        query: &ListRequestsQuery,
    ) -> Result<OperationEnvelope<Vec<MediaRequest>>, ClientError> {
        let mut url = self.client.route(&["media", "requests"])?;
        if let Some(status) = &query.status {
            url.query_pairs_mut().append_pair("status", status);
        }
        self.client
            .execute(self.client.http.request(Method::GET, url), request_id)
            .await
    }

    pub async fn approve_request(
        &self,
        request_id: &str,
        media_request_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, ClientError> {
        self.client.ensure_compatible(request_id).await?;
        let url = self
            .client
            .route(&["media", "requests", media_request_id, "approve"])?;
        self.client
            .execute(self.client.http.request(Method::POST, url), request_id)
            .await
    }

    pub async fn decline_request(
        &self,
        request_id: &str,
        media_request_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, ClientError> {
        self.client.ensure_compatible(request_id).await?;
        let url = self
            .client
            .route(&["media", "requests", media_request_id, "decline"])?;
        self.client
            .execute(self.client.http.request(Method::POST, url), request_id)
            .await
    }

    pub async fn list_downloads(
        &self,
        request_id: &str,
        query: &ListDownloadsQuery,
    ) -> Result<OperationEnvelope<Vec<DownloadItem>>, ClientError> {
        let mut url = self.client.route(&["media", "downloads"])?;
        if let Some(status) = &query.status {
            url.query_pairs_mut().append_pair("status", status);
        }
        self.client
            .execute(self.client.http.request(Method::GET, url), request_id)
            .await
    }

    pub async fn pause_download(
        &self,
        request_id: &str,
        download_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, ClientError> {
        self.client.ensure_compatible(request_id).await?;
        let url = self
            .client
            .route(&["media", "downloads", download_id, "pause"])?;
        self.client
            .execute(self.client.http.request(Method::POST, url), request_id)
            .await
    }

    pub async fn resume_download(
        &self,
        request_id: &str,
        download_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, ClientError> {
        self.client.ensure_compatible(request_id).await?;
        let url = self
            .client
            .route(&["media", "downloads", download_id, "resume"])?;
        self.client
            .execute(self.client.http.request(Method::POST, url), request_id)
            .await
    }

    pub async fn delete_download(
        &self,
        request_id: &str,
        download_id: &str,
        query: &DeleteDownloadQuery,
    ) -> Result<OperationEnvelope<MediaOperation>, ClientError> {
        self.client.ensure_compatible(request_id).await?;
        let mut url = self.client.route(&["media", "downloads", download_id])?;
        url.query_pairs_mut().append_pair(
            "delete_files",
            if query.delete_files { "true" } else { "false" },
        );
        self.client
            .execute(self.client.http.request(Method::DELETE, url), request_id)
            .await
    }

    pub async fn retry_download(
        &self,
        request_id: &str,
        download_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, ClientError> {
        self.client.ensure_compatible(request_id).await?;
        let url = self
            .client
            .route(&["media", "downloads", download_id, "retry"])?;
        self.client
            .execute(self.client.http.request(Method::POST, url), request_id)
            .await
    }

    pub async fn library_status(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<LibraryStatus>, ClientError> {
        let url = self.client.route(&["media", "library", "status"])?;
        self.client
            .execute(self.client.http.request(Method::GET, url), request_id)
            .await
    }

    pub async fn refresh_library(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<MediaOperation>, ClientError> {
        self.client.ensure_compatible(request_id).await?;
        let url = self.client.route(&["media", "library", "refresh"])?;
        self.client
            .execute(self.client.http.request(Method::POST, url), request_id)
            .await
    }

    pub async fn active_sessions(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<Vec<ActiveSession>>, ClientError> {
        let url = self.client.route(&["media", "sessions"])?;
        self.client
            .execute(self.client.http.request(Method::GET, url), request_id)
            .await
    }
}
