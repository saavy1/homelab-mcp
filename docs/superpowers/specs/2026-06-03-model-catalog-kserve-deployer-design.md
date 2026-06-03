# Model Catalog + KServe Deployer Design

Build a standalone Rust MCP workspace for homelab model-serving tools. The first
server is a model catalog and KServe deployer that turns known-good model recipes
into native KServe `InferenceService` resources. Spark Arena recipes are the
preferred reference source, but the system must also allow local ad hoc recipes
for models or tuning choices that are not yet stable upstream.

Repository objective:

> `homelab-mcp` turns model-serving recipes into explainable, reviewable KServe
> GitOps changes for Superbloom.

## Motivation

The homelab already has working manual KServe model deployments. The next step is
not another custom CRD or reconciler; it is a safe operator interface that can
search recipes, explain trade-offs, render native KServe manifests, and create
GitOps changes for model serving.

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
- Prefer GitOps output first: generate manifests and local patches rather than
  applying live changes.
- Establish shared Rust crates for future custom MCPs.
- Expose the workflow through an MCP server usable by Hermes and future agents.

## Non-goals

- Do not revive `nexus-ml` `ModelDeployment` CRDs or reconcilers.
- Do not build a general Kubeflow Trainer/Pipelines MCP.
- Do not implement automatic model routing yet.
- Do not require Spark Arena to be complete or authoritative for every model.
- Do not shell out to `kubectl` for normal Kubernetes operations.

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
        └── publisher
              ├── dry-run output
              └── local GitOps patch/diff
```

The MCP is a thin Rust service. It reads recipes, resolves them into a homelab
deployment intent, renders native KServe resources, and optionally writes GitOps
patches. Kubernetes remains the serving control plane.

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
- validate rendered resources where possible before creating GitOps patches.
- keep writes GitOps-first in the initial version.

Raw YAML is acceptable as a render artifact, but cluster observation should go
through `kube-rs`.

## Cluster profile

Validation depends on an explicit `ClusterProfile`, not ad hoc assumptions hidden
inside render code.

The initial shape:

```rust
ClusterProfile {
    cluster_name: String,
    gpu_nodes: Vec<GpuNodeClass>,
    storage_classes: Vec<String>,
    default_namespace: String,
    available_serving_runtimes: Vec<String>,
    max_gpu_per_pod: u32,
    ingress_mode: IngressMode,
    known_model_cache_paths: Vec<String>,
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
- `models.compare_recipes(ids)`
- `models.render_kserve(recipe_id, overrides?)`
- `models.plan_deploy(recipe_id, name, namespace, overrides?)`
- `models.write_gitops_patch(plan_id)`
- `models.diff_plan(plan_id)`
- `models.status(name, namespace)`
- `models.logs(name, namespace, tail?)`

Risk model:

- `search_recipes`, `show_recipe`, `compare_recipes`, `status`, and `logs` are
  read-only.
- `render_kserve` is pure render.
- `plan_deploy` is pure planning and returns a structured plan without writing to
  disk.
- `diff_plan` is read-only against an existing plan.
- `write_gitops_patch` writes to the local `sb` checkout and must be explicit.
- `open_gitops_pr` is a later remote-write tool, not part of v1.
- Live apply is excluded.

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
   `ClusterProfile`.
6. MCP renders KServe YAML and returns an explainable dry-run plan.
7. MCP writes a local GitOps patch only when explicitly asked.
8. ArgoCD applies the merged change.
9. MCP observes KServe/Kubernetes status and logs through `kube-rs`.

## GitOps layout

The first implementation should target the existing `sb` GitOps repository and
read local recipes from:

```text
sb/argocd/clusters/superbloom/ai/vllm/recipes/
```

It should write generated model-serving manifests under per-model directories:

```text
sb/argocd/clusters/superbloom/ai/vllm/resources/<model-name>/
  inferenceservice.yaml
  kustomization.yaml
```

The generated model directory should be wired through the existing `ai/vllm`
kustomization.

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
5. Render one known-good KServe `InferenceService`.
6. Produce a local GitOps patch into `sb`.
7. Read KServe status/logs with `kube-rs`.

No PR automation, live apply, model routing, or general Kubeflow Trainer/Pipeline
tools in v1.

## Implementation decisions

- The first MCP lives in a new standalone Rust workspace under
  `~/dev/homelab/homelab-mcp`.
- Shared crates are part of the first implementation, not a later cleanup.
- Use `rmcp`/`rmcp-macros` for MCP server/tool implementation.
- Use `kube-rs` for Kubernetes/KServe reads and future controlled writes.
- `sb` deploys the MCP service and stores local recipes/generated manifests.
- Spark Arena recipes are read from a pinned clone/cache of
  `spark-arena/recipe-registry`, configured by repository URL and commit/ref.
- The first version creates local GitOps patches only. PR automation and live
  Kubernetes apply are out of scope.

These choices keep the implementation concrete without adding a custom CRD layer.
