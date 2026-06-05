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
    Ok(list
        .iter()
        .filter_map(|cm| cm.data.as_ref()?.get("record.yaml"))
        .filter_map(|input| serde_yaml::from_str(input).ok())
        .collect())
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
        Err(error) if error.to_string().contains("404") => Ok(()),
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
    Ok(list
        .iter()
        .filter_map(|cm| cm.data.as_ref()?.get("record.yaml"))
        .filter_map(|input| serde_yaml::from_str(input).ok())
        .collect())
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
}
