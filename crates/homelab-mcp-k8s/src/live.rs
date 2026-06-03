use crate::{DownloadJobRef, DownloadStatus, KserveCondition, ModelLogs, ModelStatus};
use k8s_openapi::api::batch::v1 as batchv1;
use k8s_openapi::api::core::v1 as corev1;
use kube::core::dynamic::DynamicObject;
use kube::{
    Api, Client,
    api::{ListParams, LogParams, PostParams},
    discovery::ApiResource,
};

/// Create a kube::Client from environment (in-cluster or ~/.kube/config).
pub async fn k8s_client() -> Result<Client, kube::Error> {
    Client::try_default().await
}

/// Create a download Job on the cluster. Returns the job name.
pub async fn create_download_job(
    job: &batchv1::Job,
    namespace: &str,
) -> Result<String, kube::Error> {
    let client = k8s_client().await?;
    let jobs: Api<batchv1::Job> = Api::namespaced(client, namespace);
    let created = jobs.create(&PostParams::default(), job).await?;
    Ok(created.metadata.name.unwrap_or_default())
}

/// Check the status of a download Job by name.
pub async fn get_download_status(job_ref: &DownloadJobRef) -> Result<DownloadStatus, kube::Error> {
    let client = k8s_client().await?;
    let jobs: Api<batchv1::Job> = Api::namespaced(client, &job_ref.namespace);
    let job = match jobs.get(&job_ref.job_name).await {
        Ok(j) => j,
        Err(e) if e.to_string().contains("404") => {
            return Ok(DownloadStatus::NotStarted);
        }
        Err(e) => return Err(e),
    };

    let Some(status) = job.status else {
        return Ok(DownloadStatus::JobCreated {
            job_ref: job_ref.clone(),
        });
    };

    if let Some(conditions) = &status.conditions {
        if conditions
            .iter()
            .any(|c| c.type_ == "Complete" && c.status == "True")
        {
            return Ok(DownloadStatus::Completed {
                job_ref: job_ref.clone(),
            });
        }
        if let Some(failed) = conditions
            .iter()
            .find(|c| c.type_ == "Failed" && c.status == "True")
        {
            return Ok(DownloadStatus::Failed {
                job_ref: job_ref.clone(),
                reason: failed.message.clone().unwrap_or_else(|| "unknown".into()),
            });
        }
    }

    if status.active.unwrap_or(0) > 0 {
        Ok(DownloadStatus::Running {
            job_ref: job_ref.clone(),
        })
    } else {
        Ok(DownloadStatus::JobCreated {
            job_ref: job_ref.clone(),
        })
    }
}

/// InferenceService dynamic API resource descriptor.
fn isvc_api_resource() -> ApiResource {
    ApiResource {
        group: "serving.kserve.io".into(),
        version: "v1beta1".into(),
        kind: "InferenceService".into(),
        api_version: "serving.kserve.io/v1beta1".into(),
        plural: "inferenceservices".into(),
    }
}

/// Create an InferenceService (create-only). Returns the resource name.
pub async fn create_inferenceservice(
    manifest: serde_json::Value,
    namespace: &str,
) -> Result<String, kube::Error> {
    let ar = isvc_api_resource();
    let client = k8s_client().await?;
    let isvc: Api<DynamicObject> = Api::namespaced_with(client, namespace, &ar);
    let name = manifest
        .get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("unknown");
    let mut obj = DynamicObject::new(name, &ar).within(namespace);
    obj.data = manifest;
    let created = isvc.create(&PostParams::default(), &obj).await?;
    Ok(created.metadata.name.unwrap_or_default())
}

/// Get InferenceService status by name.
pub async fn get_inferenceservice_status(
    namespace: &str,
    name: &str,
) -> Result<ModelStatus, String> {
    let ar = isvc_api_resource();
    let client = k8s_client().await.map_err(|e| e.to_string())?;
    let isvc: Api<DynamicObject> = Api::namespaced_with(client, namespace, &ar);
    let obj = isvc
        .get(name)
        .await
        .map_err(|e| format!("get InferenceService: {e}"))?;

    let conditions: Vec<KserveCondition> = obj
        .data
        .get("status")
        .and_then(|s| s.get("conditions"))
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(KserveCondition {
                        condition_type: c.get("type")?.as_str()?.into(),
                        status: c.get("status")?.as_str()?.into(),
                        reason: c.get("reason").and_then(|r| r.as_str()).map(Into::into),
                        message: c.get("message").and_then(|m| m.as_str()).map(Into::into),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let ready = conditions
        .iter()
        .any(|c| c.condition_type == "Ready" && c.status == "True");

    let recent_events = get_events(namespace).await.unwrap_or_default();

    Ok(ModelStatus {
        namespace: namespace.into(),
        name: name.into(),
        ready,
        conditions,
        recent_events,
    })
}

/// Get recent events for a namespace.
pub async fn get_events(namespace: &str) -> Result<Vec<String>, kube::Error> {
    let client = k8s_client().await?;
    let events: Api<corev1::Event> = Api::namespaced(client, namespace);
    let list = events.list(&ListParams::default().limit(20)).await?;
    Ok(list
        .iter()
        .filter_map(|e| {
            let msg = e.message.as_ref()?;
            let name = e.involved_object.name.as_deref().unwrap_or("unknown");
            Some(format!("{name}: {msg}"))
        })
        .collect())
}

/// Get logs from the predictor pod for an InferenceService.
pub async fn get_predictor_logs(
    namespace: &str,
    name: &str,
    tail: usize,
) -> Result<ModelLogs, String> {
    let client = k8s_client().await.map_err(|e| e.to_string())?;
    let pods: Api<corev1::Pod> = Api::namespaced(client, namespace);
    let label_selector = format!("serving.kserve.io/inferenceservice={name}");
    let list = pods
        .list(&ListParams::default().labels(&label_selector).limit(1))
        .await
        .map_err(|e| e.to_string())?;

    let pod_name = list
        .iter()
        .next()
        .and_then(|p| p.metadata.name.clone())
        .unwrap_or_else(|| "no-pod-found".into());

    let client2 = k8s_client().await.map_err(|e| e.to_string())?;
    let pods: Api<corev1::Pod> = Api::namespaced(client2, namespace);
    let log_result = pods
        .logs(
            &pod_name,
            &LogParams {
                tail_lines: Some(tail as i64),
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|_| String::from("no logs available"));

    let lines = log_result.lines().map(String::from).collect();
    Ok(ModelLogs {
        namespace: namespace.into(),
        name: name.into(),
        lines,
    })
}
