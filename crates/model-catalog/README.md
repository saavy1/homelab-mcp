# model-catalog

Recipe parsing, cluster profiling, deployment planning, and KServe InferenceService rendering.

## Modules

- **`recipe`** — Parse YAML recipes via `parse_recipe_yaml` / `load_recipe_dir`. `search_recipes` filters by id or model id.
- **`profile`** — `ClusterProfile::superbloom_default()` defines the two-node K3s cluster (superbloom NAS + DGX Spark GPU). `ModelStorage` holds path/secret config.
- **`planner`** — `plan_deploy(recipe, profile, overrides)` → `ToolResult<DeploymentPlan>`. Validates GPU class, storage mode, gated model secrets. `validate_fit` checks cluster capacity.
- **`digest`** — `compute_plan_digest(plan)` — SHA-256 of canonical JSON with `plan_digest` field excluded. `plan_to_digest_input` produces the canonical form.
- **`render`** — `render_kserve_value(plan)` / `render_kserve_yaml(plan)` produce InferenceService JSON/YAML with homelab-mcp labels.
- **`types`** — `Recipe`, `DeploymentPlan`, `ApplyMode` (default `CreateOnly`), `ModelSpec`, `HardwareSpec`, `RuntimeSpec`, `ServingSpec`.

## Recipes

Recipe YAMLs live in the directory pointed to by `MODEL_CATALOG_RECIPE_DIR` (default `/etc/model-catalog/recipes`). Each recipe describes a model, its runtime, hardware requirements, and serving config.

## Testing

Insta golden snapshots for rendered InferenceService YAML (digest field removed for stability):
```bash
cargo test -p model-catalog
INSTA_UPDATE=always cargo test -p model-catalog  # update snapshots
```
