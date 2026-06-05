use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ConfigMap;
use kube::{
    Api, Client,
    api::{DeleteParams, ListParams, Patch, PatchParams},
};
use model_catalog::{RuntimeDeploymentRecord, RuntimeRecipeRecord};

const RECIPE_LABEL: &str = "homelab.saavylab.dev/model-catalog-kind=runtime-recipe";
const DEPLOYMENT_LABEL: &str = "homelab.saavylab.dev/model-catalog-kind=runtime-deployment";

fn runtime_name(prefix: &str, id: &str) -> String {
    format!("{}-{}", prefix, homelab_mcp_core::sanitize_dns_name(id))
}

/// Produce a Kubernetes label-safe value that is guaranteed to be ≤63 characters.
/// Short safe values are preserved unchanged. Long values are truncated to a prefix
/// followed by a deterministic 8-character hex hash suffix.
fn bounded_label_value(s: &str) -> String {
    let sanitized = homelab_mcp_core::sanitize_label_value(s);
    const MAX_LEN: usize = 63;

    if sanitized.len() <= MAX_LEN {
        return sanitized;
    }

    // Deterministic FNV-1a hash, formatted as 8 hex characters.
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

    // Reserve space for '-' separator + hash suffix.
    let max_prefix_len = MAX_LEN - 1 - hash.len();
    let mut prefix = sanitized[..max_prefix_len.min(sanitized.len())].to_string();

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

fn decode_record<T: serde::de::DeserializeOwned>(cm: &ConfigMap) -> Result<T, kube::Error> {
    let name = cm.metadata.name.as_deref().unwrap_or("<unknown>");
    let data = cm.data.as_ref().ok_or_else(|| {
        kube::Error::Service(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("ConfigMap {name} is missing data section"),
        )))
    })?;
    let raw = data.get("record.yaml").ok_or_else(|| {
        kube::Error::Service(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("ConfigMap {name} is missing record.yaml"),
        )))
    })?;
    serde_yaml::from_str(raw).map_err(|error| {
        kube::Error::Service(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("ConfigMap {name} has invalid record.yaml: {error}"),
        )))
    })
}

pub async fn upsert_runtime_recipe(
    client: Client,
    namespace: &str,
    record: &RuntimeRecipeRecord,
) -> Result<String, kube::Error> {
    let api: Api<ConfigMap> = Api::namespaced(client, namespace);
    let name = runtime_name("model-recipe", &record.recipe.id);
    let mut labels = BTreeMap::new();
    labels.insert(
        "homelab.saavylab.dev/model-catalog-kind".into(),
        "runtime-recipe".into(),
    );
    labels.insert(
        "homelab.saavylab.dev/recipe-id".into(),
        bounded_label_value(&record.recipe.id),
    );
    let mut data = BTreeMap::new();
    data.insert(
        "record.yaml".into(),
        serde_yaml::to_string(record).map_err(|error| kube::Error::Service(Box::new(error)))?,
    );
    let cm = ConfigMap {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.into()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    };
    let patch = Patch::Apply(&cm);
    let params = PatchParams::apply("model-catalog-mcp").force();
    let applied = api.patch(&name, &params, &patch).await?;
    Ok(applied.metadata.name.unwrap_or(name))
}

pub async fn list_runtime_recipes(
    client: Client,
    namespace: &str,
) -> Result<Vec<RuntimeRecipeRecord>, kube::Error> {
    let api: Api<ConfigMap> = Api::namespaced(client, namespace);
    let list = api
        .list(&ListParams::default().labels(RECIPE_LABEL))
        .await?;
    list.iter().map(decode_record).collect()
}

pub async fn get_runtime_recipe(
    client: Client,
    namespace: &str,
    recipe_id: &str,
) -> Result<Option<RuntimeRecipeRecord>, kube::Error> {
    let records = list_runtime_recipes(client, namespace).await?;
    Ok(records
        .into_iter()
        .find(|record| record.recipe.id == recipe_id))
}

pub async fn delete_runtime_recipe(
    client: Client,
    namespace: &str,
    recipe_id: &str,
) -> Result<(), kube::Error> {
    let api: Api<ConfigMap> = Api::namespaced(client, namespace);
    let name = runtime_name("model-recipe", recipe_id);
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
    let api: Api<ConfigMap> = Api::namespaced(client, namespace);
    let name = runtime_name("model-deployment", &record.name);
    let mut labels = BTreeMap::new();
    labels.insert(
        "homelab.saavylab.dev/model-catalog-kind".into(),
        "runtime-deployment".into(),
    );
    labels.insert(
        "homelab.saavylab.dev/deployment-name".into(),
        bounded_label_value(&record.name),
    );
    let mut data = BTreeMap::new();
    data.insert(
        "record.yaml".into(),
        serde_yaml::to_string(record).map_err(|error| kube::Error::Service(Box::new(error)))?,
    );
    let cm = ConfigMap {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.into()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    };
    let patch = Patch::Apply(&cm);
    let params = PatchParams::apply("model-catalog-mcp").force();
    let applied = api.patch(&name, &params, &patch).await?;
    Ok(applied.metadata.name.unwrap_or(name))
}

pub async fn list_runtime_deployments(
    client: Client,
    namespace: &str,
) -> Result<Vec<RuntimeDeploymentRecord>, kube::Error> {
    let api: Api<ConfigMap> = Api::namespaced(client, namespace);
    let list = api
        .list(&ListParams::default().labels(DEPLOYMENT_LABEL))
        .await?;
    list.iter().map(decode_record).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_name_sanitizes_ids() {
        assert_eq!(
            runtime_name("model-recipe", "deepseek-ai/DeepSeek-V4-Flash"),
            "model-recipe-deepseek-ai-deepseek-v4-flash"
        );
    }

    #[test]
    fn decode_record_missing_record_yaml() {
        let cm = ConfigMap {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("test-cm".into()),
                ..Default::default()
            },
            data: Some(BTreeMap::new()),
            ..Default::default()
        };
        let result: Result<RuntimeRecipeRecord, kube::Error> = decode_record(&cm);
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("test-cm"),
            "error should mention ConfigMap name: {msg}"
        );
        assert!(
            msg.contains("record.yaml"),
            "error should mention record.yaml: {msg}"
        );
    }

    #[test]
    fn decode_record_malformed_yaml() {
        let mut data = BTreeMap::new();
        data.insert("record.yaml".into(), "not: valid: [yaml".into());
        let cm = ConfigMap {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("bad-cm".into()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        };
        let result: Result<RuntimeRecipeRecord, kube::Error> = decode_record(&cm);
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("bad-cm"),
            "error should mention ConfigMap name: {msg}"
        );
        assert!(
            msg.contains("record.yaml"),
            "error should mention record.yaml: {msg}"
        );
    }

    #[test]
    fn decode_record_valid_yaml() {
        let yaml = r#"
name: test-deployment
namespace: ai
recipe_id: test-recipe
target: spark
runtime_args: []
runtime_env: []
resources:
  cpu: "2"
  memory: "16Gi"
  gpu_count: 1
status: planned
last_plan_digest: abc123
created_by: test
created_at: "2024-01-01T00:00:00Z"
"#;
        let mut data = BTreeMap::new();
        data.insert("record.yaml".into(), yaml.into());
        let cm = ConfigMap {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("good-cm".into()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        };
        let record: RuntimeDeploymentRecord = decode_record(&cm).unwrap();
        assert_eq!(record.name, "test-deployment");
        assert_eq!(record.namespace, "ai");
        assert_eq!(record.recipe_id, "test-recipe");
    }

    #[test]
    fn decode_record_malformed_yaml_does_not_leak_content() {
        let mut data = BTreeMap::new();
        data.insert("record.yaml".into(), "secret-password: abc123".into());
        let cm = ConfigMap {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("bad-cm".into()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        };
        let result: Result<RuntimeRecipeRecord, kube::Error> = decode_record(&cm);
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("bad-cm"),
            "error should mention ConfigMap name: {msg}"
        );
        assert!(
            msg.contains("record.yaml"),
            "error should mention record.yaml: {msg}"
        );
        assert!(
            !msg.contains("secret-password"),
            "error should NOT leak raw record.yaml content: {msg}"
        );
        assert!(
            !msg.contains("abc123"),
            "error should NOT leak raw record.yaml content: {msg}"
        );
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
