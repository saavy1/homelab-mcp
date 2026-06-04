use homelab_mcp_core::compute_digest;
use homelab_mcp_k8s::{
    DownloadJobRef, DownloadJobSpec, DownloadStatus, build_download_job, create_download_job,
    create_inferenceservice, download_job_name, get_download_status, get_inferenceservice_status,
    get_predictor_logs,
};
use model_catalog::{
    ClusterProfile, DeployOverrides, DeploymentPlan, Recipe, load_recipe_dir, plan_deploy,
    plan_to_digest_input, render_kserve_value, search_recipes,
};
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::{info, instrument};

#[derive(Clone)]
pub struct ModelCatalogTools {
    pub recipe_dir: PathBuf,
    pub cluster_profile: ClusterProfile,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchRecipesParams {
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ShowRecipeParams {
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlanDeployParams {
    pub recipe_id: String,
    pub name: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EnsureWeightsParams {
    pub recipe_id: String,
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub plan_digest: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DownloadStatusParams {
    pub job_ref: DownloadJobRef,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ApplyPlanParams {
    pub recipe_id: String,
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub plan_digest: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ModelStatusParams {
    pub namespace: String,
    pub name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ModelLogsParams {
    pub namespace: String,
    pub name: String,
    pub tail: Option<usize>,
}

fn verify_digest(plan: &DeploymentPlan, provided_digest: &str) -> Result<(), String> {
    let expected = compute_digest(&plan_to_digest_input(plan));
    if expected != provided_digest {
        return Err(format!(
            "digest mismatch: expected {}, got {}",
            expected, provided_digest
        ));
    }
    Ok(())
}

#[tool_router(vis = "pub")]
impl ModelCatalogTools {
    #[tool(description = "Search local model recipes by recipe id or model id")]
    pub fn search_recipes(
        &self,
        Parameters(params): Parameters<SearchRecipesParams>,
    ) -> Result<String, String> {
        let recipes = self.load_recipes().map_err(|error| error.to_string())?;
        let matches = search_recipes(&recipes, params.query.as_deref());
        let ids: Vec<String> = matches
            .into_iter()
            .map(|recipe| recipe.id.clone())
            .collect();
        info!(count = ids.len(), "search_recipes");
        serde_json::to_string(&ids).map_err(|error| error.to_string())
    }

    #[tool(description = "Show one local model recipe by id")]
    pub fn show_recipe(
        &self,
        Parameters(params): Parameters<ShowRecipeParams>,
    ) -> Result<String, String> {
        let recipe = self.find_recipe(&params.id)?;
        info!(recipe_id = %params.id, "show_recipe");
        serde_json::to_string(&recipe).map_err(|error| error.to_string())
    }

    #[tool(
        description = "Plan a KServe deployment. Returns DeploymentPlan with plan_digest. Pure: no side effects."
    )]
    pub fn plan_deploy(
        &self,
        Parameters(params): Parameters<PlanDeployParams>,
    ) -> Result<String, String> {
        let recipe = self.find_recipe(&params.recipe_id)?;
        let result = plan_deploy(
            &recipe,
            &self.cluster_profile,
            DeployOverrides {
                name: params.name,
                namespace: params.namespace,
                replicas: None,
                env_overrides: Vec::new(),
            },
        );
        info!(recipe_id = %params.recipe_id, risk = ?result.risk, "plan_deploy");
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    #[tool(
        description = "Download model weights on NAS node. Creates a K8s Job if sentinel absent. Cluster write + NAS filesystem write."
    )]
    #[instrument(skip(self, params), fields(recipe_id = %params.recipe_id))]
    pub async fn ensure_weights(
        &self,
        Parameters(params): Parameters<EnsureWeightsParams>,
    ) -> Result<String, String> {
        let plan = self.derive_plan(&params.recipe_id, params.name, params.namespace)?;
        verify_digest(&plan, &params.plan_digest)?;
        let storage = &self.cluster_profile.model_storage;
        let revision = plan
            .model_revision
            .clone()
            .unwrap_or_else(|| "main".into());
        let job_name = download_job_name(&plan.model_id, &revision);

        // Check if already running/completed
        let job_ref = DownloadJobRef {
            job_name: job_name.clone(),
            namespace: storage.hf_secret_namespace.clone(),
            model_id: plan.model_id.clone(),
        };
        match get_download_status(&job_ref).await {
            Ok(DownloadStatus::Completed { .. }) => {
                info!(model_id = %plan.model_id, "weights already downloaded");
                let response = serde_json::json!({
                    "action": "already_complete",
                    "job_ref": job_ref,
                    "model_id": plan.model_id,
                    "status": "completed"
                });
                return serde_json::to_string(&response).map_err(|e| e.to_string());
            }
            Ok(DownloadStatus::Running { .. }) => {
                info!(model_id = %plan.model_id, "download already running");
                let response = serde_json::json!({
                    "action": "already_running",
                    "job_ref": job_ref,
                    "model_id": plan.model_id,
                    "status": "running"
                });
                return serde_json::to_string(&response).map_err(|e| e.to_string());
            }
            _ => {} // NotStarted or Failed — proceed to create
        }

        let spec = DownloadJobSpec {
            model_id: plan.model_id.clone(),
            revision: revision.clone(),
            nas_path: storage.nas_path.clone(),
            download_node_selector: storage.download_node_selector.clone(),
            hf_secret_name: storage.hf_secret_name.clone(),
            hf_secret_namespace: storage.hf_secret_namespace.clone(),
        };
        let job = build_download_job(&spec);
        let created_name = create_download_job(&job, &storage.hf_secret_namespace)
            .await
            .map_err(|e| format!("create download job: {e}"))?;
        info!(job_name = %created_name, model_id = %plan.model_id, "created download job");
        let response = serde_json::json!({
            "action": "created_download_job",
            "job_ref": job_ref,
            "model_id": plan.model_id,
            "nas_node": storage.download_node_selector,
            "local_dir": format!("{}/{}", storage.nas_path, plan.model_id),
        });
        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    #[tool(description = "Check the status of a weight download job by job reference")]
    #[instrument(skip(self, params), fields(job_name = %params.job_ref.job_name))]
    pub async fn download_status(
        &self,
        Parameters(params): Parameters<DownloadStatusParams>,
    ) -> Result<String, String> {
        let status = get_download_status(&params.job_ref)
            .await
            .map_err(|e| format!("get download status: {e}"))?;
        info!(job_name = %params.job_ref.job_name, status = ?status, "download_status");
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    #[tool(
        description = "Apply a KServe InferenceService to the cluster. Default create_only. Cluster write. Refuses if sentinel absent."
    )]
    #[instrument(skip(self, params), fields(recipe_id = %params.recipe_id))]
    pub async fn apply_plan(
        &self,
        Parameters(params): Parameters<ApplyPlanParams>,
    ) -> Result<String, String> {
        let plan = self.derive_plan(&params.recipe_id, params.name, params.namespace)?;
        verify_digest(&plan, &params.plan_digest)?;

        // Sentinel check: verify download completed
        let job_ref = DownloadJobRef {
            job_name: download_job_name(
                &plan.model_id,
                &plan
                    .model_revision
                    .clone()
                    .unwrap_or_else(|| "main".into()),
            ),
            namespace: self
                .cluster_profile
                .model_storage
                .hf_secret_namespace
                .clone(),
            model_id: plan.model_id.clone(),
        };
        let dl_status = get_download_status(&job_ref)
            .await
            .map_err(|e| format!("sentinel check: {e}"))?;
        if !matches!(dl_status, DownloadStatus::Completed { .. }) {
            return Err(format!(
                "weights not ready: download status is {:?}. Run ensure_weights first.",
                dl_status
            ));
        }

        let value = render_kserve_value(&plan);
        let created = create_inferenceservice(value, &plan.namespace)
            .await
            .map_err(|e| format!("create InferenceService: {e}"))?;
        info!(name = %plan.name, namespace = %plan.namespace, "applied InferenceService");
        let response = serde_json::json!({
            "action": "created_inferenceservice",
            "name": plan.name,
            "namespace": plan.namespace,
            "mode": "CreateOnly",
            "risk": "cluster-write",
            "created_name": created,
        });
        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    #[tool(description = "Return KServe model status from Kubernetes")]
    #[instrument(skip(self, params), fields(name = %params.name, namespace = %params.namespace))]
    pub async fn status(
        &self,
        Parameters(params): Parameters<ModelStatusParams>,
    ) -> Result<String, String> {
        let status = get_inferenceservice_status(&params.namespace, &params.name)
            .await
            .map_err(|e| e.to_string())?;
        info!(name = %params.name, namespace = %params.namespace, ready = status.ready, "status");
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    #[tool(description = "Return recent KServe model logs from Kubernetes")]
    #[instrument(skip(self, params), fields(name = %params.name, namespace = %params.namespace))]
    pub async fn logs(
        &self,
        Parameters(params): Parameters<ModelLogsParams>,
    ) -> Result<String, String> {
        let tail = params.tail.unwrap_or(100);
        let logs = get_predictor_logs(&params.namespace, &params.name, tail)
            .await
            .map_err(|e| e.to_string())?;
        info!(name = %params.name, namespace = %params.namespace, line_count = logs.lines.len(), "logs");
        serde_json::to_string(&logs).map_err(|e| e.to_string())
    }
}

impl ModelCatalogTools {
    fn derive_plan(
        &self,
        recipe_id: &str,
        name: Option<String>,
        namespace: Option<String>,
    ) -> Result<DeploymentPlan, String> {
        let recipe = self.find_recipe(recipe_id)?;
        let result = plan_deploy(
            &recipe,
            &self.cluster_profile,
            DeployOverrides {
                name,
                namespace,
                replicas: None,
                env_overrides: Vec::new(),
            },
        );
        if !result.issues.is_empty() {
            return Err(serde_json::to_string(&result.issues).map_err(|error| error.to_string())?);
        }
        Ok(result.data)
    }

    fn load_recipes(&self) -> Result<Vec<Recipe>, String> {
        load_recipe_dir(&self.recipe_dir).map_err(|error| error.to_string())
    }

    fn find_recipe(&self, id: &str) -> Result<Recipe, String> {
        self.load_recipes()?
            .into_iter()
            .find(|recipe| recipe.id == id)
            .ok_or_else(|| format!("recipe not found: {id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> ModelCatalogTools {
        ModelCatalogTools {
            recipe_dir: PathBuf::from("../../crates/model-catalog/tests/fixtures/local-recipes"),
            cluster_profile: ClusterProfile::superbloom_default(),
        }
    }

    #[test]
    fn search_recipes_returns_known_fixture() {
        let output = tools()
            .search_recipes(Parameters(SearchRecipesParams {
                query: Some("qwen".into()),
            }))
            .expect("search");
        assert!(output.contains("qwen3-8b"));
    }

    #[test]
    fn plan_deploy_returns_plan_with_digest() {
        let output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        assert!(output.contains("fits cluster superbloom"));
        assert!(output.contains("plan_digest"));
    }

    #[tokio::test]
    async fn ensure_weights_accepts_valid_digest() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        let plan_value: serde_json::Value = serde_json::from_str(&plan_output).expect("parse plan");
        let digest = plan_value["data"]["plan_digest"]
            .as_str()
            .expect("digest")
            .to_string();
        let result = tools()
            .ensure_weights(Parameters(EnsureWeightsParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
                plan_digest: digest,
            }))
            .await;
        // Without a cluster the kube API call fails, but it must NOT be a digest error
        match result {
            Ok(s) => assert!(s.contains("created_download_job") || s.contains("already")),
            Err(e) => assert!(!e.contains("digest mismatch"), "unexpected digest error: {e}"),
        }
    }

    #[tokio::test]
    async fn ensure_weights_rejects_wrong_digest() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        assert!(plan_output.contains("plan_digest"));
        let result = tools()
            .ensure_weights(Parameters(EnsureWeightsParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
                plan_digest: "wrong-digest".into(),
            }))
            .await;
        let err = result.expect_err("should reject wrong digest");
        assert!(err.contains("digest mismatch"), "expected digest mismatch, got: {err}");
    }

    #[tokio::test]
    async fn apply_plan_refuses_with_wrong_digest() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        assert!(plan_output.contains("plan_digest"));
        let result = tools()
            .apply_plan(Parameters(ApplyPlanParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
                plan_digest: "wrong-digest".into(),
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("digest mismatch"));
    }
}
