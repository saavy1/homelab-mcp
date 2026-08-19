# Homelab CLI and Curated API Design

**Date:** 2026-08-19
**Status:** Approved

## Summary

Replace Hermes's media MCP integration with a Rust `homelab` CLI backed by a curated, in-cluster `homelab-api`. Give Hermes a dedicated Kubernetes identity for native read access and controlled pod restarts through `kubectl`; do not recreate Kubernetes inspection behind custom tools.

The first release contains only the media API. The service is designed to accept additional bounded homelab modules later, but it is not a generic API gateway, Kubernetes proxy, shell executor, or arbitrary HTTP relay.

## Goals

- Make `homelab` the stable, agent-friendly interface for specialized homelab operations.
- Remove MCP from the media path rather than maintaining parallel permanent interfaces.
- Keep media backend credentials in Kubernetes instead of copying them to the Mac.
- Reuse existing Jellyseerr, SABnzbd, and Jellyfin client code only where tests prove the desired behavior.
- Keep business logic independent of CLI and HTTP transports.
- Give Hermes useful Kubernetes visibility through native `kubectl` with least-privilege RBAC.
- Permit controller-managed pod restarts in selected application namespaces.
- Produce structured, attributable results and audit records.
- Leave a clean path for future media, storage, model, or observability modules without building them now.

## Non-goals

- Replacing `kubectl` or the Kubernetes API.
- Giving `homelab-api` generic Kubernetes mutation capability.
- Migrating `model-catalog-mcp` or `grafana-mcp` in this release.
- Replacing Tailscale SSH or designing host-level ZFS/systemd operations.
- Building a general API gateway, workflow engine, or agent runtime.
- Adopting kagent, ToolHive, or agentgateway.
- Restricting Hermes's entire local terminal in this release.
- Hardening the existing public media web routes; this is recorded as a follow-up risk.

## Decisions

1. Hermes uses two explicit surfaces:
   - restricted `kubectl` for Kubernetes reads and selected pod deletion;
   - `homelab media ...` for specialized media operations.
2. The CLI calls one curated `homelab-api` over the tailnet.
3. `homelab-api` calls private ClusterIP media services and holds their API credentials.
4. Media MCP is removed through a clean replacement; there is no parallel deployment or compatibility interface.
5. Tailnet policy is sufficient client authentication for the first release.
6. The API is named for the homelab, but only the media module is implemented initially.
7. ArgoCD remains the source of truth for workload manifests. Raw workload patch/update is not granted.

## Runtime Architecture

```text
Hermes on saavys-mac-mini-3
├── kubectl --kubeconfig ~/.kube/hermes
│   └── K3s API → dedicated ServiceAccount → enumerated RBAC
└── homelab media ...
    └── HTTP over encrypted tailnet
        └── homelab-api in namespace hermes
            ├── Jellyseerr ClusterIP
            ├── SABnzbd ClusterIP
            └── Jellyfin ClusterIP
```

The tailnet encrypts transport. TLS at the application endpoint is optional for this topology and must not be described as providing the client identity unless it is actually configured.

### Why a curated API instead of direct backend calls

Direct calls are technically possible: Caddy publicly routes the media applications at `watch.saavylab.dev`, `shows.saavylab.dev`, `movies.saavylab.dev`, `downloads.saavylab.dev`, and `fbi.saavylab.dev`.

The curated API is still preferred because:

- backend API credentials remain in Kubernetes;
- Hermes receives only the operations represented by the homelab contract;
- a same-user process on the Mac cannot recover backend keys from the CLI configuration;
- audit, validation, timeouts, retry policy, and redaction are centralized;
- the existing `media-mcp` already supplies the in-cluster service, credentials, and backend clients, so this is a protocol conversion rather than a new infrastructure tier.

The service must never become a generic reverse proxy. Every operation has a typed request, typed result, explicit risk, and dedicated implementation.

## Workspace Structure

The final names may follow existing workspace conventions, but dependency direction is mandatory.

```text
crates/
  homelab-core/
  homelab-media/
  homelab-api-model/
  homelab-client/
servers/
  homelab-api/
cli/
  homelab/
```

### `homelab-core`

Transport-independent shared contracts:

- `OperationResult<T>`
- `OperationError`
- `RiskLevel`
- `Issue`
- `Provenance`
- request/correlation identifiers

It must not depend on rmcp, clap, axum, reqwest, kube, or backend-specific clients.

### `homelab-media`

Owns media application behavior and typed backend ports:

- search and item lookup;
- request creation and request actions;
- download inspection and actions;
- Jellyfin item lookup and library refresh;
- backend health synthesis;
- input validation, retry classification, and secret redaction.

Backend URLs and credentials enter through server composition/configuration, not operation parameters.

### `homelab-api-model`

Owns versioned HTTP request and response types. It may reuse core result types but contains no server implementation.

### `homelab-client`

Typed client used by the CLI. It owns:

- base URL resolution;
- connect/request timeouts;
- protocol-version checks;
- JSON serialization;
- mapping transport failures into stable client errors.

It does not retry mutations automatically.

### `homelab-api`

Axum composition root deployed in Kubernetes. It constructs real backend clients, mounts versioned routes, emits audit events, and serves health/readiness probes.

### `homelab` CLI

Clap adapter installed on the Mac. It parses typed arguments, calls `homelab-client`, and renders JSON or human-readable tables. It contains no media business rules.

## CLI Contract

Representative commands:

```text
homelab capabilities
homelab media health
homelab media search --query <text>
homelab media item show --item-id <id>
homelab media request create --media-id <id> --media-type <movie|tv>
homelab media requests list [--status <status>]
homelab media requests approve --request-id <id>
homelab media requests decline --request-id <id>
homelab media downloads list [--status <status>]
homelab media downloads pause --download-id <id>
homelab media downloads resume --download-id <id>
homelab media downloads delete --download-id <id> [--delete-files]
homelab media downloads retry --download-id <id>
homelab media library status
homelab media library refresh
homelab media sessions list
```

Rules:

- JSON is the default output. `--output table` is for humans.
- No interactive prompts or confirmations.
- Destructive or mutating commands require exact typed identifiers.
- No command accepts a backend URL, credential, shell fragment, raw JSON body, or arbitrary path.
- Every invocation creates or accepts a correlation ID.
- Mutations are never retried automatically unless the operation contract explicitly marks them retry-safe.
- Output schemas are stable within an API major version.
- Error text is useful to humans, but clients can branch on machine-readable codes.

Suggested exit classes:

- `0`: operation completed successfully;
- `2`: invalid CLI input;
- `3`: authentication or policy denial;
- `4`: not found or state conflict;
- `5`: backend unavailable or timeout;
- `6`: partial operation result.

Example envelope:

```json
{
  "ok": true,
  "operation": "media.requests.approve",
  "request_id": "01J...",
  "risk": "write",
  "summary": "Approved media request 456",
  "data": {},
  "issues": [],
  "provenance": {
    "service": "jellyseerr",
    "timestamp": "2026-08-19T20:00:00Z"
  }
}
```

## HTTP Contract

All routes live below `/api/v1`. Initial routes mirror the CLI contract rather than exposing arbitrary backend paths.

```text
GET  /api/v1/capabilities
GET  /api/v1/health
GET  /api/v1/media/search
GET  /api/v1/media/items/{id}
POST /api/v1/media/requests
GET  /api/v1/media/requests
POST /api/v1/media/requests/{id}/approve
POST /api/v1/media/requests/{id}/decline
GET  /api/v1/media/downloads
POST /api/v1/media/downloads/{id}/pause
POST /api/v1/media/downloads/{id}/resume
DELETE /api/v1/media/downloads/{id}
POST /api/v1/media/downloads/{id}/retry
GET  /api/v1/media/library/status
POST /api/v1/media/library/refresh
GET  /api/v1/media/sessions
```

The exact resource names should preserve current observable media behavior. The implementation plan must inventory all existing MCP tools and account for each one as migrated or intentionally removed before deleting MCP.

API requirements:

- bounded request bodies and query lengths;
- explicit content type;
- request and backend timeouts;
- no backend error bodies passed through verbatim;
- no backend credentials in errors, traces, or metrics;
- readiness fails when required configuration is absent;
- liveness does not depend on every backend being healthy;
- capability response includes API major/minor and CLI compatibility range.

## Kubernetes Identity and RBAC

Create ServiceAccount `hermes-agent` in namespace `hermes` and a dedicated kubeconfig at `~/.kube/hermes` on the Mac.

### Read access

An enumerated ClusterRole may grant `get`, `list`, and `watch` for:

- namespaces and nodes;
- pods and pod logs;
- events;
- services, endpoints, and EndpointSlices;
- PersistentVolumes and PersistentVolumeClaims;
- Deployments, ReplicaSets, StatefulSets, and DaemonSets;
- Jobs and CronJobs;
- Ingresses and NetworkPolicies;
- HorizontalPodAutoscalers;
- node and pod metrics when available;
- selected ArgoCD/Flux status resources needed to observe reconciliation.

Do not grant wildcards. Exclude:

- Secrets;
- ConfigMaps;
- ServiceAccounts and token subresources;
- Roles, ClusterRoles, and bindings;
- certificate-signing and authentication APIs;
- pod exec, attach, port-forward, proxy, and ephemeral containers.

Pod logs are inherently capable of containing application secrets. Redaction at the CLI cannot make unsafe application logging safe.

### Restart access

Use namespace-scoped Roles granting only `get`, `list`, `watch`, and `delete` on `pods`. Do not grant `deletecollection`.

Initial eligible application namespaces:

- `bazarr`
- `ddns`
- `game-servers`
- `hermes`
- `home-assistant`
- `jellyfin`
- `jellyseerr`
- `prowlarr`
- `radarr`
- `sabnzbd`
- `sonarr`
- `zot`

Initial excluded namespaces include:

- `alloy`
- `argocd`
- `caddy-system`
- `cert-manager`
- `external-secrets`
- `flux-system`
- `kube-system`
- `monitoring`
- `tailscale`

Kubernetes RBAC cannot distinguish controller-owned pods from standalone pods or Deployment pods from future StatefulSet pods in the same namespace. This limitation is accepted for the initial raw-`kubectl` restart capability. Namespace additions require review.

### Credential construction

Use a dedicated service-account bearer token and CA data. Store the kubeconfig as mode `0600`. A long-lived token is accepted for unattended homelab operation; document rotation and replace it after device compromise or role changes.

The server endpoint must use the current K3s certificate SAN or a stable name added to the K3s TLS SAN configuration.

### Required removal of the admin path

The current NixOS K3s configuration sets `--write-kubeconfig-mode=644`. That made `/etc/rancher/k3s/k3s.yaml`, including its cluster-admin client credential, readable to Hermes over Tailscale SSH.

The migration must:

1. change generated K3s kubeconfig mode to `0600`;
2. rebuild and verify the file is not readable as `saavy` without sudo;
3. replace the Mac's existing admin kubeconfig with the Hermes kubeconfig;
4. verify `sudo -n` remains unavailable to Hermes's SSH user;
5. remove exported/local copies of the admin kubeconfig from Hermes-accessible paths.

Without these steps, the restricted ServiceAccount provides no meaningful boundary.

## Tailnet and Media API Security

The first release relies on tailnet policy rather than a second application bearer token.

- Expose only `homelab-api` to the tailnet.
- Restrict its destination port to the Mac mini and explicitly approved operator devices.
- Keep backend services on ClusterIP for API-to-backend traffic, even though Caddy separately exposes their public web interfaces.
- Never accept caller-supplied backend hosts.
- Treat tailnet identity as a device/user perimeter, not a process-level identity: another process running as the same Mac user can call the API.
- If stronger per-caller attribution becomes necessary, add Tailscale identity verification or application authentication as a separate design.

## Audit and Observability

### Kubernetes

Enable a minimal K3s API audit policy for `system:serviceaccount:hermes:hermes-agent`:

- metadata-level events for reads;
- metadata sufficient to identify pod deletes;
- no Secret request/response bodies;
- bounded retention and rotation.

Record user, verb, resource, namespace, name, response code, and request URI.

### Homelab API

Emit one structured completion event per operation:

- correlation ID;
- operation name;
- risk level;
- result class;
- duration;
- target identifier where non-sensitive;
- backend name;
- retry classification.

Do not log API keys, authorization headers, full backend error bodies, search queries by default, or media metadata that is unnecessary for operations.

## Failure Semantics

- Backend unavailable: return a typed unavailable error and do not claim success.
- Backend timeout: cancel the request where possible; mutation outcome may be `unknown` if the backend accepted it before timeout.
- Partial multi-backend health: return data for successful backends plus explicit issues; do not collapse partial health into success.
- API/CLI version mismatch: fail before mutation.
- Tailnet unavailable: CLI reports transport failure; it does not fall back to public backend URLs.
- Missing server credential: readiness fails and the affected operation returns configuration unavailable.
- Kubernetes denial: preserve the API server's forbidden result; do not retry through SSH or another credential.
- Pod restart: Hermes must identify the exact namespace and pod; controller recreation is verified separately.

## Testing Design

### Core and media application tests

- validation boundaries for identifiers, status filters, and query lengths;
- backend error classification and redaction;
- timeout and unknown-mutation outcomes;
- partial health aggregation;
- every mutating operation's retry-safety declaration;
- behavior against realistic fake Jellyseerr, SABnzbd, and Jellyfin responses.

### HTTP contract tests

- each route invokes the corresponding application operation;
- malformed and oversized requests are rejected;
- unknown fields follow one consistent policy;
- response envelopes and error codes remain stable;
- credentials and backend response bodies are never returned;
- capability negotiation rejects incompatible clients.

### CLI tests

- argument parsing for every command;
- JSON output and exit class for success and each error family;
- no prompts when stdin is closed;
- mutation commands require exact identifiers;
- CLI process smoke test against a real test HTTP server.

### RBAC verification

Use `kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent` to prove both sides of the boundary.

Required positive checks:

- list/get pods and workload controllers;
- read pod logs;
- list events and storage metadata;
- delete a named pod in an allowed disposable namespace.

Required negative checks:

- get/list Secrets and ConfigMaps;
- exec, attach, and port-forward;
- create or patch workloads;
- delete pods in excluded namespaces;
- create tokens or modify RBAC;
- read `/etc/rancher/k3s/k3s.yaml` as the SSH user.

A disposable Deployment must prove a deleted pod is recreated and becomes Ready. Do not use a stateful production workload for this check.

### Production smoke checks

- CLI capability negotiation;
- media API health with all backend statuses visible;
- real search and list operations;
- one controlled reversible write or an explicitly prepared disposable media request;
- audit record correlation from CLI request through API completion;
- Hermes `kubectl` reads, denied Secret access, and disposable pod restart.

## Rollout

1. **Define the retained contract**
   - inventory every current media MCP operation;
   - classify each operation as retained, corrected, or removed;
   - add application-level behavioral tests for the desired behavior rather than preserving known-broken behavior.
2. **Build the replacement**
   - separate useful backend clients from rmcp handlers;
   - implement the core, media application layer, HTTP API, typed client, and CLI;
   - remove rmcp handlers and dependencies from the replacement workspace.
3. **Create restricted Kubernetes identity**
   - commit ServiceAccount, ClusterRole, Roles, and bindings through GitOps;
   - generate the dedicated kubeconfig;
   - run all positive and negative `can-i` checks.
4. **Close the admin bypass**
   - change K3s kubeconfig mode from `0644` to `0600`;
   - deploy and verify SSH-user denial;
   - install the restricted kubeconfig on the Mac and remove its admin copy.
5. **Prepare the CLI**
   - produce a pinned `aarch64-apple-darwin` binary with checksum;
   - install it outside the repository on the Mac;
   - configure only the new homelab API base URL, not backend credentials.
6. **Replace media MCP with `homelab-api`**
   - remove the media MCP workload, service, tailnet hostname, secret name, and GitOps application;
   - deploy the separately named `homelab-api` workload, service, secret, telemetry identity, and tailnet hostname;
   - expose `/api/v1` only and preserve private backend ClusterIP access.
7. **Cut Hermes over**
   - remove the media MCP registration and stale skill instructions;
   - update the Hermes homelab skill/configuration to use `homelab`;
   - confirm no configured client references the old endpoint;
   - run production smoke checks from the same Hermes execution context.
8. **Observe**
   - inspect API error rate, backend failures, and Kubernetes audit events through at least one normal Hermes workflow.

The broken media MCP provides no availability target, so the application cutover is intentionally fix-forward with no parallel compatibility deployment. Git history remains the emergency recovery path, not a maintained fallback. The Kubernetes credential and NixOS permission changes remain independently reversible if their verification gates fail.

## Follow-up Risks and Deferred Work

- Sonarr, Radarr, SABnzbd, Jellyseerr, Prowlarr, Bazarr, and Jellyfin are publicly routed through Caddy/Cloudflare-backed DNS. Review application authentication, Cloudflare policy, and whether every route needs public exposure.
- The long-lived Kubernetes service-account token needs an explicit rotation procedure.
- Tailscale policy identifies a device/user, not the Hermes process.
- Raw pod deletion remains broader than a controller-aware restart operation within allowed namespaces.
- Host-level Tailscale SSH remains a separate authority path.
- `model-catalog-mcp` and `grafana-mcp` require separate migration decisions.

## Rejected Alternatives

### Direct CLI to public media APIs

Rejected for the first release because it distributes privileged backend credentials to the Mac, weakens enforceable policy, and duplicates centralized audit/error handling. The core/backend separation keeps a future direct composition possible without designing it now.

### Keep MCP beside CLI permanently

Rejected because it doubles transport contracts and configuration while Hermes needs only one specialized surface.

### Reimplement Kubernetes operations in the homelab API

Rejected because Kubernetes already provides the API, client, RBAC, audit model, and ArgoCD reconciliation semantics.

### Adopt kagent, ToolHive, or agentgateway

Rejected for this scope. kagent overlaps Hermes's agent runtime; ToolHive is optimized for managing MCP servers; agentgateway adds a gateway control plane. None is needed for one curated service and one CLI client.

## Acceptance Criteria

- Hermes uses a dedicated non-admin kubeconfig.
- The SSH user cannot read the K3s admin kubeconfig without sudo.
- Required Kubernetes reads and allowed named pod deletes succeed.
- Secrets, exec/attach/port-forward, workload mutation, RBAC mutation, and excluded-namespace pod deletion fail.
- `homelab` covers every retained media operation with stable JSON output.
- Backend credentials exist only in Kubernetes secret/configuration paths, not the Mac CLI config.
- The deployed service exposes the versioned homelab API over the tailnet.
- Hermes completes a real media workflow through the CLI.
- The media MCP endpoint, registration, code, manifests, dependencies, and stale documentation are removed.
- API and Kubernetes audit records identify the exercised operations without exposing credentials.
