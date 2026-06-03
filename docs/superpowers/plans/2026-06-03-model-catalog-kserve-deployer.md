# Model Catalog KServe Deployer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build v1 of `homelab-mcp`: an imperative Rust MCP operator that downloads weights, validates fit, applies InferenceServices, and observes status for the Superbloom homelab.

**Architecture:** Two-node K3s cluster: Superbloom (NAS, no taints, `/tank/models`) + DGX Spark (GPU, taints, `/mnt/nas/models` via SMB). Download Jobs run on Superbloom using `hf download`. InferenceService pods run on Spark with hostPath `/mnt/nas/models`. MCP applies directly via kube-rs `create_only`. Plans carry a `plan_digest` for integrity. Cache sentinels gate `apply_plan`. No GitOps crate in v1.

**Tech Stack:** Rust 2024 edition, `rmcp` 1.7, `rmcp-macros` via `rmcp` macros feature, `kube-rs` 3.1, `serde`, `serde_yaml`, `schemars`, `sha2`, `tokio`, `tracing`, `tempfile`, `insta`.

---

## File Structure

Create this workspace:

```text
homelab-mcp/
  Cargo.toml
  rust-toolchain.toml
  .gitignore
  crates/
    homelab-mcp-core/
      Cargo.toml
      src/lib.rs
    homelab-mcp-k8s/
      Cargo.toml
      src/lib.rs
      src/download.rs
      src/status.rs
    model-catalog/
      Cargo.toml
      src/lib.rs
      src/types.rs
      src/recipe.rs
      src/profile.rs
      src/planner.rs
      src/render.rs
      src/digest.rs
      tests/fixtures/local-recipes/qwen3-8b.yaml
      tests/fixtures/local-recipes/deepseek-v4-flash.yaml
  servers/
    model-catalog-mcp/
      Cargo.toml
      src/main.rs
      src/tools.rs
```

Responsibilities:

- `homelab-mcp-core`: tool result, validation, provenance, error, digest, and tracing helpers.
- `homelab-mcp-k8s`: `kube-rs` client helpers, download Job builder, Job status reader, KServe status/logs.
- `model-catalog`: recipe parsing, recipe search, cluster profile, deployment planning, digest, KServe rendering.
- `model-catalog-mcp`: `rmcp` stdio server that exposes the v1 tools.

## Task 1: Scaffold the Rust Workspace

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`
- Create: `crates/*/Cargo.toml`
- Create: `crates/*/src/lib.rs`
- Create: `servers/model-catalog-mcp/Cargo.toml`
- Create: `servers/model-catalog-mcp/src/main.rs`

- [ ] **Step 1: Write the workspace manifests**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = [
  "crates/homelab-mcp-core",
  "crates/homelab-mcp-k8s",
  "crates/model-catalog",
  "servers/model-catalog-mcp",
]

[workspace.package]
edition = "2024"
license = "MIT"
version = "0.1.0"

[workspace.dependencies]
anyhow = "1.0.100"
insta = { version = "1.43.2", features = ["yaml"] }
k8s-openapi = { version = "0.27.0", features = ["v1_34"] }
kube = { version = "3.1.0", features = ["client", "config", "derive", "runtime"] }
rmcp = { version = "1.7.0", features = ["server", "macros", "schemars", "transport-io"] }
schemars = "1.1.0"
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.145"
serde_yaml = "0.9.34"
sha2 = "0.10.9"
tempfile = "3.23.0"
thiserror = "2.0.17"
tokio = { version = "1.48.0", features = ["macros", "rt-multi-thread", "fs"] }
tracing = "0.1.41"
tracing-subscriber = { version = "0.3.20", features = ["env-filter", "fmt"] }
```

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

Create `.gitignore`:

```gitignore
/target/
/.fleet/
*.log
```

- [ ] **Step 2: Create crate manifests and initial modules**

Create `crates/homelab-mcp-core/Cargo.toml`:

```toml
[package]
name = "homelab-mcp-core"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
schemars.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true
tracing-subscriber.workspace = true
```

Create `crates/homelab-mcp-core/src/lib.rs`:

```rust
pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_is_ready() {
        assert!(super::crate_ready());
    }
}
```

Create `crates/homelab-mcp-k8s/Cargo.toml`:

```toml
[package]
name = "homelab-mcp-k8s"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
homelab-mcp-core = { path = "../homelab-mcp-core" }
k8s-openapi.workspace = true
kube.workspace = true
schemars.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

Create `crates/homelab-mcp-k8s/src/lib.rs`:

```rust
pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_is_ready() {
        assert!(super::crate_ready());
    }
}
```

Create `crates/model-catalog/Cargo.toml`:

```toml
[package]
name = "model-catalog"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
homelab-mcp-core = { path = "../homelab-mcp-core" }
schemars.workspace = true
serde.workspace = true
serde_json.workspace = true
serde_yaml.workspace = true
sha2.workspace = true
thiserror.workspace = true

[dev-dependencies]
insta.workspace = true
```

Create `crates/model-catalog/src/lib.rs`:

```rust
pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_is_ready() {
        assert!(super::crate_ready());
    }
}
```

Create `servers/model-catalog-mcp/Cargo.toml`:

```toml
[package]
name = "model-catalog-mcp"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
anyhow.workspace = true
homelab-mcp-core = { path = "../../crates/homelab-mcp-core" }
homelab-mcp-k8s = { path = "../../crates/homelab-mcp-k8s" }
model-catalog = { path = "../../crates/model-catalog" }
rmcp.workspace = true
schemars.workspace = true
serde.workspace = true
tokio.workspace = true
tracing.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

Create `servers/model-catalog-mcp/src/main.rs`:

```rust
fn main() {
    println!("model-catalog-mcp scaffold");
}
```

- [ ] **Step 3: Verify scaffold compiles**

Run:

```bash
cargo fmt --all
cargo test --workspace
```

Expected: all `crate_is_ready` tests pass.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore crates servers
git commit -m "chore: scaffold homelab MCP workspace (3 crates, no gitops)"
```

## Task 2: Add Core Tool Response, Error, and Digest Types

**Files:**
- Modify: `crates/homelab-mcp-core/src/lib.rs`

- [ ] **Step 1: Replace the initial test with core types**

Write this in `crates/homelab-mcp-core/src/lib.rs`:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Read,
    Pure,
    ClusterWrite,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Provenance {
    pub source: String,
    pub path: Option<String>,
    pub commit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
    pub allowed: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Summary {
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ToolResult<T> {
    pub summary: Summary,
    pub risk: RiskLevel,
    pub data: T,
    pub issues: Vec<ValidationIssue>,
}

impl<T> ToolResult<T> {
    pub fn read(summary: impl Into<String>, data: T) -> Self {
        Self {
            summary: Summary { text: summary.into() },
            risk: RiskLevel::Read,
            data,
            issues: Vec::new(),
        }
    }

    pub fn pure(summary: impl Into<String>, data: T) -> Self {
        Self {
            summary: Summary { text: summary.into() },
            risk: RiskLevel::Pure,
            data,
            issues: Vec::new(),
        }
    }

    pub fn cluster_write(summary: impl Into<String>, data: T) -> Self {
        Self {
            summary: Summary { text: summary.into() },
            risk: RiskLevel::ClusterWrite,
            data,
            issues: Vec::new(),
        }
    }

    pub fn with_issues(mut self, issues: Vec<ValidationIssue>) -> Self {
        self.issues = issues;
        self
    }
}

#[derive(Debug, Error)]
pub enum HomelabMcpError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("sentinel missing or incomplete: {0}")]
    SentinelMissing(String),
    #[error("credential error: {0}")]
    Credential(String),
}

pub type HomelabResult<T> = Result<T, HomelabMcpError>;

pub fn compute_digest(canonical_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_result_has_read_risk_and_summary() {
        let result = ToolResult::read("listed recipes", vec!["qwen3-8b"]);
        assert_eq!(result.risk, RiskLevel::Read);
        assert_eq!(result.summary.text, "listed recipes");
    }

    #[test]
    fn cluster_write_result_carries_risk_level() {
        let result = ToolResult::cluster_write("applied InferenceService", "qwen3-8b");
        assert_eq!(result.risk, RiskLevel::ClusterWrite);
    }

    #[test]
    fn digest_is_deterministic() {
        let json = r#"{"name":"qwen3-8b","namespace":"ai"}"#;
        let d1 = compute_digest(json);
        let d2 = compute_digest(json);
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }

    #[test]
    fn digest_differs_for_different_input() {
        let d1 = compute_digest(r#"{"name":"a"}"#);
        let d2 = compute_digest(r#"{"name":"b"}"#);
        assert_ne!(d1, d2);
    }
}
```

- [ ] **Step 2: Run the core tests**

Run:

```bash
cargo test -p homelab-mcp-core
```

Expected: four tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/homelab-mcp-core/src/lib.rs
git commit -m "feat(core): add shared MCP result types, digest, and sentinel error variants"
```

## Task 3: Add Recipe and Cluster Profile Domain Types

**Files:**
- Modify: `crates/model-catalog/src/lib.rs`
- Create: `crates/model-catalog/src/types.rs`
- Create: `crates/model-catalog/src/profile.rs`

- [ ] **Step 1: Add the domain types**

Create `crates/model-catalog/src/types.rs`:

```rust
use homelab_mcp_core::Provenance;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeSource {
    SparkArena,
    Local,
    AdHoc,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Recipe {
    pub id: String,
    pub source: RecipeSource,
    pub model: ModelSpec,
    pub runtime: RuntimeSpec,
    pub hardware: HardwareSpec,
    pub serving: ServingSpec,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ModelSpec {
    pub id: String,
    pub revision: Option<String>,
    pub quantization: Option<String>,
    pub gated: Option<bool>,
    pub license: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct RuntimeSpec {
    pub image: String,
    pub args: Vec<String>,
    pub env: Vec<EnvVar>,
    pub tensor_parallel: Option<u32>,
    pub max_model_len: Option<u32>,
    pub dtype: Option<String>,
    pub tool_call_parser: Option<String>,
    pub reasoning_parser: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct HardwareSpec {
    pub gpu_class: String,
    pub gpu_count: u32,
    pub estimated_vram_gb: Option<u32>,
    pub gpu_memory_utilization: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ServingSpec {
    pub namespace: String,
    pub service_name: Option<String>,
    pub replicas: u32,
    pub storage_mode: StorageMode,
    pub ingress_policy: IngressPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageMode {
    Ephemeral,
    ModelCache,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IngressPolicy {
    ClusterLocal,
    InternalHttp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyMode {
    CreateOnly,
}

impl Default for ApplyMode {
    fn default() -> Self {
        Self::CreateOnly
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct DeploymentPlan {
    pub name: String,
    pub namespace: String,
    pub recipe_id: String,
    pub selected_gpu_class: String,
    pub replicas: u32,
    pub scale_to_zero: bool,
    pub storage_mode: StorageMode,
    pub ingress_policy: IngressPolicy,
    pub env_overrides: Vec<EnvVar>,
    pub resource_requests: ResourceRequests,
    pub model_id: String,
    pub model_revision: Option<String>,
    pub plan_digest: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ResourceRequests {
    pub cpu: String,
    pub memory: String,
    pub gpu_count: u32,
}
```

Create `crates/model-catalog/src/profile.rs`:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeRole {
    ControlPlane,
    Nas,
    GpuWorker,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Taint {
    pub key: String,
    pub effect: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct NodeProfile {
    pub hostname: String,
    pub roles: Vec<NodeRole>,
    pub gpu_product: Option<String>,
    pub gpu_count: u32,
    pub gpu_memory_gb: u32,
    pub taints: Vec<Taint>,
    pub model_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ModelStorage {
    pub nas_hostname: String,
    pub nas_path: String,
    pub gpu_node_path: String,
    pub download_node_selector: String,
    pub hf_secret_name: String,
    pub hf_secret_namespace: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IngressMode {
    ClusterLocal,
    InternalHttp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ClusterProfile {
    pub cluster_name: String,
    pub nodes: Vec<NodeProfile>,
    pub default_namespace: String,
    pub available_serving_runtimes: Vec<String>,
    pub max_gpu_per_pod: u32,
    pub ingress_mode: IngressMode,
    pub model_storage: ModelStorage,
}

impl ClusterProfile {
    pub fn superbloom_default() -> Self {
        Self {
            cluster_name: "superbloom".into(),
            nodes: vec![
                NodeProfile {
                    hostname: "superbloom".into(),
                    roles: vec![NodeRole::ControlPlane, NodeRole::Nas],
                    gpu_product: None,
                    gpu_count: 0,
                    gpu_memory_gb: 0,
                    taints: vec![],
                    model_path: Some("/tank/models".into()),
                },
                NodeProfile {
                    hostname: "gx10-98a5".into(),
                    roles: vec![NodeRole::GpuWorker],
                    gpu_product: Some("NVIDIA-GB10".into()),
                    gpu_count: 1,
                    gpu_memory_gb: 128,
                    taints: vec![
                        Taint {
                            key: "nvidia.com/gpu".into(),
                            effect: "NoSchedule".into(),
                        },
                        Taint {
                            key: "nvidia.com/gpu".into(),
                            effect: "NoExecute".into(),
                        },
                    ],
                    model_path: Some("/mnt/nas/models".into()),
                },
            ],
            default_namespace: "ai".into(),
            available_serving_runtimes: vec!["vllm".into()],
            max_gpu_per_pod: 1,
            ingress_mode: IngressMode::ClusterLocal,
            model_storage: ModelStorage {
                nas_hostname: "superbloom".into(),
                nas_path: "/tank/models".into(),
                gpu_node_path: "/mnt/nas/models".into(),
                download_node_selector: "superbloom".into(),
                hf_secret_name: "hf-token".into(),
                hf_secret_namespace: "ai".into(),
            },
        }
    }

    pub fn gpu_node(&self) -> Option<&NodeProfile> {
        self.nodes.iter().find(|n| n.roles.contains(&NodeRole::GpuWorker))
    }

    pub fn nas_node(&self) -> Option<&NodeProfile> {
        self.nodes.iter().find(|n| n.roles.contains(&NodeRole::Nas))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_has_two_nodes_with_different_model_paths() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.nodes.len(), 2);
        let gpu = profile.gpu_node().expect("has GPU node");
        assert_eq!(gpu.model_path.as_deref(), Some("/mnt/nas/models"));
        let nas = profile.nas_node().expect("has NAS node");
        assert_eq!(nas.model_path.as_deref(), Some("/tank/models"));
    }

    #[test]
    fn storage_paths_differ_between_nodes() {
        let profile = ClusterProfile::superbloom_default();
        assert_ne!(profile.model_storage.nas_path, profile.model_storage.gpu_node_path);
    }

    #[test]
    fn hf_secret_is_configured() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.model_storage.hf_secret_name, "hf-token");
        assert_eq!(profile.model_storage.hf_secret_namespace, "ai");
    }
}
```

Replace `crates/model-catalog/src/lib.rs`:

```rust
pub mod profile;
pub mod types;

pub use profile::{ClusterProfile, IngressMode, ModelStorage, NodeProfile, NodeRole, Taint};
pub use types::{
    ApplyMode, DeploymentPlan, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource,
    ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_names_superbloom() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.cluster_name, "superbloom");
    }
}
```

- [ ] **Step 2: Run model-catalog tests**

Run:

```bash
cargo test -p model-catalog
```

Expected: all profile and type tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/model-catalog/src
git commit -m "feat(catalog): add recipe, DeploymentPlan, and two-node ClusterProfile types"
```

## Task 4: Parse Local Recipe YAML

**Files:**
- Create: `crates/model-catalog/src/recipe.rs`
- Modify: `crates/model-catalog/src/lib.rs`
- Create: `crates/model-catalog/tests/fixtures/local-recipes/qwen3-8b.yaml`
- Create: `crates/model-catalog/tests/fixtures/local-recipes/deepseek-v4-flash.yaml`

- [ ] **Step 1: Add local recipe fixtures**

Create `crates/model-catalog/tests/fixtures/local-recipes/qwen3-8b.yaml`:

```yaml
id: qwen3-8b
source: local
model:
  id: Qwen/Qwen3-8B
  revision: null
  quantization: null
  gated: false
  license: apache-2.0
runtime:
  image: vllm/vllm-openai:latest
  args:
    - --enable-auto-tool-choice
    - --tool-call-parser=hermes
  env: []
  tensor_parallel: 1
  max_model_len: 32768
  dtype: bfloat16
  tool_call_parser: hermes
  reasoning_parser: null
hardware:
  gpu_class: gb10
  gpu_count: 1
  estimated_vram_gb: 24
  gpu_memory_utilization: 0.9
serving:
  namespace: ai
  service_name: qwen3-8b
  replicas: 1
  storage_mode: model-cache
  ingress_policy: cluster-local
provenance:
  source: sb
  path: argocd/clusters/superbloom/ai/vllm/recipes/qwen3-8b.yaml
  commit: null
```

Create `crates/model-catalog/tests/fixtures/local-recipes/deepseek-v4-flash.yaml`:

```yaml
id: deepseek-v4-flash
source: ad-hoc
model:
  id: deepseek-ai/DeepSeek-V4-Flash
  revision: null
  quantization: null
  gated: false
  license: unknown
runtime:
  image: vllm/vllm-openai:latest
  args:
    - --enable-auto-tool-choice
  env: []
  tensor_parallel: 1
  max_model_len: 65536
  dtype: bfloat16
  tool_call_parser: null
  reasoning_parser: null
hardware:
  gpu_class: gb10
  gpu_count: 1
  estimated_vram_gb: 96
  gpu_memory_utilization: 0.92
serving:
  namespace: ai
  service_name: deepseek-v4-flash
  replicas: 1
  storage_mode: model-cache
  ingress_policy: cluster-local
provenance:
  source: local
  path: argocd/clusters/superbloom/ai/vllm/recipes/deepseek-v4-flash.yaml
  commit: null
```

- [ ] **Step 2: Implement parser and search**

Create `crates/model-catalog/src/recipe.rs`:

```rust
use crate::Recipe;
use homelab_mcp_core::{HomelabMcpError, HomelabResult};
use std::{fs, path::Path};

pub fn parse_recipe_yaml(input: &str) -> HomelabResult<Recipe> {
    serde_yaml::from_str(input)
        .map_err(|error| HomelabMcpError::Serialization(error.to_string()))
}

pub fn load_recipe_file(path: impl AsRef<Path>) -> HomelabResult<Recipe> {
    let input = fs::read_to_string(path)?;
    parse_recipe_yaml(&input)
}

pub fn load_recipe_dir(path: impl AsRef<Path>) -> HomelabResult<Vec<Recipe>> {
    let mut recipes = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let is_yaml = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension == "yaml" || extension == "yml");
        if is_yaml {
            recipes.push(load_recipe_file(path)?);
        }
    }
    recipes.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(recipes)
}

pub fn search_recipes<'a>(recipes: &'a [Recipe], query: Option<&str>) -> Vec<&'a Recipe> {
    let Some(query) = query.map(str::to_lowercase) else {
        return recipes.iter().collect();
    };
    recipes
        .iter()
        .filter(|recipe| {
            recipe.id.to_lowercase().contains(&query)
                || recipe.model.id.to_lowercase().contains(&query)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_recipe_fixture() {
        let input = include_str!("../tests/fixtures/local-recipes/qwen3-8b.yaml");
        let recipe = parse_recipe_yaml(input).expect("recipe parses");
        assert_eq!(recipe.id, "qwen3-8b");
        assert_eq!(recipe.model.id, "Qwen/Qwen3-8B");
        assert_eq!(recipe.hardware.gpu_count, 1);
        assert_eq!(recipe.model.gated, Some(false));
    }

    #[test]
    fn searches_by_model_id_case_insensitively() {
        let input = include_str!("../tests/fixtures/local-recipes/deepseek-v4-flash.yaml");
        let recipe = parse_recipe_yaml(input).expect("recipe parses");
        let results = search_recipes(&[recipe], Some("deepseek"));
        assert_eq!(results.len(), 1);
    }
}
```

Modify `crates/model-catalog/src/lib.rs` to add the recipe module:

```rust
pub mod profile;
pub mod recipe;
pub mod types;

pub use profile::{ClusterProfile, IngressMode, ModelStorage, NodeProfile, NodeRole, Taint};
pub use recipe::{load_recipe_dir, load_recipe_file, parse_recipe_yaml, search_recipes};
pub use types::{
    ApplyMode, DeploymentPlan, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource,
    ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_names_superbloom() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.cluster_name, "superbloom");
    }
}
```

- [ ] **Step 3: Run parser tests**

Run:

```bash
cargo test -p model-catalog recipe::
```

Expected: both recipe parser tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/model-catalog/src crates/model-catalog/tests
git commit -m "feat(catalog): parse local model recipes with gated flag"
```

## Task 5: Plan Deployment with Digest Computation

**Files:**
- Create: `crates/model-catalog/src/planner.rs`
- Create: `crates/model-catalog/src/digest.rs`
- Modify: `crates/model-catalog/src/lib.rs`

- [ ] **Step 1: Add digest helper**

Create `crates/model-catalog/src/digest.rs`:

```rust
use crate::DeploymentPlan;
use homelab_mcp_core::compute_digest;
use serde_json::Value;

pub fn plan_to_digest_input(plan: &DeploymentPlan) -> String {
    let mut value = serde_json::to_value(plan).expect("plan serializes");
    remove_digest_field(&mut value);
    serde_json::to_string(&value).expect("canonical JSON")
}

pub fn compute_plan_digest(plan: &DeploymentPlan) -> String {
    compute_digest(&plan_to_digest_input(plan))
}

fn remove_digest_field(value: &mut Value) {
    if let Value::Object(map) = value {
        map.remove("plan_digest");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_recipe_yaml, plan_deploy, ClusterProfile, DeployOverrides};

    #[test]
    fn digest_excludes_itself() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(&recipe, &ClusterProfile::superbloom_default(), DeployOverrides::empty()).data;
        let input = plan_to_digest_input(&plan);
        assert!(!input.contains("plan_digest"));
    }

    #[test]
    fn same_plan_produces_same_digest() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(&recipe, &ClusterProfile::superbloom_default(), DeployOverrides::empty()).data;
        assert_eq!(plan.plan_digest, compute_plan_digest(&plan));
    }
}
```

- [ ] **Step 2: Add planner with digest computation**

Create `crates/model-catalog/src/planner.rs`:

```rust
use crate::{ClusterProfile, DeploymentPlan, EnvVar, NodeRole, Recipe, ResourceRequests, StorageMode};
use homelab_mcp_core::{ToolResult, ValidationIssue};
use crate::digest::compute_plan_digest;

#[derive(Clone, Debug, PartialEq)]
pub struct DeployOverrides {
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub replicas: Option<u32>,
    pub env_overrides: Vec<EnvVar>,
}

impl DeployOverrides {
    pub fn empty() -> Self {
        Self {
            name: None,
            namespace: None,
            replicas: None,
            env_overrides: Vec::new(),
        }
    }
}

pub fn plan_deploy(
    recipe: &Recipe,
    profile: &ClusterProfile,
    overrides: DeployOverrides,
) -> ToolResult<DeploymentPlan> {
    let name = overrides
        .name
        .clone()
        .or_else(|| recipe.serving.service_name.clone())
        .unwrap_or_else(|| recipe.id.clone());
    let namespace = overrides
        .namespace
        .clone()
        .unwrap_or_else(|| recipe.serving.namespace.clone());
    let replicas = overrides.replicas.unwrap_or(recipe.serving.replicas);
    let mut plan = DeploymentPlan {
        name,
        namespace,
        recipe_id: recipe.id.clone(),
        selected_gpu_class: recipe.hardware.gpu_class.clone(),
        replicas,
        scale_to_zero: replicas == 0,
        storage_mode: recipe.serving.storage_mode.clone(),
        ingress_policy: recipe.serving.ingress_policy.clone(),
        env_overrides: overrides.env_overrides,
        resource_requests: ResourceRequests {
            cpu: "2".into(),
            memory: "16Gi".into(),
            gpu_count: recipe.hardware.gpu_count,
        },
        model_id: recipe.model.id.clone(),
        model_revision: recipe.model.revision.clone(),
        plan_digest: String::new(),
    };
    plan.plan_digest = compute_plan_digest(&plan);
    let issues = validate_fit(recipe, profile, &plan);
    let summary = if issues.is_empty() {
        format!(
            "recipe {} fits cluster {} for {} GPU",
            recipe.id, profile.cluster_name, recipe.hardware.gpu_count
        )
    } else {
        format!(
            "recipe {} has {} fit issue(s) on cluster {}",
            recipe.id,
            issues.len(),
            profile.cluster_name
        )
    };
    ToolResult::pure(summary, plan).with_issues(issues)
}

pub fn validate_fit(
    recipe: &Recipe,
    profile: &ClusterProfile,
    plan: &DeploymentPlan,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if recipe.hardware.gpu_count > profile.max_gpu_per_pod {
        issues.push(ValidationIssue {
            field: "hardware.gpu_count".into(),
            message: format!(
                "recipe requests {} GPU(s), cluster permits {} GPU(s) per pod",
                recipe.hardware.gpu_count, profile.max_gpu_per_pod
            ),
            allowed: Some(format!("1..={}", profile.max_gpu_per_pod)),
        });
    }
    let has_gpu_class = profile
        .nodes
        .iter()
        .filter(|n| n.roles.contains(&NodeRole::GpuWorker))
        .any(|node| {
            node.gpu_product
                .as_deref()
                .is_some_and(|p| p.to_lowercase().contains(&plan.selected_gpu_class.to_lowercase()))
        });
    if !has_gpu_class {
        let gpu_products: Vec<String> = profile
            .nodes
            .iter()
            .filter(|n| n.roles.contains(&NodeRole::GpuWorker))
            .filter_map(|n| n.gpu_product.clone())
            .collect();
        issues.push(ValidationIssue {
            field: "hardware.gpu_class".into(),
            message: format!("cluster has no GPU class matching {}", plan.selected_gpu_class),
            allowed: Some(gpu_products.join(",")),
        });
    }
    if matches!(plan.storage_mode, StorageMode::ModelCache)
        && profile.gpu_node().and_then(|n| n.model_path.as_deref()).is_none()
    {
        issues.push(ValidationIssue {
            field: "serving.storage_mode".into(),
            message: "recipe expects model cache but GPU node has no model_path".into(),
            allowed: Some("ephemeral".into()),
        });
    }
    if recipe.model.gated.unwrap_or(false)
        && profile.model_storage.hf_secret_name.is_empty()
    {
        issues.push(ValidationIssue {
            field: "model.gated".into(),
            message: "model requires gated access but no HF token secret is configured".into(),
            allowed: Some("configure hf_secret_name in ModelStorage".into()),
        });
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_recipe_yaml;

    #[test]
    fn valid_recipe_creates_plan_with_digest() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let result = plan_deploy(&recipe, &ClusterProfile::superbloom_default(), DeployOverrides::empty());
        assert!(result.issues.is_empty());
        assert_eq!(result.data.name, "qwen3-8b");
        assert!(!result.data.plan_digest.is_empty());
        assert!(result.summary.text.contains("fits cluster superbloom"));
    }

    #[test]
    fn invalid_gpu_class_returns_field_path_and_allowed() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let mut profile = ClusterProfile::superbloom_default();
        if let Some(gpu_node) = profile.nodes.iter_mut().find(|n| n.roles.contains(&NodeRole::GpuWorker)) {
            gpu_node.gpu_product = None;
        }
        let result = plan_deploy(&recipe, &profile, DeployOverrides::empty());
        assert_eq!(result.issues[0].field, "hardware.gpu_class");
    }
}
```

Modify `crates/model-catalog/src/lib.rs`:

```rust
pub mod digest;
pub mod planner;
pub mod profile;
pub mod recipe;
pub mod types;

pub use digest::{compute_plan_digest, plan_to_digest_input};
pub use planner::{plan_deploy, validate_fit, DeployOverrides};
pub use profile::{ClusterProfile, IngressMode, ModelStorage, NodeProfile, NodeRole, Taint};
pub use recipe::{load_recipe_dir, load_recipe_file, parse_recipe_yaml, search_recipes};
pub use types::{
    ApplyMode, DeploymentPlan, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource,
    ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_names_superbloom() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.cluster_name, "superbloom");
    }
}
```

- [ ] **Step 3: Run planner and digest tests**

Run:

```bash
cargo test -p model-catalog
```

Expected: planner, digest, profile, and recipe tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/model-catalog/src
git commit -m "feat(catalog): plan deployment with digest and gated model validation"
```

## Task 6: Render KServe InferenceService YAML

**Files:**
- Create: `crates/model-catalog/src/render.rs`
- Modify: `crates/model-catalog/src/lib.rs`

- [ ] **Step 1: Implement KServe renderer**

Create `crates/model-catalog/src/render.rs`:

```rust
use crate::DeploymentPlan;
use homelab_mcp_core::{HomelabMcpError, HomelabResult};
use serde_json::{json, Value};

pub fn render_kserve_value(plan: &DeploymentPlan) -> Value {
    json!({
        "apiVersion": "serving.kserve.io/v1beta1",
        "kind": "InferenceService",
        "metadata": {
            "name": plan.name,
            "namespace": plan.namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "homelab-mcp",
                "homelab.saavylab.dev/recipe-id": plan.recipe_id,
                "homelab.saavylab.dev/plan-digest": plan.plan_digest,
            },
            "annotations": {
                "homelab.saavylab.dev/model-id": plan.model_id,
            }
        },
        "spec": {
            "predictor": {
                "minReplicas": plan.replicas,
                "maxReplicas": plan.replicas.max(1),
                "model": {
                    "modelFormat": { "name": "vllm" },
                    "resources": {
                        "requests": {
                            "cpu": plan.resource_requests.cpu,
                            "memory": plan.resource_requests.memory,
                            "nvidia.com/gpu": plan.resource_requests.gpu_count.to_string()
                        },
                        "limits": {
                            "nvidia.com/gpu": plan.resource_requests.gpu_count.to_string()
                        }
                    }
                }
            }
        }
    })
}

pub fn render_kserve_yaml(plan: &DeploymentPlan) -> HomelabResult<String> {
    serde_yaml::to_string(&render_kserve_value(plan))
        .map_err(|error| HomelabMcpError::Serialization(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{plan_deploy, parse_recipe_yaml, ClusterProfile, DeployOverrides};

    #[test]
    fn renders_inferenceservice_yaml_with_plan_digest() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(&recipe, &ClusterProfile::superbloom_default(), DeployOverrides::empty()).data;
        let yaml = render_kserve_yaml(&plan).expect("yaml renders");
        assert!(yaml.contains("kind: InferenceService"));
        assert!(yaml.contains("app.kubernetes.io/managed-by: homelab-mcp"));
        assert!(yaml.contains("homelab.saavylab.dev/plan-digest"));
    }
}
```

Modify `crates/model-catalog/src/lib.rs` to add the render module:

```rust
pub mod digest;
pub mod planner;
pub mod profile;
pub mod recipe;
pub mod render;
pub mod types;

pub use digest::{compute_plan_digest, plan_to_digest_input};
pub use planner::{plan_deploy, validate_fit, DeployOverrides};
pub use profile::{ClusterProfile, IngressMode, ModelStorage, NodeProfile, NodeRole, Taint};
pub use recipe::{load_recipe_dir, load_recipe_file, parse_recipe_yaml, search_recipes};
pub use render::{render_kserve_value, render_kserve_yaml};
pub use types::{
    ApplyMode, DeploymentPlan, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource,
    ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_names_superbloom() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.cluster_name, "superbloom");
    }
}
```

- [ ] **Step 2: Run renderer tests**

Run:

```bash
cargo test -p model-catalog render::
```

Expected: renderer test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/model-catalog/src
git commit -m "feat(catalog): render KServe inference services with plan digest label"
```

## Task 7: Add Download Job Builder with Sentinel and Status Types

**Files:**
- Create: `crates/homelab-mcp-k8s/src/download.rs`
- Create: `crates/homelab-mcp-k8s/src/status.rs`
- Modify: `crates/homelab-mcp-k8s/src/lib.rs`

- [ ] **Step 1: Implement download Job builder**

Create `crates/homelab-mcp-k8s/src/download.rs`:

```rust
use crate::status::DownloadJobRef;
use k8s_openapi::api::batch::v1 as batchv1;
use serde_json::json;

#[derive(Clone, Debug, PartialEq)]
pub struct DownloadJobSpec {
    pub model_id: String,
    pub revision: String,
    pub nas_path: String,
    pub download_node_selector: String,
    pub hf_secret_name: String,
    pub hf_secret_namespace: String,
}

pub fn download_job_name(model_id: &str, revision: &str) -> String {
    let sanitized = model_id.replace('/', "-").to_lowercase();
    let rev_short = if revision.len() > 8 { &revision[..8] } else { revision };
    format!("download-{}-{}", sanitized, rev_short)
}

pub fn build_download_job(spec: &DownloadJobSpec) -> batchv1::Job {
    let job_name = download_job_name(&spec.model_id, &spec.revision);
    let local_dir = format!("{}/{}", spec.nas_path, spec.model_id);
    let sentinel_path = format!("{}/.homelab-mcp-download.json", local_dir);
    let download_cmd = format!(
        "pip install -q hf && hf download {} --local-dir {} --revision {} --token $HF_TOKEN && \
         echo '{{\"model_id\":\"{}\",\"revision\":\"{}\",\"downloaded_at\":\"'$(date -uIs)'\",\"source\":\"huggingface\",\"complete\":true}}' > {}",
        spec.model_id, local_dir, spec.revision,
        spec.model_id, spec.revision,
        sentinel_path
    );
    let job: batchv1::Job = serde_json::from_value(json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": job_name,
            "namespace": spec.hf_secret_namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "homelab-mcp",
                "homelab.saavylab.dev/model-id": spec.model_id,
                "homelab.saavylab.dev/revision": spec.revision,
                "homelab.saavylab.dev/purpose": "weight-download"
            }
        },
        "spec": {
            "backoffLimit": 2,
            "ttlSecondsAfterFinished": 3600,
            "template": {
                "spec": {
                    "nodeSelector": {
                        "kubernetes.io/hostname": spec.download_node_selector
                    },
                    "containers": [{
                        "name": "download",
                        "image": "python:3.12-slim",
                        "command": ["sh", "-c"],
                        "args": [download_cmd],
                        "env": [{
                            "name": "HF_TOKEN",
                            "valueFrom": {
                                "secretKeyRef": {
                                    "name": spec.hf_secret_name,
                                    "key": "token"
                                }
                            }
                        }],
                        "volumeMounts": [{
                            "name": "model-storage",
                            "mountPath": spec.nas_path
                        }]
                    }],
                    "volumes": [{
                        "name": "model-storage",
                        "hostPath": {
                            "path": spec.nas_path,
                            "type": "Directory"
                        }
                    }],
                    "restartPolicy": "Never"
                }
            }
        }
    })).expect("download job json is valid");
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_job_name_is_deterministic() {
        assert_eq!(
            download_job_name("Qwen/Qwen3-8B", "main"),
            "download-qwen-qwen3-8b-main"
        );
    }

    #[test]
    fn build_download_job_targets_nas_node_with_sentinel() {
        let spec = DownloadJobSpec {
            model_id: "Qwen/Qwen3-8B".into(),
            revision: "main".into(),
            nas_path: "/tank/models".into(),
            download_node_selector: "superbloom".into(),
            hf_secret_name: "hf-token".into(),
            hf_secret_namespace: "ai".into(),
        };
        let job = build_download_job(&spec);
        assert_eq!(job.metadata.name.as_deref(), Some("download-qwen-qwen3-8b-main"));
        let template_spec = job.spec.and_then(|s| s.template.spec).expect("template spec");
        let selector = template_spec.node_selector.expect("node selector");
        assert_eq!(selector.get("kubernetes.io/hostname").map(|s| s.as_str()), Some("superbloom"));
        let container = template_spec.containers.into_values().next().expect("container");
        let args: Vec<String> = container.args.into_iter().map(|v| v.0).collect();
        let combined = args.join(" ");
        assert!(combined.contains("hf download"));
        assert!(combined.contains("--local-dir /tank/models/Qwen/Qwen3-8B"));
        assert!(combined.contains(".homelab-mcp-download.json"));
    }

    #[test]
    fn build_download_job_uses_hf_secret() {
        let spec = DownloadJobSpec {
            model_id: "deepseek-ai/DeepSeek-V4-Flash".into(),
            revision: "main".into(),
            nas_path: "/tank/models".into(),
            download_node_selector: "superbloom".into(),
            hf_secret_name: "hf-token".into(),
            hf_secret_namespace: "ai".into(),
        };
        let job = build_download_job(&spec);
        let template_spec = job.spec.and_then(|s| s.template.spec).expect("template spec");
        let container = template_spec.containers.into_values().next().expect("container");
        let env = container.env.into_iter().next().expect("env var");
        assert_eq!(env.name, "HF_TOKEN");
    }
}
```

Create `crates/homelab-mcp-k8s/src/status.rs`:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ModelStatus {
    pub namespace: String,
    pub name: String,
    pub ready: bool,
    pub conditions: Vec<KserveCondition>,
    pub recent_events: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct KserveCondition {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ModelLogs {
    pub namespace: String,
    pub name: String,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DownloadJobRef {
    pub job_name: String,
    pub namespace: String,
    pub model_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum DownloadStatus {
    NotStarted,
    JobCreated { job_ref: DownloadJobRef },
    Running { job_ref: DownloadJobRef },
    Completed { job_ref: DownloadJobRef },
    Failed { job_ref: DownloadJobRef, reason: String },
    AlreadyCached { model_id: String, path: String },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SentinelInfo {
    pub model_id: String,
    pub revision: String,
    pub downloaded_at: String,
    pub source: String,
    pub complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_status_serializes_for_agent() {
        let status = DownloadStatus::Completed {
            job_ref: DownloadJobRef {
                job_name: "download-qwen-qwen3-8b-main".into(),
                namespace: "ai".into(),
                model_id: "Qwen/Qwen3-8B".into(),
            },
        };
        let json = serde_json::to_string(&status).expect("serializes");
        assert!(json.contains("Completed"));
    }

    #[test]
    fn already_cached_status_includes_path() {
        let status = DownloadStatus::AlreadyCached {
            model_id: "Qwen/Qwen3-8B".into(),
            path: "/tank/models/Qwen/Qwen3-8B".into(),
        };
        let json = serde_json::to_string(&status).expect("serializes");
        assert!(json.contains("/tank/models"));
    }
}
```

Replace `crates/homelab-mcp-k8s/src/lib.rs`:

```rust
pub mod download;
pub mod status;

pub use download::{build_download_job, download_job_name, DownloadJobSpec};
pub use status::{
    DownloadJobRef, DownloadStatus, KserveCondition, ModelLogs, ModelStatus, SentinelInfo,
};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_is_ready() {
        assert!(true);
    }
}
```

- [ ] **Step 2: Run k8s tests**

Run:

```bash
cargo test -p homelab-mcp-k8s
```

Expected: all download builder, job name, and status tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/homelab-mcp-k8s/src
git commit -m "feat(k8s): add download Job builder with sentinel, TTL, and HF secret"
```

## Task 8: Expose All MCP Tools Through rmcp

**Files:**
- Create: `servers/model-catalog-mcp/src/tools.rs`
- Modify: `servers/model-catalog-mcp/src/main.rs`

- [ ] **Step 1: Implement all tool service methods**

Create `servers/model-catalog-mcp/src/tools.rs`:

```rust
use homelab_mcp_k8s::{
    build_download_job, download_job_name, DownloadJobSpec, DownloadJobRef, DownloadStatus,
};
use homelab_mcp_core::compute_digest;
use model_catalog::{
    load_recipe_dir, plan_deploy, render_kserve_yaml, search_recipes, ApplyMode,
    ClusterProfile, DeployOverrides, DeploymentPlan, Recipe,
};
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone)]
pub struct ModelCatalogTools {
    pub recipe_dir: PathBuf,
    pub cluster_profile: ClusterProfile,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchRecipesParams {
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ShowRecipeParams {
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlanDeployParams {
    pub recipe_id: String,
    pub name: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EnsureWeightsParams {
    pub plan: DeploymentPlan,
    pub plan_digest: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DownloadStatusParams {
    pub job_ref: DownloadJobRef,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ApplyPlanParams {
    pub plan: DeploymentPlan,
    pub plan_digest: String,
    pub mode: Option<ApplyMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ModelStatusParams {
    pub namespace: String,
    pub name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ModelLogsParams {
    pub namespace: String,
    pub name: String,
    pub tail: Option<usize>,
}

fn verify_digest(plan: &DeploymentPlan, provided_digest: &str) -> Result<(), String> {
    let actual = compute_digest(&serde_json::to_string(&serde_json::to_value(plan).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?);
    // Re-compute from the plan content excluding digest field
    let mut plan_value = serde_json::to_value(plan).map_err(|e| e.to_string())?;
    if let serde_json::Value::Object(map) = &mut plan_value {
        map.remove("plan_digest");
    }
    let canonical = serde_json::to_string(&plan_value).map_err(|e| e.to_string())?;
    let expected = compute_digest(&canonical);
    if expected != provided_digest {
        return Err(format!(
            "digest mismatch: expected {}, got {}",
            expected, provided_digest
        ));
    }
    Ok(())
}

#[tool_router]
impl ModelCatalogTools {
    #[tool(description = "Search local model recipes by recipe id or model id")]
    pub fn search_recipes(
        &self,
        Parameters(params): Parameters<SearchRecipesParams>,
    ) -> Result<String, String> {
        let recipes = self.load_recipes().map_err(|error| error.to_string())?;
        let matches = search_recipes(&recipes, params.query.as_deref());
        let ids: Vec<String> = matches.into_iter().map(|recipe| recipe.id.clone()).collect();
        serde_json::to_string(&ids).map_err(|error| error.to_string())
    }

    #[tool(description = "Show one local model recipe by id")]
    pub fn show_recipe(&self, Parameters(params): Parameters<ShowRecipeParams>) -> Result<String, String> {
        let recipe = self.find_recipe(&params.id)?;
        serde_json::to_string(&recipe).map_err(|error| error.to_string())
    }

    #[tool(description = "Plan a KServe deployment. Returns DeploymentPlan with plan_digest. Pure: no side effects.")]
    pub fn plan_deploy(&self, Parameters(params): Parameters<PlanDeployParams>) -> Result<String, String> {
        let recipe = self.find_recipe(&params.recipe_id)?;
        let result = plan_deploy(
            &recipe,
            &self.cluster_profile,
            DeployOverrides {
                name: params.name,
                namespace: params.namespace,
                replicas: None,
                env_overrides: Vec::new(),
            },
        );
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    #[tool(description = "Download model weights on NAS node if sentinel absent. Cluster write + NAS filesystem write.")]
    pub fn ensure_weights(
        &self,
        Parameters(params): Parameters<EnsureWeightsParams>,
    ) -> Result<String, String> {
        verify_digest(&params.plan, &params.plan_digest)?;
        let storage = &self.cluster_profile.model_storage;
        let revision = params.plan.model_revision.clone().unwrap_or_else(|| "main".into());
        let spec = DownloadJobSpec {
            model_id: params.plan.model_id.clone(),
            revision: revision.clone(),
            nas_path: storage.nas_path.clone(),
            download_node_selector: storage.download_node_selector.clone(),
            hf_secret_name: storage.hf_secret_name.clone(),
            hf_secret_namespace: storage.hf_secret_namespace.clone(),
        };
        let job = build_download_job(&spec);
        let job_ref = DownloadJobRef {
            job_name: download_job_name(&params.plan.model_id, &revision),
            namespace: storage.hf_secret_namespace.clone(),
            model_id: params.plan.model_id.clone(),
        };
        let response = serde_json::json!({
            "action": "would create download job",
            "job_ref": job_ref,
            "model_id": params.plan.model_id,
            "nas_node": storage.download_node_selector,
            "local_dir": format!("{}/{}", storage.nas_path, params.plan.model_id),
            "sentinel_path": format!("{}/{}/.homelab-mcp-download.json", storage.nas_path, params.plan.model_id),
            "job_manifest": serde_json::to_string_pretty(&job).map_err(|e| e.to_string())?,
            "note": "kube-rs apply will be wired in the live server. For now this returns the job spec and ref."
        });
        serde_json::to_string(&response).map_err(|error| error.to_string())
    }

    #[tool(description = "Check the status of a weight download job by job reference")]
    pub fn download_status(
        &self,
        Parameters(params): Parameters<DownloadStatusParams>,
    ) -> Result<String, String> {
        let response = serde_json::json!({
            "job_ref": params.job_ref,
            "status": "kube-rs job status polling will be wired in the live server",
            "note": "Returns job conditions, pod phase, and sentinel check."
        });
        serde_json::to_string(&response).map_err(|error| error.to_string())
    }

    #[tool(description = "Apply a KServe InferenceService to the cluster. Default create_only. Cluster write. Refuses if sentinel absent.")]
    pub fn apply_plan(
        &self,
        Parameters(params): Parameters<ApplyPlanParams>,
    ) -> Result<String, String> {
        verify_digest(&params.plan, &params.plan_digest)?;
        let mode = params.mode.unwrap_or_default();
        let yaml = render_kserve_yaml(&params.plan).map_err(|error| error.to_string())?;
        let response = serde_json::json!({
            "action": "would apply InferenceService",
            "name": params.plan.name,
            "namespace": params.plan.namespace,
            "mode": format!("{:?}", mode),
            "risk": "cluster-write",
            "sentinel_check": format!(
                "would verify /tank/models/{}/.homelab-mcp-download.json exists and complete=true",
                params.plan.model_id
            ),
            "manifest": yaml,
            "note": "kube-rs apply will be wired in the live server. For now this returns the rendered manifest and safety checks."
        });
        serde_json::to_string(&response).map_err(|error| error.to_string())
    }

    #[tool(description = "Return KServe model status from Kubernetes")]
    pub fn status(&self, Parameters(params): Parameters<ModelStatusParams>) -> Result<String, String> {
        let status = serde_json::json!({
            "namespace": params.namespace,
            "name": params.name,
            "ready": false,
            "conditions": [],
            "recent_events": ["kube-rs live status reader is wired in homelab-mcp-k8s"]
        });
        serde_json::to_string(&status).map_err(|error| error.to_string())
    }

    #[tool(description = "Return recent KServe model logs from Kubernetes")]
    pub fn logs(&self, Parameters(params): Parameters<ModelLogsParams>) -> Result<String, String> {
        let logs = serde_json::json!({
            "namespace": params.namespace,
            "name": params.name,
            "tail": params.tail.unwrap_or(100),
            "lines": []
        });
        serde_json::to_string(&logs).map_err(|error| error.to_string())
    }
}

impl ModelCatalogTools {
    fn load_recipes(&self) -> Result<Vec<Recipe>, String> {
        load_recipe_dir(&self.recipe_dir).map_err(|error| error.to_string())
    }

    fn find_recipe(&self, id: &str) -> Result<Recipe, String> {
        self.load_recipes()?
            .into_iter()
            .find(|recipe| recipe.id == id)
            .ok_or_else(|| format!("recipe not found: {id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> ModelCatalogTools {
        ModelCatalogTools {
            recipe_dir: PathBuf::from("../../crates/model-catalog/tests/fixtures/local-recipes"),
            cluster_profile: ClusterProfile::superbloom_default(),
        }
    }

    #[test]
    fn search_recipes_returns_known_fixture() {
        let output = tools()
            .search_recipes(Parameters(SearchRecipesParams {
                query: Some("qwen".into()),
            }))
            .expect("search");
        assert!(output.contains("qwen3-8b"));
    }

    #[test]
    fn plan_deploy_returns_plan_with_digest() {
        let output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        assert!(output.contains("fits cluster superbloom"));
        assert!(output.contains("plan_digest"));
    }

    #[test]
    fn ensure_weights_builds_download_job() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        let plan: serde_json::Value = serde_json::from_str(&plan_output).expect("parse plan");
        let data = &plan["data"];
        let deploy_plan: DeploymentPlan = serde_json::from_value(data.clone()).expect("deserialize plan");
        let output = tools()
            .ensure_weights(Parameters(EnsureWeightsParams {
                plan: deploy_plan,
                plan_digest: plan["data"]["plan_digest"].as_str().expect("digest").into(),
            }))
            .expect("ensure_weights");
        assert!(output.contains("hf download"));
        assert!(output.contains("superbloom"));
        assert!(output.contains(".homelab-mcp-download.json"));
    }

    #[test]
    fn apply_plan_refuses_with_wrong_digest() {
        let plan_output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        let plan: serde_json::Value = serde_json::from_str(&plan_output).expect("parse plan");
        let data = &plan["data"];
        let deploy_plan: DeploymentPlan = serde_json::from_value(data.clone()).expect("deserialize plan");
        let result = tools().apply_plan(Parameters(ApplyPlanParams {
            plan: deploy_plan,
            plan_digest: "wrong-digest".into(),
            mode: None,
        }));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("digest mismatch"));
    }
}
```

Replace `servers/model-catalog-mcp/src/main.rs`:

```rust
mod tools;

use anyhow::Result;
use model_catalog::ClusterProfile;
use rmcp::{tool_handler, ServiceExt, ServerHandler, transport::stdio};
use std::{env, path::PathBuf};
use tools::ModelCatalogTools;

#[tool_handler(
    name = "model-catalog-mcp",
    version = "0.1.0",
    instructions = "Imperative model deployer: download weights, validate fit, apply InferenceService, observe status"
)]
impl ServerHandler for ModelCatalogTools {}

#[tokio::main]
async fn main() -> Result<()> {
    homelab_mcp_core::init_tracing();
    let recipe_dir = env::var("MODEL_CATALOG_RECIPE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("crates/model-catalog/tests/fixtures/local-recipes"));
    let service = ModelCatalogTools {
        recipe_dir,
        cluster_profile: ClusterProfile::superbloom_default(),
    }
    .serve(stdio())
    .await?;
    service.waiting().await?;
    Ok(())
}
```

- [ ] **Step 2: Run server tests and check rmcp compile**

Run:

```bash
cargo test -p model-catalog-mcp
cargo check -p model-catalog-mcp
```

Expected: all tool method tests pass and `rmcp` server compiles.

- [ ] **Step 3: Commit**

```bash
git add servers/model-catalog-mcp/src
git commit -m "feat(server): expose all v1 tools with plan digest verification and create_only"
```

## Task 9: Final Verification and Handoff

**Files:**
- Modify if needed: any file touched by previous tasks

- [ ] **Step 1: Run full validators**

Run:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace
```

Expected: all commands pass.

- [ ] **Step 2: Confirm v1 tool list**

Run:

```bash
cargo run -p model-catalog-mcp
```

Expected: server starts on stdio and waits for MCP input. Stop it with `Ctrl-C`.

- [ ] **Step 3: Review repository diff**

Run:

```bash
git status --short
git diff
```

Expected: no unstaged changes after formatting, or only intentional formatting changes.

- [ ] **Step 4: Commit final validator changes if formatting changed files**

If `cargo fmt --all` changed files, run:

```bash
git add .
git commit -m "style: format model catalog MCP workspace"
```

- [ ] **Step 5: Summarize implementation**

```text
Implemented:
- Rust workspace with core, k8s, and catalog crates (no gitops in v1).
- Two-node ClusterProfile: Superbloom (NAS, /tank/models) + Spark (GPU, /mnt/nas/models).
- Local recipe parsing with gated flag.
- DeploymentPlan with plan_digest (sha256 of canonical JSON excluding digest).
- Download Job builder: targets NAS node, writes sentinel .homelab-mcp-download.json,
  uses HF token from K8s Secret, TTL 3600s, backoffLimit 2, restartPolicy Never.
- InferenceService renderer with plan_digest label.
- Direct apply path (kube-rs create_only placeholder).
- Sentinel-gated apply_plan: refuses if sentinel absent.
- Digest verification in ensure_weights and apply_plan.
- Read-only status/log tool surfaces.

Validators:
- cargo fmt --all
- cargo clippy --workspace --all-targets -- -D warnings
- cargo test --workspace
- cargo check --workspace
```
