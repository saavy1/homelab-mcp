use homelab_mcp_core::ValidationIssue;

use crate::{ClusterProfile, DeploymentPlan, Recipe};

pub fn validate_plan_policy(
    recipe: &Recipe,
    profile: &ClusterProfile,
    plan: &DeploymentPlan,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if enables_auto_tool_choice(plan) && !has_tool_call_parser(plan) {
        issues.push(ValidationIssue {
            field: "runtime.args".into(),
            message: "--enable-auto-tool-choice requires --tool-call-parser".into(),
            allowed: Some("--tool-call-parser hermes or --tool-call-parser=hermes".into()),
        });
    }

    if recipe
        .model
        .id
        .eq_ignore_ascii_case("deepseek-ai/DeepSeek-V4-Flash")
        && !has_kv_cache_dtype_fp8(plan)
    {
        issues.push(ValidationIssue {
            field: "runtime.args".into(),
            message: "DeepSeek V4 Flash requires fp8 KV cache on the current vLLM runtime".into(),
            allowed: Some("--kv-cache-dtype fp8 or --kv-cache-dtype=fp8".into()),
        });
    }

    if plan.resource_requests.gpu_count > profile.max_gpu_per_pod {
        issues.push(ValidationIssue {
            field: "resource_requests.gpu_count".into(),
            message: format!(
                "deployment requests {} GPUs, profile permits {} per pod",
                plan.resource_requests.gpu_count, profile.max_gpu_per_pod
            ),
            allowed: Some(format!("1..={}", profile.max_gpu_per_pod)),
        });
    }

    let allowed_root = &profile.model_storage.gpu_node_path;
    if !plan.model_path.starts_with(allowed_root) {
        issues.push(ValidationIssue {
            field: "model_path".into(),
            message: format!("model path {} is outside approved root", plan.model_path),
            allowed: Some(allowed_root.clone()),
        });
    }

    issues
}

fn enables_auto_tool_choice(plan: &DeploymentPlan) -> bool {
    plan.runtime_args
        .iter()
        .any(|arg| arg == "--enable-auto-tool-choice")
}

fn has_tool_call_parser(plan: &DeploymentPlan) -> bool {
    has_arg_value(&plan.runtime_args, "--tool-call-parser", "hermes")
        || plan
            .runtime_args
            .iter()
            .any(|arg| arg.starts_with("--tool-call-parser="))
}

fn has_kv_cache_dtype_fp8(plan: &DeploymentPlan) -> bool {
    has_arg_value(&plan.runtime_args, "--kv-cache-dtype", "fp8")
        || plan
            .runtime_args
            .iter()
            .any(|arg| arg == "--kv-cache-dtype=fp8")
}

fn has_arg_value(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2)
        .any(|window| window[0] == flag && window[1] == value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClusterProfile, DeployOverrides, parse_recipe_yaml, plan_deploy};

    #[test]
    fn rejects_auto_tool_choice_without_parser() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/deepseek-v4-flash.yaml"
        ))
        .expect("recipe parses");
        let mut plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        plan.runtime_args = vec![
            "--enable-auto-tool-choice".into(),
            "--kv-cache-dtype=fp8".into(),
        ];

        let issues = validate_plan_policy(&recipe, &ClusterProfile::superbloom_default(), &plan);

        assert!(issues.iter().any(|issue| {
            issue.field == "runtime.args" && issue.message.contains("requires --tool-call-parser")
        }));
    }

    #[test]
    fn rejects_deepseek_without_fp8_kv_cache() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/deepseek-v4-flash.yaml"
        ))
        .expect("recipe parses");
        let mut plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        plan.runtime_args = vec!["--tool-call-parser=hermes".into()];

        let issues = validate_plan_policy(&recipe, &ClusterProfile::superbloom_default(), &plan);

        assert!(issues.iter().any(|issue| {
            issue.field == "runtime.args" && issue.message.contains("requires fp8 KV cache")
        }));
    }

    #[test]
    fn accepts_deepseek_with_fp8_and_parser() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/deepseek-v4-flash.yaml"
        ))
        .expect("recipe parses");
        let mut plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        plan.runtime_args = vec![
            "--enable-auto-tool-choice".into(),
            "--tool-call-parser".into(),
            "hermes".into(),
            "--kv-cache-dtype".into(),
            "fp8".into(),
        ];

        let issues = validate_plan_policy(&recipe, &ClusterProfile::superbloom_default(), &plan);

        assert!(issues.is_empty());
    }

    #[test]
    fn accepts_split_form_tool_call_parser() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/deepseek-v4-flash.yaml"
        ))
        .expect("recipe parses");
        let mut plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        plan.runtime_args = vec![
            "--enable-auto-tool-choice".into(),
            "--tool-call-parser".into(),
            "hermes".into(),
            "--kv-cache-dtype=fp8".into(),
        ];

        let issues = validate_plan_policy(&recipe, &ClusterProfile::superbloom_default(), &plan);

        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn accepts_split_form_kv_cache_dtype() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/deepseek-v4-flash.yaml"
        ))
        .expect("recipe parses");
        let mut plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        plan.runtime_args = vec![
            "--tool-call-parser=hermes".into(),
            "--kv-cache-dtype".into(),
            "fp8".into(),
        ];

        let issues = validate_plan_policy(&recipe, &ClusterProfile::superbloom_default(), &plan);

        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn rejects_gpu_count_exceeding_profile_max() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let mut plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        plan.resource_requests.gpu_count = 4;

        let issues = validate_plan_policy(&recipe, &ClusterProfile::superbloom_default(), &plan);

        assert!(issues.iter().any(|issue| {
            issue.field == "resource_requests.gpu_count"
                && issue.message.contains("profile permits 1 per pod")
        }));
    }

    #[test]
    fn rejects_model_path_outside_approved_root() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let mut plan = plan_deploy(
            &recipe,
            &ClusterProfile::superbloom_default(),
            DeployOverrides::empty(),
        )
        .data;
        plan.model_path = "/tmp/models/Qwen/Qwen3-8B".into();

        let issues = validate_plan_policy(&recipe, &ClusterProfile::superbloom_default(), &plan);

        assert!(issues.iter().any(|issue| {
            issue.field == "model_path" && issue.message.contains("outside approved root")
        }));
    }
}
