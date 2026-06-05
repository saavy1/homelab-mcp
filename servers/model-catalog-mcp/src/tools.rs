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
    pub spark_arena_dir: PathBuf,
    pub runtime_state_namespace: String,
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

#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
pub struct PlanDeployParams {
    pub recipe_id: String,
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub runtime_args: Option<Vec<String>>,
    pub runtime_env: Option<Vec<model_catalog::EnvVar>>,
    pub env_overrides: Option<Vec<model_catalog::EnvVar>>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub gpu_count: Option<u32>,
    pub readiness_timeout_seconds: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
pub struct EnsureWeightsParams {
    pub recipe_id: String,
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub plan_digest: String,
    pub runtime_args: Option<Vec<String>>,
    pub runtime_env: Option<Vec<model_catalog::EnvVar>>,
    pub env_overrides: Option<Vec<model_catalog::EnvVar>>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub gpu_count: Option<u32>,
    pub readiness_timeout_seconds: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DownloadStatusParams {
    pub job_ref: DownloadJobRef,
}

#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
pub struct ApplyPlanParams {
    pub recipe_id: String,
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub plan_digest: String,
    pub runtime_args: Option<Vec<String>>,
    pub runtime_env: Option<Vec<model_catalog::EnvVar>>,
    pub env_overrides: Option<Vec<model_catalog::EnvVar>>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub gpu_count: Option<u32>,
    pub readiness_timeout_seconds: Option<u32>,
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchSparkArenaRecipesParams {
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ShowSparkArenaRecipeParams {
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ImportSparkArenaRecipeParams {
    pub id: String,
    pub created_by: Option<String>,
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

fn resource_requests_from_params(
    cpu: Option<String>,
    memory: Option<String>,
    gpu_count: Option<u32>,
) -> Option<model_catalog::ResourceRequests> {
    match (cpu, memory, gpu_count) {
        (None, None, None) => None,
        (cpu, memory, gpu_count) => Some(model_catalog::ResourceRequests {
            cpu: cpu.unwrap_or_else(|| "2".into()),
            memory: memory.unwrap_or_else(|| "16Gi".into()),
            gpu_count: gpu_count.unwrap_or(1),
        }),
    }
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
                runtime_args: params.runtime_args.unwrap_or_default(),
                runtime_env: params.runtime_env.unwrap_or_default(),
                env_overrides: params.env_overrides.unwrap_or_default(),
                resource_requests: resource_requests_from_params(
                    params.cpu,
                    params.memory,
                    params.gpu_count,
                ),
                readiness_timeout_seconds: params.readiness_timeout_seconds,
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
        let plan = self.derive_plan(
            &params.recipe_id,
            DeployOverrides {
                name: params.name,
                namespace: params.namespace,
                replicas: None,
                runtime_args: params.runtime_args.unwrap_or_default(),
                runtime_env: params.runtime_env.unwrap_or_default(),
                env_overrides: params.env_overrides.unwrap_or_default(),
                resource_requests: resource_requests_from_params(
                    params.cpu,
                    params.memory,
                    params.gpu_count,
                ),
                readiness_timeout_seconds: params.readiness_timeout_seconds,
            },
        )?;
        verify_digest(&plan, &params.plan_digest)?;
        let storage = &self.cluster_profile.model_storage;
        let revision = plan.model_revision.clone().unwrap_or_else(|| "main".into());
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
        let plan = self.derive_plan(
            &params.recipe_id,
            DeployOverrides {
                name: params.name,
                namespace: params.namespace,
                replicas: None,
                runtime_args: params.runtime_args.unwrap_or_default(),
                runtime_env: params.runtime_env.unwrap_or_default(),
                env_overrides: params.env_overrides.unwrap_or_default(),
                resource_requests: resource_requests_from_params(
                    params.cpu,
                    params.memory,
                    params.gpu_count,
                ),
                readiness_timeout_seconds: params.readiness_timeout_seconds,
            },
        )?;
        verify_digest(&plan, &params.plan_digest)?;

        // Sentinel check: verify download completed
        let job_ref = DownloadJobRef {
            job_name: download_job_name(
                &plan.model_id,
                &plan.model_revision.clone().unwrap_or_else(|| "main".into()),
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

    #[tool(description = "Search Spark Arena model recipes available to import")]
    pub fn search_spark_arena_recipes(
        &self,
        Parameters(params): Parameters<SearchSparkArenaRecipesParams>,
    ) -> Result<String, String> {
        let recipes = model_catalog::load_spark_arena_recipes(&self.spark_arena_dir)
            .map_err(|error| error.to_string())?;
        let matches = model_catalog::search_spark_arena_recipes(&recipes, params.query.as_deref());
        serde_json::to_string(&homelab_mcp_core::ToolResult::read(
            format!("found {} Spark Arena recipe(s)", matches.len()),
            matches,
        ))
        .map_err(|error| error.to_string())
    }

    #[tool(description = "Show one Spark Arena recipe by id before importing")]
    pub fn show_spark_arena_recipe(
        &self,
        Parameters(params): Parameters<ShowSparkArenaRecipeParams>,
    ) -> Result<String, String> {
        let recipes = model_catalog::load_spark_arena_recipes(&self.spark_arena_dir)
            .map_err(|error| error.to_string())?;
        let recipe = recipes
            .into_iter()
            .find(|recipe| recipe.id == params.id)
            .ok_or_else(|| format!("Spark Arena recipe not found: {}", params.id))?;
        serde_json::to_string(&homelab_mcp_core::ToolResult::read(
            format!("loaded Spark Arena recipe {}", recipe.id),
            recipe,
        ))
        .map_err(|error| error.to_string())
    }

    #[tool(description = "Import a Spark Arena recipe into runtime model state")]
    pub async fn import_spark_arena_recipe(
        &self,
        Parameters(params): Parameters<ImportSparkArenaRecipeParams>,
    ) -> Result<String, String> {
        let recipes = model_catalog::load_spark_arena_recipes(&self.spark_arena_dir)
            .map_err(|error| error.to_string())?;
        let recipe = recipes
            .into_iter()
            .find(|recipe| recipe.id == params.id)
            .ok_or_else(|| format!("Spark Arena recipe not found: {}", params.id))?;
        let now = chrono::Utc::now().to_rfc3339();
        let record = model_catalog::RuntimeRecipeRecord {
            recipe,
            created_by: params.created_by.unwrap_or_else(|| "hermes".into()),
            created_at: now.clone(),
            updated_at: now,
        };
        let client = homelab_mcp_k8s::k8s_client()
            .await
            .map_err(|error| error.to_string())?;
        let name =
            homelab_mcp_k8s::upsert_runtime_recipe(client, &self.runtime_state_namespace, &record)
                .await
                .map_err(|error| error.to_string())?;
        serde_json::to_string(&homelab_mcp_core::ToolResult::cluster_write(
            format!("imported runtime recipe {}", record.recipe.id),
            serde_json::json!({ "configmap": name, "recipe_id": record.recipe.id }),
        ))
        .map_err(|error| error.to_string())
    }
}

impl ModelCatalogTools {
    fn derive_plan(
        &self,
        recipe_id: &str,
        overrides: DeployOverrides,
    ) -> Result<DeploymentPlan, String> {
        let recipe = self.find_recipe(recipe_id)?;
        let result = plan_deploy(&recipe, &self.cluster_profile, overrides);
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
            spark_arena_dir: PathBuf::from(
                "../../crates/model-catalog/tests/fixtures/local-recipes",
            ),
            runtime_state_namespace: "hermes".into(),
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
                runtime_args: None,
                runtime_env: None,
                env_overrides: None,
                cpu: None,
                memory: None,
                gpu_count: None,
                readiness_timeout_seconds: None,
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
                runtime_args: None,
                runtime_env: None,
                env_overrides: None,
                cpu: None,
                memory: None,
                gpu_count: None,
                readiness_timeout_seconds: None,
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
                runtime_args: None,
                runtime_env: None,
                env_overrides: None,
                cpu: None,
                memory: None,
                gpu_count: None,
                readiness_timeout_seconds: None,
            }))
            .await;
        // Without a cluster the kube API call fails, but it must NOT be a digest error
        match result {
            Ok(s) => assert!(s.contains("created_download_job") || s.contains("already")),
            Err(e) => assert!(
                !e.contains("digest mismatch"),
                "unexpected digest error: {e}"
            ),
        }
    }

    #[tokio::test]
    async fn ensure_weights_rejects_wrong_digest() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
                runtime_args: None,
                runtime_env: None,
                env_overrides: None,
                cpu: None,
                memory: None,
                gpu_count: None,
                readiness_timeout_seconds: None,
            }))
            .expect("plan");
        assert!(plan_output.contains("plan_digest"));
        let result = tools()
            .ensure_weights(Parameters(EnsureWeightsParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
                plan_digest: "wrong-digest".into(),
                runtime_args: None,
                runtime_env: None,
                env_overrides: None,
                cpu: None,
                memory: None,
                gpu_count: None,
                readiness_timeout_seconds: None,
            }))
            .await;
        let err = result.expect_err("should reject wrong digest");
        assert!(
            err.contains("digest mismatch"),
            "expected digest mismatch, got: {err}"
        );
    }

    #[tokio::test]
    async fn apply_plan_refuses_with_wrong_digest() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
                runtime_args: None,
                runtime_env: None,
                env_overrides: None,
                cpu: None,
                memory: None,
                gpu_count: None,
                readiness_timeout_seconds: None,
            }))
            .expect("plan");
        assert!(plan_output.contains("plan_digest"));
        let result = tools()
            .apply_plan(Parameters(ApplyPlanParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
                plan_digest: "wrong-digest".into(),
                runtime_args: None,
                runtime_env: None,
                env_overrides: None,
                cpu: None,
                memory: None,
                gpu_count: None,
                readiness_timeout_seconds: None,
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("digest mismatch"));
    }

    fn full_override_params() -> PlanDeployParams {
        PlanDeployParams {
            recipe_id: "qwen3-8b".into(),
            name: Some("custom-name".into()),
            namespace: Some("custom-ns".into()),
            runtime_args: Some(vec!["--max-model-len".into(), "4096".into()]),
            runtime_env: Some(vec![model_catalog::EnvVar {
                name: "FOO".into(),
                value: "bar".into(),
            }]),
            env_overrides: Some(vec![model_catalog::EnvVar {
                name: "BAZ".into(),
                value: "qux".into(),
            }]),
            cpu: Some("4".into()),
            memory: Some("32Gi".into()),
            gpu_count: Some(1),
            readiness_timeout_seconds: Some(120),
        }
    }

    fn ensure_weights_from_plan_params(
        params: &PlanDeployParams,
        plan_digest: String,
    ) -> EnsureWeightsParams {
        EnsureWeightsParams {
            recipe_id: params.recipe_id.clone(),
            name: params.name.clone(),
            namespace: params.namespace.clone(),
            plan_digest,
            runtime_args: params.runtime_args.clone(),
            runtime_env: params.runtime_env.clone(),
            env_overrides: params.env_overrides.clone(),
            cpu: params.cpu.clone(),
            memory: params.memory.clone(),
            gpu_count: params.gpu_count,
            readiness_timeout_seconds: params.readiness_timeout_seconds,
        }
    }

    fn apply_plan_from_plan_params(
        params: &PlanDeployParams,
        plan_digest: String,
    ) -> ApplyPlanParams {
        ApplyPlanParams {
            recipe_id: params.recipe_id.clone(),
            name: params.name.clone(),
            namespace: params.namespace.clone(),
            plan_digest,
            runtime_args: params.runtime_args.clone(),
            runtime_env: params.runtime_env.clone(),
            env_overrides: params.env_overrides.clone(),
            cpu: params.cpu.clone(),
            memory: params.memory.clone(),
            gpu_count: params.gpu_count,
            readiness_timeout_seconds: params.readiness_timeout_seconds,
        }
    }

    #[test]
    fn plan_deploy_returns_overrides_in_plan() {
        let params = full_override_params();
        let output = tools().plan_deploy(Parameters(params)).expect("plan");
        let plan_value: serde_json::Value = serde_json::from_str(&output).expect("parse plan");
        let data = &plan_value["data"];
        assert_eq!(data["name"], "custom-name");
        assert_eq!(data["namespace"], "custom-ns");
        let args: Vec<String> = serde_json::from_value(data["runtime_args"].clone()).expect("args");
        assert!(args.contains(&"--max-model-len".into()));
        assert!(args.contains(&"4096".into()));
        let env: Vec<model_catalog::EnvVar> =
            serde_json::from_value(data["runtime_env"].clone()).expect("runtime_env");
        assert!(env.iter().any(|e| e.name == "FOO" && e.value == "bar"));
        let env_overrides: Vec<model_catalog::EnvVar> =
            serde_json::from_value(data["env_overrides"].clone()).expect("env_overrides");
        assert!(
            env_overrides
                .iter()
                .any(|e| e.name == "BAZ" && e.value == "qux")
        );
        let resources = &data["resource_requests"];
        assert_eq!(resources["cpu"], "4");
        assert_eq!(resources["memory"], "32Gi");
        assert_eq!(resources["gpu_count"], 1);
        assert_eq!(data["readiness_timeout_seconds"], 120);
    }

    #[tokio::test]
    async fn ensure_weights_rejects_wrong_digest_with_overrides() {
        let params = full_override_params();
        let plan_output = tools()
            .plan_deploy(Parameters(params.clone()))
            .expect("plan");
        let plan_value: serde_json::Value = serde_json::from_str(&plan_output).expect("parse plan");
        let correct_digest = plan_value["data"]["plan_digest"]
            .as_str()
            .expect("digest")
            .to_string();

        let result = tools()
            .ensure_weights(Parameters(ensure_weights_from_plan_params(
                &params,
                "wrong-digest".into(),
            )))
            .await;
        let err = result.expect_err("should reject wrong digest");
        assert!(
            err.contains("digest mismatch"),
            "expected digest mismatch, got: {err}"
        );
        assert!(
            err.contains(&correct_digest),
            "expected error to contain correct override-derived digest {correct_digest}, got: {err}"
        );
    }

    #[tokio::test]
    async fn apply_plan_rejects_wrong_digest_with_overrides() {
        let params = full_override_params();
        let plan_output = tools()
            .plan_deploy(Parameters(params.clone()))
            .expect("plan");
        let plan_value: serde_json::Value = serde_json::from_str(&plan_output).expect("parse plan");
        let correct_digest = plan_value["data"]["plan_digest"]
            .as_str()
            .expect("digest")
            .to_string();

        let result = tools()
            .apply_plan(Parameters(apply_plan_from_plan_params(
                &params,
                "wrong-digest".into(),
            )))
            .await;
        let err = result.expect_err("should reject wrong digest");
        assert!(
            err.contains("digest mismatch"),
            "expected digest mismatch, got: {err}"
        );
        assert!(
            err.contains(&correct_digest),
            "expected error to contain correct override-derived digest {correct_digest}, got: {err}"
        );
    }

    #[test]
    fn search_spark_arena_recipes_returns_lfm_fixture() {
        let output = tools()
            .search_spark_arena_recipes(Parameters(SearchSparkArenaRecipesParams {
                query: Some("lfm".into()),
            }))
            .expect("search spark arena");
        let result: serde_json::Value = serde_json::from_str(&output).expect("parse result");
        let data = result["data"].as_array().expect("data array");
        assert!(!data.is_empty());
        let first = &data[0];
        assert_eq!(first["id"], "lfm25-350m");
        assert!(
            first["required_args"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg == "--language-model-only")
        );
    }

    #[test]
    fn search_spark_arena_recipes_returns_empty_for_missing_query() {
        let output = tools()
            .search_spark_arena_recipes(Parameters(SearchSparkArenaRecipesParams {
                query: Some("no-such-model".into()),
            }))
            .expect("search spark arena");
        let result: serde_json::Value = serde_json::from_str(&output).expect("parse result");
        let data = result["data"].as_array().expect("data array");
        assert!(data.is_empty());
    }

    #[test]
    fn show_spark_arena_recipe_returns_known_fixture() {
        let output = tools()
            .show_spark_arena_recipe(Parameters(ShowSparkArenaRecipeParams {
                id: "lfm25-350m".into(),
            }))
            .expect("show spark arena recipe");
        let result: serde_json::Value = serde_json::from_str(&output).expect("parse result");
        let data = &result["data"];
        assert_eq!(data["id"], "lfm25-350m");
        assert_eq!(data["model"]["id"], "LiquidAI/LFM2.5-350M");
    }

    #[test]
    fn show_spark_arena_recipe_returns_error_for_missing_id() {
        let result = tools().show_spark_arena_recipe(Parameters(ShowSparkArenaRecipeParams {
            id: "nonexistent-recipe".into(),
        }));
        let err = result.expect_err("should fail for missing recipe");
        assert!(err.contains("Spark Arena recipe not found"));
    }
}
