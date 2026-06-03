# homelab-mcp-core

Shared types and utilities for all homelab-mcp servers.

## Key types

- **`ToolResult<T>`** — Unified return type for MCP tools. Carries `summary`, `risk` level (`Read`/`Pure`/`ClusterWrite`), `data`, and `issues`.
- **`RiskLevel`** — Enum: `Read` (no side effects), `Pure` (computation only), `ClusterWrite` (mutates K8s resources).
- **`HomelabMcpError`** — Error variants: `Validation`, `NotFound`, `Io`, `Serialization`, `DigestMismatch`, `SentinelMissing`, `Credential`.
- **`compute_digest(canonical_json)`** — SHA-256 hex digest of a canonical JSON string.

## Tracing

`init_tracing()` sets up structured JSON logging via `tracing-subscriber`. Controlled by `RUST_LOG` env var. Emits JSON suitable for Grafana/Loki.
