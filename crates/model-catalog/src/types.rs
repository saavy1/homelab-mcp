use homelab_mcp_core::Provenance;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeSource {
    SparkArena,
    Local,
    AdHoc,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeEngine {
    #[default]
    Vllm,
    Sglang,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Recipe {
    pub id: String,
    pub source: RecipeSource,
    pub model: ModelSpec,
    pub runtime: RuntimeSpec,
    pub hardware: HardwareSpec,
    pub serving: ServingSpec,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ModelSpec {
    pub id: String,
    pub revision: Option<String>,
    pub quantization: Option<String>,
    pub gated: Option<bool>,
    pub license: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct RuntimeSpec {
    pub image: String,
    pub args: Vec<String>,
    pub env: Vec<EnvVar>,
    pub tensor_parallel: Option<u32>,
    pub max_model_len: Option<u32>,
    pub dtype: Option<String>,
    pub tool_call_parser: Option<String>,
    pub reasoning_parser: Option<String>,
    #[serde(default)]
    pub engine: RuntimeEngine,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct HardwareSpec {
    pub gpu_class: String,
    pub gpu_count: u32,
    pub estimated_vram_gb: Option<u32>,
    pub gpu_memory_utilization: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ServingSpec {
    pub namespace: String,
    pub service_name: Option<String>,
    pub replicas: u32,
    pub storage_mode: StorageMode,
    pub ingress_policy: IngressPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageMode {
    Ephemeral,
    ModelCache,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IngressPolicy {
    ClusterLocal,
    InternalHttp,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyMode {
    #[default]
    CreateOnly,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct DeploymentPlan {
    pub name: String,
    pub namespace: String,
    pub recipe_id: String,
    pub runtime_image: String,
    pub runtime_args: Vec<String>,
    pub runtime_env: Vec<EnvVar>,
    pub selected_gpu_class: String,
    pub replicas: u32,
    pub scale_to_zero: bool,
    pub storage_mode: StorageMode,
    pub ingress_policy: IngressPolicy,
    pub env_overrides: Vec<EnvVar>,
    pub resource_requests: ResourceRequests,
    pub readiness_timeout_seconds: u32,
    pub model_id: String,
    pub model_revision: Option<String>,
    pub model_path: String,
    pub runtime_engine: RuntimeEngine,
    pub runtime_port: u16,
    pub plan_digest: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ResourceRequests {
    pub cpu: String,
    pub memory: String,
    pub gpu_count: u32,
}
