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
    let args = render_runtime_args(plan, model_arg, served_model_arg);
    let env = render_runtime_env(plan);

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
                    "image": plan.runtime_image,
                    "command": ["python3", "-m", "vllm.entrypoints.openai.api_server"],
                    "args": args,
                    "env": env,
                    "resources": {
                        "requests": {
                            "cpu": plan.resource_requests.cpu,
                            "memory": plan.resource_requests.memory,
                            "nvidia.com/gpu": plan.resource_requests.gpu_count.to_string()
                        },
                        "limits": {
                            "cpu": plan.resource_requests.cpu,
                            "memory": plan.resource_requests.memory,
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
                    {
                        "key": "nvidia.com/gpu",
                        "operator": "Equal",
                        "value": "true",
                        "effect": "NoSchedule"
                    },
                    {
                        "key": "nvidia.com/gpu",
                        "operator": "Equal",
                        "value": "true",
                        "effect": "NoExecute"
                    }
                ],
                "nodeSelector": {
                    "nvidia.com/gpu.product": "NVIDIA-GB10"
                }
            }
        }
    })
}

fn render_runtime_args(
    plan: &DeploymentPlan,
    model_arg: String,
    served_model_arg: String,
) -> Vec<String> {
    let mut args = vec![
        model_arg,
        served_model_arg,
        "--host=0.0.0.0".to_string(),
        "--port=8080".to_string(),
    ];
    let runtime_args = &plan.runtime_args;
    let mut i = 0;
    while i < runtime_args.len() {
        let arg = &runtime_args[i];
        if is_server_managed_arg(arg) {
            // For split-form managed flags (e.g. --port 9001), skip the following value too.
            if !arg.contains('=') && i + 1 < runtime_args.len() {
                i += 1;
            }
            i += 1;
            continue;
        }
        args.push(arg.clone());
        i += 1;
    }
    args
}

fn is_server_managed_arg(arg: &str) -> bool {
    matches!(arg, "--host" | "--port" | "--model" | "--served-model-name")
        || arg.starts_with("--host=")
        || arg.starts_with("--port=")
        || arg.starts_with("--model=")
        || arg.starts_with("--served-model-name=")
}

fn render_runtime_env(plan: &DeploymentPlan) -> Vec<Value> {
    let mut env = vec![
        json!({ "name": "HF_HUB_OFFLINE", "value": "1" }),
        json!({ "name": "TRANSFORMERS_OFFLINE", "value": "1" }),
    ];
    for item in plan.runtime_env.iter().chain(plan.env_overrides.iter()) {
        env.retain(|existing| existing.get("name").and_then(Value::as_str) != Some(&item.name));
        env.push(json!({ "name": item.name, "value": item.value }));
    }
    env
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

    #[test]
    fn renders_runtime_args_from_plan() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/deepseek-v4-flash.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides {
                runtime_args: vec!["--kv-cache-dtype".into(), "fp8".into()],
                ..DeployOverrides::empty()
            },
        )
        .data;

        let value = render_kserve_value(&plan);
        let args = value["spec"]["predictor"]["containers"][0]["args"]
            .as_array()
            .expect("args array");

        assert!(args.iter().any(|arg| arg == "--kv-cache-dtype"));
        assert!(args.iter().any(|arg| arg == "fp8"));
    }

    #[test]
    fn renders_gpu_tolerations_with_required_value() {
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
        let value = render_kserve_value(&plan);
        let tolerations = value["spec"]["predictor"]["tolerations"]
            .as_array()
            .expect("tolerations array");

        assert!(tolerations.iter().any(|tol| {
            tol["key"] == "nvidia.com/gpu"
                && tol["operator"] == "Equal"
                && tol["value"] == "true"
                && tol["effect"] == "NoSchedule"
        }));
        assert!(tolerations.iter().any(|tol| {
            tol["key"] == "nvidia.com/gpu"
                && tol["operator"] == "Equal"
                && tol["value"] == "true"
                && tol["effect"] == "NoExecute"
        }));
    }

    #[test]
    fn split_form_managed_args_are_filtered_with_defaults() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides {
                runtime_args: vec![
                    "--port".into(),
                    "9001".into(),
                    "--host".into(),
                    "127.0.0.1".into(),
                ],
                ..DeployOverrides::empty()
            },
        )
        .data;
        let value = render_kserve_value(&plan);
        let args = value["spec"]["predictor"]["containers"][0]["args"]
            .as_array()
            .expect("args array");

        let arg_strs: Vec<&str> = args.iter().map(|v| v.as_str().unwrap()).collect();

        // Split-form managed args and their values must not appear
        assert!(!arg_strs.contains(&"--port"));
        assert!(!arg_strs.contains(&"9001"));
        assert!(!arg_strs.contains(&"--host"));
        assert!(!arg_strs.contains(&"127.0.0.1"));

        // Server-managed defaults must still be present
        assert!(arg_strs.contains(&"--host=0.0.0.0"));
        assert!(arg_strs.contains(&"--port=8080"));
    }

    #[test]
    fn distinct_flags_with_same_value_remain_intact() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides {
                runtime_args: vec!["--foo".into(), "same".into(), "--bar".into(), "same".into()],
                ..DeployOverrides::empty()
            },
        )
        .data;
        let value = render_kserve_value(&plan);
        let args = value["spec"]["predictor"]["containers"][0]["args"]
            .as_array()
            .expect("args array");

        let arg_strs: Vec<&str> = args.iter().map(|v| v.as_str().unwrap()).collect();

        // Both flags and both shared values must be present in order.
        let foo_idx = arg_strs
            .iter()
            .position(|&a| a == "--foo")
            .expect("--foo present");
        let bar_idx = arg_strs
            .iter()
            .position(|&a| a == "--bar")
            .expect("--bar present");
        assert!(foo_idx < bar_idx, "--foo must appear before --bar");
        assert_eq!(arg_strs[foo_idx + 1], "same");
        assert_eq!(arg_strs[bar_idx + 1], "same");

        // Count occurrences of "same" — must appear exactly twice.
        assert_eq!(
            arg_strs.iter().filter(|&&a| a == "same").count(),
            2,
            "value 'same' must appear twice, not deduplicated"
        );
    }
}
