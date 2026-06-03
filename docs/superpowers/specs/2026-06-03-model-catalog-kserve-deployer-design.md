# Model Catalog + KServe Deployer Design

Build a standalone Rust MCP workspace for homelab model-serving tools. The first
server is a model catalog and KServe deployer that turns known-good model recipes
into native KServe `InferenceService` resources. Spark Arena recipes are the
preferred reference source, but the system must also allow local ad hoc recipes
for models or tuning choices that are not yet stable upstream.

Repository objective:

> `homelab-mcp` turns model-serving recipes into explainable, reviewable KServe
> deployments for Superbloom, with direct kube-rs apply and weight-download jobs.

## Motivation

The homelab already has working manual KServe model deployments. The next step is
not another custom CRD or reconciler; it is a safe operator interface that can
search recipes, explain trade-offs, render native KServe manifests, download
weights, apply deployments, and observe status.

Kubeflow is actively designing an upstream MCP focused on Kubeflow SDK/Trainer
workflows. This design intentionally avoids duplicating that scope. It focuses on
KServe inference and model-serving recipes. If the upstream Kubeflow MCP becomes
useful for Trainer/Pipelines, Hermes can adopt it as a separate MCP later.

## Repository boundary

This work lives in a new standalone repo under `~/dev/homelab`, not in Nexus.
Nexus is treated as dead/legacy for this effort.

Responsibilities:

- `homelab-mcp`: Rust MCP servers, shared crates, tests, and container builds.
- `sb`: GitOps deployment manifests and generated model-serving resources.
- upstream Kubeflow MCP: future Trainer/Pipelines integration, if useful.

## Goals

- Use Spark Arena recipe registry data as the primary external reference.
- Support local recipes and overrides for experimental models such as DeepSeek V4
  Flash.
- Render native KServe `InferenceService` manifests instead of creating a custom
  model CRD.
- Apply deployments directly via `kube-rs` (create InferenceService, create
  download Jobs) rather than requiring a GitOps round-trip for every change.
- Download model weights via K8s Jobs on the NAS node using `hf download`.
- Establish shared Rust crates for future custom MCPs.
- Expose the workflow through an MCP server usable by Hermes and future agents.

## Non-goals

- Do not revive `nexus-ml` `ModelDeployment` CRDs or reconcilers.
- Do not build a general Kubeflow Trainer/Pipelines MCP.
- Do not implement automatic model routing yet.
- Do not require Spark Arena to be complete or authoritative for every model.
- Do not shell out to `kubectl` for normal Kubernetes operations.
- Do not require a GitOps commit-PR-ArgoCD round-trip for model deploy. Direct
  apply via kube-rs is the primary path.

## Architecture

```text
Hermes / agent
        │
        ▼
Model Catalog MCP
        │
        ├── shared MCP crates
        │     ├── rmcp server/tool setup
        │     ├── common response/error types
        │     ├── kube-rs helpers
        │     └── GitOps patch helpers
        │
        ├── recipe sources
        │     ├── Spark Arena registry clone/cache
        │     └── local homelab recipe directory
        │
        ├── resolver
        │     ├── normalize recipe metadata
        │     ├── apply local overrides
        │     ├── lower recipe to DeploymentIntent
        │     └── validate against ClusterProfile
        │
        ├── renderer
        │     └── KServe InferenceService YAML
        │
        ├── deployer (kube-rs direct apply)
        │     ├── ensure_weights → K8s Job on NAS node (hf download)
        │     ├── download_status → Job/pod status
        │     └── apply_plan → create InferenceService
        │
        └── observer (kube-rs read)
              ├── status → InferenceService conditions, pods, events
              └── logs → predictor pod logs
```

The MCP is a thin Rust service. It reads recipes, resolves them into a homelab
deployment intent, renders native KServe resources, downloads weights via K8s
Jobs on the NAS node, applies deployments directly via kube-rs, and observes
status. Kubernetes remains the serving control plane.

## Storage and download topology

Superbloom is a two-node K3s cluster with asymmetric storage:

```text
Superbloom (NAS + control-plane):
  /tank/models/     ← ZFS dataset, no node taints, runs all infra workloads
  K3s server

GX10-98a5 (DGX Spark, GPU worker):
  /mnt/nas/models/  ← SMB mount from Superbloom over 2.5Gbps link
  K3s agent
  Taints: nvidia.com/gpu=true:NoSchedule, nvidia.com/gpu=true:NoExecute
```

Weight download Jobs schedule on Superbloom (no taints) via `nodeSelector:
kubernetes.io/hostname=superbloom`. They run `hf download <model-id>
--local-dir /tank/models/<model-id>` writing directly to local ZFS.

InferenceService predictor pods schedule on the Spark (GPU taints + tolerations
from the vLLM ClusterServingRuntime). They mount weights via `hostPath:
/mnt/nas/models` (the SMB mount on the Spark), with `mountPath: /tank/models`
inside the container so vLLM args don't change.

The vLLM ClusterServingRuntime (already deployed via `sb` GitOps) provides the
pod template: image, args, tolerations, nodeSelector, and volume spec. The MCP
creates `InferenceService` resources that select this runtime by
`modelFormat.name: vllm`.

## Rust workspace shape

The repository should start as a Rust workspace designed for multiple MCPs:

```text
homelab-mcp/
  crates/
    homelab-mcp-core/        # shared tool response, error, config, telemetry types
    homelab-mcp-k8s/         # kube-rs client helpers and typed K8s/KServe access
    homelab-mcp-gitops/      # GitOps patch/branch/diff helpers
    model-catalog/           # recipe parsing, normalization, overrides, rendering
  servers/
    model-catalog-mcp/       # rmcp server binary
```

Shared crates should be extracted early because this is the first of many custom
MCPs. The goal is to avoid copying server setup, response schemas, error
formatting, Kubernetes access, and GitOps workflows into every future MCP.

The shared crates must stay boring and tiny until a second MCP proves more
abstraction is needed:

- `homelab-mcp-core`: `ToolResult<T>`, `ToolError`, `ValidationIssue`,
  `Summary`, `Provenance`, config loading, and tracing setup.
- `homelab-mcp-k8s`: kube client factory, KServe `InferenceService` status
  reader, pods/events/logs helpers, and cluster profile discovery.
- `homelab-mcp-gitops`: repository path abstraction, write files, produce diff,
  and optional local branch/commit helpers.

Do not build a grand MCP framework in v1.

## MCP stack

Use `rmcp` as the MCP SDK. Use `rmcp-macros` where it reduces boilerplate without
hiding tool schemas or making tests harder to read.

MCP conventions:

- tools return structured JSON-friendly outputs with concise summaries.
- risky operations are explicit tools, not hidden side effects of read tools.
- tool schemas should be stable and easy for LLM clients to reason about.
- the server should support the transport Hermes needs first; additional
  transports can be added behind shared core abstractions.

## Kubernetes access

Use `kube-rs` and typed/dynamic Kubernetes API clients for cluster interaction.
Do not shell out to `kubectl` or patch raw YAML through commands for normal
operations.

Expected Kubernetes responsibilities:

- read KServe `InferenceService` status and conditions.
- read related pods, events, and logs for debugging context.
- create download Jobs on the NAS node (Superbloom) to fetch weights via
  `hf download`.
- check download Job completion and pod logs.
- create `InferenceService` resources directly via kube-rs apply.
- validate rendered resources where possible before applying.

All cluster reads and writes go through `kube-rs`. Raw YAML is acceptable as a
render artifact and for GitOps patch export, but live cluster operations must use
typed/dynamic API clients.

## Cluster profile

Validation depends on an explicit `ClusterProfile`, not ad hoc assumptions hidden
inside render code.

The initial shape:

```rust
ClusterProfile {
    cluster_name: String,
    nodes: Vec<NodeProfile>,
    default_namespace: String,
    available_serving_runtimes: Vec<String>,
    max_gpu_per_pod: u32,
    ingress_mode: IngressMode,
    model_storage: ModelStorage,
}

NodeProfile {
    hostname: String,
    roles: Vec<NodeRole>,         // control-plane, gpu-worker, nas
    gpu_product: Option<String>,  // e.g. "NVIDIA-GB10"
    gpu_count: u32,
    gpu_memory_gb: u32,
    taints: Vec<Taint>,
    model_path: Option<String>,   // /tank/models on NAS, /mnt/nas/models on GPU
}

ModelStorage {
    nas_hostname: String,          // "superbloom" — where weights are stored
    nas_path: String,              // "/tank/models" — ZFS dataset on NAS
    gpu_node_path: String,         // "/mnt/nas/models" — SMB mount on GPU nodes
    download_node_selector: String,// nodeSelector for download Jobs
}
```

The Superbloom defaults:

```text
ClusterProfile {
    cluster_name: "superbloom",
    nodes: [
        NodeProfile { hostname: "superbloom", roles: [ControlPlane, Nas],
                      gpu: None, model_path: Some("/tank/models"),
                      taints: [] },
        NodeProfile { hostname: "gx10-98a5", roles: [GpuWorker],
                      gpu_product: Some("NVIDIA-GB10"), gpu_count: 1, gpu_memory_gb: 128,
                      model_path: Some("/mnt/nas/models"),
                      taints: [nvidia.com/gpu=true:NoSchedule, NoExecute] },
    ],
    model_storage: ModelStorage {
        nas_hostname: "superbloom",
        nas_path: "/tank/models",
        gpu_node_path: "/mnt/nas/models",
        download_node_selector: "kubernetes.io/hostname=superbloom",
    },
    ...
}
```

The profile can start from static config and later merge in live `kube-rs`
discovery. Its job is to make `plan_deploy` explain why a recipe appears to fit
or not fit Superbloom.

## Recipe model

The internal recipe shape should be deliberately boring:

- `id`: stable local identifier.
- `source`: `spark-arena`, `local`, or `ad-hoc`.
- `model`: Hugging Face model id, revision, quantization, and license notes when
  known.
- `runtime`: vLLM image, command args, env vars, tensor parallelism, max context,
  dtype, tool-call parser, reasoning parser.
- `hardware`: target GPU class, GPU count, estimated VRAM, minimum/known-good
  memory utilization.
- `serving`: KServe namespace, service name, scaling settings, storage/cache
  hints.
- `provenance`: upstream recipe path/commit or local file path.

Spark Arena recipes are imported into this shape. Local recipes use the same
shape and may either stand alone or override imported fields.

## Lowering model

Keep upstream recipe data separate from homelab deployment intent:

```text
Recipe -> DeploymentIntent -> KServeManifest
```

`Recipe` describes source facts and recommended runtime assumptions.
`DeploymentIntent` describes the chosen Superbloom deployment:

- `name`
- `namespace`
- `recipe_id`
- `selected_gpu_class`
- `replicas`
- `scale_to_zero`
- `storage_mode`
- `ingress_policy`
- `env_overrides`
- `resource_requests`

This boundary prevents Spark Arena's schema from leaking into the serving API and
keeps local cluster choices explicit.

## MCP tools

Initial tools:

- `models.search_recipes(query?, source?, include_experimental?)`
- `models.show_recipe(id)`
- `models.plan_deploy(recipe_id, name, namespace, overrides?)`
- `models.ensure_weights(recipe_id)` -- creates K8s Job on NAS node if weights
  not cached, returns job name
- `models.download_status(recipe_id)` -- checks download job completion/pod logs
- `models.apply_plan(plan_id)` -- creates `InferenceService` via kube-rs
- `models.status(name, namespace)`
- `models.logs(name, namespace, tail?)`

Risk model:

- `search_recipes`, `show_recipe`, `status`, and `logs` are read-only.
- `plan_deploy` is pure planning and returns a structured plan without writing to
  disk or cluster.
- `ensure_weights` is a local-write that creates a K8s Job on the NAS node to
  download weights. It checks `/tank/models/<model-id>` existence first and
  returns immediately if weights are already cached.
- `download_status` is read-only against an existing download Job.
- `apply_plan` is a cluster-write that creates an `InferenceService` via kube-rs.
  It should only be called after `ensure_weights` confirms weights are present.
- GitOps patch export (`write_gitops_patch`) is a separate explicit tool for
  auditing or PR-based workflows, not the primary deploy path.

Future tools worth adding after v1:

- `models.explain_fit(recipe_id, cluster_profile?)`
- `models.validate_overrides(recipe_id, overrides)`
- `models.smoke_test(name, namespace, prompt?)`

## Data flow

1. User asks Hermes to deploy or evaluate a model.
2. MCP searches Spark Arena and local recipes.
3. MCP shows likely recipes, including provenance and hardware assumptions.
4. User or agent selects a recipe and optional overrides.
5. MCP lowers the recipe to a `DeploymentIntent` and validates it against
   `ClusterProfile`. Returns an explainable dry-run plan.
6. User or agent calls `ensure_weights`. MCP creates a K8s Job on the NAS node
   (Superbloom) that runs `hf download <model-id> --local-dir /tank/models/<model-id>`
   if weights are not already present. Returns job name.
7. Agent polls `download_status` until the download job completes.
8. User or agent calls `apply_plan`. MCP creates the `InferenceService` via
   kube-rs direct apply.
9. KServe reconciles, schedules the predictor pod on the GPU node (Spark), which
   reads weights from hostPath `/mnt/nas/models`.
10. MCP observes KServe/Kubernetes status and logs through `kube-rs`.

## GitOps layout

Local recipes are stored in the `sb` GitOps repository:

```text
sb/argocd/clusters/superbloom/ai/vllm/recipes/
```

The primary deploy path is direct kube-rs apply. For auditing or PR-based
workflows, `write_gitops_patch` can export generated manifests under per-model
directories:

```text
sb/argocd/clusters/superbloom/ai/vllm/resources/<model-name>/
  inferenceservice.yaml
  kustomization.yaml
```

Generated resources should include labels/annotations for:

- recipe id
- recipe source
- upstream Spark Arena path and commit when available
- model id/revision

Avoid owning-user annotations in v1 unless a concrete audit need appears.

## Error handling

Tool responses should be structured for agent reasoning:

- validation errors include the exact field and accepted range/value.
- missing recipe errors include similar recipe suggestions.
- render errors distinguish unsupported recipe fields from unsafe overrides.
- fit explanations include recipe provenance, cluster profile assumptions, and
  suggested alternatives when a recipe does not fit.
- status/log tools include KServe conditions, pod state, recent events, and a
  concise summary.
- local patch failures return the target path/diff state so the operator can
  recover manually.

## Testing

Unit tests:

- parse representative Spark Arena recipes.
- parse local recipe files.
- merge overrides deterministically.
- render stable KServe YAML snapshots.
- reject unsafe or invalid overrides.
- test shared response/error helpers independently.
- test golden explainability responses, including field paths, allowed ranges,
  provenance, and suggested alternatives.

Integration tests:

- run MCP tools against fixture recipe directories.
- validate rendered manifests with Kubernetes schema tooling where available.
- exercise `kube-rs` clients against mocks or a test API where practical.
- dry-run GitOps patch generation in a temporary checkout.

Cluster validation:

- deploy one known-small recipe through the GitOps path.
- verify ArgoCD sync, KServe readiness, pod logs, and an inference smoke test.

## V1 cutline

V1 should be intentionally small:

1. Parse local recipe YAML from `sb`.
2. Import/cache enough Spark Arena metadata to preserve provenance.
3. Normalize to internal `Recipe`.
4. Lower `Recipe` to `DeploymentIntent`.
5. Validate fit against `ClusterProfile`.
6. Render KServe `InferenceService` YAML.
7. Create download Jobs on the NAS node via kube-rs (`ensure_weights`).
8. Check download Job status (`download_status`).
9. Apply `InferenceService` directly via kube-rs (`apply_plan`).
10. Read KServe status/logs with `kube-rs`.

No PR automation, GitOps-only patch mode, model routing, or general Kubeflow
Trainer/Pipeline tools in v1.

## Implementation decisions

- The first MCP lives in a new standalone Rust workspace under
  `~/dev/homelab/homelab-mcp`.
- Shared crates are part of the first implementation, not a later cleanup.
- Use `rmcp`/`rmcp-macros` for MCP server/tool implementation.
- Use `kube-rs` for Kubernetes/KServe reads and direct apply writes.
- `sb` deploys the MCP service and stores local recipes/generated manifests.
- Spark Arena recipes are read from a pinned clone/cache of
  `spark-arena/recipe-registry`, configured by repository URL and commit/ref.
- Direct apply is the primary deploy path: `apply_plan` creates the
  `InferenceService` via kube-rs. GitOps patch export is available as a
  secondary tool for auditing or PR-based workflows.
- Download Jobs run on the NAS node (Superbloom) using `hf download`
  (the current Hugging Face CLI, replacing deprecated `huggingface-cli download`).
- The vLLM ClusterServingRuntime already deployed in `sb` provides the pod
  template. The MCP creates `InferenceService` resources that select it.

These choices keep the implementation concrete without adding a custom CRD layer.
