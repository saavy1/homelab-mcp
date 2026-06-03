mod tools;

use anyhow::Result;
use model_catalog::ClusterProfile;
use rmcp::{tool_handler, ServiceExt, ServerHandler, transport::stdio};
use std::{env, path::PathBuf};
use tools::ModelCatalogTools;

#[tool_handler(
    name = "model-catalog-mcp",
    version = "0.1.0",
    instructions = "Imperative model deployer: download weights, validate fit, apply InferenceService, observe status"
)]
impl ServerHandler for ModelCatalogTools {}

#[tokio::main]
async fn main() -> Result<()> {
    homelab_mcp_core::init_tracing();
    let recipe_dir = env::var("MODEL_CATALOG_RECIPE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("crates/model-catalog/tests/fixtures/local-recipes"));
    let service = ModelCatalogTools {
        recipe_dir,
        cluster_profile: ClusterProfile::superbloom_default(),
    }
    .serve(stdio())
    .await?;
    service.waiting().await?;
    Ok(())
}
