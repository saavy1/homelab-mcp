use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeRole {
    ControlPlane,
    Nas,
    GpuWorker,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Taint {
    pub key: String,
    pub effect: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct NodeProfile {
    pub hostname: String,
    pub roles: Vec<NodeRole>,
    pub gpu_product: Option<String>,
    pub gpu_count: u32,
    pub gpu_memory_gb: u32,
    pub taints: Vec<Taint>,
    pub model_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ModelStorage {
    pub nas_hostname: String,
    pub nas_path: String,
    pub gpu_node_path: String,
    pub download_node_selector: String,
    pub hf_secret_name: String,
    pub hf_secret_namespace: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IngressMode {
    ClusterLocal,
    InternalHttp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ClusterProfile {
    pub cluster_name: String,
    pub nodes: Vec<NodeProfile>,
    pub default_namespace: String,
    pub available_serving_runtimes: Vec<String>,
    pub max_gpu_per_pod: u32,
    pub ingress_mode: IngressMode,
    pub model_storage: ModelStorage,
}

impl ClusterProfile {
    pub fn superbloom_default() -> Self {
        Self {
            cluster_name: "superbloom".into(),
            nodes: vec![
                NodeProfile {
                    hostname: "superbloom".into(),
                    roles: vec![NodeRole::ControlPlane, NodeRole::Nas],
                    gpu_product: None,
                    gpu_count: 0,
                    gpu_memory_gb: 0,
                    taints: vec![],
                    model_path: Some("/tank/models".into()),
                },
                NodeProfile {
                    hostname: "gx10-98a5".into(),
                    roles: vec![NodeRole::GpuWorker],
                    gpu_product: Some("NVIDIA-GB10".into()),
                    gpu_count: 1,
                    gpu_memory_gb: 128,
                    taints: vec![
                        Taint {
                            key: "nvidia.com/gpu".into(),
                            effect: "NoSchedule".into(),
                        },
                        Taint {
                            key: "nvidia.com/gpu".into(),
                            effect: "NoExecute".into(),
                        },
                    ],
                    model_path: Some("/mnt/nas/models".into()),
                },
            ],
            default_namespace: "ai".into(),
            available_serving_runtimes: vec!["vllm".into()],
            max_gpu_per_pod: 1,
            ingress_mode: IngressMode::ClusterLocal,
            model_storage: ModelStorage {
                nas_hostname: "superbloom".into(),
                nas_path: "/tank/models".into(),
                gpu_node_path: "/mnt/nas/models".into(),
                download_node_selector: "superbloom".into(),
                hf_secret_name: "hf-token".into(),
                hf_secret_namespace: "ai".into(),
            },
        }
    }

    pub fn gpu_node(&self) -> Option<&NodeProfile> {
        self.nodes.iter().find(|n| n.roles.contains(&NodeRole::GpuWorker))
    }

    pub fn nas_node(&self) -> Option<&NodeProfile> {
        self.nodes.iter().find(|n| n.roles.contains(&NodeRole::Nas))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_has_two_nodes_with_different_model_paths() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.nodes.len(), 2);
        let gpu = profile.gpu_node().expect("has GPU node");
        assert_eq!(gpu.model_path.as_deref(), Some("/mnt/nas/models"));
        let nas = profile.nas_node().expect("has NAS node");
        assert_eq!(nas.model_path.as_deref(), Some("/tank/models"));
    }

    #[test]
    fn storage_paths_differ_between_nodes() {
        let profile = ClusterProfile::superbloom_default();
        assert_ne!(profile.model_storage.nas_path, profile.model_storage.gpu_node_path);
    }

    #[test]
    fn hf_secret_is_configured() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.model_storage.hf_secret_name, "hf-token");
        assert_eq!(profile.model_storage.hf_secret_namespace, "ai");
    }
}
