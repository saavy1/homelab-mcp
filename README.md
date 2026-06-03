# homelab-mcp

Custom Rust MCP servers for the Superbloom homelab. Each server is a standalone
binary in `servers/`, backed by shared crates in `crates/`.

## Servers

| Server | Description |
|--------|-------------|
| [`model-catalog-mcp`](servers/model-catalog-mcp/) | Imperative model deployer: search recipes, download weights on NAS, apply KServe InferenceServices, observe status |

## Crates

| Crate | Description |
|-------|-------------|
| [`homelab-mcp-core`](crates/homelab-mcp-core/) | `ToolResult<T>`, `RiskLevel`, `compute_digest`, error types, tracing init |
| [`homelab-mcp-k8s`](crates/homelab-mcp-k8s/) | kube-rs live client: download Job CRUD, InferenceService apply, status/logs/events readers |
| [`model-catalog`](crates/model-catalog/) | Recipe parsing, cluster profile, deployment planning, KServe YAML rendering |

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Servers use [rmcp](https://github.com/anthropics/rmcp) with streamable HTTP transport.
Set `PORT` (default 8080) and server-specific env vars.

## Deployment

Built via GitHub Actions, pushed to GHCR. Deployed as ArgoCD apps in the
[sb](https://github.com/saavy1/sb) GitOps repo. Runs on the NAS node (superbloom).
