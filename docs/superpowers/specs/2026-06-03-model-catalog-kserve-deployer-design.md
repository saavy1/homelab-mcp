# Model Catalog + KServe Deployer Design

Build a standalone Rust MCP workspace for homelab model-serving tools. The first
server is a model catalog and KServe deployer that turns known-good model recipes
into native KServe `InferenceService` resources. Spark Arena recipes are the
preferred reference source, but the system must also allow local ad hoc recipes
for models or tuning choices that are not yet stable upstream.

Repository objective:

> `homelab-mcp` is an imperative cluster operator: it downloads weights, validates
> fit, applies InferenceServices, and observes status. GitOps is secondary.

## Philosophical stance

This is **not** a GitOps-first safe publisher. It is a **direct cluster deployer**
with recipe validation and cache orchestration. The philosophical shift is
intentional:

- Old design: GitOps-first safe renderer. Agent generates manifests, human
  reviews via PR, ArgoCD applies.
- New design: Imperative operator interface. Agent validates, downloads, applies,
  and observes directly via kube-rs.

For a personal homelab where Hermes operates a single-user K3s cluster, this is
the right trade. Model iteration is annoying with GitOps round-trips. Hermes
should do the boring operational work: find recipe, check fit, download weights,
apply InferenceService, watch KServe, show logs.

Direct apply intentionally allows runtime model deployments to exist outside
GitOps. The source of truth for stable platform infrastructure (ArgoCD, Flux,
cert-manager, etc.) remains `sb`; the source of truth for ad hoc model
deployments is the live cluster plus MCP labels/annotations. Important
deployments can be exported back to `sb` using `write_gitops_patch` (post-v1).

For anything multi-user or production-like, this design would need additional
guardrails. For Superbloom, it is appropriate.

## Motivation

The homelab already has working manual KServe model deployments. The next step is
not another custom CRD or reconciler; it is an imperative operator interface that
can search recipes, explain trade-offs, download weights, apply deployments, and
observe status -- all without requiring a GitOps round-trip per deploy.

Kubeflow is actively designing an upstream MCP focused on Kubeflow SDK/Trainer
workflows. This design intentionally avoids duplicating that scope. It focuses on
KServe inference and model-serving recipes. If the upstream Kubeflow MCP becomes
useful for Trainer/Pipelines, Hermes can adopt it as a separate MCP later.

## Repository boundary

This work lives in a new standalone repo under `~/dev/homelab`, not in Nexus.
Nexus is treated as dead/legacy for this effort.

Responsibilities:

- `homelab-mcp`: Rust MCP servers, shared crates, tests, and container builds.
- `sb`: GitOps deployment manifests for platform infrastructure (ArgoCD, Flux,
  cert-manager, KServe controller, vLLM runtime, MCP server deployments).
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
- Do not require a GitOps commit-PR-ArgoCD round-trip for model deploy.
- Do not accept raw HF tokens in tool arguments. Tokens come from K8s Secrets.
- Do not build GitOps patch export in v1. It is a post-v1 audit convenience.

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
        │     └── kube-rs helpers
        │
        ├── recipe sources
        │     ├── Spark Arena registry clone/cache
        │     └── local homelab recipe directory
        │
        ├── resolver
        │     ├── normalize recipe metadata
        │     ├── apply local overrides
        │     ├── lower recipe to DeploymentPlan
        │     └── validate against ClusterProfile
        │
        ├── renderer
        │     └── KServe InferenceService YAML
        │
        ├── deployer (kube-rs direct apply)
        │     ├── ensure_weights → K8s Job on NAS node (hf download)
        │     ├── download_status → Job/pod status
        │     └── apply_plan → create InferenceService (create_only default)
        │
        └── observer (kube-rs read)
              ├── status → InferenceService conditions, pods, events
              └── logs → predictor pod logs
```

The MCP is a thin Rust service. It reads recipes, resolves them into a deployment
plan with a verifiable digest, downloads weights via K8s Jobs on the NAS node,
applies deployments directly via kube-rs, and observes status. Kubernetes remains
the serving control plane.

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

## Model cache path convention

Model IDs from Hugging Face contain `/` (e.g. `deepseek-ai/DeepSeek-V4-Flash`).
The existing convention on Superbloom uses **nested paths** matching the HF
namespace structure:

```text
/tank/models/
  Qwen/
    Qwen3-8B/
      config.json
      model.safetensors
      .homelab-mcp-download.json   ← sentinel
  deepseek-ai/
    DeepSeek-V4-Flash/
      ...
      .homelab-mcp-download.json   ← sentinel
```

The download command is:

```text
hf download deepseek-ai/DeepSeek-V4-Flash --local-dir /tank/models/deepseek-ai/DeepSeek-V4-Flash
```

HF's `--local-dir` already creates the nested directory structure. This matches
the existing pattern on the NAS. No path canonicalization is needed because HF
model IDs are safe filesystem path segments when used as `--local-dir` targets.

## Cache sentinel

A directory existing is not sufficient proof that weights are fully downloaded.
`ensure_weights` checks for a sentinel file:

```text
/tank/models/<model-id>/.homelab-mcp-download.json
```

Sentinel contents:

```json
{
  "model_id": "deepseek-ai/DeepSeek-V4-Flash",
  "revision": "main",
  "downloaded_at": "2026-06-03T22:15:00Z",
  "source": "huggingface",
  "complete": true
}
```

`ensure_weights` checks the sentinel, not just the directory. If the sentinel
is absent or `complete` is false, weights are considered not cached and a
download Job is created.

The download Job is responsible for writing the sentinel on success. This is
done by appending a sentinel-write command after the `hf download` invocation:

```text
hf download <model-id> --local-dir <path> --token $HF_TOKEN &&
echo '{"model_id":"<model-id>","revision":"<rev>","downloaded_at":"'$(date -uIs)'","source":"huggingface","complete":true}' > <path>/.homelab-mcp-download.json
```

## Download Job lifecycle

Download Jobs are created with:

- **Deterministic name** derived from model ID and revision digest:
  `download-<sanitized-model-id>-<revision-short>`.
- **`ttlSecondsAfterFinished`**: completed Jobs are cleaned up automatically.
  Suggested value: `3600` (1 hour).
- **Labels** for recipe, model ID, and revision for later querying.
- **`restartPolicy: Never`** with `backoffLimit: 2`.
- **`nodeSelector`** targeting Superbloom for direct ZFS writes.

`download_status` resolves Jobs by deterministic name or labels, not by
recipe_id alone. Multiple downloads for the same model at different revisions
may coexist briefly.

## Hugging Face credentials

- The MCP never accepts raw HF tokens in tool arguments.
- Download Jobs reference a pre-existing K8s Secret (e.g. `hf-token` in the
  `ai` namespace) containing the token.
- The recipe may declare whether gated access is expected (`model.gated: true`).
- `ensure_weights` returns a structured credential error if the recipe requires
  auth and no secret is configured for the target namespace.

Job env:

```yaml
env:
  - name: HF_TOKEN
    valueFrom:
      secretKeyRef:
        name: hf-token
        key: token
```

The `hf-token` Secret is managed via SOPS in `sb`, like other homelab secrets.

## Rust workspace shape

The v1 repository starts with three crates -- no GitOps crate until the primary
path works:

```text
homelab-mcp/
  crates/
    homelab-mcp-core/        # shared tool response, error, config, telemetry types
    homelab-mcp-k8s/         # kube-rs client helpers, download Job builder, status readers
    model-catalog/           # recipe parsing, normalization, overrides, rendering
  servers/
    model-catalog-mcp/       # rmcp server binary
```

Shared crates must stay boring and tiny until a second MCP proves more
abstraction is needed:

- `homelab-mcp-core`: `ToolResult<T>`, `ToolError`, `ValidationIssue`,
  `Summary`, `Provenance`, `RiskLevel`, config loading, and tracing setup.
- `homelab-mcp-k8s`: kube client factory, download Job builder, Job status
  reader, KServe `InferenceService` status reader, pods/events/logs helpers,
  and cluster profile discovery.

A `homelab-mcp-gitops` crate for patch export will be added post-v1 when the
primary deploy path is proven.

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
- create `InferenceService` resources directly via kube-rs (create_only default).
- validate rendered resources where possible before applying.

All cluster reads and writes go through `kube-rs`.

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
    hf_secret_name: String,       // "hf-token" — K8s Secret for HF auth
    hf_secret_namespace: String,  // namespace of the HF token Secret
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
        hf_secret_name: "hf-token",
        hf_secret_namespace: "ai",
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
- `model`: Hugging Face model id, revision, quantization, gated flag, and
  license notes when known.
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
Recipe -> DeploymentPlan (with plan_digest)
```

`Recipe` describes source facts and recommended runtime assumptions.
`DeploymentPlan` describes the chosen Superbloom deployment plus a verifiable
digest:

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
- `plan_digest`: `sha256(canonical_json(plan_without_digest))`

The digest is computed over the plan content excluding the digest field itself.
`ensure_weights` and `apply_plan` receive the full plan plus the digest. They
verify the digest before acting. This prevents an agent from accidentally
applying a mutated or stale plan while avoiding persistent server-side state.

This boundary prevents Spark Arena's schema from leaking into the serving API
and keeps local cluster choices explicit.

## MCP tools

Initial tools:

- `models.search_recipes(query?, source?, include_experimental?)`
- `models.show_recipe(id)`
- `models.plan_deploy(recipe_id, name, namespace, overrides?)`
  → returns `DeploymentPlan` with `plan_digest`
- `models.ensure_weights(plan, plan_digest)` -- creates K8s Job on NAS node if
  sentinel absent/incomplete, returns `DownloadJobRef`
- `models.download_status(job_ref)` -- checks download Job completion/pod logs
- `models.apply_plan(plan, plan_digest, mode?)` -- creates `InferenceService`
  via kube-rs, default `create_only`
- `models.status(name, namespace)`
- `models.logs(name, namespace, tail?)`

Risk model:

| Tool                 | Risk                                       |
| -------------------- | ------------------------------------------ |
| `search_recipes`     | read                                       |
| `show_recipe`        | read                                       |
| `plan_deploy`        | pure (no side effects)                     |
| `ensure_weights`     | cluster write + NAS filesystem write       |
| `download_status`    | read                                       |
| `apply_plan`         | cluster write                              |
| `status`             | read                                       |
| `logs`               | read                                       |

Safety latch: `apply_plan` refuses unless the model cache sentinel exists for
the plan's target model. Even if an agent skips `ensure_weights`, `apply_plan`
can reject with:

```text
refusing apply: model cache sentinel not found at /tank/models/<model-id>/.homelab-mcp-download.json
```

Apply idempotency: default mode is `create_only`. If the InferenceService
already exists, `apply_plan` fails with a clear error instead of mutating a
running deployment. Future modes (`server_side_apply`, `replace_owned_fields`)
can be added when needed.

Future tools worth adding after v1:

- `models.explain_fit(recipe_id, cluster_profile?)`
- `models.validate_overrides(recipe_id, overrides)`
- `models.smoke_test(name, namespace, prompt?)`
- `models.write_gitops_patch(plan)` -- export rendered manifests to `sb`

## Data flow

1. User asks Hermes to deploy or evaluate a model.
2. MCP searches Spark Arena and local recipes.
3. MCP shows likely recipes, including provenance and hardware assumptions.
4. User or agent selects a recipe and optional overrides.
5. MCP lowers the recipe to a `DeploymentPlan` with `plan_digest` and validates
   it against `ClusterProfile`. Returns an explainable dry-run plan.
6. User or agent calls `ensure_weights(plan, plan_digest)`. MCP verifies the
   digest, checks the cache sentinel, and creates a K8s Job on the NAS node
   (Superbloom) that runs `hf download <model-id> --local-dir /tank/models/<model-id>`
   if the sentinel is absent or incomplete. Returns `DownloadJobRef`.
7. Agent polls `download_status(job_ref)` until the download Job completes and
   the sentinel is written.
8. User or agent calls `apply_plan(plan, plan_digest)`. MCP verifies the digest,
   checks the sentinel, and creates the `InferenceService` via kube-rs
   `create_only` apply.
9. KServe reconciles, schedules the predictor pod on the GPU node (Spark), which
   reads weights from hostPath `/mnt/nas/models`.
10. MCP observes KServe/Kubernetes status and logs through `kube-rs`.

## Error handling

Tool responses should be structured for agent reasoning:

- validation errors include the exact field and accepted range/value.
- missing recipe errors include similar recipe suggestions.
- render errors distinguish unsupported recipe fields from unsafe overrides.
- fit explanations include recipe provenance, cluster profile assumptions, and
  suggested alternatives when a recipe does not fit.
- sentinel check failures return the expected sentinel path and current state.
- credential errors specify which secret is missing and whether the model is
  gated.
- apply errors distinguish "already exists" from "sentinel missing" from
  "digest mismatch".
- status/log tools include KServe conditions, pod state, recent events, and a
  concise summary.

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
- verify plan digest computation over canonical JSON.
- verify sentinel check logic (absent, incomplete, present).
- verify download Job targets NAS node with correct paths.
- verify `apply_plan` refuses when sentinel is absent.

Integration tests:

- run MCP tools against fixture recipe directories.
- validate rendered manifests with Kubernetes schema tooling where available.
- exercise `kube-rs` clients against mocks or a test API where practical.

Cluster validation:

- run `ensure_weights` for one known-small/gated-free recipe.
- verify download Job schedules on Superbloom and writes sentinel metadata.
- apply one `InferenceService` via kube-rs `create_only`.
- verify KServe readiness, predictor pod scheduling on GPU node, pod logs.
- optionally export equivalent GitOps patch and compare rendered manifest.

## V1 cutline

V1 should be intentionally small:

1. Parse local recipe YAML from `sb`.
2. Import/cache enough Spark Arena metadata to preserve provenance.
3. Normalize to internal `Recipe`.
4. Lower `Recipe` to `DeploymentPlan` with `plan_digest`.
5. Validate fit against `ClusterProfile`.
6. Render KServe `InferenceService` YAML.
7. Create download Jobs on the NAS node via kube-rs (`ensure_weights`).
8. Check download Job status (`download_status`).
9. Apply `InferenceService` directly via kube-rs `create_only` (`apply_plan`).
10. Read KServe status/logs with `kube-rs`.

No PR automation, GitOps patch export, model routing, or general Kubeflow
Trainer/Pipeline tools in v1.

## Implementation decisions

- The first MCP lives in a new standalone Rust workspace under
  `~/dev/homelab/homelab-mcp`.
- Shared crates are part of the first implementation, not a later cleanup.
- Use `rmcp`/`rmcp-macros` for MCP server/tool implementation.
- Use `kube-rs` for Kubernetes/KServe reads and direct apply writes.
- `sb` deploys the MCP service and stores local recipes; it does not store
  generated model manifests (those live only in the cluster).
- Spark Arena recipes are read from a pinned clone/cache of
  `spark-arena/recipe-registry`, configured by repository URL and commit/ref.
- Direct apply is the primary deploy path. GitOps patch export is post-v1.
- Download Jobs run on the NAS node (Superbloom) using `hf download`
  (the current Hugging Face CLI, replacing deprecated `huggingface-cli download`).
- The vLLM ClusterServingRuntime already deployed in `sb` provides the pod
  template. The MCP creates `InferenceService` resources that select it.
- Plans carry a `plan_digest` for integrity verification. There is no
  server-side plan store.
- Apply defaults to `create_only` to avoid mutating running deployments.
- Cache sentinels (`.homelab-mcp-download.json`) gate `apply_plan`.
- HF tokens come from K8s Secrets, never from tool arguments.
- Model cache paths use the nested HF convention (`/tank/models/org/model/`).

These choices keep the implementation concrete without adding a custom CRD
layer, and they name the beast: this is an imperative operator, not a renderer.
