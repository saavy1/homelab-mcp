use homelab_mcp_core::compute_digest;
use homelab_mcp_k8s::{DownloadJobRef, DownloadJobSpec, build_download_job, download_job_name};
use model_catalog::{
    ApplyMode, ClusterProfile, DeployOverrides, DeploymentPlan, Recipe, load_recipe_dir,
    plan_deploy, render_kserve_yaml, search_recipes,
};
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;
use std::path::PathBuf;

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
    pub plan: DeploymentPlan,
    pub plan_digest: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DownloadStatusParams {
    pub job_ref: DownloadJobRef,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ApplyPlanParams {
    pub plan: DeploymentPlan,
    pub plan_digest: String,
    pub mode: Option<ApplyMode>,
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
    let mut plan_value = serde_json::to_value(plan).map_err(|e| e.to_string())?;
    if let serde_json::Value::Object(map) = &mut plan_value {
        map.remove("plan_digest");
    }
    let canonical = serde_json::to_string(&plan_value).map_err(|e| e.to_string())?;
    let expected = compute_digest(&canonical);
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
        serde_json::to_string(&ids).map_err(|error| error.to_string())
    }

    #[tool(description = "Show one local model recipe by id")]
    pub fn show_recipe(
        &self,
        Parameters(params): Parameters<ShowRecipeParams>,
    ) -> Result<String, String> {
        let recipe = self.find_recipe(&params.id)?;
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
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    #[tool(
        description = "Download model weights on NAS node if sentinel absent. Cluster write + NAS filesystem write."
    )]
    pub fn ensure_weights(
        &self,
        Parameters(params): Parameters<EnsureWeightsParams>,
    ) -> Result<String, String> {
        verify_digest(&params.plan, &params.plan_digest)?;
        let storage = &self.cluster_profile.model_storage;
        let revision = params
            .plan
            .model_revision
            .clone()
            .unwrap_or_else(|| "main".into());
        let spec = DownloadJobSpec {
            model_id: params.plan.model_id.clone(),
            revision: revision.clone(),
            nas_path: storage.nas_path.clone(),
            download_node_selector: storage.download_node_selector.clone(),
            hf_secret_name: storage.hf_secret_name.clone(),
            hf_secret_namespace: storage.hf_secret_namespace.clone(),
        };
        let job = build_download_job(&spec);
        let job_ref = DownloadJobRef {
            job_name: download_job_name(&params.plan.model_id, &revision),
            namespace: storage.hf_secret_namespace.clone(),
            model_id: params.plan.model_id.clone(),
        };
        let response = serde_json::json!({
            "action": "would create download job",
            "job_ref": job_ref,
            "model_id": params.plan.model_id,
            "nas_node": storage.download_node_selector,
            "local_dir": format!("{}/{}", storage.nas_path, params.plan.model_id),
            "sentinel_path": format!("{}/{}/.homelab-mcp-download.json", storage.nas_path, params.plan.model_id),
            "job_manifest": serde_json::to_string_pretty(&job).map_err(|e| e.to_string())?,
            "note": "kube-rs apply will be wired in the live server."
        });
        serde_json::to_string(&response).map_err(|error| error.to_string())
    }

    #[tool(description = "Check the status of a weight download job by job reference")]
    pub fn download_status(
        &self,
        Parameters(params): Parameters<DownloadStatusParams>,
    ) -> Result<String, String> {
        let response = serde_json::json!({
            "job_ref": params.job_ref,
            "status": "kube-rs job status polling will be wired in the live server",
            "note": "Returns job conditions, pod phase, and sentinel check."
        });
        serde_json::to_string(&response).map_err(|error| error.to_string())
    }

    #[tool(
        description = "Apply a KServe InferenceService to the cluster. Default create_only. Cluster write. Refuses if sentinel absent."
    )]
    pub fn apply_plan(
        &self,
        Parameters(params): Parameters<ApplyPlanParams>,
    ) -> Result<String, String> {
        verify_digest(&params.plan, &params.plan_digest)?;
        let mode = params.mode.unwrap_or_default();
        let yaml = render_kserve_yaml(&params.plan).map_err(|error| error.to_string())?;
        let response = serde_json::json!({
            "action": "would apply InferenceService",
            "name": params.plan.name,
            "namespace": params.plan.namespace,
            "mode": format!("{:?}", mode),
            "risk": "cluster-write",
            "sentinel_check": format!(
                "would verify /tank/models/{}/.homelab-mcp-download.json exists and complete=true",
                params.plan.model_id
            ),
            "manifest": yaml,
            "note": "kube-rs apply will be wired in the live server."
        });
        serde_json::to_string(&response).map_err(|error| error.to_string())
    }

    #[tool(description = "Return KServe model status from Kubernetes")]
    pub fn status(
        &self,
        Parameters(params): Parameters<ModelStatusParams>,
    ) -> Result<String, String> {
        let status = serde_json::json!({
            "namespace": params.namespace,
            "name": params.name,
            "ready": false,
            "conditions": [],
            "recent_events": ["kube-rs live status reader is wired in homelab-mcp-k8s"]
        });
        serde_json::to_string(&status).map_err(|error| error.to_string())
    }

    #[tool(description = "Return recent KServe model logs from Kubernetes")]
    pub fn logs(&self, Parameters(params): Parameters<ModelLogsParams>) -> Result<String, String> {
        let logs = serde_json::json!({
            "namespace": params.namespace,
            "name": params.name,
            "tail": params.tail.unwrap_or(100),
            "lines": []
        });
        serde_json::to_string(&logs).map_err(|error| error.to_string())
    }
}

impl ModelCatalogTools {
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

    #[test]
    fn ensure_weights_builds_download_job() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        let plan_value: serde_json::Value = serde_json::from_str(&plan_output).expect("parse plan");
        let data = &plan_value["data"];
        let deploy_plan: DeploymentPlan =
            serde_json::from_value(data.clone()).expect("deserialize plan");
        let digest = plan_value["data"]["plan_digest"]
            .as_str()
            .expect("digest")
            .to_string();
        let output = tools()
            .ensure_weights(Parameters(EnsureWeightsParams {
                plan: deploy_plan,
                plan_digest: digest,
            }))
            .expect("ensure_weights");
        assert!(output.contains("hf download"));
        assert!(output.contains("superbloom"));
        assert!(output.contains(".homelab-mcp-download.json"));
    }

    #[test]
    fn apply_plan_refuses_with_wrong_digest() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        let plan_value: serde_json::Value = serde_json::from_str(&plan_output).expect("parse plan");
        let data = &plan_value["data"];
        let deploy_plan: DeploymentPlan =
            serde_json::from_value(data.clone()).expect("deserialize plan");
        let result = tools().apply_plan(Parameters(ApplyPlanParams {
            plan: deploy_plan,
            plan_digest: "wrong-digest".into(),
            mode: None,
        }));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("digest mismatch"));
    }
}
