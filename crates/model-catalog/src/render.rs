use crate::DeploymentPlan;
use homelab_mcp_core::{HomelabMcpError, HomelabResult};
use serde_json::{Value, json};

pub fn render_kserve_value(plan: &DeploymentPlan) -> Value {
    json!({
        "apiVersion": "serving.kserve.io/v1beta1",
        "kind": "InferenceService",
        "metadata": {
            "name": plan.name,
            "namespace": plan.namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "homelab-mcp",
                "homelab.saavylab.dev/recipe-id": plan.recipe_id,
                "homelab.saavylab.dev/plan-digest": plan.plan_digest,
            },
            "annotations": {
                "homelab.saavylab.dev/model-id": plan.model_id,
            }
        },
        "spec": {
            "predictor": {
                "minReplicas": plan.replicas,
                "maxReplicas": plan.replicas.max(1),
                "model": {
                    "modelFormat": { "name": "vllm" },
                    "resources": {
                        "requests": {
                            "cpu": plan.resource_requests.cpu,
                            "memory": plan.resource_requests.memory,
                            "nvidia.com/gpu": plan.resource_requests.gpu_count.to_string()
                        },
                        "limits": {
                            "nvidia.com/gpu": plan.resource_requests.gpu_count.to_string()
                        }
                    }
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
}
