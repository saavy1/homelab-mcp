# homelab-mcp

Rust services, libraries, and an agent-friendly CLI for Superbloom homelab
operations. Services live in `servers/`, with shared crates in `crates/`.

## Servers

| Server | Description |
|--------|-------------|
| [`model-catalog-mcp`](servers/model-catalog-mcp/) | Imperative model deployer: search recipes, download weights on NAS, apply KServe InferenceServices, observe status |
| [`homelab-api`](servers/homelab-api/) | Versioned HTTP API for media search, requests, downloads, library operations, and sessions |

## Crates

| Crate | Description |
|-------|-------------|
| [`homelab-api-model`](crates/homelab-api-model/) | Stable versioned HTTP envelopes, API models, errors, and capability metadata |
| [`homelab-client`](crates/homelab-client/) | Curated typed Rust client for the homelab API |
| [`homelab-cli`](crates/homelab-cli/) | Agent-friendly `homelab` CLI backed by the typed client |
| [`homelab-media`](crates/homelab-media/) | Transport-neutral media operations and upstream clients |
| [`homelab-core`](crates/homelab-core/) | transport-neutral operation contracts, MCP `ToolResult<T>` compatibility, digest helpers, error types, tracing init |
| [`homelab-mcp-k8s`](crates/homelab-mcp-k8s/) | kube-rs live client: download Job CRUD, InferenceService apply, status/logs/events readers |
| [`model-catalog`](crates/model-catalog/) | Recipe parsing, cluster profile, deployment planning, KServe YAML rendering |

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The model catalog server uses [rmcp](https://github.com/modelcontextprotocol/rust-sdk)
with streamable HTTP transport. The media API is available at `/api/v1`; set
`HOMELAB_API_URL` to that base URL when using the `homelab` CLI.

To check whether a TV season is complete, use the TV catalog ID returned by
search as `--media-id`:

```bash
homelab media search --query "Rick and Morty"
homelab media library availability --media-id 60625 --season 3
```

Season availability compares Jellyseerr episode announcements with episode
presence in Jellyfin. Season `0` means specials. The availability operation is
read-only.

Build the homelab API image locally with:

```bash
docker build -f servers/homelab-api/Dockerfile -t homelab-api:local .
```

## Deployment

Built via GitHub Actions, pushed to GHCR. Deployed as ArgoCD apps in the
[sb](https://github.com/saavy1/sb) GitOps repo. Runs on the NAS node (superbloom).
