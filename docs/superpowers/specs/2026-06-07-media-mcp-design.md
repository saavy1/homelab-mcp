# Media MCP Design

## Goal

Build a task-oriented Rust MCP server that lets Hermes operate the media stack through high-level actions instead of raw API calls. The first supported services are Jellyseerr, SABnzbd, and Jellyfin.

## Context

- Hermes now runs outside k3s on the Mac Mini, but this MCP server should run inside k3s and be reachable remotely by Hermes.
- `homelab-mcp` is the implementation home for Rust MCP servers using streamable HTTP.
- `sb` remains the GitOps deployment home for k3s manifests.
- Media services already run in k3s:
  - Jellyseerr: `jellyseerr.jellyseerr.svc.cluster.local:5055`
  - SABnzbd: `sabnzbd.sabnzbd.svc.cluster.local:8080`
  - Jellyfin: `jellyfin.jellyfin.svc.cluster.local:8096`

## Architecture

Add a new Rust server at `homelab-mcp/servers/media-mcp`. It will follow the existing `model-catalog-mcp` pattern:

- streamable HTTP MCP transport
- `/health` endpoint
- JSON responses
- structured tracing
- Docker image published to GHCR
- Kubernetes `Deployment` and `Service` in `sb`

Use a dedicated Kubernetes secret named `media-mcp-env` for service credentials and base URLs. Do not reuse `hermes-env`.

The k3s deployment should run in the `hermes` namespace as a `ClusterIP` service. Hermes on the Mac Mini connects to the MCP service through the chosen remote route; the server itself only needs to expose HTTP inside the cluster unless a later deployment decision requires an ingress or tunnel.

## Components

### `media-mcp` server

Owns MCP tool definitions, request validation, result shaping, health checks, and service client wiring.

### Jellyseerr client

Handles discovery, request creation, request listing, approval, and decline operations.

### SABnzbd client

Handles queue/history reads plus download pause, resume, delete, and retry operations.

### Jellyfin client

Handles library status/details, library refresh, active sessions, and item details.

Client modules may initially live inside `servers/media-mcp/src/clients/`. Extract them into a shared crate only if they become reusable outside this server.

## Tool Surface

Expose outcome-oriented tools:

| Tool | Purpose | Primary service |
| --- | --- | --- |
| `search_media` | Search for movies/shows to request or inspect | Jellyseerr |
| `request_media` | Request a movie or show | Jellyseerr |
| `list_requests` | List pending/approved/available requests | Jellyseerr |
| `approve_request` | Approve a media request | Jellyseerr |
| `decline_request` | Decline a media request | Jellyseerr |
| `list_downloads` | List active/failed/completed downloads | SABnzbd |
| `pause_download` | Pause a download | SABnzbd |
| `resume_download` | Resume a paused download | SABnzbd |
| `delete_download` | Delete a queued or historical download | SABnzbd |
| `retry_failed_download` | Retry a failed download | SABnzbd |
| `get_library_status` | Summarize Jellyfin library state | Jellyfin |
| `refresh_library` | Trigger a Jellyfin library scan | Jellyfin |
| `get_active_sessions` | Show active Jellyfin playback sessions | Jellyfin |
| `get_item_details` | Fetch details for a Jellyfin item | Jellyfin |

Do not expose generic raw HTTP proxy tools in the first version. If an unsupported operation is needed, add a typed task-oriented tool.

## Data Flow

1. Hermes calls a high-level MCP tool.
2. `media-mcp` validates parameters and resolves the relevant service client.
3. The client calls the upstream service API using credentials from `media-mcp-env`.
4. `media-mcp` normalizes successful responses into predictable JSON while preserving source-specific details under a service-specific field.
5. Upstream failures are mapped to structured MCP errors.

## Error Handling

Errors should include:

- `service`
- `operation`
- `status` when an HTTP status exists
- `retryable`
- `message`

Logs must never include API keys, tokens, or full credential-bearing URLs. Write tools are allowed directly, but operations such as `delete_download` must require explicit stable identifiers and return a summary of what was affected.

## Configuration

Expected environment variables:

- `PORT`
- `JELLYSEERR_BASE_URL`
- `JELLYSEERR_API_KEY`
- `SABNZBD_BASE_URL`
- `SABNZBD_API_KEY`
- `JELLYFIN_BASE_URL`
- `JELLYFIN_API_KEY`
- `RUST_LOG`
- optional OpenTelemetry variables matching `model-catalog-mcp`

The Kubernetes deployment should source credentials from `media-mcp-env`. Non-secret internal base URLs may be plain env vars in the deployment.

## GitOps Deployment

Add an ArgoCD app under `sb/argocd/clusters/superbloom/infra/media-mcp/` with resources mirroring `infra/model-catalog-mcp`:

- `app.yaml`
- `kustomization.yaml`
- `resources/deployment.yaml`
- `resources/service.yaml`
- `resources/kustomization.yaml`

The deployment should use a non-root security context, read-only root filesystem, dropped capabilities, liveness/readiness probes on `/health`, and labels tying it to Hermes.

## Testing

Unit and integration tests should cover:

- request parameter validation
- response normalization
- upstream HTTP success/failure mapping
- secret redaction in error/log paths where practical
- each write tool’s required identifier behavior

Use mock HTTP servers for Jellyseerr, SABnzbd, and Jellyfin instead of calling live services in automated tests.

Validation commands:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

For GitOps manifests, validate with the repository’s existing YAML/Kustomize checks if present; otherwise inspect generated manifests with `kubectl kustomize` or equivalent during implementation.

## Out of Scope

- Generic raw HTTP proxy tools
- Sonarr/Radarr/Prowlarr/Bazarr support
- Ingress or public exposure for `media-mcp`
- A Hermes skill for media workflows
- Automatic credential creation or SOPS encryption

## Open Decisions Resolved

- Implementation language: Rust.
- Server home: `homelab-mcp`.
- Deployment home: `sb` GitOps.
- Runtime location: k3s.
- Safety model: direct write tools are acceptable.
- Secret strategy: dedicated `media-mcp-env`.
