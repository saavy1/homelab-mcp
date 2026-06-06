use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{
    Api, Client,
    api::{DeleteParams, ListParams, Patch, PatchParams},
};
use model_catalog::{
    ModelDeployment, ModelDeploymentStatus, ModelRecipe, RuntimeDeploymentRecord,
    RuntimeRecipeRecord, deployment_parts_to_record, deployment_record_to_spec,
    deployment_record_to_status, recipe_record_to_spec, recipe_spec_to_record,
};

fn runtime_name(prefix: &str, id: &str) -> String {
    format!("{}-{}", prefix, homelab_mcp_core::sanitize_dns_name(id))
}

fn recipe_resource_name(recipe_id: &str) -> String {
    runtime_name("model-recipe", recipe_id)
}

fn deployment_resource_name(name: &str) -> String {
    runtime_name("model-deployment", name)
}

/// Produce a Kubernetes label-safe value that is guaranteed to be ≤63 characters.
///
/// Sanitization rules:
/// - Only ASCII alphanumeric, '-', '_', '.' are preserved.
/// - All other characters are mapped to '-'.
/// - Leading and trailing non-alphanumeric characters are trimmed.
/// - Short safe values are preserved unchanged.
/// - Long values are truncated to a prefix followed by a deterministic hash suffix.
/// - If sanitization yields an empty string, a deterministic hash fallback is used.
fn bounded_label_value(s: &str) -> String {
    const MAX_LEN: usize = 63;

    // Step 1: Sanitize characters. Allowed: ASCII alphanumeric, '-', '_', '.'.
    let sanitized: String = s
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Step 2: Deterministic FNV-1a hash of the full sanitized string.
    let hash = {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;
        let mut h = FNV_OFFSET;
        for byte in sanitized.bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        format!("{:08x}", h)
    };

    // Step 3: Trim leading/trailing non-alphanumeric characters.
    let trimmed = sanitized.trim_matches(|c: char| !c.is_ascii_alphanumeric());

    // Step 4: Empty after trimming → return deterministic hash fallback.
    if trimmed.is_empty() {
        return hash;
    }

    // Step 5: Short enough → return directly.
    if trimmed.len() <= MAX_LEN {
        return trimmed.to_string();
    }

    // Step 6: Long value → truncate prefix + append hash.
    let max_prefix_len = MAX_LEN - 1 - hash.len();

    // Safe byte-slice because sanitized output is ASCII-only.
    let prefix_bytes = &trimmed.as_bytes()[..max_prefix_len.min(trimmed.len())];
    let mut prefix = String::from_utf8(prefix_bytes.to_vec()).unwrap();

    // Trim trailing non-alphanumeric characters.
    while let Some(last) = prefix.chars().last() {
        if last.is_ascii_alphanumeric() {
            break;
        }
        prefix.pop();
    }

    // Trim leading non-alphanumeric characters.
    while let Some(first) = prefix.chars().next() {
        if first.is_ascii_alphanumeric() {
            break;
        }
        prefix.remove(0);
    }

    if prefix.is_empty() {
        hash
    } else {
        format!("{}-{}", prefix, hash)
    }
}

pub async fn upsert_runtime_recipe(
    client: Client,
    namespace: &str,
    record: &RuntimeRecipeRecord,
) -> Result<String, kube::Error> {
    let api: Api<ModelRecipe> = Api::namespaced(client, namespace);
    let name = recipe_resource_name(&record.recipe.id);
    let mut recipe = ModelRecipe::new(&name, recipe_record_to_spec(record));
    recipe.metadata = ObjectMeta {
        name: Some(name.clone()),
        namespace: Some(namespace.into()),
        labels: Some(BTreeMap::from([
            (
                "app.kubernetes.io/part-of".into(),
                "model-catalog-mcp".into(),
            ),
            ("models.saavylab.dev/kind".into(), "recipe".into()),
            (
                "models.saavylab.dev/recipe-id".into(),
                bounded_label_value(&record.recipe.id),
            ),
        ])),
        ..Default::default()
    };
    let patch = Patch::Apply(&recipe);
    let params = PatchParams::apply("model-catalog-mcp").force();
    let applied = api.patch(&name, &params, &patch).await?;
    Ok(applied.metadata.name.unwrap_or(name))
}

pub async fn list_runtime_recipes(
    client: Client,
    namespace: &str,
) -> Result<Vec<RuntimeRecipeRecord>, kube::Error> {
    let api: Api<ModelRecipe> = Api::namespaced(client, namespace);
    let list = api
        .list(&ListParams::default().labels("models.saavylab.dev/kind=recipe"))
        .await?;
    Ok(list
        .iter()
        .map(|recipe| recipe_spec_to_record(&recipe.spec))
        .collect())
}

pub async fn get_runtime_recipe(
    client: Client,
    namespace: &str,
    recipe_id: &str,
) -> Result<Option<RuntimeRecipeRecord>, kube::Error> {
    let api: Api<ModelRecipe> = Api::namespaced(client, namespace);
    let name = recipe_resource_name(recipe_id);
    match api.get_opt(&name).await? {
        Some(recipe) => Ok(Some(recipe_spec_to_record(&recipe.spec))),
        None => Ok(None),
    }
}

pub async fn delete_runtime_recipe(
    client: Client,
    namespace: &str,
    recipe_id: &str,
) -> Result<(), kube::Error> {
    let api: Api<ModelRecipe> = Api::namespaced(client, namespace);
    let name = recipe_resource_name(recipe_id);
    match api.delete(&name, &DeleteParams::default()).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(status)) if status.code == 404 => Ok(()),
        Err(error) => Err(error),
    }
}

pub async fn upsert_runtime_deployment(
    client: Client,
    namespace: &str,
    record: &RuntimeDeploymentRecord,
) -> Result<String, kube::Error> {
    let api: Api<ModelDeployment> = Api::namespaced(client.clone(), namespace);
    let name = deployment_resource_name(&record.name);
    let mut deployment = ModelDeployment::new(&name, deployment_record_to_spec(record));
    deployment.metadata = ObjectMeta {
        name: Some(name.clone()),
        namespace: Some(namespace.into()),
        labels: Some(BTreeMap::from([
            (
                "app.kubernetes.io/part-of".into(),
                "model-catalog-mcp".into(),
            ),
            ("models.saavylab.dev/kind".into(), "deployment".into()),
            (
                "models.saavylab.dev/deployment-name".into(),
                bounded_label_value(&record.name),
            ),
            (
                "models.saavylab.dev/target".into(),
                bounded_label_value(&record.target),
            ),
        ])),
        ..Default::default()
    };
    let patch = Patch::Apply(&deployment);
    let params = PatchParams::apply("model-catalog-mcp").force();
    let applied = api.patch(&name, &params, &patch).await?;
    let result = applied.metadata.name.unwrap_or(name);
    update_runtime_deployment_status(
        client,
        namespace,
        &record.name,
        &deployment_record_to_status(record),
    )
    .await?;
    Ok(result)
}

pub async fn list_runtime_deployments(
    client: Client,
    namespace: &str,
) -> Result<Vec<RuntimeDeploymentRecord>, kube::Error> {
    let api: Api<ModelDeployment> = Api::namespaced(client, namespace);
    let list = api
        .list(&ListParams::default().labels("models.saavylab.dev/kind=deployment"))
        .await?;
    Ok(list
        .iter()
        .map(|deployment| deployment_parts_to_record(&deployment.spec, deployment.status.as_ref()))
        .collect())
}

pub async fn get_runtime_deployment(
    client: Client,
    namespace: &str,
    name: &str,
) -> Result<Option<RuntimeDeploymentRecord>, kube::Error> {
    let api: Api<ModelDeployment> = Api::namespaced(client, namespace);
    let name = deployment_resource_name(name);
    match api.get_opt(&name).await? {
        Some(deployment) => Ok(Some(deployment_parts_to_record(
            &deployment.spec,
            deployment.status.as_ref(),
        ))),
        None => Ok(None),
    }
}

pub async fn update_runtime_deployment_status(
    client: Client,
    namespace: &str,
    name: &str,
    status: &ModelDeploymentStatus,
) -> Result<(), kube::Error> {
    let api: Api<ModelDeployment> = Api::namespaced(client, namespace);
    let resource_name = deployment_resource_name(name);
    let patch = serde_json::json!({ "status": status });
    api.patch_status(
        &resource_name,
        &PatchParams::default(),
        &Patch::Merge(&patch),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_resource_name_sanitizes_recipe_ids() {
        assert_eq!(
            recipe_resource_name("deepseek-ai/DeepSeek-V4-Flash"),
            "model-recipe-deepseek-ai-deepseek-v4-flash"
        );
    }

    #[test]
    fn deployment_resource_name_sanitizes_names() {
        assert_eq!(
            deployment_resource_name("lfm25-350m"),
            "model-deployment-lfm25-350m"
        );
    }

    #[test]
    fn bounded_label_value_keeps_values_under_limit() {
        let value = bounded_label_value(
            "deepseek-ai/DeepSeek-V4-Flash-with-a-very-long-suffix-that-exceeds-kubernetes-label-limits",
        );
        assert!(value.len() <= 63);
    }

    #[test]
    fn bounded_label_value_spaces_replaced() {
        let result = bounded_label_value("foo bar");
        assert!(!result.contains(' '), "should not contain space: {result}");
        assert_eq!(result, "foo-bar", "spaces should map to hyphens: {result}");
    }

    #[test]
    fn bounded_label_value_trims_punctuation_ends() {
        let result = bounded_label_value("-bad-");
        let first = result.chars().next().unwrap();
        let last = result.chars().last().unwrap();
        assert!(
            first.is_ascii_alphanumeric(),
            "should start with alphanumeric: {result}"
        );
        assert!(
            last.is_ascii_alphanumeric(),
            "should end with alphanumeric: {result}"
        );
        assert!(
            result.contains("bad"),
            "should contain core value: {result}"
        );
    }

    #[test]
    fn bounded_label_value_non_ascii_fallback() {
        let result = bounded_label_value("你好世界");
        assert!(!result.is_empty(), "should not be empty: {result}");
        assert!(result.len() <= 63, "should fit label limit: {result}");
        assert!(
            result
                .chars()
                .all(|c| { c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' }),
            "should only contain ASCII label-safe chars: {result}"
        );
    }

    #[test]
    fn bounded_label_value_long_invalid_deterministic() {
        let long = "foo bar baz ".repeat(20);
        let r1 = bounded_label_value(&long);
        let r2 = bounded_label_value(&long);
        assert_eq!(r1, r2, "should be deterministic");
        assert!(r1.len() <= 63, "should fit label limit: {r1}");
        assert!(!r1.contains(' '), "should not contain space: {r1}");
    }

    #[test]
    fn bounded_label_value_short_unchanged() {
        assert_eq!(bounded_label_value("short"), "short");
        assert_eq!(
            bounded_label_value("deepseek-ai/DeepSeek-V4-Flash"),
            "deepseek-ai-deepseek-v4-flash"
        );
    }

    #[test]
    fn bounded_label_value_exactly_63() {
        let s = "a".repeat(63);
        assert_eq!(bounded_label_value(&s), s);
    }

    #[test]
    fn bounded_label_value_long_truncated() {
        let s = "a".repeat(100);
        let result = bounded_label_value(&s);
        assert!(result.len() <= 63, "result length {} > 63", result.len());
        assert!(result.starts_with('a'), "should preserve prefix");
        assert!(result.contains('-'), "should contain hash separator");
    }

    #[test]
    fn bounded_label_value_deterministic() {
        let long = "deepseek-ai/DeepSeek-V4-Flash-Extended-Version-With-Extra-Long-Identifier-That-Exceeds-The-Sixty-Three-Character-Limit";
        let r1 = bounded_label_value(long);
        let r2 = bounded_label_value(long);
        assert_eq!(r1, r2, "should be deterministic");
        assert!(r1.len() <= 63, "should fit label limit: {r1}");
    }

    #[test]
    fn bounded_label_value_trims_invalid_ends() {
        let long = "a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x.y.z.1.2.3.4.5.6.7.8.9.0.a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x.y.z";
        let result = bounded_label_value(long);
        assert!(result.len() <= 63, "should fit label limit: {result}");
        let first = result.chars().next().unwrap();
        let last = result.chars().last().unwrap();
        assert!(
            first.is_ascii_alphanumeric(),
            "should start with alphanumeric: {result}"
        );
        assert!(
            last.is_ascii_alphanumeric(),
            "should end with alphanumeric: {result}"
        );
    }
}
