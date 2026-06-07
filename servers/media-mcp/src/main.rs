mod tools;

use anyhow::Result;
use rmcp::{
    ServerHandler, tool_handler,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use std::{env, net::SocketAddr, sync::Arc};
use tools::MediaTools;

#[tool_handler(
    name = "media-mcp",
    version = "0.1.0",
    instructions = "Task-oriented media operator for Jellyseerr, SABnzbd, and Jellyfin. \
        Use high-level tools for media requests, download queue control, and Jellyfin library/session operations."
)]
impl ServerHandler for MediaTools {}

#[tokio::main]
async fn main() -> Result<()> {
    homelab_mcp_core::init_tracing_with_service("media-mcp");
    let port: u16 = env::var("PORT").unwrap_or_else(|_| "8080".into()).parse()?;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
            "0.0.0.0".to_string(),
            "media-mcp.hermes.svc.cluster.local".to_string(),
        ]);
    let session_manager = Arc::new(LocalSessionManager::default());
    let service = StreamableHttpService::new(
        || Ok(MediaTools {}),
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
