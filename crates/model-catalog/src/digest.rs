use crate::DeploymentPlan;
use homelab_mcp_core::compute_digest;
use serde_json::Value;

pub fn plan_to_digest_input(plan: &DeploymentPlan) -> String {
    let mut value = serde_json::to_value(plan).expect("plan serializes");
    remove_digest_field(&mut value);
    serde_json::to_string(&value).expect("canonical JSON")
}

pub fn compute_plan_digest(plan: &DeploymentPlan) -> String {
    compute_digest(&plan_to_digest_input(plan))
}

fn remove_digest_field(value: &mut Value) {
    if let Value::Object(map) = value {
        map.remove("plan_digest");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClusterProfile, DeployOverrides, parse_recipe_yaml, plan_deploy};

    #[test]
    fn digest_excludes_itself() {
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
        let input = plan_to_digest_input(&plan);
        assert!(!input.contains("plan_digest"));
    }

    #[test]
    fn same_plan_produces_same_digest() {
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
        assert_eq!(plan.plan_digest, compute_plan_digest(&plan));
    }
}
