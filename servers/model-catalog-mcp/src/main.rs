mod tools;

use anyhow::Result;
use model_catalog::ClusterProfile;
use rmcp::{
    ServerHandler, tool_handler,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use std::{env, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tools::ModelCatalogTools;

#[tool_handler(
    name = "model-catalog-mcp",
    version = "0.1.0",
    instructions = "Imperative model deployer for the Superbloom homelab. \
        Workflow: search_recipes → plan_deploy → ensure_weights → deploy_model → list_deployments. \
        Stable recipes are loaded from mounted YAML files. Runtime recipes and deployments are \
        stored as models.saavylab.dev CRDs and managed through typed MCP tools. \
        Recipe env vars must be {name, value} objects, not KEY=VALUE strings."
)]
impl ServerHandler for ModelCatalogTools {}

#[tokio::main]
async fn main() -> Result<()> {
    homelab_mcp_core::init_tracing_with_service("model-catalog-mcp");
    let recipe_dir = env::var("MODEL_CATALOG_RECIPE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/model-catalog/recipes"));
    let spark_arena_dir = env::var("MODEL_CATALOG_SPARK_ARENA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/model-catalog/spark-arena"));
    let runtime_state_namespace =
        env::var("MODEL_CATALOG_STATE_NAMESPACE").unwrap_or_else(|_| "hermes".into());
    let reconciler_namespace = runtime_state_namespace.clone();
    match homelab_mcp_k8s::k8s_client().await {
        Ok(client) => {
            tokio::spawn(homelab_mcp_k8s::run_model_deployment_reconciler(
                client,
                reconciler_namespace,
                Duration::from_secs(15),
            ));
        }
        Err(error) => {
            tracing::warn!(%error, "model deployment reconciler disabled: Kubernetes client unavailable");
        }
    }
    let prometheus_base_url = env::var("PROMETHEUS_BASE_URL").ok();
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
            "model-catalog-mcp.hermes.svc.cluster.local".to_string(),
        ]);
    let session_manager = Arc::new(LocalSessionManager::default());
    let service = StreamableHttpService::new(
        move || {
            Ok(ModelCatalogTools {
                recipe_dir: recipe_dir.clone(),
                spark_arena_dir: spark_arena_dir.clone(),
                runtime_state_namespace: runtime_state_namespace.clone(),
                cluster_profile: ClusterProfile::superbloom_default(),
                prometheus_base_url: prometheus_base_url.clone(),
            })
        },
        session_manager,
        config,
    );

    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .fallback_service(service);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("model-catalog-mcp listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
