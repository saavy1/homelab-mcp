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
            format!(
                "ConfigMap {name} has invalid record.yaml: {error}\nrecord.yaml content:\n{raw}"
            ),
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
        homelab_mcp_core::sanitize_label_value(&record.recipe.id),
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
        homelab_mcp_core::sanitize_label_value(&record.name),
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
}
