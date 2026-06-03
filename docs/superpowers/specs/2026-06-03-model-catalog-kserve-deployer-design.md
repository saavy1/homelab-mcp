# Model Catalog + KServe Deployer Design

Build a standalone Rust MCP workspace for homelab model-serving tools. The first
server is a model catalog and KServe deployer that turns known-good model recipes
into native KServe `InferenceService` resources. Spark Arena recipes are the
preferred reference source, but the system must also allow local ad hoc recipes
for models or tuning choices that are not yet stable upstream.

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
- Prefer GitOps output first: generate manifests and PR-ready patches rather than
  applying live changes by default.
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
        │     └── validate GPU/runtime constraints
        │
        ├── renderer
        │     └── KServe InferenceService YAML
        │
        └── publisher
              ├── dry-run output
              └── GitOps branch/PR patch
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

## MCP tools

Initial tools:

- `models.search_recipes(query?, source?, include_experimental?)`
- `models.show_recipe(id)`
- `models.compare_recipes(ids)`
- `models.render_kserve(recipe_id, overrides?)`
- `models.plan_deploy(recipe_id, name, namespace, overrides?)`
- `models.open_gitops_pr(plan_id)`
- `models.status(name, namespace)`
- `models.logs(name, namespace, tail?)`

Risk model:

- Search/show/compare/render/status/logs are read-only.
- `plan_deploy` writes only local plan artifacts or a dry-run result.
- `open_gitops_pr` is the first write operation and must be explicit.
- Live apply is excluded from the first implementation.

## Data flow

1. User asks Hermes to deploy or evaluate a model.
2. MCP searches Spark Arena and local recipes.
3. MCP shows likely recipes, including provenance and hardware assumptions.
4. User or agent selects a recipe and optional overrides.
5. MCP renders KServe YAML and validates it against known cluster constraints.
6. MCP creates a GitOps patch/branch/PR for review.
7. ArgoCD applies the merged change.
8. MCP observes KServe/Kubernetes status and logs through `kube-rs`.

## GitOps layout

The first implementation should target the existing `sb` GitOps repository and
write model-serving manifests under
`sb/argocd/clusters/superbloom/ai/vllm/resources/`. If the implementation needs
per-model subdirectories, it should create them under that path and wire them
through the existing `ai/vllm` kustomization.

Generated resources should include labels/annotations for:

- recipe id
- recipe source
- upstream Spark Arena path and commit when available
- model id/revision
- owning agent/user context when available

## Error handling

Tool responses should be structured for agent reasoning:

- validation errors include the exact field and accepted range/value.
- missing recipe errors include similar recipe suggestions.
- render errors distinguish unsupported recipe fields from unsafe overrides.
- status/log tools include KServe conditions, pod state, recent events, and a
  concise summary.
- PR creation failures return the branch/path/diff state so the operator can
  recover manually.

## Testing

Unit tests:

- parse representative Spark Arena recipes.
- parse local recipe files.
- merge overrides deterministically.
- render stable KServe YAML snapshots.
- reject unsafe or invalid overrides.
- test shared response/error helpers independently.

Integration tests:

- run MCP tools against fixture recipe directories.
- validate rendered manifests with Kubernetes schema tooling where available.
- exercise `kube-rs` clients against mocks or a test API where practical.
- dry-run GitOps patch generation in a temporary checkout.

Cluster validation:

- deploy one known-small recipe through the GitOps path.
- verify ArgoCD sync, KServe readiness, pod logs, and an inference smoke test.

## Implementation decisions

- The first MCP lives in a new standalone Rust workspace under
  `~/dev/homelab/homelab-mcp`.
- Shared crates are part of the first implementation, not a later cleanup.
- Use `rmcp`/`rmcp-macros` for MCP server/tool implementation.
- Use `kube-rs` for Kubernetes/KServe reads and future controlled writes.
- `sb` deploys the MCP service and stores local recipes/generated manifests.
- Spark Arena recipes are read from a pinned clone/cache of
  `spark-arena/recipe-registry`, configured by repository URL and commit/ref.
- The first version creates GitOps patches only. Live Kubernetes apply is out of
  scope.

These choices keep the implementation concrete without adding a custom CRD layer.
