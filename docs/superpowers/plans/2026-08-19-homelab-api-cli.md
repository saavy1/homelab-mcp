# Homelab API and CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken media MCP service with a typed `homelab-api` and installable Rust `homelab` CLI while preserving only tested Jellyseerr, SABnzbd, and Jellyfin behavior.

**Architecture:** Backend clients and media application behavior live in transport-independent crates. An Axum server exposes a versioned, curated HTTP API over the tailnet; a typed Rust client powers a Clap CLI on the Mac. The implementation removes MCP code, configuration, image automation, and GitOps manifests through a clean cutover.

**Tech Stack:** Rust 2024, Axum 0.8, Reqwest 0.12 with rustls, Clap 4, Serde/Schemars, Tokio, Tower HTTP, OpenTelemetry, ArgoCD, Flux/SOPS, Tailscale Kubernetes Operator, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-08-19-homelab-cli-api-design.md`

## Global Constraints

- The first API release implements media operations only.
- The API is curated: no arbitrary backend URL, path, JSON body, credential, shell fragment, or Kubernetes proxy input.
- Backend credentials remain in Kubernetes and never enter CLI configuration or output.
- JSON is the CLI default; no command prompts on stdin.
- No mutation is automatically retried.
- Public DTOs contain normalized fields, not raw upstream response bodies.
- The API uses `/api/v1`; incompatible CLI/API major versions fail before mutation.
- The new binary exposes no MCP endpoint and does not depend on `rmcp`.
- The final tree contains no media MCP code, workflow, manifests, registration, or stale hostname.
- `model-catalog-mcp` and `grafana-mcp` remain operational and otherwise unchanged.
- Do not add Sonarr, Radarr, Prowlarr, or Bazarr clients in this release.
- Existing tests are moved or replaced only when the new observable contract requires it.

---

### Task 1: Make the shared core transport-neutral

**Files:**
- Move: `crates/homelab-mcp-core/` → `crates/homelab-core/`
- Modify: `Cargo.toml`
- Modify: `crates/homelab-core/Cargo.toml`
- Modify: every workspace `Cargo.toml` dependency currently named `homelab-mcp-core`
- Modify: Rust imports returned by `lsp references` for `ToolResult`, `RiskLevel`, `init_tracing_with_service`, and `HomelabMcpError`
- Modify: `crates/homelab-core/src/lib.rs`

**Interfaces:**
- Consumes: existing `ToolResult<T>`, digest helpers, name sanitizers, and tracing initialization used by model-catalog.
- Produces:
  - `OperationEnvelope<T>`
  - `OperationError`
  - `ErrorCode`
  - `ExecutionProvenance`
  - `RiskLevel::{Read, Pure, Write, Destructive, ClusterWrite}`
  - renamed `HomelabError` and `HomelabResult<T>`
  - existing model-catalog-facing behavior under the package/import name `homelab_core`

- [ ] **Step 1: Locate every core caller before the rename**

Run LSP references for `ToolResult`, `RiskLevel`, `init_tracing_with_service`, and `HomelabMcpError`. Record all returned files; the rename must update every caller in one commit.

- [ ] **Step 2: Add failing operation-envelope tests**

Add tests in `crates/homelab-mcp-core/src/lib.rs` proving:

```rust
#[test]
fn operation_success_serializes_stable_fields() {
    let result = OperationEnvelope::success(
        "media.search",
        "req-1",
        RiskLevel::Read,
        "searched media",
        vec!["Alien"],
        ExecutionProvenance::service("jellyseerr"),
    );
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["operation"], "media.search");
    assert_eq!(json["risk"], "read");
    assert_eq!(json["data"][0], "Alien");
    assert!(json.get("error").is_none());
}

#[test]
fn operation_failure_has_no_data_and_preserves_retryability() {
    let result = OperationEnvelope::<serde_json::Value>::failure(
        "media.search",
        "req-2",
        RiskLevel::Read,
        OperationError::new(ErrorCode::Unavailable, "jellyseerr unavailable", true),
        ExecutionProvenance::service("jellyseerr"),
    );
    assert!(!result.ok);
    assert!(result.data.is_none());
    assert!(result.error.as_ref().unwrap().retryable);
}
```

- [ ] **Step 3: Run the focused tests and observe failure**

Run: `cargo test -p homelab-mcp-core operation_`

Expected: compile failure because the new envelope types do not exist.

- [ ] **Step 4: Implement the neutral contracts**

Implement these shapes in `src/lib.rs`:

```rust
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Validation,
    Forbidden,
    NotFound,
    Conflict,
    Unavailable,
    Timeout,
    UnknownOutcome,
    Internal,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct OperationError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ExecutionProvenance {
    pub service: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct OperationEnvelope<T> {
    pub ok: bool,
    pub operation: String,
    pub request_id: String,
    pub risk: RiskLevel,
    pub summary: Summary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ValidationIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OperationError>,
    pub provenance: ExecutionProvenance,
}
```

Add constructors `OperationError::new`, `ExecutionProvenance::service`, `OperationEnvelope::success`, and `OperationEnvelope::failure`. Rename `HomelabMcpError` to `HomelabError` without a deprecated alias. Preserve `ToolResult<T>` because the still-supported model MCP consumes that existing contract.

- [ ] **Step 5: Rename the package and update all callers**

Move the directory, set package name `homelab-core`, change dependency keys to `homelab-core`, and update imports to `homelab_core`. Do not leave a package alias or old directory.

- [ ] **Step 6: Verify the workspace contract**

Run:

```bash
cargo test -p homelab-core
cargo test -p model-catalog-mcp
cargo check --workspace
```

Expected: all pass; model-catalog behavior remains unchanged.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/homelab-core crates/homelab-mcp-k8s crates/model-catalog servers/model-catalog-mcp servers/media-mcp
git commit -m "refactor: make homelab core transport neutral"
```

---

### Task 2: Define versioned media API models

**Files:**
- Create: `crates/homelab-api-model/Cargo.toml`
- Create: `crates/homelab-api-model/src/lib.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `homelab_core::{OperationEnvelope, RiskLevel}`.
- Produces:
  - `ApiVersion`, `Capabilities`
  - `MediaType`
  - `SearchMediaQuery`, `CreateMediaRequest`, `ListRequestsQuery`, `ListDownloadsQuery`, `DeleteDownloadQuery`
  - `MediaSearchItem`, `MediaRequest`, `DownloadItem`, `LibraryStatus`, `ActiveSession`, `MediaOperation`, `BackendHealth`, `MediaHealth`
  - constants `API_MAJOR: u16 = 1`, `API_MINOR: u16 = 0`

- [ ] **Step 1: Add failing DTO serialization tests**

Create `src/lib.rs` with a test module first. Tests must prove snake-case enums, absent raw upstream bodies, bounded media types, and capability version fields:

```rust
#[test]
fn media_type_accepts_only_movie_or_tv() {
    assert_eq!(serde_json::from_str::<MediaType>(r#""movie""#).unwrap(), MediaType::Movie);
    assert!(serde_json::from_str::<MediaType>(r#""music""#).is_err());
}

#[test]
fn search_item_has_no_raw_source_field() {
    let item = MediaSearchItem {
        id: "100".into(),
        media_type: MediaType::Movie,
        title: "Alien".into(),
        year: Some(1979),
        status: Some("available".into()),
    };
    let value = serde_json::to_value(item).unwrap();
    assert!(value.get("source").is_none());
}
```

- [ ] **Step 2: Run and observe failure**

Run: `cargo test -p homelab-api-model`

Expected: package/type compile failure.

- [ ] **Step 3: Implement the DTOs**

Use `#[serde(deny_unknown_fields)]` on mutation request bodies. Keep query structs tolerant only where Axum requires it. Exact required fields:

```rust
pub struct CreateMediaRequest {
    pub media_id: i64,
    pub media_type: MediaType,
}

pub struct DeleteDownloadQuery {
    #[serde(default)]
    pub delete_files: bool,
}

pub struct Capabilities {
    pub api: ApiVersion,
    pub compatible_cli_major: u16,
    pub operations: Vec<String>,
}
```

`MediaHealth` contains one `BackendHealth` for each configured backend and an overall status that can represent `healthy`, `degraded`, or `unavailable`. Do not put `serde_json::Value` in public response DTOs.

- [ ] **Step 4: Verify models**

Run: `cargo test -p homelab-api-model`

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/homelab-api-model
git commit -m "feat: define homelab media API contract"
```

---

### Task 3: Extract and correct the media application layer

**Files:**
- Create: `crates/homelab-media/Cargo.toml`
- Create: `crates/homelab-media/src/lib.rs`
- Create: `crates/homelab-media/src/config.rs`
- Create: `crates/homelab-media/src/error.rs`
- Create: `crates/homelab-media/src/service.rs`
- Create: `crates/homelab-media/src/clients/{mod.rs,jellyseerr.rs,sabnzbd.rs,jellyfin.rs}`
- Create: `crates/homelab-media/tests/common.rs`
- Create: `crates/homelab-media/tests/{jellyseerr.rs,sabnzbd.rs,jellyfin.rs,service.rs}`
- Modify: `Cargo.toml`
- Reference only: `servers/media-mcp/src/{clients,config.rs,error.rs,models.rs,tools.rs}`
- Reference only: `servers/media-mcp/tests/`

**Interfaces:**
- Consumes: API-model request/response DTOs and core envelopes/errors.
- Produces:
  - `MediaConfig::from_env() -> Result<MediaConfig, MediaError>`
  - `MediaService::new(MediaConfig, reqwest::Client) -> MediaService`
  - one `MediaService` async method per retained media operation, each accepting `request_id: &str` and returning `OperationEnvelope<T>` or `MediaError`
  - `MediaError::{Config, Validation, Upstream, Transport, Serialization}` with `error_code()`, `retryable()`, and redacted `public_message()`

- [ ] **Step 1: Move backend behavior tests to the new crate**

Copy and adapt the existing mock-Axum tests before copying implementation. Retain tests that prove:

- Jellyseerr search normalization and query encoding;
- TV requests exclude season zero and include available seasons;
- request list/status normalization;
- approve/decline exact-ID behavior;
- SABnzbd queue/history normalization;
- pause/resume/delete/retry action validation;
- blank download IDs fail before an upstream request;
- Jellyfin library counts, refresh, sessions, and item details;
- upstream authorization bodies and API keys are redacted.

Change imports to `homelab_media` and assertions to the API-model DTOs.

- [ ] **Step 2: Add failing service-level tests**

Use mock backend servers and prove:

```rust
#[tokio::test]
async fn health_reports_degraded_when_one_backend_fails() {
    // Jellyseerr and Jellyfin return success; SABnzbd returns 503.
    let result = service.health("req-health").await;
    assert!(result.ok);
    let health = result.data.unwrap();
    assert_eq!(health.status, HealthStatus::Degraded);
    assert_eq!(health.backends.iter().filter(|b| !b.healthy).count(), 1);
}

#[tokio::test]
async fn timeout_after_mutation_is_unknown_outcome_and_not_retryable() {
    let error = service.pause_download("req-pause", "nzo-1").await.unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::UnknownOutcome);
    assert!(!error.retryable());
}
```

- [ ] **Step 3: Run and observe failure**

Run: `cargo test -p homelab-media`

Expected: package/type compile failure.

- [ ] **Step 4: Port only useful backend code**

Move normalization and request logic from the current clients. Replace `MediaMcpError` with `MediaError`, and replace public models containing `source: Value` with normalized API DTOs. Raw `Value` may exist inside a backend parser but must not escape the media crate.

Build one Reqwest client in the composition root with explicit connect and total timeouts; clone its handle into the three backend clients. Keep API keys inside `ServiceConfig`, whose `Debug` implementation redacts them.

- [ ] **Step 5: Implement `MediaService`**

Implement these methods with the exact operation names shown:

```rust
health("media.health")
search("media.search")
create_request("media.requests.create")
list_requests("media.requests.list")
approve_request("media.requests.approve")
decline_request("media.requests.decline")
list_downloads("media.downloads.list")
pause_download("media.downloads.pause")
resume_download("media.downloads.resume")
delete_download("media.downloads.delete")
retry_download("media.downloads.retry")
library_status("media.library.status")
refresh_library("media.library.refresh")
active_sessions("media.sessions.list")
item_details("media.items.show")
```

Reads use `RiskLevel::Read`; request/action/refresh methods use `RiskLevel::Write`; download deletion with `delete_files=true` uses `RiskLevel::Destructive`. Generate no request IDs inside the service—the caller supplies one.

For health, probe all three backends concurrently with `tokio::join!`. Return a successful envelope with `degraded` and explicit issues when some backends fail; return `unavailable` data when all fail. Health must never claim all backends healthy without probing them.

- [ ] **Step 6: Verify application behavior**

Run:

```bash
cargo test -p homelab-media
cargo clippy -p homelab-media --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/homelab-media
git commit -m "feat: extract tested media application layer"
```

---

### Task 4: Build the versioned Axum API

**Files:**
- Create: `servers/homelab-api/Cargo.toml`
- Create: `servers/homelab-api/Dockerfile`
- Create: `servers/homelab-api/src/lib.rs`
- Create: `servers/homelab-api/src/main.rs`
- Create: `servers/homelab-api/src/error.rs`
- Create: `servers/homelab-api/src/routes/{mod.rs,media.rs}`
- Create: `servers/homelab-api/tests/api.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `MediaService`, API-model DTOs, core envelopes, and tracing initialization.
- Produces:
  - `build_router(MediaService) -> axum::Router`
  - all `/api/v1` routes in the approved spec
  - `/livez` process liveness and `/readyz` configuration readiness
  - `X-Request-Id` acceptance/generation and response propagation
  - stable HTTP status mapping for `ErrorCode`

- [ ] **Step 1: Write failing router contract tests**

Build the router against mock upstream services and test it using `tower::ServiceExt::oneshot`. Cover:

```rust
#[tokio::test]
async fn incompatible_client_major_is_rejected_before_mutation() { /* X-Homelab-Api-Major: 2 -> 409 */ }

#[tokio::test]
async fn create_request_rejects_unknown_fields() { /* body with backend_url -> 422 */ }

#[tokio::test]
async fn delete_download_defaults_delete_files_false() { /* DELETE route, no query */ }

#[tokio::test]
async fn upstream_secret_body_is_not_returned() { /* mock 401 body contains key; response does not */ }
```

Also assert `/mcp` returns `404` and no route accepts a caller-supplied upstream host.

- [ ] **Step 2: Run and observe failure**

Run: `cargo test -p homelab-api --test api`

Expected: package/router compile failure.

- [ ] **Step 3: Implement route and status mapping**

Mount exactly:

```text
GET    /api/v1/capabilities
GET    /api/v1/health
GET    /api/v1/media/search
GET    /api/v1/media/items/{id}
POST   /api/v1/media/requests
GET    /api/v1/media/requests
POST   /api/v1/media/requests/{id}/approve
POST   /api/v1/media/requests/{id}/decline
GET    /api/v1/media/downloads
POST   /api/v1/media/downloads/{id}/pause
POST   /api/v1/media/downloads/{id}/resume
DELETE /api/v1/media/downloads/{id}
POST   /api/v1/media/downloads/{id}/retry
GET    /api/v1/media/library/status
POST   /api/v1/media/library/refresh
GET    /api/v1/media/sessions
GET    /livez
GET    /readyz
```

Map validation to `422`, forbidden to `403`, not-found to `404`, conflict/version mismatch to `409`, unavailable/timeout/unknown-outcome to `503`, and internal errors to `500`. Every JSON error remains an `OperationEnvelope` with `ok=false`.

Apply a request-body limit and request timeout with Tower HTTP. Do not apply a timeout that converts a completed upstream mutation into a generic retryable error; preserve `UnknownOutcome`.

- [ ] **Step 4: Add structured completion events**

For every operation log `request_id`, `operation`, `risk`, result class, duration, backend, and non-sensitive target ID. Do not log search query text, authorization headers, backend error bodies, or media result bodies.

- [ ] **Step 5: Implement the binary**

`main.rs` loads `MediaConfig`, constructs one configured Reqwest client, initializes tracing with service name `homelab-api`, binds `PORT` default `8080`, and serves `build_router`. Missing required configuration exits nonzero before listening.

- [ ] **Step 6: Verify API behavior and image build**

Run:

```bash
cargo test -p homelab-api
cargo clippy -p homelab-api --all-targets -- -D warnings
docker build -f servers/homelab-api/Dockerfile -t homelab-api:plan-check .
```

Expected: tests and image build pass; image entrypoint is `homelab-api` and runs as distroless nonroot.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock servers/homelab-api
git commit -m "feat: expose curated homelab media API"
```

---

### Task 5: Build the typed Rust client

**Files:**
- Create: `crates/homelab-client/Cargo.toml`
- Create: `crates/homelab-client/src/lib.rs`
- Create: `crates/homelab-client/src/error.rs`
- Create: `crates/homelab-client/src/media.rs`
- Create: `crates/homelab-client/tests/client.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: every request/response DTO from `homelab-api-model`.
- Produces:
  - `HomelabClient::new(base_url: Url, http: reqwest::Client) -> Result<Self, ClientError>`
  - `HomelabClient::capabilities()`
  - `HomelabClient::media() -> MediaClient<'_>`
  - one typed `MediaClient` method per API operation
  - `ClientError::{InvalidBaseUrl, IncompatibleApi, Transport, Api, Decode}`

- [ ] **Step 1: Add failing transport tests**

Use a mock Axum API to prove:

- base URL joins paths without dropping `/api/v1`;
- request ID and API-major headers are sent;
- response request ID is retained;
- incompatible capability response fails before a later mutation;
- a timed-out mutation is not retried;
- non-JSON error bodies become redacted decode errors rather than being printed verbatim.

- [ ] **Step 2: Run and observe failure**

Run: `cargo test -p homelab-client`

Expected: package/type compile failure.

- [ ] **Step 3: Implement the client**

Use `url::Url` rather than string concatenation. Construct request paths from fixed constants. Each method accepts a caller-provided request ID and returns `OperationEnvelope<T>`. Cache a successfully checked capability result only for the lifetime of the client process; never skip major-version validation for a mutation.

Do not implement generic `get(path)` or `post(path, Value)` as public methods.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p homelab-client
cargo clippy -p homelab-client --all-targets -- -D warnings
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/homelab-client
git commit -m "feat: add typed homelab API client"
```

---

### Task 6: Build the agent-friendly CLI and release artifact

**Files:**
- Create: `crates/homelab-cli/Cargo.toml`
- Create: `crates/homelab-cli/src/main.rs`
- Create: `crates/homelab-cli/src/args.rs`
- Create: `crates/homelab-cli/src/render.rs`
- Create: `crates/homelab-cli/tests/cli.rs`
- Create: `.github/workflows/release-homelab-cli.yml`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `HomelabClient` and all API DTOs.
- Produces: executable `homelab`, environment variable `HOMELAB_API_URL`, optional `--request-id`, and commands in the approved CLI contract.

- [ ] **Step 1: Write failing CLI process tests**

Start a mock API and invoke `std::process::Command::new(env!("CARGO_BIN_EXE_homelab"))` so the tests exercise the compiled binary rather than calling argument parsers directly. Prove:

```text
homelab capabilities
homelab media search --query Alien
homelab media request create --media-id 100 --media-type movie
homelab media downloads delete --download-id nzo-1 --delete-files
homelab media library status
homelab media sessions list
```

Assertions:

- stdout is one valid JSON document by default;
- invalid arguments exit `2` without making an HTTP request;
- API forbidden exits `3`;
- not-found/conflict exits `4`;
- unavailable/timeout exits `5`;
- partial result exits `6` while retaining the envelope;
- closed stdin never blocks;
- `--output table` prints a bounded human-readable view and no credential fields.

- [ ] **Step 2: Run and observe failure**

Run: `cargo test -p homelab-cli --test cli`

Expected: package/binary compile failure.

- [ ] **Step 3: Implement exact Clap enums**

Model each command as nested `#[derive(Subcommand)]` enums. Use typed integer media IDs, `MediaType` value enums, exact string request/download/item IDs, and a boolean `--delete-files`. Do not expose generic method/path/body flags.

Use `HOMELAB_API_URL` with no default public fallback. If it is absent, fail with exit `2` and a structured error on stdout. Generate a UUID/ULID request ID when the caller omits one; do not include host username or session secrets.

- [ ] **Step 4: Implement stable rendering and exit mapping**

Serialize `OperationEnvelope<T>` directly for JSON. Keep diagnostics out of stdout. Map `ErrorCode` and partial health exactly to the specified exit classes. Table rendering may be operation-specific but must consume the same typed result.

- [ ] **Step 5: Add macOS release workflow**

On tags matching `homelab-v*`, use a macOS ARM-capable runner to:

```bash
cargo build --locked --release -p homelab-cli --target aarch64-apple-darwin
install -m 0755 target/aarch64-apple-darwin/release/homelab dist/homelab
tar -C dist -czf homelab-aarch64-apple-darwin.tar.gz homelab
shasum -a 256 homelab-aarch64-apple-darwin.tar.gz > homelab-aarch64-apple-darwin.tar.gz.sha256
```

Create the GitHub release with `gh release create "$GITHUB_REF_NAME"` and upload the archive plus checksum using the workflow token with `contents: write`. Do not add a long-lived release secret.

- [ ] **Step 6: Verify CLI and workflow inputs**

Run:

```bash
cargo test -p homelab-cli
cargo clippy -p homelab-cli --all-targets -- -D warnings
cargo build --locked --release -p homelab-cli
```

Expected: all pass and `target/release/homelab capabilities` fails cleanly only because no API URL is configured.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/homelab-cli .github/workflows/release-homelab-cli.yml
git commit -m "feat: add installable homelab CLI"
```

---

### Task 7: Remove media MCP code and rename image automation

**Files:**
- Delete: `servers/media-mcp/`
- Delete: `.github/workflows/build-media-mcp.yml`
- Create: `.github/workflows/build-homelab-api.yml`
- Modify: `Cargo.toml`
- Modify: `README.md`
- Modify: server/crate README references found by a scoped search for `media-mcp` and `homelab-mcp-core`

**Interfaces:**
- Consumes: completed API, client, CLI, and media crates.
- Produces: workspace with no media MCP package or `rmcp` dependency outside the still-supported model MCP.

- [ ] **Step 1: Prove the new workspace before deletion**

Run:

```bash
cargo test -p homelab-media -p homelab-api -p homelab-client -p homelab-cli
cargo check --workspace
```

Expected: pass.

- [ ] **Step 2: Delete the old package and update membership**

Remove `servers/media-mcp`, remove its workspace member, and remove documentation that instructs clients to use the media MCP endpoint. Do not leave forwarding modules, package aliases, or deprecated handlers.

- [ ] **Step 3: Replace the image workflow**

Create `build-homelab-api.yml` from the existing image workflow with:

```text
name: Build homelab-api
IMAGE_NAME: ${{ github.repository_owner }}/homelab-api
file: servers/homelab-api/Dockerfile
platforms: linux/amd64
```

Its path filter includes the new core/media/API-model crates, `servers/homelab-api/**`, Cargo files, and the workflow itself. Remove every `media-mcp` workflow/path/image reference.

- [ ] **Step 4: Verify clean removal**

Run a scoped repository search for `media-mcp`, `/mcp`, and `homelab-mcp-core`. Every remaining match must belong to historical design/plan context or the untouched model MCP's `rmcp` dependency; no executable/configuration path may remain.

Run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: replace media MCP with homelab API"
```

---

### Task 8: Replace the GitOps workload and prove the CLI path

**Files in `sb`:**
- Delete: `argocd/clusters/superbloom/infra/media-mcp/`
- Create: `argocd/clusters/superbloom/infra/homelab-api/app.yaml`
- Create: `argocd/clusters/superbloom/infra/homelab-api/resources/{deployment.yaml,service.yaml,kustomization.yaml}`
- Modify: `argocd/clusters/superbloom/infra/kustomization.yaml`
- Delete: `flux/clusters/superbloom/secrets/media-mcp/`
- Create: `flux/clusters/superbloom/secrets/homelab-api/{kustomization.yaml,homelab-api-env.enc.yaml}`
- Modify: `flux/clusters/superbloom/secrets/kustomization.yaml`
- Modify machine-local Hermes skill: `/Users/saavy/.hermes/skills/devops/homelab-ops/SKILL.md`
- Remove configured MCP server with: `hermes mcp remove media`

**Interfaces:**
- Consumes: the immutable `ghcr.io/saavy1/homelab-api` digest reported by the successful Task 7 image workflow and the CLI release archive/checksum.
- Produces: tailnet-only `homelab-api` endpoint, installed `homelab` binary, and Hermes configuration containing no media MCP registration.

- [ ] **Step 1: Create the new encrypted secret without exposing plaintext**

Create the target directory, copy `media-mcp-env.enc.yaml` to `homelab-api-env.enc.yaml`, then run `sops set --in-place homelab-api-env.enc.yaml '["metadata"]["name"]' '"homelab-api-env"'`. Preserve namespace `hermes` and the three encrypted backend keys. Run `sops filestatus` on the new file and verify `stringData` values remain `ENC[...]`; never create a decrypted intermediate file.

- [ ] **Step 2: Write the replacement manifests**

The Deployment uses:

```text
image: ghcr.io/saavy1/homelab-api@sha256:${IMAGE_DIGEST}
PORT=8080
JELLYSEERR_BASE_URL=http://jellyseerr.jellyseerr.svc.cluster.local:5055
SABNZBD_BASE_URL=http://sabnzbd.sabnzbd.svc.cluster.local:8080
JELLYFIN_BASE_URL=http://jellyfin.jellyfin.svc.cluster.local:8096
OTEL_SERVICE_NAME=homelab-api
```

Before editing the manifest, resolve `IMAGE_DIGEST` from the successful pushed image with `docker buildx imagetools inspect`; substitute the resulting 64-hex digest into the committed YAML. Do not commit the literal `${IMAGE_DIGEST}` string or a mutable `latest` tag.

Reference `homelab-api-env`; keep nonroot, read-only-root-filesystem, dropped capabilities, resource requests/limits, `/livez` liveness, and `/readyz` readiness. The Service annotations use `tailscale.com/hostname: homelab-api` and `tailscale.com/tags: tag:homelab-api`; do not preserve `media-mcp` as an alias.

- [ ] **Step 3: Render GitOps locally**

Run `kubectl kustomize argocd/clusters/superbloom/infra` and `kubectl kustomize flux/clusters/superbloom/secrets`. Expected rendered objects:

- one `infra-homelab-api` ArgoCD Application;
- one `homelab-api` Deployment and Service in `hermes`;
- one SOPS-encrypted `homelab-api-env` Secret source;
- zero media MCP objects.

- [ ] **Step 4: Commit and push the clean replacement**

```bash
git add -A
git commit -m "feat: deploy curated homelab API"
git push origin main
```

Wait for ArgoCD/Flux reconciliation. Do not manually create permanent cluster resources.

- [ ] **Step 5: Restrict the new tailnet service**

In the Tailscale policy, make `tag:k8s-operator` an owner of `tag:homelab-api`. Define a host alias for `saavys-mac-mini-3` using the current value of `tailscale ip -4` (observed as `100.123.6.13` during planning), then grant only that host TCP port `8080` to `tag:homelab-api`:

```json
{
  "tagOwners": {
    "tag:homelab-api": ["tag:k8s-operator"]
  },
  "hosts": {
    "saavys-mac-mini-3": "100.123.6.13"
  },
  "grants": [
    {
      "src": ["saavys-mac-mini-3"],
      "dst": ["tag:homelab-api"],
      "ip": ["tcp:8080"]
    }
  ]
}
```

Merge these entries into the existing HuJSON policy rather than replacing unrelated rules. Use the Tailscale policy preview/tests to prove the Mac is accepted and an unapproved tailnet device is not. Audit existing broad grants so none independently grants all devices access to `tag:homelab-api`.

- [ ] **Step 6: Verify the deployed API**

From the Mac, call only the new tailnet endpoint:

```bash
curl --fail --silent http://homelab-api.tailc2db57.ts.net:8080/livez
curl --fail --silent http://homelab-api.tailc2db57.ts.net:8080/readyz
```

Expected: both succeed. Verify `/mcp` returns `404`. From an unapproved tailnet device, TCP port `8080` must be denied. Inspect the Deployment image digest, Ready condition, logs, and one structured request-completion event.

- [ ] **Step 7: Install the pinned CLI**

Download the tagged `aarch64-apple-darwin` archive and checksum on `saavys-mac-mini-3`, run `shasum -a 256 -c`, install `homelab` mode `0755` into the user's existing local-bin directory, and configure `HOMELAB_API_URL` to the new tailnet `/api/v1` base. Do not place backend keys in shell configuration.

- [ ] **Step 8: Run production smoke checks**

From the same user and environment Hermes uses:

```bash
homelab capabilities
homelab media health
homelab media search --query Alien
homelab media requests list
homelab media downloads list
homelab media library status
homelab media sessions list
homelab media library refresh
```

Expected: each prints one JSON envelope; capability major is `1`; health shows each backend explicitly; no credential or raw upstream response is present. Library refresh is the first real write check because it is non-destructive and safe to repeat manually. Verify its correlation ID in `homelab-api` logs.

- [ ] **Step 9: Remove Hermes MCP registration and update its skill**

Run `hermes mcp remove media`. Update `/Users/saavy/.hermes/skills/devops/homelab-ops/SKILL.md` so media examples invoke `homelab`; remove the old media MCP hostname and tool names. Run `hermes gateway restart`, then `hermes gateway status` and `hermes mcp list`. Prove the `media` server is absent while the `homelab` executable is available through terminal and the edited local skill remains enabled.

