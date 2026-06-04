use crate::DeploymentPlan;
use homelab_mcp_core::{HomelabMcpError, HomelabResult, sanitize_label_value};
use serde_json::{Value, json};

pub fn render_kserve_value(plan: &DeploymentPlan) -> Value {
    // The GPU node mounts the NAS at /mnt/nas/models; model files live at
    // /mnt/nas/models/<org>/<model>. We pass the full path to --model.
    let model_arg = format!("--model={}", plan.model_path);
    let served_model_arg = format!("--served-model-name={}", plan.name);
    // Derive the NAS mount point: parent of org/model in the model_path.
    // e.g. /mnt/nas/models/LiquidAI/LFM2.5-350M -> /mnt/nas/models
    let mount_path = {
        let parts: Vec<&str> = plan.model_path.split('/').collect();
        parts[..parts.len() - 2].join("/")
    };

    json!({
        "apiVersion": "serving.kserve.io/v1beta1",
        "kind": "InferenceService",
        "metadata": {
            "name": plan.name,
            "namespace": plan.namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "homelab-mcp",
                "homelab.saavylab.dev/recipe-id": sanitize_label_value(&plan.recipe_id),
                "homelab.saavylab.dev/plan-digest": &plan.plan_digest[..63],
            },
            "annotations": {
                "homelab.saavylab.dev/model-id": plan.model_id,
                "homelab.saavylab.dev/plan-digest": plan.plan_digest,
            }
        },
        "spec": {
            "predictor": {
                "minReplicas": plan.replicas,
                "maxReplicas": plan.replicas.max(1),
                "containers": [{
                    "name": "kserve-container",
                    "image": "vllm/vllm-openai:latest",
                    "command": ["python3", "-m", "vllm.entrypoints.openai.api_server"],
                    "args": [model_arg, served_model_arg,
                             "--host=0.0.0.0", "--port=8080",
                             "--trust-remote-code"],
                    "env": [
                        { "name": "HF_HUB_OFFLINE", "value": "1" },
                        { "name": "TRANSFORMERS_OFFLINE", "value": "1" }
                    ],
                    "resources": {
                        "requests": {
                            "cpu": plan.resource_requests.cpu,
                            "memory": plan.resource_requests.memory,
                            "nvidia.com/gpu": plan.resource_requests.gpu_count.to_string()
                        },
                        "limits": {
                            "nvidia.com/gpu": plan.resource_requests.gpu_count.to_string()
                        }
                    },
                    "volumeMounts": [{
                        "name": "nas-models",
                        "mountPath": mount_path,
                        "readOnly": true
                    }]
                }],
                "volumes": [{
                    "name": "nas-models",
                    "hostPath": {
                        "path": mount_path,
                        "type": "Directory"
                    }
                }],
                "tolerations": [
                    { "key": "nvidia.com/gpu", "operator": "Exists", "effect": "NoSchedule" },
                    { "key": "nvidia.com/gpu", "operator": "Exists", "effect": "NoExecute" }
                ],
                "nodeSelector": {
                    "nvidia.com/gpu.product": "NVIDIA-GB10"
                }
            }
        }
    })
}

pub fn render_kserve_yaml(plan: &DeploymentPlan) -> HomelabResult<String> {
    serde_yaml::to_string(&render_kserve_value(plan))
        .map_err(|error| HomelabMcpError::Serialization(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClusterProfile, DeployOverrides, parse_recipe_yaml, plan_deploy};

    #[test]
    fn renders_inferenceservice_yaml_with_plan_digest() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        let yaml = render_kserve_yaml(&plan).expect("yaml renders");
        assert!(yaml.contains("kind: InferenceService"));
        assert!(yaml.contains("app.kubernetes.io/managed-by: homelab-mcp"));
        assert!(yaml.contains("homelab.saavylab.dev/plan-digest"));
    }

    #[test]
    fn snapshot_qwen3_8b_inferenceservice() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        // Remove the plan_digest so the snapshot is stable across code changes
        let mut value = render_kserve_value(&plan);
        if let Some(labels) = value
            .get_mut("metadata")
            .and_then(|m| m.get_mut("labels"))
            .and_then(|l| l.as_object_mut())
        {
            labels.remove("homelab.saavylab.dev/plan-digest");
        }
        insta::assert_yaml_snapshot!(value);
    }

    #[test]
    fn snapshot_deepseek_v4_flash_inferenceservice() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/deepseek-v4-flash.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        let mut value = render_kserve_value(&plan);
        if let Some(labels) = value
            .get_mut("metadata")
            .and_then(|m| m.get_mut("labels"))
            .and_then(|l| l.as_object_mut())
        {
            labels.remove("homelab.saavylab.dev/plan-digest");
        }
        insta::assert_yaml_snapshot!(value);
    }
}
