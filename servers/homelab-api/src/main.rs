use homelab_api::build_router;
use homelab_core::init_tracing_with_service;
use homelab_media::{MediaConfig, MediaService, build_http_client};
use std::{env, error::Error};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing_with_service("homelab-api");
    let config = MediaConfig::from_env()?;
    let http = build_http_client()?;
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()?;
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    axum::serve(listener, build_router(MediaService::new(config, http))).await?;
    Ok(())
}
