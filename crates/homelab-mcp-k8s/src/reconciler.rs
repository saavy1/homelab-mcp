use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::{Api, Client, api::ListParams, core::dynamic::DynamicObject, discovery::ApiResource};
use model_catalog::{DeploymentState, ModelDeployment, ModelDeploymentStatus};

use crate::update_runtime_deployment_status;

/// Build a `ModelDeploymentStatus` with transition-time logic.
///
/// `last_transition_time` is updated only when `state` differs from the
/// existing status (or when there is no existing status).
fn build_status(
    existing: Option<&ModelDeploymentStatus>,
    state: DeploymentState,
    observed_generation: Option<i64>,
    kserve_ready: bool,
    failure_reason: Option<String>,
    url: Option<String>,
) -> ModelDeploymentStatus {
    let last_transition_time = match existing {
        Some(existing) if existing.state == state => existing.last_transition_time.clone(),
        _ => Some(chrono::Utc::now().to_rfc3339()),
    };

    ModelDeploymentStatus {
        state,
        observed_generation,
        last_transition_time,
        failure_reason,
        kserve_ready,
        url,
    }
}

/// Compute a `ModelDeploymentStatus` from KServe `InferenceService` conditions.
///
/// * `Ready=True`  → `state=Ready`, `kserve_ready=true`, failure reason cleared.
/// * `Ready=False` → `state=Failed`, `kserve_ready=false`, failure reason from
///   condition message or reason.
/// * `Ready=Unknown` or missing Ready → `state=Applying`, `kserve_ready=false`.
pub fn status_from_kserve_conditions(
    existing: Option<&ModelDeploymentStatus>,
    observed_generation: Option<i64>,
    conditions: Vec<Condition>,
    url: Option<String>,
) -> ModelDeploymentStatus {
    let ready = conditions.iter().find(|c| c.type_ == "Ready");

    match ready.map(|c| c.status.as_str()) {
        Some("True") => build_status(
            existing,
            DeploymentState::Ready,
            observed_generation,
            true,
            None,
            url,
        ),
        Some("False") => {
            let reason = ready
                .map(|c| c.message.clone())
                .filter(|m| !m.is_empty())
                .or_else(|| ready.map(|c| c.reason.clone()).filter(|r| !r.is_empty()))
                .unwrap_or_else(|| "KServe Ready condition is False".into());
            build_status(
                existing,
                DeploymentState::Failed,
                observed_generation,
                false,
                Some(reason),
                url,
            )
        }
        _ => build_status(
            existing,
            DeploymentState::Applying,
            observed_generation,
            false,
            None,
            url,
        ),
    }
}

/// Compute a `ModelDeploymentStatus` when the matching InferenceService is missing.
pub fn missing_inferenceservice_status(
    existing: Option<&ModelDeploymentStatus>,
    observed_generation: Option<i64>,
) -> ModelDeploymentStatus {
    let url = existing.and_then(|e| e.url.clone());
    build_status(
        existing,
        DeploymentState::Failed,
        observed_generation,
        false,
        Some("KServe InferenceService not found".into()),
        url,
    )
}

/// Return a dynamic API handle for KServe InferenceServices.
pub fn inferenceservice_api(client: Client, namespace: &str) -> Api<DynamicObject> {
    let ar = ApiResource {
        group: "serving.kserve.io".into(),
        version: "v1beta1".into(),
        kind: "InferenceService".into(),
        api_version: "serving.kserve.io/v1beta1".into(),
        plural: "inferenceservices".into(),
    };
    Api::namespaced_with(client, namespace, &ar)
}

/// Parse `status.conditions` from a dynamic InferenceService object.
pub fn dynamic_conditions(object: &DynamicObject) -> Vec<Condition> {
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
    use k8s_openapi::jiff::Timestamp;

    object
        .data
        .get("status")
        .and_then(|s| s.get("conditions"))
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let type_ = c.get("type")?.as_str()?.to_string();
                    let status = c.get("status")?.as_str()?.to_string();
                    let reason = c
                        .get("reason")
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    let message = c
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("")
                        .to_string();
                    let last_transition_time = c
                        .get("lastTransitionTime")
                        .and_then(|lt| serde_json::from_value(lt.clone()).ok())
                        .unwrap_or_else(|| Time(Timestamp::from_second(0).unwrap()));

                    Some(Condition {
                        last_transition_time,
                        message,
                        observed_generation: None,
                        reason,
                        status,
                        type_,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Read `status.url` from a dynamic InferenceService object.
pub fn dynamic_url(object: &DynamicObject) -> Option<String> {
    object
        .data
        .get("status")
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .map(String::from)
}

/// Reconcile all active `ModelDeployment` records once.
///
/// 1. Lists `ModelDeployment` CRDs in `state_namespace` filtered by
///    `models.saavylab.dev/kind=deployment`.
/// 2. Skips deployments whose status state is `Stopped`.
/// 3. Fetches the matching KServe `InferenceService` in
///    `deployment.spec.namespace` / `deployment.spec.name`.
/// 4. Computes and patches the deployment status accordingly.
///
/// Per-deployment errors are logged and do **not** abort the loop.
pub async fn reconcile_model_deployments_once(
    client: Client,
    state_namespace: &str,
) -> Result<(), kube::Error> {
    let api: Api<ModelDeployment> = Api::namespaced(client.clone(), state_namespace);
    let list = api
        .list(&ListParams::default().labels("models.saavylab.dev/kind=deployment"))
        .await?;

    for deployment in list {
        let existing_status = deployment.status.as_ref();

        if let Some(s) = existing_status
            && s.state == DeploymentState::Stopped
        {
            continue;
        }

        let isvc_name = &deployment.spec.name;
        let isvc_namespace = &deployment.spec.namespace;
        let isvc_api = inferenceservice_api(client.clone(), isvc_namespace);

        let new_status = match isvc_api.get(isvc_name).await {
            Ok(object) => {
                let conditions = dynamic_conditions(&object);
                let url = dynamic_url(&object);
                status_from_kserve_conditions(
                    existing_status,
                    deployment.metadata.generation,
                    conditions,
                    url,
                )
            }
            Err(kube::Error::Api(err)) if err.code == 404 => {
                missing_inferenceservice_status(existing_status, deployment.metadata.generation)
            }
            Err(e) => {
                tracing::warn!(
                    deployment = %deployment.spec.name,
                    namespace = %isvc_namespace,
                    error = %e,
                    "failed to get InferenceService"
                );
                continue;
            }
        };

        if let Err(e) = update_runtime_deployment_status(
            client.clone(),
            state_namespace,
            &deployment.spec.name,
            &new_status,
        )
        .await
        {
            tracing::warn!(
                deployment = %deployment.spec.name,
                error = %e,
                "failed to update deployment status"
            );
        }
    }

    Ok(())
}

/// Run the model-deployment reconciler in a tokio interval loop.
///
/// Errors from each reconciliation pass are logged as warnings; the loop
/// never panics or exits on transient failures.
pub async fn run_model_deployment_reconciler(
    client: Client,
    state_namespace: String,
    interval: std::time::Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        if let Err(e) = reconcile_model_deployments_once(client.clone(), &state_namespace).await {
            tracing::warn!(
                namespace = %state_namespace,
                error = %e,
                "model deployment reconciliation failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;

    fn make_condition(
        type_: &str,
        status: &str,
        reason: Option<&str>,
        message: Option<&str>,
    ) -> Condition {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
        let now = Time(k8s_openapi::jiff::Timestamp::now());
        Condition {
            last_transition_time: now,
            message: message.unwrap_or("").into(),
            observed_generation: None,
            reason: reason.unwrap_or("").into(),
            status: status.into(),
            type_: type_.into(),
        }
    }

    #[test]
    fn ready_true_maps_to_ready_and_kserve_ready_true() {
        let conditions = vec![make_condition("Ready", "True", None, None)];
        let status =
            status_from_kserve_conditions(None, Some(1), conditions, Some("http://url".into()));
        assert_eq!(status.state, DeploymentState::Ready);
        assert!(status.kserve_ready);
        assert_eq!(status.failure_reason, None);
        assert_eq!(status.url, Some("http://url".into()));
        assert_eq!(status.observed_generation, Some(1));
        assert!(status.last_transition_time.is_some());
    }

    #[test]
    fn ready_false_maps_to_failed_with_failure_reason() {
        let conditions = vec![make_condition(
            "Ready",
            "False",
            Some("CrashLoopBackOff"),
            Some("container crashed"),
        )];
        let status = status_from_kserve_conditions(None, Some(2), conditions, None);
        assert_eq!(status.state, DeploymentState::Failed);
        assert!(!status.kserve_ready);
        assert_eq!(status.failure_reason, Some("container crashed".into()));
        assert_eq!(status.observed_generation, Some(2));
    }

    #[test]
    fn ready_false_uses_reason_when_message_missing() {
        let conditions = vec![make_condition("Ready", "False", Some("SomeReason"), None)];
        let status = status_from_kserve_conditions(None, None, conditions, None);
        assert_eq!(status.state, DeploymentState::Failed);
        assert_eq!(status.failure_reason, Some("SomeReason".into()));
    }

    #[test]
    fn missing_ready_condition_maps_to_applying() {
        let conditions = vec![make_condition("Initialized", "True", None, None)];
        let status = status_from_kserve_conditions(None, Some(1), conditions, None);
        assert_eq!(status.state, DeploymentState::Applying);
        assert!(!status.kserve_ready);
    }

    #[test]
    fn ready_unknown_maps_to_applying_and_kserve_ready_false() {
        let conditions = vec![make_condition("Ready", "Unknown", None, None)];
        let status = status_from_kserve_conditions(None, Some(1), conditions, None);
        assert_eq!(status.state, DeploymentState::Applying);
        assert!(!status.kserve_ready);
        assert_eq!(status.failure_reason, None);
        assert_eq!(status.observed_generation, Some(1));
    }

    #[test]
    fn missing_inferenceservice_maps_to_failed_with_exact_reason() {
        let status = missing_inferenceservice_status(None, Some(1));
        assert_eq!(status.state, DeploymentState::Failed);
        assert!(!status.kserve_ready);
        assert_eq!(
            status.failure_reason,
            Some("KServe InferenceService not found".into())
        );
        assert_eq!(status.observed_generation, Some(1));
    }

    #[test]
    fn transition_time_preserved_when_state_unchanged() {
        let existing = ModelDeploymentStatus {
            state: DeploymentState::Ready,
            observed_generation: Some(1),
            last_transition_time: Some("2024-01-01T00:00:00Z".into()),
            failure_reason: None,
            kserve_ready: true,
            url: Some("http://old".into()),
        };
        let conditions = vec![make_condition("Ready", "True", None, None)];
        let status = status_from_kserve_conditions(
            Some(&existing),
            Some(2),
            conditions,
            Some("http://new".into()),
        );
        assert_eq!(status.state, DeploymentState::Ready);
        assert_eq!(
            status.last_transition_time,
            Some("2024-01-01T00:00:00Z".into())
        );
        assert_eq!(status.url, Some("http://new".into()));
    }

    #[test]
    fn transition_time_updated_when_state_changes() {
        let existing = ModelDeploymentStatus {
            state: DeploymentState::Applying,
            observed_generation: Some(1),
            last_transition_time: Some("2024-01-01T00:00:00Z".into()),
            failure_reason: None,
            kserve_ready: false,
            url: None,
        };
        let conditions = vec![make_condition("Ready", "True", None, None)];
        let status = status_from_kserve_conditions(
            Some(&existing),
            Some(2),
            conditions,
            Some("http://url".into()),
        );
        assert_eq!(status.state, DeploymentState::Ready);
        assert!(status.last_transition_time.is_some());
        assert_ne!(
            status.last_transition_time,
            Some("2024-01-01T00:00:00Z".into())
        );
    }

    #[test]
    fn missing_inferenceservice_preserves_existing_url() {
        let existing = ModelDeploymentStatus {
            state: DeploymentState::Ready,
            observed_generation: Some(1),
            last_transition_time: Some("2024-01-01T00:00:00Z".into()),
            failure_reason: None,
            kserve_ready: true,
            url: Some("http://existing".into()),
        };
        let status = missing_inferenceservice_status(Some(&existing), Some(2));
        assert_eq!(status.state, DeploymentState::Failed);
        assert_eq!(status.url, Some("http://existing".into()));
        assert_eq!(status.observed_generation, Some(2));
        assert_ne!(
            status.last_transition_time,
            Some("2024-01-01T00:00:00Z".into())
        );
    }

    #[test]
    fn dynamic_parsing_extracts_conditions_and_url_from_kserve_json() {
        let ar = ApiResource {
            group: "serving.kserve.io".into(),
            version: "v1beta1".into(),
            kind: "InferenceService".into(),
            api_version: "serving.kserve.io/v1beta1".into(),
            plural: "inferenceservices".into(),
        };
        let mut obj = DynamicObject::new("my-model", &ar);
        obj.data = serde_json::json!({
            "status": {
                "url": "http://my-model.example.com",
                "conditions": [
                    {
                        "type": "Ready",
                        "status": "True",
                        "lastTransitionTime": "2024-01-01T00:00:00Z",
                        "reason": "AllGood",
                        "message": "Deployment is ready"
                    },
                    {
                        "type": "Predictable",
                        "status": "True",
                        "lastTransitionTime": "2024-01-01T00:00:00Z",
                        "reason": "Predictable",
                        "message": "Predictable"
                    }
                ]
            }
        });

        let conditions = dynamic_conditions(&obj);
        assert_eq!(conditions.len(), 2);
        assert_eq!(conditions[0].type_, "Ready");
        assert_eq!(conditions[0].status, "True");
        assert_eq!(conditions[0].reason, "AllGood");
        assert_eq!(conditions[0].message, "Deployment is ready");

        let url = dynamic_url(&obj);
        assert_eq!(url, Some("http://my-model.example.com".to_string()));
    }

    #[test]
    fn dynamic_parsing_handles_empty_status() {
        let ar = ApiResource {
            group: "serving.kserve.io".into(),
            version: "v1beta1".into(),
            kind: "InferenceService".into(),
            api_version: "serving.kserve.io/v1beta1".into(),
            plural: "inferenceservices".into(),
        };
        let mut obj = DynamicObject::new("my-model", &ar);
        obj.data = serde_json::json!({"status": {}});

        let conditions = dynamic_conditions(&obj);
        assert!(conditions.is_empty());
        assert_eq!(dynamic_url(&obj), None);
    }

    #[test]
    fn dynamic_parsing_handles_sparse_kserve_conditions() {
        let ar = ApiResource {
            group: "serving.kserve.io".into(),
            version: "v1beta1".into(),
            kind: "InferenceService".into(),
            api_version: "serving.kserve.io/v1beta1".into(),
            plural: "inferenceservices".into(),
        };
        let mut obj = DynamicObject::new("my-model", &ar);
        obj.data = serde_json::json!({
            "status": {
                "conditions": [
                    {
                        "type": "Ready",
                        "status": "True",
                        "lastTransitionTime": "2024-01-01T00:00:00Z"
                    }
                ]
            }
        });

        let conditions = dynamic_conditions(&obj);
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].type_, "Ready");
        assert_eq!(conditions[0].status, "True");
        assert_eq!(conditions[0].reason, "");
        assert_eq!(conditions[0].message, "");

        let status = status_from_kserve_conditions(None, Some(1), conditions, None);
        assert_eq!(status.state, DeploymentState::Ready);
        assert!(status.kserve_ready);
    }
}
