use crate::types::{EnvVar, Recipe, ResourceRequests};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RendererMode {
    Kserve,
    DirectVllm,
    ExternalOpenAiCompatible,
    MacMiniMlx,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct RuntimeProfile {
    pub id: String,
    pub target: String,
    pub renderer_mode: RendererMode,
    pub allowed_images: Vec<String>,
    pub allowed_model_roots: Vec<String>,
    pub max_resources: ResourceRequests,
    pub default_resources: ResourceRequests,
}

impl RuntimeProfile {
    pub fn spark_vllm_medium() -> Self {
        Self {
            id: "spark-vllm-medium".into(),
            target: "spark".into(),
            renderer_mode: RendererMode::Kserve,
            allowed_images: vec!["vllm/vllm-openai:latest".into()],
            allowed_model_roots: vec!["/mnt/nas/models".into()],
            max_resources: ResourceRequests {
                cpu: "16".into(),
                memory: "96Gi".into(),
                gpu_count: 1,
            },
            default_resources: ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct RuntimeRecipeRecord {
    pub recipe: Recipe,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentState {
    Planned,
    Applying,
    Ready,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct RuntimeDeploymentRecord {
    pub name: String,
    pub namespace: String,
    pub recipe_id: String,
    pub target: String,
    pub runtime_args: Vec<String>,
    pub runtime_env: Vec<EnvVar>,
    pub resources: ResourceRequests,
    pub status: DeploymentState,
    pub last_plan_digest: String,
    pub created_by: String,
    pub created_at: String,
    pub failure_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spark_profile_is_kserve() {
        let profile = RuntimeProfile::spark_vllm_medium();
        assert_eq!(profile.renderer_mode, RendererMode::Kserve);
    }

    #[test]
    fn spark_profile_allows_vllm_image() {
        let profile = RuntimeProfile::spark_vllm_medium();
        assert!(
            profile
                .allowed_images
                .contains(&"vllm/vllm-openai:latest".to_string())
        );
    }

    #[test]
    fn spark_profile_allows_model_root() {
        let profile = RuntimeProfile::spark_vllm_medium();
        assert!(
            profile
                .allowed_model_roots
                .contains(&"/mnt/nas/models".to_string())
        );
    }

    #[test]
    fn spark_profile_max_resources_match() {
        let profile = RuntimeProfile::spark_vllm_medium();
        assert_eq!(profile.max_resources.cpu, "16");
        assert_eq!(profile.max_resources.memory, "96Gi");
        assert_eq!(profile.max_resources.gpu_count, 1);
    }

    #[test]
    fn spark_profile_default_resources_match() {
        let profile = RuntimeProfile::spark_vllm_medium();
        assert_eq!(profile.default_resources.cpu, "2");
        assert_eq!(profile.default_resources.memory, "16Gi");
        assert_eq!(profile.default_resources.gpu_count, 1);
    }
}
