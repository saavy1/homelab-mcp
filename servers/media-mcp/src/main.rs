use anyhow::Result;
use media_mcp::{config, tools::MediaTools};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::{env, net::SocketAddr, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    homelab_mcp_core::init_tracing_with_service("media-mcp");
    let port: u16 = env::var("PORT").unwrap_or_else(|_| "8080".into()).parse()?;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(config::mcp_allowed_hosts_from_env());
    let session_manager = Arc::new(LocalSessionManager::default());
    let media_config = config::MediaConfig::from_env()?;
    let service = StreamableHttpService::new(
        move || {
            Ok(MediaTools::new(
                media_config.clone(),
                reqwest::Client::new(),
            ))
        },
        session_manager,
        config,
    );

    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .fallback_service(service);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "media-mcp listening");
    axum::serve(listener, app).await?;
    Ok(())
}
