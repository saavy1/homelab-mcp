use crate::digest::compute_plan_digest;
use crate::policy::validate_plan_policy;
use crate::{
    ClusterProfile, DeploymentPlan, EnvVar, NodeRole, Recipe, ResourceRequests, RuntimeEngine,
    StorageMode,
};
use homelab_mcp_core::{ToolResult, ValidationIssue, sanitize_dns_name};

#[derive(Clone, Debug, PartialEq)]
pub struct DeployOverrides {
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub replicas: Option<u32>,
    pub runtime_args: Vec<String>,
    pub runtime_env: Vec<EnvVar>,
    pub env_overrides: Vec<EnvVar>,
    pub resource_requests: Option<ResourceRequests>,
    pub readiness_timeout_seconds: Option<u32>,
}

impl DeployOverrides {
    pub fn empty() -> Self {
        Self {
            name: None,
            namespace: None,
            replicas: None,
            runtime_args: Vec::new(),
            runtime_env: Vec::new(),
            env_overrides: Vec::new(),
            resource_requests: None,
            readiness_timeout_seconds: None,
        }
    }
}

#[derive(Clone, Debug)]
enum ArgEntry {
    /// A CLI flag and its associated token(s). The key is the flag name
    /// (e.g. `--max-model-len`), and tokens are the raw strings that make up
    /// the entry in the original arg list.
    Flag { key: String, tokens: Vec<String> },
    /// A positional / non-flag argument.
    Positional(String),
}

/// Parse a flat token list into flag entries and positional args.
///
/// * `--flag=value`  → one `Flag` entry keyed by `--flag`.
/// * `--flag value`  → one `Flag` entry keyed by `--flag` when `value`
///   does not start with `--`.
/// * `--flag`        → one `Flag` entry (boolean flag) when the next token
///   is absent or starts with `--`.
/// * anything else   → `Positional`.
fn parse_args(args: &[String]) -> Vec<ArgEntry> {
    let mut entries = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with("--") {
            if arg.contains('=') {
                // --flag=value form
                let key = arg.split('=').next().unwrap_or(arg).to_string();
                entries.push(ArgEntry::Flag {
                    key,
                    tokens: vec![arg.clone()],
                });
                i += 1;
            } else if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                // --flag value form
                let key = arg.clone();
                entries.push(ArgEntry::Flag {
                    key,
                    tokens: vec![arg.clone(), args[i + 1].clone()],
                });
                i += 2;
            } else {
                // boolean flag
                let key = arg.clone();
                entries.push(ArgEntry::Flag {
                    key,
                    tokens: vec![arg.clone()],
                });
                i += 1;
            }
        } else {
            entries.push(ArgEntry::Positional(arg.clone()));
            i += 1;
        }
    }
    entries
}

fn entries_to_args(entries: &[ArgEntry]) -> Vec<String> {
    let mut result = Vec::new();
    for entry in entries {
        match entry {
            ArgEntry::Flag { tokens, .. } => result.extend(tokens.iter().cloned()),
            ArgEntry::Positional(v) => result.push(v.clone()),
        }
    }
    result
}

fn merge_args(default_args: &[String], override_args: &[String]) -> Vec<String> {
    let mut defaults = parse_args(default_args);
    let overrides = parse_args(override_args);

    for override_entry in overrides {
        if let ArgEntry::Flag { key, .. } = &override_entry {
            defaults.retain(|entry| match entry {
                ArgEntry::Flag { key: k, .. } => k != key,
                ArgEntry::Positional(_) => true,
            });
        }
        defaults.push(override_entry);
    }

    entries_to_args(&defaults)
}

fn merge_env(default_env: &[EnvVar], override_env: &[EnvVar]) -> Vec<EnvVar> {
    let mut merged = default_env.to_vec();
    for item in override_env {
        merged.retain(|existing| existing.name != item.name);
        merged.push(item.clone());
    }
    merged
}

pub fn plan_deploy(
    recipe: &Recipe,
    profile: &ClusterProfile,
    overrides: DeployOverrides,
) -> ToolResult<DeploymentPlan> {
    let name = sanitize_dns_name(
        &overrides
            .name
            .clone()
            .or_else(|| recipe.serving.service_name.clone())
            .unwrap_or_else(|| recipe.id.clone()),
    );
    let namespace = overrides
        .namespace
        .clone()
        .unwrap_or_else(|| recipe.serving.namespace.clone());
    let replicas = overrides.replicas.unwrap_or(recipe.serving.replicas);
    let runtime_engine = recipe.runtime.engine.clone();
    let runtime_port = recipe.runtime.port.unwrap_or(match runtime_engine {
        RuntimeEngine::Vllm => 8080,
        RuntimeEngine::Sglang => 8000,
    });
    let mut plan = DeploymentPlan {
        name,
        namespace,
        recipe_id: recipe.id.clone(),
        runtime_image: recipe.runtime.image.clone(),
        runtime_args: merge_args(&recipe.runtime.args, &overrides.runtime_args),
        runtime_env: merge_env(&recipe.runtime.env, &overrides.runtime_env),
        selected_gpu_class: recipe.hardware.gpu_class.clone(),
        replicas,
        scale_to_zero: replicas == 0,
        storage_mode: recipe.serving.storage_mode.clone(),
        ingress_policy: recipe.serving.ingress_policy.clone(),
        env_overrides: overrides.env_overrides,
        resource_requests: overrides.resource_requests.unwrap_or(ResourceRequests {
            cpu: "2".into(),
            memory: "16Gi".into(),
            gpu_count: recipe.hardware.gpu_count,
        }),
        readiness_timeout_seconds: overrides.readiness_timeout_seconds.unwrap_or(900),
        model_id: recipe.model.id.clone(),
        model_revision: recipe.model.revision.clone(),
        model_path: format!(
            "{}/{}",
            profile.model_storage.gpu_node_path, recipe.model.id
        ),
        runtime_engine,
        runtime_port,
        plan_digest: String::new(),
    };
    plan.plan_digest = compute_plan_digest(&plan);
    let mut issues = validate_fit(recipe, profile, &plan);
    let policy_issues = validate_plan_policy(recipe, profile, &plan);
    let existing_fields: std::collections::HashSet<String> =
        issues.iter().map(|i| i.field.clone()).collect();
    for issue in policy_issues {
        if !existing_fields.contains(&issue.field) {
            issues.push(issue);
        }
    }
    let summary = if issues.is_empty() {
        format!(
            "recipe {} fits cluster {} for {} GPU",
            recipe.id, profile.cluster_name, recipe.hardware.gpu_count
        )
    } else {
        format!(
            "recipe {} has {} fit issue(s) on cluster {}",
            recipe.id,
            issues.len(),
            profile.cluster_name
        )
    };
    ToolResult::pure(summary, plan).with_issues(issues)
}

pub fn validate_fit(
    recipe: &Recipe,
    profile: &ClusterProfile,
    plan: &DeploymentPlan,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if plan.resource_requests.gpu_count > profile.max_gpu_per_pod {
        issues.push(ValidationIssue {
            field: "resource_requests.gpu_count".into(),
            message: format!(
                "plan requests {} GPU(s), cluster permits {} GPU(s) per pod",
                plan.resource_requests.gpu_count, profile.max_gpu_per_pod
            ),
            allowed: Some(format!("1..={}", profile.max_gpu_per_pod)),
        });
    }
    let has_gpu_class = profile
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&NodeRole::GpuWorker))
        .any(|node| {
            node.gpu_product.as_deref().is_some_and(|p| {
                p.to_lowercase()
                    .contains(&plan.selected_gpu_class.to_lowercase())
            })
        });
    if !has_gpu_class {
        let gpu_products: Vec<String> = profile
            .nodes
            .iter()
            .filter(|n| n.roles.contains(&NodeRole::GpuWorker))
            .filter_map(|n| n.gpu_product.clone())
            .collect();
        issues.push(ValidationIssue {
            field: "hardware.gpu_class".into(),
            message: format!(
                "cluster has no GPU class matching {}",
                plan.selected_gpu_class
            ),
            allowed: Some(gpu_products.join(",")),
        });
    }
    if matches!(plan.storage_mode, StorageMode::ModelCache)
        && profile
            .gpu_node()
            .and_then(|n| n.model_path.as_deref())
            .is_none()
    {
        issues.push(ValidationIssue {
            field: "serving.storage_mode".into(),
            message: "recipe expects model cache but GPU node has no model_path".into(),
            allowed: Some("ephemeral".into()),
        });
    }
    if recipe.model.gated.unwrap_or(false) && profile.model_storage.hf_secret_name.is_empty() {
        issues.push(ValidationIssue {
            field: "model.gated".into(),
            message: "model requires gated access but no HF token secret is configured".into(),
            allowed: Some("configure hf_secret_name in ModelStorage".into()),
        });
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_recipe_yaml;

    #[test]
    fn valid_recipe_creates_plan_with_digest() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let result = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        );
        assert!(result.issues.is_empty());
        assert_eq!(result.data.name, "qwen3-8b");
        assert!(!result.data.plan_digest.is_empty());
        assert!(result.summary.text.contains("fits cluster superbloom"));
    }

    #[test]
    fn invalid_gpu_class_returns_field_path_and_allowed() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let mut profile = ClusterProfile::superbloom_default();
        if let Some(gpu_node) = profile
            .nodes
            .iter_mut()
            .find(|n| n.roles.contains(&NodeRole::GpuWorker))
        {
            gpu_node.gpu_product = None;
        }
        let result = plan_deploy(&recipe, &profile, DeployOverrides::empty());
        assert_eq!(result.issues[0].field, "hardware.gpu_class");
    }

    #[test]
    fn plan_deploy_merges_runtime_args_and_env_overrides() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/deepseek-v4-flash.yaml"
        ))
        .expect("recipe parses");
        let result = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides {
                name: None,
                namespace: None,
                replicas: None,
                runtime_args: vec![
                    "--kv-cache-dtype".into(),
                    "fp8".into(),
                    "--tool-call-parser".into(),
                    "hermes".into(),
                ],
                runtime_env: vec![EnvVar {
                    name: "VLLM_TEST".into(),
                    value: "enabled".into(),
                }],
                env_overrides: Vec::new(),
                resource_requests: Some(ResourceRequests {
                    cpu: "4".into(),
                    memory: "32Gi".into(),
                    gpu_count: 1,
                }),
                readiness_timeout_seconds: Some(1200),
            },
        );

        assert!(
            result
                .data
                .runtime_args
                .contains(&"--kv-cache-dtype".into())
        );
        assert!(result.data.runtime_args.contains(&"fp8".into()));
        assert!(
            result
                .data
                .runtime_env
                .iter()
                .any(|item| item.name == "VLLM_TEST" && item.value == "enabled")
        );
        assert_eq!(result.data.resource_requests.memory, "32Gi");
        assert_eq!(result.data.readiness_timeout_seconds, 1200);
    }

    #[test]
    fn gpu_override_exceeds_max_per_pod_returns_issue() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let result = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides {
                resource_requests: Some(ResourceRequests {
                    cpu: "2".into(),
                    memory: "16Gi".into(),
                    gpu_count: 2,
                }),
                ..DeployOverrides::empty()
            },
        );
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].field, "resource_requests.gpu_count");
        assert!(
            result.issues[0]
                .message
                .contains("cluster permits 1 GPU(s) per pod")
        );
    }

    #[test]
    fn split_form_flag_override_replaces_default_without_orphan_value() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let result = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides {
                runtime_args: vec!["--max-model-len".into(), "8192".into()],
                ..DeployOverrides::empty()
            },
        );

        let args = result.data.runtime_args;
        // The override should replace the default, not just append the value.
        let idx_flag = args
            .iter()
            .position(|a| a == "--max-model-len")
            .expect("flag present");
        assert_eq!(args[idx_flag + 1], "8192");
        assert!(
            !args.contains(&"32768".to_string()),
            "orphan default value 32768 must not remain"
        );
    }

    #[test]
    fn recipe_without_engine_defaults_to_vllm_port_8080() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let result = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        );
        assert_eq!(result.data.runtime_engine, RuntimeEngine::Vllm);
        assert_eq!(result.data.runtime_port, 8080);
    }

    #[test]
    fn sglang_recipe_defaults_port_8000() {
        let yaml = r#"
id: sglang-test
source: local
model:
  id: test/model
runtime:
  image: sglang/sglang:latest
  engine: sglang
  args: []
  env: []
hardware:
  gpu_class: gb10
  gpu_count: 1
serving:
  namespace: ai
  replicas: 1
  storage_mode: ephemeral
  ingress_policy: cluster-local
provenance:
  source: local
"#;
        let recipe = parse_recipe_yaml(yaml).expect("recipe parses");
        let result = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        );
        assert_eq!(result.data.runtime_engine, RuntimeEngine::Sglang);
        assert_eq!(result.data.runtime_port, 8000);
    }

    #[test]
    fn explicit_runtime_port_overrides_default() {
        let yaml = r#"
id: explicit-port
source: local
model:
  id: test/model
runtime:
  image: sglang/sglang:latest
  engine: sglang
  port: 9000
  args: []
  env: []
hardware:
  gpu_class: gb10
  gpu_count: 1
serving:
  namespace: ai
  replicas: 1
  storage_mode: ephemeral
  ingress_policy: cluster-local
provenance:
  source: local
"#;
        let recipe = parse_recipe_yaml(yaml).expect("recipe parses");
        let result = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        );
        assert_eq!(result.data.runtime_engine, RuntimeEngine::Sglang);
        assert_eq!(result.data.runtime_port, 9000);
    }

    #[test]
    fn digest_changes_when_engine_or_port_changes() {
        let yaml_vllm = r#"
id: digest-test
source: local
model:
  id: test/model
runtime:
  image: vllm/vllm-openai:latest
  args: []
  env: []
hardware:
  gpu_class: gb10
  gpu_count: 1
serving:
  namespace: ai
  replicas: 1
  storage_mode: ephemeral
  ingress_policy: cluster-local
provenance:
  source: local
"#;
        let recipe_vllm = parse_recipe_yaml(yaml_vllm).expect("recipe parses");
        let plan_vllm = plan_deploy(
            &recipe_vllm,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;

        let yaml_sglang = r#"
id: digest-test
source: local
model:
  id: test/model
runtime:
  image: vllm/vllm-openai:latest
  engine: sglang
  args: []
  env: []
hardware:
  gpu_class: gb10
  gpu_count: 1
serving:
  namespace: ai
  replicas: 1
  storage_mode: ephemeral
  ingress_policy: cluster-local
provenance:
  source: local
"#;
        let recipe_sglang = parse_recipe_yaml(yaml_sglang).expect("recipe parses");
        let plan_sglang = plan_deploy(
            &recipe_sglang,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;

        assert_ne!(
            plan_vllm.plan_digest, plan_sglang.plan_digest,
            "digest must differ when engine changes"
        );
    }
}
