mod args;
mod render;

use args::{
    Cli, Command, DownloadsCommand, ItemCommand, LibraryCommand, MediaCommand, OutputFormat,
    RequestCommand, RequestsCommand, SessionsCommand,
};
use clap::{Parser, error::ErrorKind};
use homelab_api_model::{
    CreateMediaRequest, DeleteDownloadQuery, HealthStatus, ListDownloadsQuery, ListRequestsQuery,
    MediaHealth, OperationEnvelope, SearchMediaQuery,
};
use homelab_client::{ClientError, HomelabClient};
use serde::Serialize;
use serde_json::{Value, json};
use std::{env, process, time::Duration};
use url::Url;
use uuid::Uuid;

const EXIT_SUCCESS: i32 = 0;
const EXIT_INTERNAL: i32 = 1;
const EXIT_INVALID_INPUT: i32 = 2;
const EXIT_FORBIDDEN: i32 = 3;
const EXIT_CONFLICT: i32 = 4;
const EXIT_UNAVAILABLE: i32 = 5;
const EXIT_PARTIAL: i32 = 6;

#[tokio::main]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(error.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) =>
        {
            let code = if error.print().is_ok() {
                EXIT_SUCCESS
            } else {
                EXIT_INTERNAL
            };
            process::exit(code);
        }
        Err(_) => {
            let request_id = Uuid::new_v4().to_string();
            let value = local_error(
                "cli.parse",
                &request_id,
                "read",
                "validation",
                "invalid command arguments",
                false,
            );
            let code = if render::structured(&value, OutputFormat::Json).is_ok() {
                EXIT_INVALID_INPUT
            } else {
                EXIT_INTERNAL
            };
            process::exit(code);
        }
    };

    let request_id = cli
        .request_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let (operation, risk) = cli.command.operation_and_risk();
    let base_url = match configured_base_url() {
        Ok(url) => url,
        Err(message) => {
            let value = local_error(
                operation,
                &request_id,
                risk,
                "validation",
                message,
                false,
            );
            let code = if render::structured(&value, cli.output).is_ok() {
                EXIT_INVALID_INPUT
            } else {
                EXIT_INTERNAL
            };
            process::exit(code);
        }
    };

    let http = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(http) => http,
        Err(_) => {
            let value = local_error(
                operation,
                &request_id,
                risk,
                "internal",
                "homelab HTTP client could not be initialized",
                false,
            );
            let _ = render::structured(&value, cli.output);
            process::exit(EXIT_INTERNAL);
        }
    };
    let client = match HomelabClient::new(base_url, http) {
        Ok(client) => client,
        Err(error) => {
            let code = finish_error(error, operation, risk, &request_id, cli.output);
            process::exit(code);
        }
    };

    let code = dispatch(&client, cli.command, &request_id, cli.output).await;
    process::exit(code);
}

fn configured_base_url() -> Result<Url, &'static str> {
    let value = env::var_os("HOMELAB_API_URL").ok_or("HOMELAB_API_URL is required")?;
    let value = value
        .into_string()
        .map_err(|_| "HOMELAB_API_URL must be valid Unicode")?;
    Url::parse(&value).map_err(|_| "HOMELAB_API_URL must be an absolute HTTP(S) URL ending in /api/v1")
}

async fn dispatch(
    client: &HomelabClient,
    command: Command,
    request_id: &str,
    output: OutputFormat,
) -> i32 {
    match command {
        Command::Capabilities => complete(
            client.capabilities(request_id).await,
            "capabilities.show",
            "read",
            request_id,
            output,
        ),
        Command::Media { command } => match command {
            MediaCommand::Health => complete_health(
                client.media().health(request_id).await,
                request_id,
                output,
            ),
            MediaCommand::Search { query } => complete(
                client
                    .media()
                    .search(request_id, &SearchMediaQuery { query })
                    .await,
                "media.search",
                "read",
                request_id,
                output,
            ),
            MediaCommand::Item { command } => match command {
                ItemCommand::Show { item_id } => complete(
                    client.media().item_details(request_id, &item_id).await,
                    "media.items.show",
                    "read",
                    request_id,
                    output,
                ),
            },
            MediaCommand::Request { command } => match command {
                RequestCommand::Create {
                    media_id,
                    media_type,
                } => complete(
                    client
                        .media()
                        .create_request(
                            request_id,
                            &CreateMediaRequest {
                                media_id,
                                media_type: media_type.into(),
                            },
                        )
                        .await,
                    "media.requests.create",
                    "write",
                    request_id,
                    output,
                ),
            },
            MediaCommand::Requests { command } => match command {
                RequestsCommand::List { status } => complete(
                    client
                        .media()
                        .list_requests(request_id, &ListRequestsQuery { status })
                        .await,
                    "media.requests.list",
                    "read",
                    request_id,
                    output,
                ),
                RequestsCommand::Approve {
                    request_id: media_request_id,
                } => complete(
                    client
                        .media()
                        .approve_request(request_id, &media_request_id)
                        .await,
                    "media.requests.approve",
                    "write",
                    request_id,
                    output,
                ),
                RequestsCommand::Decline {
                    request_id: media_request_id,
                } => complete(
                    client
                        .media()
                        .decline_request(request_id, &media_request_id)
                        .await,
                    "media.requests.decline",
                    "write",
                    request_id,
                    output,
                ),
            },
            MediaCommand::Downloads { command } => match command {
                DownloadsCommand::List { status } => complete(
                    client
                        .media()
                        .list_downloads(request_id, &ListDownloadsQuery { status })
                        .await,
                    "media.downloads.list",
                    "read",
                    request_id,
                    output,
                ),
                DownloadsCommand::Pause { download_id } => complete(
                    client.media().pause_download(request_id, &download_id).await,
                    "media.downloads.pause",
                    "write",
                    request_id,
                    output,
                ),
                DownloadsCommand::Resume { download_id } => complete(
                    client.media().resume_download(request_id, &download_id).await,
                    "media.downloads.resume",
                    "write",
                    request_id,
                    output,
                ),
                DownloadsCommand::Delete {
                    download_id,
                    delete_files,
                } => complete(
                    client
                        .media()
                        .delete_download(
                            request_id,
                            &download_id,
                            &DeleteDownloadQuery { delete_files },
                        )
                        .await,
                    "media.downloads.delete",
                    if delete_files { "destructive" } else { "write" },
                    request_id,
                    output,
                ),
                DownloadsCommand::Retry { download_id } => complete(
                    client.media().retry_download(request_id, &download_id).await,
                    "media.downloads.retry",
                    "write",
                    request_id,
                    output,
                ),
            },
            MediaCommand::Library { command } => match command {
                LibraryCommand::Status => complete(
                    client.media().library_status(request_id).await,
                    "media.library.status",
                    "read",
                    request_id,
                    output,
                ),
                LibraryCommand::Refresh => complete(
                    client.media().refresh_library(request_id).await,
                    "media.library.refresh",
                    "write",
                    request_id,
                    output,
                ),
            },
            MediaCommand::Sessions { command } => match command {
                SessionsCommand::List => complete(
                    client.media().active_sessions(request_id).await,
                    "media.sessions.list",
                    "read",
                    request_id,
                    output,
                ),
            },
        },
    }
}

fn complete<T: Serialize>(
    result: Result<OperationEnvelope<T>, ClientError>,
    operation: &str,
    risk: &str,
    request_id: &str,
    output: OutputFormat,
) -> i32 {
    match result {
        Ok(envelope) => {
            let code = success_exit(&envelope);
            if render::envelope(&envelope, output).is_ok() {
                code
            } else {
                EXIT_INTERNAL
            }
        }
        Err(error) => finish_error(error, operation, risk, request_id, output),
    }
}

fn complete_health(
    result: Result<OperationEnvelope<MediaHealth>, ClientError>,
    request_id: &str,
    output: OutputFormat,
) -> i32 {
    match result {
        Ok(envelope) => {
            let code = if !envelope.ok {
                envelope_error_exit(&envelope)
            } else {
                match envelope.data.as_ref().map(|health| health.status) {
                    Some(HealthStatus::Healthy) | None => success_exit(&envelope),
                    Some(HealthStatus::Degraded) => EXIT_PARTIAL,
                    Some(HealthStatus::Unavailable) => EXIT_UNAVAILABLE,
                }
            };
            if render::envelope(&envelope, output).is_ok() {
                code
            } else {
                EXIT_INTERNAL
            }
        }
        Err(error) => finish_error(error, "media.health", "read", request_id, output),
    }
}

fn success_exit<T: Serialize>(envelope: &OperationEnvelope<T>) -> i32 {
    if !envelope.ok {
        envelope_error_exit(envelope)
    } else if envelope.issues.is_empty() {
        EXIT_SUCCESS
    } else {
        EXIT_PARTIAL
    }
}

fn envelope_error_exit<T>(envelope: &OperationEnvelope<T>) -> i32 {
    let code = envelope
        .error
        .as_ref()
        .and_then(|error| serde_json::to_value(&error.code).ok())
        .and_then(|value| value.as_str().map(str::to_owned));
    error_code_exit(code.as_deref())
}

fn error_code_exit(code: Option<&str>) -> i32 {
    match code {
        Some("validation") => EXIT_INVALID_INPUT,
        Some("forbidden") => EXIT_FORBIDDEN,
        Some("not_found" | "conflict") => EXIT_CONFLICT,
        Some("unavailable" | "timeout" | "unknown_outcome") => EXIT_UNAVAILABLE,
        Some("internal") | None | Some(_) => EXIT_INTERNAL,
    }
}

fn finish_error(
    error: ClientError,
    operation: &str,
    risk: &str,
    request_id: &str,
    output: OutputFormat,
) -> i32 {
    match error {
        ClientError::Api { envelope, .. } => {
            let code = envelope_error_exit(&envelope);
            if render::envelope(&envelope, output).is_ok() {
                code
            } else {
                EXIT_INTERNAL
            }
        }
        ClientError::InvalidBaseUrl { .. } => render_local_error(
            operation,
            request_id,
            risk,
            "validation",
            "HOMELAB_API_URL must be an absolute HTTP(S) URL ending in /api/v1",
            false,
            output,
        ),
        ClientError::IncompatibleApi { .. } => render_local_error(
            operation,
            request_id,
            risk,
            "conflict",
            "homelab API is incompatible with this CLI",
            false,
            output,
        ),
        ClientError::Transport(error) => {
            let timeout = error.is_timeout();
            render_local_error(
                operation,
                request_id,
                risk,
                if timeout { "timeout" } else { "unavailable" },
                if timeout {
                    "homelab API request timed out"
                } else {
                    "homelab API is unavailable"
                },
                true,
                output,
            )
        }
        ClientError::Decode {
            request_id: response_request_id,
            ..
        } => render_local_error(
            operation,
            response_request_id.as_deref().unwrap_or(request_id),
            risk,
            "internal",
            "homelab API returned an invalid response",
            false,
            output,
        ),
    }
}

fn render_local_error(
    operation: &str,
    request_id: &str,
    risk: &str,
    code: &str,
    message: &str,
    retryable: bool,
    output: OutputFormat,
) -> i32 {
    let value = local_error(operation, request_id, risk, code, message, retryable);
    if render::structured(&value, output).is_ok() {
        error_code_exit(Some(code))
    } else {
        EXIT_INTERNAL
    }
}

fn local_error(
    operation: &str,
    request_id: &str,
    risk: &str,
    code: &str,
    message: &str,
    retryable: bool,
) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "request_id": request_id,
        "risk": risk,
        "summary": {"text": message},
        "error": {"code": code, "message": message, "retryable": retryable},
        "provenance": {"service": "homelab-cli", "timestamp": chrono::Utc::now()}
    })
}
