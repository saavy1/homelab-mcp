# Hermes Kubernetes Access Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Hermes's exposed K3s cluster-admin credential with a dedicated read-plus-selected-pod-restart identity, close the world-readable server kubeconfig path, and audit Hermes Kubernetes requests.

**Architecture:** ArgoCD installs an enumerated read ClusterRole, a namespaced pod-restart ClusterRole bound only in approved application namespaces, a dedicated ServiceAccount, and an intentionally long-lived bootstrap token. NixOS changes the generated K3s admin kubeconfig to root-only and enables metadata-only API auditing for the Hermes identity. The Mac receives only the restricted kubeconfig; human cluster administration remains behind Tailscale SSH plus interactive sudo or a separate user boundary.

**Tech Stack:** Kubernetes RBAC and audit policy, K3s, NixOS 26.05 flakes, ArgoCD/Kustomize, Tailscale, macOS kubeconfig.

**Spec:** `docs/superpowers/specs/2026-08-19-homelab-cli-api-design.md`

## Global Constraints

- ServiceAccount name: `hermes-agent`; namespace: `hermes`.
- No wildcard API groups, resources, or verbs.
- No access to Secrets, ConfigMaps, ServiceAccounts, token subresources, RBAC, certificate-signing, authentication APIs, exec, attach, port-forward, proxy, or ephemeral containers.
- Pod restart means `delete` on a named pod; never grant `deletecollection`.
- Restart access is namespace-scoped and excludes infrastructure/system namespaces.
- ArgoCD remains authoritative; do not grant raw workload create/update/patch/delete.
- `/etc/rancher/k3s/k3s.yaml` must become mode `0600` and unreadable to `saavy` without sudo.
- The same macOS account cannot retain a readable cluster-admin kubeconfig after Hermes begins using the restricted identity.
- Long-lived service-account token use is an explicit unattended-homelab tradeoff and requires rotation instructions.
- Never print, paste into chat, log, commit, or pass the token in command-line arguments visible to unrelated processes.
- K3s audit captures request metadata for the Hermes identity only; it captures no request or response bodies.

---

### Task 1: Declare the dedicated Hermes identity and read policy

**Files in `sb`:**
- Create: `argocd/clusters/superbloom/infra/hermes-access/app.yaml`
- Create: `argocd/clusters/superbloom/infra/hermes-access/resources/kustomization.yaml`
- Create: `argocd/clusters/superbloom/infra/hermes-access/resources/service-account.yaml`
- Create: `argocd/clusters/superbloom/infra/hermes-access/resources/read-rbac.yaml`
- Create: `argocd/clusters/superbloom/infra/hermes-access/resources/token-secret.yaml`
- Modify: `argocd/clusters/superbloom/infra/kustomization.yaml`

**Interfaces:**
- Consumes: existing namespace `hermes` and ArgoCD application-of-applications layout.
- Produces:
  - `ServiceAccount/hermes-agent`
  - `Secret/hermes-agent-token` of type `kubernetes.io/service-account-token`
  - `ClusterRole/hermes-agent-read`
  - `ClusterRoleBinding/hermes-agent-read`
  - ArgoCD application `infra-hermes-access`

- [ ] **Step 1: Write the resource skeleton and render it to prove it is incomplete**

Create the Kustomization listing the four resource files, add `hermes-access/` to the parent infra Kustomization, and run `kubectl kustomize argocd/clusters/superbloom/infra/hermes-access/resources`.

Expected initial failure: referenced RBAC files do not yet exist.

- [ ] **Step 2: Add ServiceAccount and token Secret**

Use this exact relationship without token data in Git:

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: hermes-agent
  namespace: hermes
---
apiVersion: v1
kind: Secret
metadata:
  name: hermes-agent-token
  namespace: hermes
  annotations:
    kubernetes.io/service-account.name: hermes-agent
type: kubernetes.io/service-account-token
```

The token controller populates `token` and `ca.crt` after reconciliation. Do not put this Secret under SOPS because Git contains no credential value.

- [ ] **Step 3: Add the enumerated read ClusterRole**

Grant only the following:

```yaml
rules:
  - apiGroups: [""]
    resources: ["namespaces", "nodes", "pods", "events", "services", "endpoints", "persistentvolumes", "persistentvolumeclaims"]
    verbs: ["get", "list", "watch"]
  - apiGroups: [""]
    resources: ["pods/log"]
    verbs: ["get"]
  - apiGroups: ["events.k8s.io"]
    resources: ["events"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["deployments", "replicasets", "statefulsets", "daemonsets"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["batch"]
    resources: ["jobs", "cronjobs"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses", "networkpolicies"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["discovery.k8s.io"]
    resources: ["endpointslices"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["autoscaling"]
    resources: ["horizontalpodautoscalers"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["storage.k8s.io"]
    resources: ["storageclasses", "csinodes", "csidrivers", "volumeattachments"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["nodes", "pods"]
    verbs: ["get", "list"]
  - apiGroups: ["argoproj.io"]
    resources: ["applications", "applicationsets"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["kustomize.toolkit.fluxcd.io"]
    resources: ["kustomizations"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["helm.toolkit.fluxcd.io"]
    resources: ["helmreleases"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["source.toolkit.fluxcd.io"]
    resources: ["gitrepositories", "helmrepositories", "ocirepositories"]
    verbs: ["get", "list", "watch"]
```

Bind it cluster-wide only to `system:serviceaccount:hermes:hermes-agent`. Do not add `configmaps` for convenience.

- [ ] **Step 4: Add the ArgoCD Application**

Follow the current infra application pattern with repo `https://github.com/saavy1/sb.git`, path `argocd/clusters/superbloom/infra/hermes-access/resources`, destination namespace `hermes`, automated prune/self-heal, and `CreateNamespace=true`.

- [ ] **Step 5: Render and statically inspect**

Run Kustomize against both the new resource directory and parent infra root. Inspect the rendered RBAC rules and prove searches for these terms return no granted resource/verb:

```text
secrets
configmaps
serviceaccounts/token
roles
clusterroles
pods/exec
pods/attach
pods/portforward
pods/ephemeralcontainers
create
update
patch
delete
*
```

The token Secret type/annotation is expected; it grants no permission itself.

- [ ] **Step 6: Commit**

```bash
git add argocd/clusters/superbloom/infra/hermes-access argocd/clusters/superbloom/infra/kustomization.yaml
git commit -m "feat: add restricted Hermes Kubernetes identity"
```

---

### Task 2: Bind named-pod restart rights only in application namespaces

**Files in `sb`:**
- Create: `argocd/clusters/superbloom/infra/hermes-access/resources/restart-rbac.yaml`
- Modify: `argocd/clusters/superbloom/infra/hermes-access/resources/kustomization.yaml`

**Interfaces:**
- Consumes: `ServiceAccount/hermes-agent`.
- Produces: `ClusterRole/hermes-agent-pod-restarter` and one RoleBinding in each approved namespace.

- [ ] **Step 1: Add a failing manifest assertion**

Before adding restart RBAC, render resources and check for `hermes-agent-pod-restarter`.

Expected: no match.

- [ ] **Step 2: Define the reusable namespaced ClusterRole**

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: hermes-agent-pod-restarter
rules:
  - apiGroups: [""]
    resources: ["pods"]
    verbs: ["get", "list", "watch", "delete"]
```

Despite being a ClusterRole, it has no effect until a namespace RoleBinding references it. Do not create a ClusterRoleBinding.

- [ ] **Step 3: Add explicit RoleBindings**

Create one `RoleBinding/hermes-agent-pod-restarter` in each namespace:

```text
bazarr
ddns
game-servers
hermes
home-assistant
jellyfin
jellyseerr
prowlarr
radarr
sabnzbd
sonarr
zot
```

Every binding uses:

```yaml
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: hermes-agent-pod-restarter
subjects:
  - kind: ServiceAccount
    name: hermes-agent
    namespace: hermes
```

Do not generate bindings dynamically at runtime; the reviewed list in Git is the policy.

- [ ] **Step 4: Assert exclusions in rendered YAML**

Prove there is no restart RoleBinding in:

```text
alloy
argocd
caddy-system
cert-manager
external-secrets
flux-system
kube-system
monitoring
tailscale
```

Also prove the restart ClusterRole lacks `deletecollection`, workload resources, and every non-core API group.

- [ ] **Step 5: Render and commit**

Run Kustomize for the application resources and parent infra root. Expected: success and exactly 12 restart RoleBindings.

```bash
git add argocd/clusters/superbloom/infra/hermes-access/resources
git commit -m "feat: allow Hermes pod restarts in app namespaces"
```

---

### Task 3: Make K3s admin credentials root-only and enable audit metadata

**Files in `sb`:**
- Modify: `nixos/modules/k3s.nix`

**Interfaces:**
- Consumes: existing K3s server flags and stable API SAN `100.66.91.56`.
- Produces:
  - `/etc/rancher/k3s/k3s.yaml` mode `0600`
  - `/etc/rancher/k3s/audit-policy.yaml` mode `0600`
  - rotating `/var/log/k3s/audit.log`
  - metadata events only for user `system:serviceaccount:hermes:hermes-agent`

- [ ] **Step 1: Add a failing Nix evaluation assertion**

Evaluate the configured K3s flags and assert that `--write-kubeconfig-mode=600` exists and `--write-kubeconfig-mode=644` does not.

Expected before change: failure because `644` is configured.

- [ ] **Step 2: Declare the audit policy in Nix**

Add:

```nix
environment.etc."rancher/k3s/audit-policy.yaml" = {
  mode = "0600";
  text = ''
    apiVersion: audit.k8s.io/v1
    kind: Policy
    omitStages:
      - RequestReceived
    rules:
      - level: Metadata
        users:
          - system:serviceaccount:hermes:hermes-agent
      - level: None
  '';
};
```

Metadata level records no request or response body. The terminal `None` rule prevents a surprise all-cluster audit stream.

- [ ] **Step 3: Replace the K3s flags**

Keep the current Traefik, TLS SAN, and Flannel settings. Replace `644` with `600`, then add:

```nix
"--kube-apiserver-arg=audit-policy-file=/etc/rancher/k3s/audit-policy.yaml"
"--kube-apiserver-arg=audit-log-path=/var/log/k3s/audit.log"
"--kube-apiserver-arg=audit-log-maxage=14"
"--kube-apiserver-arg=audit-log-maxbackup=5"
"--kube-apiserver-arg=audit-log-maxsize=100"
```

Do not change the TLS SAN or Tailscale interface in this task.

- [ ] **Step 4: Evaluate and build without activation**

Run from the `nixos` directory:

```bash
nix flake check --no-build
nix build .#nixosConfigurations.superbloom.config.system.build.toplevel --no-link --print-out-paths
```

Expected: both succeed. Inspect the evaluated flags and generated `/etc` derivation to confirm the audit policy content and `0600` mode.

- [ ] **Step 5: Commit**

```bash
git add nixos/modules/k3s.nix
git commit -m "fix(nixos): restrict and audit Kubernetes access"
```

---

### Task 4: Deploy RBAC and prove both sides of the permission boundary

**Files:**
- No new source files.
- Live verification against Superbloom after pushing the two `sb` commits.

**Interfaces:**
- Consumes: reconciled `infra-hermes-access` ArgoCD application.
- Produces: evidence that the ServiceAccount exists, token is populated, reads work, and forbidden operations fail before the admin path is closed.

- [ ] **Step 1: Push and wait for reconciliation**

Push the RBAC commits to `main`. Wait for ArgoCD to report the application Synced and Healthy. Verify the ServiceAccount, ClusterRoles, bindings, and populated token Secret exist.

- [ ] **Step 2: Run required positive authorization checks as the ServiceAccount**

Use the server's admin context only for impersonation checks:

```bash
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent list pods --all-namespaces
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent get pods --subresource=log -n home-assistant
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent list events --all-namespaces
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent list persistentvolumes
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent delete pods -n home-assistant
```

Expected: `yes` for each.

- [ ] **Step 3: Run required negative checks**

Check at minimum:

```bash
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent list secrets --all-namespaces
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent list configmaps --all-namespaces
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent create pods/exec -n home-assistant
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent create pods/attach -n home-assistant
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent create pods/portforward -n home-assistant
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent patch deployments -n home-assistant
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent delete pods -n kube-system
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent deletecollection pods -n home-assistant
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent create serviceaccounts/token -n hermes
kubectl auth can-i --as=system:serviceaccount:hermes:hermes-agent create rolebindings -n hermes
```

Expected: `no` for each. Any unexpected `yes` blocks deployment.

- [ ] **Step 4: Prove real token behavior without printing it**

In a private shell with tracing disabled, construct a temporary mode-`0600` kubeconfig from the populated token Secret and CA. Run a real `kubectl get pods -A` and a real forbidden `kubectl get secrets -A`. Expected: pods succeed; Secrets return Forbidden. Delete the temporary file.

Never run token extraction through a model-visible command transcript.

- [ ] **Step 5: Remove stale orphaned RBAC after the new identity passes**

The live cluster currently has orphaned `ClusterRole/hermes` and `ClusterRoleBinding/hermes` targeting the absent `ServiceAccount/hermes`. After confirming the subject is still absent and the new identity passes all checks, delete those two orphaned objects explicitly. Recheck that only `hermes-agent-*` RBAC remains for this client path.

---

### Task 5: Activate NixOS hardening and install the restricted Mac kubeconfig

**Files:**
- Live NixOS activation on Superbloom.
- Machine-local `~/.kube/hermes` and `~/.kube/config` on `saavys-mac-mini-3`.
- Optional tracked shell configuration only if the Mac's owning repository already tracks environment variables.

**Interfaces:**
- Consumes: verified ServiceAccount token/CA, server `https://100.66.91.56:6443`, and built NixOS generation.
- Produces: Hermes's default restricted Kubernetes context and no same-user admin credential path.

- [ ] **Step 1: Prepare recovery before activation**

Confirm interactive LAN/Tailscale SSH and interactive sudo work. Record the current NixOS generation and verify the previous generation is bootable. Keep a root shell available during the K3s restart; do not rely on Kubernetes to recover Kubernetes.

- [ ] **Step 2: Activate the tested NixOS generation**

Run `sudo nixos-rebuild test --flake .#superbloom` first. Expected: K3s may restart, then returns active. Verify:

```bash
stat -c '%a %U %G' /etc/rancher/k3s/k3s.yaml
sudo test -r /etc/rancher/k3s/k3s.yaml
sudo systemctl is-active k3s
```

Expected mode/owner: `600 root root`; K3s active. As `saavy`, `test -r /etc/rancher/k3s/k3s.yaml` must fail. If verification passes, persist with `sudo nixos-rebuild switch --flake .#superbloom`.

- [ ] **Step 3: Construct the Mac kubeconfig privately**

This step is performed by the human in a private local terminal, not through Hermes or a model-visible transcript. Create `~/.kube/hermes` with mode `0600` containing:

```yaml
apiVersion: v1
kind: Config
clusters:
  - name: superbloom
    cluster:
      server: https://100.66.91.56:6443
      certificate-authority-data: ${HERMES_CA_B64}
users:
  - name: hermes-agent
    user:
      token: ${HERMES_TOKEN}
contexts:
  - name: hermes@superbloom
    context:
      cluster: superbloom
      user: hermes-agent
current-context: hermes@superbloom
```

With shell tracing disabled, set `HERMES_CA_B64` from `.data.ca\.crt` and set `HERMES_TOKEN` by decoding `.data.token` from `Secret/hermes-agent-token`:

```bash
set +x
umask 077
mkdir -p ~/.kube
HERMES_CA_B64="$(kubectl -n hermes get secret hermes-agent-token -o jsonpath='{.data.ca\.crt}')"
HERMES_TOKEN="$(kubectl -n hermes get secret hermes-agent-token -o jsonpath='{.data.token}' | base64 -D)"
```

Render the shown YAML directly to `~/.kube/hermes`, expand the two variables, run `chmod 600 ~/.kube/hermes`, then `unset HERMES_CA_B64 HERMES_TOKEN`.

Do not place the token in shell history or process arguments. Clear temporary variables/files after writing.

- [ ] **Step 4: Remove the same-user admin credential**

Back up the Mac's old admin kubeconfig only to an encrypted location not readable by the Hermes macOS account, or delete it if the server root copy plus interactive sudo is the chosen recovery path. Replace `~/.kube/config` with a mode-`0600` copy or symlink to `~/.kube/hermes`. Search Hermes-readable configuration and session files for embedded K3s `client-key-data`; remove verified copies.

Do not claim an RBAC boundary while any admin kubeconfig remains readable by the same account.

- [ ] **Step 5: Prove the default Mac context**

Without `--kubeconfig`, run:

```bash
kubectl config current-context
kubectl get pods -A
kubectl get secrets -A
kubectl auth can-i patch deployments -n home-assistant
```

Expected: context `hermes@superbloom`; pod read succeeds; Secret read is Forbidden; patch reports `no`.

- [ ] **Step 6: Prove a disposable pod restart**

Temporarily add `restart-smoke.yaml` to `argocd/clusters/superbloom/infra/hermes-access/resources/kustomization.yaml` and commit/push this disposable Deployment:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: hermes-restart-smoke
  namespace: hermes
spec:
  replicas: 1
  selector:
    matchLabels:
      app: hermes-restart-smoke
  template:
    metadata:
      labels:
        app: hermes-restart-smoke
    spec:
      containers:
        - name: pause
          image: registry.k8s.io/pause:3.10
          securityContext:
            allowPrivilegeEscalation: false
            capabilities:
              drop: ["ALL"]
```

Wait for ArgoCD and `kubectl rollout status deployment/hermes-restart-smoke -n hermes`. With the restricted Mac context, record the pod name and UID, delete that exact pod, wait for rollout readiness, and prove the replacement has a different UID. Then remove `restart-smoke.yaml` from Git and the Kustomization, commit/push the removal, and wait for ArgoCD to prune the Deployment. Do not test against Home Assistant, a database, a StatefulSet, or a standalone pod.

- [ ] **Step 7: Verify audit events and rotation settings**

Using the allowed pod list, denied Secret list, and disposable pod delete already exercised in prior steps, inspect `/var/log/k3s/audit.log` as root on Superbloom and confirm entries identify:

```text
system:serviceaccount:hermes:hermes-agent
list pods
delete pods
forbidden Secret request response code
```

Confirm entries contain metadata but no token, request body, response object, or Secret value. Confirm K3s arguments include max age `14`, backups `5`, and size `100` MiB.

- [ ] **Step 8: Document token rotation**

Add a `Hermes kubeconfig token rotation` section to `argocd/README.md` with this exact procedure:

1. create a second annotated service-account-token Secret;
2. wait for token and CA population;
3. build and test a second restricted kubeconfig;
4. atomically replace `~/.kube/hermes` mode `0600`;
5. verify positive and negative access;
6. delete the old token Secret;
7. confirm the old kubeconfig receives Unauthorized.

Commit only the procedure, never credential material.
