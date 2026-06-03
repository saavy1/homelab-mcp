# model-catalog-mcp

MCP server for imperative model deployment on the Superbloom homelab. Exposes 8 tools over streamable HTTP (rmcp + axum).

## Tools

| Tool | Risk | Description |
|------|------|-------------|
| `search_recipes` | read | Search local model recipes by id or model id |
| `show_recipe` | read | Show one recipe by id |
| `plan_deploy` | pure | Plan deployment with plan_digest, validate fit against cluster |
| `ensure_weights` | cluster+fs write | Download weights on NAS node. Idempotent: detects running/completed Jobs |
| `download_status` | read | Check download Job status (NotStarted/Running/Completed/Failed) |
| `apply_plan` | cluster write | Create InferenceService. Sentinel-gated: refuses if weights not ready |
| `status` | read | Read InferenceService conditions + events |
| `logs` | read | Read predictor pod logs |

## Safety

- Plans carry a `plan_digest` (SHA-256) verified before any mutation
- `apply_plan` is gated by the download sentinel — refuses if weights aren't ready
- `apply_plan` defaults to `create_only` — won't mutate existing InferenceServices
- HF tokens come from K8s Secrets, never from tool arguments

## Env vars

| Var | Default | Description |
|-----|---------|-------------|
| `PORT` | 8080 | HTTP listen port |
| `MODEL_CATALOG_RECIPE_DIR` | `/etc/model-catalog/recipes` | Recipe YAML directory |

## K8s access

Uses `kube::Client::try_default()` — auto-detectcts in-cluster ServiceAccount or local `~/.kube/config`. The in-cluster ServiceAccount needs RBAC for Jobs, InferenceServices, pods, events, and secrets (see sb GitOps manifests).

## Logging

Structured JSON via `tracing-subscriber`. Controlled by `RUST_LOG`. All mutating tools carry `#[instrument]` spans with `model_id`, `job_name`, `namespace` fields for Grafana/Loki.
