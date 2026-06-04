use crate::digest::compute_plan_digest;
use crate::{
    ClusterProfile, DeploymentPlan, EnvVar, NodeRole, Recipe, ResourceRequests, StorageMode,
};
use homelab_mcp_core::{ToolResult, ValidationIssue, sanitize_dns_name};

#[derive(Clone, Debug, PartialEq)]
pub struct DeployOverrides {
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub replicas: Option<u32>,
    pub env_overrides: Vec<EnvVar>,
}

impl DeployOverrides {
    pub fn empty() -> Self {
        Self {
            name: None,
            namespace: None,
            replicas: None,
            env_overrides: Vec::new(),
        }
    }
}

pub fn plan_deploy(
    recipe: &Recipe,
    profile: &ClusterProfile,
    overrides: DeployOverrides,
) -> ToolResult<DeploymentPlan> {
    let name = sanitize_dns_name(&overrides
        .name
        .clone()
        .or_else(|| recipe.serving.service_name.clone())
        .unwrap_or_else(|| recipe.id.clone()));
    let namespace = overrides
        .namespace
        .clone()
        .unwrap_or_else(|| recipe.serving.namespace.clone());
    let replicas = overrides.replicas.unwrap_or(recipe.serving.replicas);
    let mut plan = DeploymentPlan {
        name,
        namespace,
        recipe_id: recipe.id.clone(),
        selected_gpu_class: recipe.hardware.gpu_class.clone(),
        replicas,
        scale_to_zero: replicas == 0,
        storage_mode: recipe.serving.storage_mode.clone(),
        ingress_policy: recipe.serving.ingress_policy.clone(),
        env_overrides: overrides.env_overrides,
        resource_requests: ResourceRequests {
            cpu: "2".into(),
            memory: "16Gi".into(),
            gpu_count: recipe.hardware.gpu_count,
        },
        model_id: recipe.model.id.clone(),
        model_revision: recipe.model.revision.clone(),
        model_path: format!(
            "{}/{}",
            profile.model_storage.gpu_node_path,
            recipe.model.id
        ),
        plan_digest: String::new(),
    };
    plan.plan_digest = compute_plan_digest(&plan);
    let issues = validate_fit(recipe, profile, &plan);
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
    if recipe.hardware.gpu_count > profile.max_gpu_per_pod {
        issues.push(ValidationIssue {
            field: "hardware.gpu_count".into(),
            message: format!(
                "recipe requests {} GPU(s), cluster permits {} GPU(s) per pod",
                recipe.hardware.gpu_count, profile.max_gpu_per_pod
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
}
