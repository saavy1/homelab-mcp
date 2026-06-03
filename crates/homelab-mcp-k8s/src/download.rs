use k8s_openapi::api::batch::v1 as batchv1;
use serde_json::json;

#[derive(Clone, Debug, PartialEq)]
pub struct DownloadJobSpec {
    pub model_id: String,
    pub revision: String,
    pub nas_path: String,
    pub download_node_selector: String,
    pub hf_secret_name: String,
    pub hf_secret_namespace: String,
}

pub fn download_job_name(model_id: &str, revision: &str) -> String {
    let sanitized = model_id.replace('/', "-").to_lowercase();
    let rev_short = if revision.len() > 8 { &revision[..8] } else { revision };
    format!("download-{}-{}", sanitized, rev_short)
}

pub fn build_download_job(spec: &DownloadJobSpec) -> batchv1::Job {
    let job_name = download_job_name(&spec.model_id, &spec.revision);
    let local_dir = format!("{}/{}", spec.nas_path, spec.model_id);
    let sentinel_path = format!("{}/.homelab-mcp-download.json", local_dir);
    let download_cmd = format!(
        "pip install -q hf && hf download {} --local-dir {} --revision {} --token $HF_TOKEN && \
         echo '{{\"model_id\":\"{}\",\"revision\":\"{}\",\"downloaded_at\":\"'$(date -uIs)'\",\"source\":\"huggingface\",\"complete\":true}}' > {}",
        spec.model_id, local_dir, spec.revision,
        spec.model_id, spec.revision,
        sentinel_path
    );
    let job: batchv1::Job = serde_json::from_value(json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": job_name,
            "namespace": spec.hf_secret_namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "homelab-mcp",
                "homelab.saavylab.dev/model-id": spec.model_id,
                "homelab.saavylab.dev/revision": spec.revision,
                "homelab.saavylab.dev/purpose": "weight-download"
            }
        },
        "spec": {
            "backoffLimit": 2,
            "ttlSecondsAfterFinished": 3600,
            "template": {
                "spec": {
                    "nodeSelector": {
                        "kubernetes.io/hostname": spec.download_node_selector
                    },
                    "containers": [{
                        "name": "download",
                        "image": "python:3.12-slim",
                        "command": ["sh", "-c"],
                        "args": [download_cmd],
                        "env": [{
                            "name": "HF_TOKEN",
                            "valueFrom": {
                                "secretKeyRef": {
                                    "name": spec.hf_secret_name,
                                    "key": "token"
                                }
                            }
                        }],
                        "volumeMounts": [{
                            "name": "model-storage",
                            "mountPath": spec.nas_path
                        }]
                    }],
                    "volumes": [{
                        "name": "model-storage",
                        "hostPath": {
                            "path": spec.nas_path,
                            "type": "Directory"
                        }
                    }],
                    "restartPolicy": "Never"
                }
            }
        }
    })).expect("download job json is valid");
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_job_name_is_deterministic() {
        assert_eq!(
            download_job_name("Qwen/Qwen3-8B", "main"),
            "download-qwen-qwen3-8b-main"
        );
    }

    #[test]
    fn build_download_job_targets_nas_node_with_sentinel() {
        let spec = DownloadJobSpec {
            model_id: "Qwen/Qwen3-8B".into(),
            revision: "main".into(),
            nas_path: "/tank/models".into(),
            download_node_selector: "superbloom".into(),
            hf_secret_name: "hf-token".into(),
            hf_secret_namespace: "ai".into(),
        };
        let job = build_download_job(&spec);
        assert_eq!(job.metadata.name.as_deref(), Some("download-qwen-qwen3-8b-main"));
        let template_spec = job.spec.and_then(|s| s.template.spec).expect("template spec");
        let selector = template_spec.node_selector.expect("node selector");
        assert_eq!(selector.get("kubernetes.io/hostname").map(|s| s.as_str()), Some("superbloom"));
        let container = template_spec.containers.into_iter().next().expect("container");
        let args: Vec<String> = container.args.into_iter().flatten().collect();
        let combined = args.join(" ");
        assert!(combined.contains("hf download"));
        assert!(combined.contains("--local-dir /tank/models/Qwen/Qwen3-8B"));
        assert!(combined.contains(".homelab-mcp-download.json"));
    }

    #[test]
    fn build_download_job_uses_hf_secret() {
        let spec = DownloadJobSpec {
            model_id: "deepseek-ai/DeepSeek-V4-Flash".into(),
            revision: "main".into(),
            nas_path: "/tank/models".into(),
            download_node_selector: "superbloom".into(),
            hf_secret_name: "hf-token".into(),
            hf_secret_namespace: "ai".into(),
        };
        let job = build_download_job(&spec);
        let template_spec = job.spec.and_then(|s| s.template.spec).expect("template spec");
        let container = template_spec.containers.into_iter().next().expect("container");
        let env = container.env.into_iter().flatten().next().expect("env var");
        assert_eq!(env.name, "HF_TOKEN");
    }
}
