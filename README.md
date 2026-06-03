# homelab-mcp

Custom Rust MCP servers for the Superbloom homelab. Each server is a standalone
binary in the `servers/` directory, backed by shared crates in `crates/`.

## Servers

| Server | Description |
|--------|-------------|
| `model-catalog-mcp` | Imperative model deployer: search recipes, download weights on NAS, apply KServe InferenceServices, observe status |

## Crates

| Crate | Description |
|-------|-------------|
| `homelab-mcp-core` | Shared types: `ToolResult`, `RiskLevel`, `ValidationIssue`, `compute_digest`, error variants |
| `homelab-mcp-k8s` | kube-rs helpers: download Job builder, status/log readers, sentinel types |
| `model-catalog` | Recipe parsing, cluster profile, deployment planning, KServe rendering |

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Servers use the rmcp SDK with streamable HTTP transport. Set `PORT` (default
8080) and server-specific env vars.

## Deployment

Built via GitHub Actions and pushed to GHCR. Deployed as ArgoCD apps in the
[sb](https://github.com/saavy1/sb) GitOps repo, running on the NAS node.

## Safety

- Plans carry a `plan_digest` (SHA-256) verified before any mutation
- Model apply defaults to `create_only` and is gated by cache sentinels
- Secrets come from K8s, never from tool arguments
