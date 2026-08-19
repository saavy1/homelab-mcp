# Task 4 RED/GREEN Report

## Scope

Implemented the curated `homelab-api` Axum server and binary. The router exposes only the approved `/api/v1` operations plus `/livez` and `/readyz`; it has no MCP or generic-proxy route or dependency. The server composes `MediaService`, enforces API-major compatibility before upstream mutations, bounds bodies and input fields, maps all application errors into structured envelopes, propagates request IDs, and emits one redacted completion event per operation.

## RED

1. After adding the package manifest and router contract tests, `cargo test -p homelab-api --test api` failed with `E0432 unresolved import homelab_api`. This was the expected missing-library/router failure.
2. The first GREEN attempt compiled after the server was added, but the focused suite exposed the body-limit contract: `request_body_limit_returns_a_structured_error` received `422` instead of `413`. The JSON rejection path was then taught to preserve `413` while returning a validation envelope.
3. `bodyless_mutations_reject_caller_supplied_fields` failed with `200` instead of `422`, proving action routes silently accepted a caller-supplied `backend_url` body. All bodyless mutations now consume the bounded body and reject any non-empty payload before the upstream call.
4. `identifiers_reject_path_syntax_before_backend_calls` failed with `404` instead of `422` for `%2E%2E`, proving path syntax reached an upstream URL. Identifier validation now permits only bounded ASCII identifier characters and rejects dot traversal, slashes, URL syntax, and the SABnzbd `all` sentinel before dispatch.
5. `oversized_request_emits_one_redacted_completion_event` observed two completion events instead of one. Both the handler and the outer body-limit adapter had logged the already-structured `413`. The adapter now replaces and logs only unstructured Tower HTTP limit responses.
6. A parallel full focused run made the completion-event test observe zero events while the isolated and single-threaded runs passed. The cause was a thread-local tracing subscriber racing other tests/callsite interest. The test now installs one process-global capture subscriber and filters by its unique request ID; two consecutive parallel runs passed.

A temporary HEAD-method characterization expected `405`, but Axum intentionally serves HEAD for GET routes. It was removed because it tested framework behavior rather than a project contract; unsupported application methods and unmounted paths remain covered as `405` and `404` respectively.

## GREEN implementation

- Mounted the 16 fixed versioned API routes and two probes; `/mcp`, `/api/v1/mcp`, proxy-like paths, and undocumented actions return `404`.
- Added strict request/query/path validation, a 64 KiB Tower HTTP body limit, read-request timeout, and mutation handling that leaves backend timeouts classified as non-retryable `unknown_outcome`.
- Added `X-Request-Id` acceptance/generation, envelope correlation, and response propagation.
- Added exact HTTP mapping: validation `422` (oversized body `413`), forbidden `403`, not found `404`, conflict/version mismatch `409`, unavailable/timeout/unknown outcome `503`, and internal `500`.
- Added redacted structured completion events with request ID, operation, risk, result class, duration, backend, non-sensitive target ID, and retry classification.
- Added a binary that initializes tracing as `homelab-api`, validates `MediaConfig`, creates one configured Reqwest client, defaults `PORT` to `8080`, and fails before binding when required credentials are missing.
- Added a distroless nonroot image definition with `homelab-api` as the entrypoint.

## Verification

- `cargo test -p homelab-api --test api` — 14 passed; repeated twice consecutively after the tracing fix.
- `cargo test -p homelab-api` — final focused verification passed 14 tests across four suites with zero failures.
- Live binary smoke: `/api/v1/capabilities` returned `200`, a typed version `1.0` envelope, and matching generated request ID header/body; `/mcp` returned `404`.
- Audit smoke with `RUST_LOG=info`: one JSON completion event was observed for request `smoke-audit-1` with operation, risk, result class, duration, backend, target ID, and retry fields.
- Missing-configuration integration test starts the built binary without API keys and confirms a nonzero exit.

Per assignment, formatter, linter, Docker build, and project-wide test suites were not run.
