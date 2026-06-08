# media-mcp

Task-oriented MCP server for Superbloom media operations. It exposes high-level
tools for Jellyseerr requests, SABnzbd queue/history control, and Jellyfin
library/session inspection.

## Tools

| Tool | Service | Description |
|------|---------|-------------|
| `health` | media-mcp | Return service health |
| `search_media` | Jellyseerr | Search for movies or series by query |
| `request_media` | Jellyseerr | Request a movie or series by media type and media id |
| `list_requests` | Jellyseerr | List media requests, optionally filtered by status |
| `approve_request` | Jellyseerr | Approve a media request |
| `decline_request` | Jellyseerr | Decline a media request |
| `list_downloads` | SABnzbd | List active queue and failed history downloads |
| `pause_download` | SABnzbd | Pause a download by `nzo_id` |
| `resume_download` | SABnzbd | Resume a download by `nzo_id` |
| `delete_download` | SABnzbd | Delete a queue or history download by `nzo_id` |
| `retry_failed_download` | SABnzbd | Retry a failed history download by `nzo_id` |
| `get_library_status` | Jellyfin | Return library item counts |
| `refresh_library` | Jellyfin | Trigger a Jellyfin library refresh |
| `get_active_sessions` | Jellyfin | List active playback sessions |
| `get_item_details` | Jellyfin | Read item details by Jellyfin item id |

## Configuration

Set `PORT` to choose the HTTP listener port. It defaults to `8080`.

| Variable | Default |
|----------|---------|
| `MCP_ALLOWED_HOSTS` | Optional comma-separated additions to the built-in allowed hosts |
| `JELLYSEERR_BASE_URL` | `http://jellyseerr.jellyseerr.svc.cluster.local:5055` |
| `JELLYSEERR_API_KEY` | Required |
| `SABNZBD_BASE_URL` | `http://sabnzbd.sabnzbd.svc.cluster.local:8080` |
| `SABNZBD_API_KEY` | Required |
| `JELLYFIN_BASE_URL` | `http://jellyfin.jellyfin.svc.cluster.local:8096` |
| `JELLYFIN_API_KEY` | Required |

## Development

```bash
cargo test -p media-mcp
cargo clippy -p media-mcp --all-targets -- -D warnings
```

Run locally:

```bash
PORT=8080 \
JELLYSEERR_API_KEY=... \
SABNZBD_API_KEY=... \
JELLYFIN_API_KEY=... \
cargo run -p media-mcp
```

Build the container image:

```bash
docker build -f servers/media-mcp/Dockerfile -t media-mcp:local .
```

## Observability

The server emits structured logs and per-tool spans through the shared
`homelab-mcp-core` tracing setup. Tool calls log start/completion/failure events
with service, operation, request id, affected id when available, upstream status,
and retryability.
