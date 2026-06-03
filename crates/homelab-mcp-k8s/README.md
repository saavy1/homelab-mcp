# homelab-mcp-k8s

kube-rs helpers for the homelab-mcp server. All functions use `kube::Client::try_default()` which auto-detects in-cluster ServiceAccount or local `~/.kube/config`.

## Modules

- **`download`** — `build_download_job(spec)` creates a batch/v1 Job targeting the NAS node. Uses `hf download`, writes a sentinel `.homelab-mcp-download.json`, TTL 3600s, backoffLimit 2.
- **`live`** — Async K8s API operations:
  - `create_download_job` / `get_download_status` — CRUD + status polling for download Jobs
  - `create_inferenceservice` — Create InferenceService via DynamicObject (serving.kserve.io/v1beta1)
  - `get_inferenceservice_status` — Read ISVC conditions + events (scoped to the ISVC name)
  - `get_predictor_logs` — Find predictor pod by label, read logs
- **`status`** — Value types: `DownloadStatus` (NotStarted/JobCreated/Running/Completed/Failed/AlreadyCached), `ModelStatus`, `SentinelInfo`, `DownloadJobRef`

## Key types

- `DownloadJobSpec` — Input for `build_download_job`: model_id, revision, nas_path, node selector, HF secret ref
- `DownloadStatus` — Enum representing Job lifecycle state
- `ModelStatus` — InferenceService conditions + events
