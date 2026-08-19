# homelab-core

Transport-neutral shared types and utilities for homelab services.

## Key types

- **`OperationEnvelope<T>`** — Stable success/failure envelope with operation metadata, provenance, issues, and optional data or error.
- **`ToolResult<T>`** — Existing MCP tool return contract retained for model-catalog compatibility.
- **`RiskLevel`** — Enum: `Read`, `Pure`, `Write`, `Destructive`, and `ClusterWrite`.
- **`HomelabError`** — Shared error type for validation, IO, serialization, digest, sentinel, and credential failures.
- **`compute_digest(canonical_json)`** — SHA-256 hex digest of a canonical JSON string.

## Tracing

`init_tracing()` sets up structured JSON logging via `tracing-subscriber`. Controlled by `RUST_LOG` env var. Emits JSON suitable for Grafana/Loki.
