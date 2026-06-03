# homelab-mcp

Imperative Rust MCP operator for the Superbloom homelab. Downloads model weights, validates fit against cluster capacity, applies KServe InferenceServices, and observes status.

## Architecture

```text
Hermes / agent
        │
        ▼
Model Catalog MCP (on NAS node)
        │
        ├── Recipe search & planning (pure)
        ├── Weight download via K8s Jobs on NAS (hf download + sentinel)
        ├── InferenceService apply via kube-rs (create_only)
        └── Status & logs observation
```

Two-node K3s cluster:
- **Superbloom** (NAS, x86_64) — stores weights at `/tank/models`, runs the MCP server and download Jobs
- **DGX Spark** (GPU, aarch64) — runs InferenceService pods, reads weights from `/mnt/nas/models` via SMB mount

## MCP Tools

| Tool | Risk | Description |
|------|------|-------------|
| `search_recipes` | read | Search local model recipes |
| `show_recipe` | read | Show one recipe by id |
| `plan_deploy` | pure | Plan deployment with plan_digest, validate fit |
| `ensure_weights` | cluster+filesystem write | Download weights on NAS if sentinel absent |
| `download_status` | read | Check download job status |
| `apply_plan` | cluster write | Create InferenceService (create_only, sentinel-gated) |
| `status` | read | Read KServe model status |
| `logs` | read | Read predictor pod logs |

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p model-catalog-mcp
```

The server listens on port 8080 (HTTP Streamable MCP transport). Set `PORT` and `MODEL_CATALOG_RECIPE_DIR` env vars.

## Deployment

Built via GitHub Actions and pushed to `ghcr.io/saavy1/model-catalog-mcp`. Deployed as an ArgoCD app in the [sb](https://github.com/saavy1/sb) GitOps repo.

## Safety

- Plans carry a `plan_digest` (SHA-256) for integrity verification
- `apply_plan` refuses if the model cache sentinel (`.homelab-mcp-download.json`) is absent
- `apply_plan` defaults to `create_only` — won't mutate existing InferenceServices
- HF tokens come from K8s Secrets, never from tool arguments
