use crate::state::{DeploymentState, RuntimeDeploymentRecord, RuntimeRecipeRecord};
use crate::types::{EnvVar, Recipe, ResourceRequests};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MODEL_CRD_GROUP: &str = "models.saavylab.dev";
pub const MODEL_CRD_VERSION: &str = "v1alpha1";

#[derive(Clone, Debug, CustomResource, Deserialize, JsonSchema, PartialEq, Serialize)]
#[kube(
    group = "models.saavylab.dev",
    version = "v1alpha1",
    kind = "ModelRecipe",
    plural = "modelrecipes",
    namespaced,
    derive = "PartialEq",
    status = "ModelRecipeStatus"
)]
#[serde(rename_all = "camelCase")]
pub struct ModelRecipeSpec {
    pub recipe: Recipe,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRecipeStatus {
    pub observed_generation: Option<i64>,
}

#[derive(Clone, Debug, CustomResource, Deserialize, JsonSchema, PartialEq, Serialize)]
#[kube(
    group = "models.saavylab.dev",
    version = "v1alpha1",
    kind = "ModelDeployment",
    plural = "modeldeployments",
    namespaced,
    derive = "PartialEq",
    status = "ModelDeploymentStatus"
)]
#[serde(rename_all = "camelCase")]
pub struct ModelDeploymentSpec {
    pub name: String,
    pub namespace: String,
    pub recipe_id: String,
    pub target: String,
    pub runtime_args: Vec<String>,
    pub runtime_env: Vec<EnvVar>,
    pub resources: ResourceRequests,
    pub last_plan_digest: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDeploymentStatus {
    pub state: DeploymentState,
    pub observed_generation: Option<i64>,
    pub last_transition_time: Option<String>,
    pub failure_reason: Option<String>,
    pub kserve_ready: bool,
    pub url: Option<String>,
}

impl Default for ModelDeploymentStatus {
    fn default() -> Self {
        Self {
            state: DeploymentState::Applying,
            observed_generation: None,
            last_transition_time: None,
            failure_reason: None,
            kserve_ready: false,
            url: None,
        }
    }
}

pub fn recipe_record_to_spec(record: &RuntimeRecipeRecord) -> ModelRecipeSpec {
    ModelRecipeSpec {
        recipe: record.recipe.clone(),
        created_by: record.created_by.clone(),
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
    }
}

pub fn recipe_spec_to_record(spec: &ModelRecipeSpec) -> RuntimeRecipeRecord {
    RuntimeRecipeRecord {
        recipe: spec.recipe.clone(),
        created_by: spec.created_by.clone(),
        created_at: spec.created_at.clone(),
        updated_at: spec.updated_at.clone(),
    }
}

pub fn deployment_record_to_spec(record: &RuntimeDeploymentRecord) -> ModelDeploymentSpec {
    ModelDeploymentSpec {
        name: record.name.clone(),
        namespace: record.namespace.clone(),
        recipe_id: record.recipe_id.clone(),
        target: record.target.clone(),
        runtime_args: record.runtime_args.clone(),
        runtime_env: record.runtime_env.clone(),
        resources: record.resources.clone(),
        last_plan_digest: record.last_plan_digest.clone(),
        created_by: record.created_by.clone(),
        created_at: record.created_at.clone(),
    }
}

pub fn deployment_record_to_status(record: &RuntimeDeploymentRecord) -> ModelDeploymentStatus {
    ModelDeploymentStatus {
        state: record.status.clone(),
        observed_generation: None,
        last_transition_time: Some(record.created_at.clone()),
        failure_reason: record.failure_reason.clone(),
        kserve_ready: record.status == DeploymentState::Ready,
        url: None,
    }
}

pub fn deployment_parts_to_record(
    spec: &ModelDeploymentSpec,
    status: Option<&ModelDeploymentStatus>,
) -> RuntimeDeploymentRecord {
    let status = status.cloned().unwrap_or_default();
    RuntimeDeploymentRecord {
        name: spec.name.clone(),
        namespace: spec.namespace.clone(),
        recipe_id: spec.recipe_id.clone(),
        target: spec.target.clone(),
        runtime_args: spec.runtime_args.clone(),
        runtime_env: spec.runtime_env.clone(),
        resources: spec.resources.clone(),
        status: status.state,
        last_plan_digest: spec.last_plan_digest.clone(),
        created_by: spec.created_by.clone(),
        created_at: spec.created_at.clone(),
        failure_reason: status.failure_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource, RuntimeSpec, ServingSpec,
        StorageMode,
    };
    use homelab_mcp_core::Provenance;

    fn sample_recipe() -> Recipe {
        Recipe {
            id: "lfm25-350m".into(),
            source: RecipeSource::SparkArena,
            model: ModelSpec {
                id: "LiquidAI/LFM2.5-350M".into(),
                revision: None,
                quantization: None,
                gated: Some(false),
                license: Some("apache-2.0".into()),
            },
            runtime: RuntimeSpec {
                image: "vllm/vllm-openai:latest".into(),
                args: vec!["--dtype=auto".into()],
                env: vec![],
                tensor_parallel: Some(1),
                max_model_len: Some(32768),
                dtype: Some("auto".into()),
                tool_call_parser: Some("hermes".into()),
                reasoning_parser: None,
            },
            hardware: HardwareSpec {
                gpu_class: "gb10".into(),
                gpu_count: 1,
                estimated_vram_gb: Some(4),
                gpu_memory_utilization: Some(0.8),
            },
            serving: ServingSpec {
                namespace: "ai".into(),
                service_name: Some("lfm25-350m".into()),
                replicas: 1,
                storage_mode: StorageMode::ModelCache,
                ingress_policy: IngressPolicy::ClusterLocal,
            },
            provenance: Provenance {
                source: "spark-arena".into(),
                path: Some("spark-arena-recipes.yaml".into()),
                commit: None,
            },
        }
    }

    #[test]
    fn recipe_record_round_trips_through_spec() {
        let record = RuntimeRecipeRecord {
            recipe: sample_recipe(),
            created_by: "hermes".into(),
            created_at: "2026-06-05T00:00:00Z".into(),
            updated_at: "2026-06-05T00:00:00Z".into(),
        };
        let spec = recipe_record_to_spec(&record);
        assert_eq!(recipe_spec_to_record(&spec), record);
    }

    #[test]
    fn deployment_record_round_trips_through_spec_and_status() {
        let record = RuntimeDeploymentRecord {
            name: "lfm25-350m".into(),
            namespace: "ai".into(),
            recipe_id: "lfm25-350m".into(),
            target: "spark".into(),
            runtime_args: vec!["--max-model-len".into(), "8192".into()],
            runtime_env: vec![],
            resources: ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
            status: DeploymentState::Applying,
            last_plan_digest: "sha256:test".into(),
            created_by: "hermes".into(),
            created_at: "2026-06-05T00:00:00Z".into(),
            failure_reason: None,
        };
        let spec = deployment_record_to_spec(&record);
        let status = deployment_record_to_status(&record);
        assert_eq!(deployment_parts_to_record(&spec, Some(&status)), record);
    }

    #[test]
    fn generated_crds_have_expected_group_and_kind() {
        use kube::CustomResourceExt;
        assert_eq!(ModelRecipe::crd().spec.group, MODEL_CRD_GROUP);
        assert_eq!(ModelDeployment::crd().spec.group, MODEL_CRD_GROUP);
        assert_eq!(ModelRecipe::crd().spec.names.kind, "ModelRecipe");
        assert_eq!(ModelDeployment::crd().spec.names.kind, "ModelDeployment");
    }
}
