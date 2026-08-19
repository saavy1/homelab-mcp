mod error;
mod media;

pub use error::ClientError;
pub use media::MediaClient;

use homelab_api_model::{API_MAJOR, Capabilities, OperationEnvelope};
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::OnceCell;
use url::Url;

const REQUEST_ID_HEADER: &str = "x-request-id";
const API_MAJOR_HEADER: &str = "x-homelab-api-major";

pub struct HomelabClient {
    base_url: Url,
    http: reqwest::Client,
    compatible_capabilities: OnceCell<Capabilities>,
}

impl HomelabClient {
    pub fn new(mut base_url: Url, http: reqwest::Client) -> Result<Self, ClientError> {
        if !matches!(base_url.scheme(), "http" | "https") || base_url.cannot_be_a_base() {
            return Err(ClientError::InvalidBaseUrl {
                reason: "an absolute HTTP(S) URL is required",
            });
        }
        if base_url.host_str().is_none() {
            return Err(ClientError::InvalidBaseUrl {
                reason: "a host is required",
            });
        }
        if !base_url.username().is_empty() || base_url.password().is_some() {
            return Err(ClientError::InvalidBaseUrl {
                reason: "embedded credentials are not allowed",
            });
        }
        if base_url.query().is_some() || base_url.fragment().is_some() {
            return Err(ClientError::InvalidBaseUrl {
                reason: "query strings and fragments are not allowed",
            });
        }

        let normalized_path = base_url.path().trim_end_matches('/');
        if !normalized_path.ends_with("/api/v1") {
            return Err(ClientError::InvalidBaseUrl {
                reason: "path must end in /api/v1",
            });
        }
        let mut path = normalized_path.to_owned();
        path.push('/');
        base_url.set_path(&path);

        Ok(Self {
            base_url,
            http,
            compatible_capabilities: OnceCell::new(),
        })
    }

    pub async fn capabilities(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<Capabilities>, ClientError> {
        let envelope = self.fetch_capabilities(request_id).await?;
        let capabilities = envelope.data.as_ref().ok_or_else(|| ClientError::Decode {
            status: StatusCode::OK,
            request_id: Some(envelope.request_id.clone()),
        })?;
        validate_compatible(capabilities)?;
        let _ = self.compatible_capabilities.set(capabilities.clone());
        Ok(envelope)
    }

    pub fn media(&self) -> MediaClient<'_> {
        MediaClient::new(self)
    }

    async fn fetch_capabilities(
        &self,
        request_id: &str,
    ) -> Result<OperationEnvelope<Capabilities>, ClientError> {
        let url = self.route(&["capabilities"])?;
        self.execute(self.http.request(Method::GET, url), request_id)
            .await
    }

    pub(crate) async fn ensure_compatible(&self, request_id: &str) -> Result<(), ClientError> {
        let capabilities = self
            .compatible_capabilities
            .get_or_try_init(|| async {
                let envelope = self.fetch_capabilities(request_id).await?;
                let capabilities = envelope.data.ok_or_else(|| ClientError::Decode {
                    status: StatusCode::OK,
                    request_id: Some(envelope.request_id),
                })?;
                validate_compatible(&capabilities)?;
                Ok::<Capabilities, ClientError>(capabilities)
            })
            .await?;
        validate_compatible(capabilities)
    }

    pub(crate) fn route(&self, segments: &[&str]) -> Result<Url, ClientError> {
        let mut url = self.base_url.clone();
        let mut path = url
            .path_segments_mut()
            .map_err(|_| ClientError::InvalidBaseUrl {
                reason: "URL cannot contain path segments",
            })?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
        drop(path);
        Ok(url)
    }

    pub(crate) async fn execute<T>(
        &self,
        request: RequestBuilder,
        request_id: &str,
    ) -> Result<OperationEnvelope<T>, ClientError>
    where
        T: DeserializeOwned,
    {
        let response = request
            .header(REQUEST_ID_HEADER, request_id)
            .header(API_MAJOR_HEADER, API_MAJOR)
            .send()
            .await
            .map_err(ClientError::Transport)?;
        let status = response.status();
        let response_request_id = response
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = response.bytes().await.map_err(ClientError::Transport)?;

        if status.is_success() {
            serde_json::from_slice(&bytes).map_err(|_| ClientError::Decode {
                status,
                request_id: response_request_id,
            })
        } else {
            let envelope =
                serde_json::from_slice::<OperationEnvelope<Value>>(&bytes).map_err(|_| {
                    ClientError::Decode {
                        status,
                        request_id: response_request_id,
                    }
                })?;
            Err(ClientError::Api {
                status,
                envelope: Box::new(envelope),
            })
        }
    }
}

fn validate_compatible(capabilities: &Capabilities) -> Result<(), ClientError> {
    if capabilities.api.major != API_MAJOR {
        return Err(ClientError::IncompatibleApi {
            expected: API_MAJOR,
            actual: capabilities.api.major,
        });
    }
    if capabilities.compatible_cli_major != API_MAJOR {
        return Err(ClientError::IncompatibleApi {
            expected: API_MAJOR,
            actual: capabilities.compatible_cli_major,
        });
    }
    Ok(())
}
