use k8s_openapi::api::core::v1::{Node, Pod};
use kube::{Api, Client, api::ListParams};
use model_catalog::{ActiveModelCapacity, CapacityReport, ResourceRequests};

/// Map a target name to the Kubernetes node name for v1.
pub fn target_to_node_name(target: &str) -> &str {
    match target {
        "spark" => "gx10-98a5",
        other => other,
    }
}

/// Check whether a node's Ready condition is True.
pub fn node_is_ready(node: &Node) -> bool {
    node.status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|condition| condition.type_ == "Ready" && condition.status == "True")
        })
}

/// Extract resource requests/limits from a Pod's containers.
fn extract_pod_resources(pod: &Pod) -> ResourceRequests {
    let mut gpu_from_requests = 0u32;
    let mut gpu_from_limits = 0u32;
    let mut cpu_str = None;
    let mut mem_str = None;

    if let Some(spec) = &pod.spec {
        for container in &spec.containers {
            if let Some(resources) = &container.resources {
                if let Some(reqs) = &resources.requests {
                    if let Some(q) = reqs.get("nvidia.com/gpu")
                        && let Ok(n) = q.0.parse::<u32>()
                    {
                        gpu_from_requests += n;
                    }
                    if cpu_str.is_none() {
                        cpu_str = reqs.get("cpu").map(|q| q.0.clone());
                    }
                    if mem_str.is_none() {
                        mem_str = reqs.get("memory").map(|q| q.0.clone());
                    }
                }
                if let Some(limits) = &resources.limits {
                    if let Some(q) = limits.get("nvidia.com/gpu")
                        && let Ok(n) = q.0.parse::<u32>()
                    {
                        gpu_from_limits += n;
                    }
                    if cpu_str.is_none() {
                        cpu_str = limits.get("cpu").map(|q| q.0.clone());
                    }
                    if mem_str.is_none() {
                        mem_str = limits.get("memory").map(|q| q.0.clone());
                    }
                }
            }
        }
    }

    let gpu_count = if gpu_from_requests > 0 {
        gpu_from_requests
    } else {
        gpu_from_limits
    };

    ResourceRequests {
        cpu: cpu_str.unwrap_or_else(|| "unknown".into()),
        memory: mem_str.unwrap_or_else(|| "unknown".into()),
        gpu_count,
    }
}

/// Build an ActiveModelCapacity entry from a Pod best-effort.
pub fn active_model_from_pod(pod: &Pod) -> Option<ActiveModelCapacity> {
    let metadata = &pod.metadata;
    let labels = metadata.labels.as_ref()?;
    let name = metadata.name.clone()?;
    let namespace = metadata.namespace.clone()?;

    let has_kserve = labels.contains_key("serving.kserve.io/inferenceservice");
    let has_homelab = labels
        .get("app.kubernetes.io/managed-by")
        .map(|s| s.as_str())
        == Some("homelab-mcp");
    if !has_kserve && !has_homelab {
        return None;
    }

    let recipe_id = labels.get("homelab.saavylab.dev/recipe-id").cloned();
    let ready = pod
        .status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|condition| condition.type_ == "Ready" && condition.status == "True")
        });

    let requested = extract_pod_resources(pod);

    Some(ActiveModelCapacity {
        name,
        namespace,
        recipe_id,
        requested,
        ready,
    })
}

/// Parse a Prometheus scalar query response for the single value.
pub fn parse_prometheus_scalar(response: &serde_json::Value) -> Option<f64> {
    response["data"]["result"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["value"].as_array())
        .and_then(|value| value.get(1))
        .and_then(|value| value.as_str())
        .and_then(|value| value.parse::<f64>().ok())
}

/// Collect a capacity report for the given target from Kubernetes and Prometheus.
pub async fn collect_capacity_report(
    client: Client,
    target: &str,
    prometheus_base_url: Option<&str>,
) -> Result<CapacityReport, String> {
    let nodes: Api<Node> = Api::all(client.clone());
    let pods: Api<Pod> = Api::all(client);
    let node_name = target_to_node_name(target);

    let node = nodes
        .get(node_name)
        .await
        .map_err(|error| format!("get node {node_name}: {error}"))?;
    let node_ready = node_is_ready(&node);

    let active_models = pods
        .list(&ListParams::default().fields(&format!("spec.nodeName={node_name}")))
        .await
        .map_err(|error| format!("list pods: {error}"))?
        .iter()
        .filter_map(active_model_from_pod)
        .collect();

    let (gpu_util, gpu_mem_used, gpu_mem_total) = match prometheus_base_url {
        Some(base) => (
            query_prometheus_scalar(base, "DCGM_FI_DEV_GPU_UTIL")
                .await
                .ok(),
            query_prometheus_scalar(base, "DCGM_FI_DEV_FB_USED * 1024 * 1024")
                .await
                .ok(),
            query_prometheus_scalar(base, "DCGM_FI_DEV_FB_TOTAL * 1024 * 1024")
                .await
                .ok(),
        ),
        None => (None, None, None),
    };

    let mut risks = Vec::new();
    if prometheus_base_url.is_none() {
        risks.push("PROMETHEUS_BASE_URL is not configured; fit uses Kubernetes state only".into());
    }

    Ok(CapacityReport {
        target: target.into(),
        node_ready,
        active_models,
        observed_gpu_utilization_percent: gpu_util,
        observed_gpu_memory_used_bytes: gpu_mem_used,
        observed_gpu_memory_total_bytes: gpu_mem_total,
        risks,
    })
}

async fn query_prometheus_scalar(base: &str, query: &str) -> Result<f64, String> {
    let url = format!("{}/api/v1/query", base.trim_end_matches('/'));
    let response: serde_json::Value = reqwest::Client::new()
        .get(url)
        .query(&[("query", query)])
        .send()
        .await
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    parse_prometheus_scalar(&response)
        .ok_or_else(|| format!("Prometheus query returned no scalar: {query}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spark_target_maps_to_node_name() {
        assert_eq!(target_to_node_name("spark"), "gx10-98a5");
    }

    #[test]
    fn other_target_passes_through() {
        assert_eq!(target_to_node_name("other-node"), "other-node");
    }

    #[test]
    fn parse_prometheus_scalar_extracts_value() {
        let response = serde_json::json!({
            "data": {
                "result": [
                    {
                        "value": [1234567890, "42.5"]
                    }
                ]
            }
        });
        assert_eq!(parse_prometheus_scalar(&response), Some(42.5));
    }

    #[test]
    fn parse_prometheus_scalar_returns_none_for_empty_result() {
        let response = serde_json::json!({
            "data": {
                "result": []
            }
        });
        assert_eq!(parse_prometheus_scalar(&response), None);
    }

    #[test]
    fn active_model_from_pod_with_kserve_labels() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {
                "name": "my-model-predictor-abc123",
                "namespace": "default",
                "labels": {
                    "serving.kserve.io/inferenceservice": "my-model"
                }
            },
            "spec": {
                "containers": [
                    {
                        "name": "kserve-container",
                        "resources": {
                            "requests": {
                                "cpu": "2",
                                "memory": "16Gi",
                                "nvidia.com/gpu": "1"
                            }
                        }
                    }
                ]
            },
            "status": {
                "conditions": [
                    {
                        "type": "Ready",
                        "status": "True"
                    }
                ]
            }
        }))
        .unwrap();
        let model = active_model_from_pod(&pod).expect("extracts kserve pod");
        assert_eq!(model.name, "my-model-predictor-abc123");
        assert_eq!(model.namespace, "default");
        assert_eq!(model.requested.gpu_count, 1);
        assert_eq!(model.requested.cpu, "2");
        assert_eq!(model.requested.memory, "16Gi");
        assert!(model.ready);
    }

    #[test]
    fn active_model_from_pod_with_homelab_labels() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {
                "name": "homelab-model",
                "namespace": "ai",
                "labels": {
                    "app.kubernetes.io/managed-by": "homelab-mcp",
                    "homelab.saavylab.dev/recipe-id": "qwen3-8b"
                }
            },
            "spec": {
                "containers": [
                    {
                        "name": "main",
                        "resources": {
                            "limits": {
                                "cpu": "4",
                                "memory": "32Gi",
                                "nvidia.com/gpu": "1"
                            }
                        }
                    }
                ]
            },
            "status": {
                "conditions": [
                    {
                        "type": "Ready",
                        "status": "False"
                    }
                ]
            }
        }))
        .unwrap();
        let model = active_model_from_pod(&pod).expect("extracts homelab pod");
        assert_eq!(model.recipe_id, Some("qwen3-8b".into()));
        assert_eq!(model.requested.gpu_count, 1);
        assert_eq!(model.requested.cpu, "4");
        assert_eq!(model.requested.memory, "32Gi");
        assert!(!model.ready);
    }

    #[test]
    fn active_model_from_pod_skips_unrelated_pods() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {
                "name": "nginx",
                "namespace": "default",
                "labels": {
                    "app": "nginx"
                }
            },
            "spec": {
                "containers": [
                    {
                        "name": "nginx"
                    }
                ]
            }
        }))
        .unwrap();
        assert!(active_model_from_pod(&pod).is_none());
    }

    #[test]
    fn active_model_from_pod_uses_zero_gpu_when_no_gpu_resource() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {
                "name": "cpu-only-model",
                "namespace": "default",
                "labels": {
                    "serving.kserve.io/inferenceservice": "cpu-model"
                }
            },
            "spec": {
                "containers": [
                    {
                        "name": "kserve-container",
                        "resources": {
                            "requests": {
                                "cpu": "2",
                                "memory": "4Gi"
                            }
                        }
                    }
                ]
            }
        }))
        .unwrap();
        let model = active_model_from_pod(&pod).expect("extracts pod");
        assert_eq!(model.requested.gpu_count, 0);
        assert_eq!(model.requested.cpu, "2");
        assert_eq!(model.requested.memory, "4Gi");
    }

    #[test]
    fn active_model_from_pod_fallback_to_limits_when_no_requests() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {
                "name": "limit-only",
                "namespace": "default",
                "labels": {
                    "serving.kserve.io/inferenceservice": "limit-model"
                }
            },
            "spec": {
                "containers": [
                    {
                        "name": "kserve-container",
                        "resources": {
                            "limits": {
                                "cpu": "8",
                                "memory": "64Gi",
                                "nvidia.com/gpu": "2"
                            }
                        }
                    }
                ]
            }
        }))
        .unwrap();
        let model = active_model_from_pod(&pod).expect("extracts pod");
        assert_eq!(model.requested.gpu_count, 2);
        assert_eq!(model.requested.cpu, "8");
        assert_eq!(model.requested.memory, "64Gi");
    }
}
