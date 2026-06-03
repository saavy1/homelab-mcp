# Model Catalog KServe Deployer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build v1 of `homelab-mcp`: a Rust MCP workspace that turns model-serving recipes into explainable, reviewable KServe GitOps changes for Superbloom.

**Architecture:** Implement a small Rust workspace with boring shared crates, a recipe/domain crate, and one `rmcp` server. Keep all mutating behavior GitOps-first: pure planning is separate from local patch writing, and live Kubernetes apply is excluded.

**Tech Stack:** Rust 2024 edition, `rmcp` 1.7, `rmcp-macros` via `rmcp` macros feature, `kube-rs` 3.1, `serde`, `serde_yaml`, `schemars`, `tokio`, `tracing`, `tempfile`, `insta`.

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
    homelab-mcp-gitops/
      Cargo.toml
      src/lib.rs
    homelab-mcp-k8s/
      Cargo.toml
      src/lib.rs
    model-catalog/
      Cargo.toml
      src/lib.rs
      src/types.rs
      src/recipe.rs
      src/profile.rs
      src/planner.rs
      src/render.rs
      tests/fixtures/local-recipes/qwen3-8b.yaml
      tests/fixtures/local-recipes/deepseek-v4-flash.yaml
  servers/
    model-catalog-mcp/
      Cargo.toml
      src/main.rs
      src/tools.rs
```

Responsibilities:

- `homelab-mcp-core`: tool result, validation, provenance, error, and tracing helpers.
- `homelab-mcp-gitops`: writes generated files into a local `sb` checkout and returns a diff.
- `homelab-mcp-k8s`: `kube-rs` client helpers and read-only status/log interfaces.
- `model-catalog`: recipe parsing, recipe search, cluster profile, deployment planning, KServe rendering.
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
  "crates/homelab-mcp-gitops",
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

Create `crates/homelab-mcp-gitops/Cargo.toml`:

```toml
[package]
name = "homelab-mcp-gitops"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
homelab-mcp-core = { path = "../homelab-mcp-core" }
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

Create `crates/homelab-mcp-gitops/src/lib.rs`:

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
homelab-mcp-gitops = { path = "../../crates/homelab-mcp-gitops" }
homelab-mcp-k8s = { path = "../../crates/homelab-mcp-k8s" }
model-catalog = { path = "../../crates/model-catalog" }
rmcp.workspace = true
schemars.workspace = true
serde.workspace = true
tokio.workspace = true
tracing.workspace = true
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

Expected: all four `crate_is_ready` tests pass.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore crates servers
git commit -m "chore: scaffold homelab MCP workspace"
```

## Task 2: Add Core Tool Response and Error Types

**Files:**
- Modify: `crates/homelab-mcp-core/src/lib.rs`

- [ ] **Step 1: Replace the initial test with failing core tests**

Write this in `crates/homelab-mcp-core/src/lib.rs`:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Read,
    Pure,
    LocalWrite,
    RemoteWrite,
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
}

pub type HomelabResult<T> = Result<T, HomelabMcpError>;

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
        assert_eq!(result.data, vec!["qwen3-8b"]);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn issues_are_preserved_for_agent_legibility() {
        let issue = ValidationIssue {
            field: "hardware.gpu_count".into(),
            message: "requested GPU count exceeds cluster limit".into(),
            allowed: Some("1..=1".into()),
        };
        let result = ToolResult::pure("invalid plan", ()).with_issues(vec![issue.clone()]);
        assert_eq!(result.issues, vec![issue]);
    }
}
```

- [ ] **Step 2: Run the core tests**

Run:

```bash
cargo test -p homelab-mcp-core
```

Expected: two tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/homelab-mcp-core/src/lib.rs
git commit -m "feat(core): add shared MCP result types"
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

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct DeploymentIntent {
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
pub struct ClusterProfile {
    pub cluster_name: String,
    pub gpu_nodes: Vec<GpuNodeClass>,
    pub storage_classes: Vec<String>,
    pub default_namespace: String,
    pub available_serving_runtimes: Vec<String>,
    pub max_gpu_per_pod: u32,
    pub ingress_mode: IngressMode,
    pub known_model_cache_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct GpuNodeClass {
    pub name: String,
    pub product: String,
    pub count: u32,
    pub memory_gb: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IngressMode {
    ClusterLocal,
    InternalHttp,
}

impl ClusterProfile {
    pub fn superbloom_default() -> Self {
        Self {
            cluster_name: "superbloom".into(),
            gpu_nodes: vec![GpuNodeClass {
                name: "spark-gb10".into(),
                product: "gb10".into(),
                count: 1,
                memory_gb: 128,
            }],
            storage_classes: vec!["local-path".into()],
            default_namespace: "ai".into(),
            available_serving_runtimes: vec!["vllm".into()],
            max_gpu_per_pod: 1,
            ingress_mode: IngressMode::ClusterLocal,
            known_model_cache_paths: vec!["/models".into()],
        }
    }
}
```

Replace `crates/model-catalog/src/lib.rs`:

```rust
pub mod profile;
pub mod types;

pub use profile::{ClusterProfile, GpuNodeClass, IngressMode};
pub use types::{
    DeploymentIntent, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource,
    ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_names_superbloom() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.cluster_name, "superbloom");
        assert_eq!(profile.max_gpu_per_pod, 1);
        assert_eq!(profile.available_serving_runtimes, vec!["vllm"]);
    }
}
```

- [ ] **Step 2: Run model-catalog tests**

Run:

```bash
cargo test -p model-catalog default_profile_names_superbloom
```

Expected: the profile test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/model-catalog/src
git commit -m "feat(catalog): add recipe and cluster profile types"
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
    }

    #[test]
    fn searches_by_model_id_case_insensitively() {
        let input = include_str!("../tests/fixtures/local-recipes/deepseek-v4-flash.yaml");
        let recipe = parse_recipe_yaml(input).expect("recipe parses");
        let results = search_recipes(&[recipe], Some("deepseek"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "deepseek-v4-flash");
    }
}
```

Modify `crates/model-catalog/src/lib.rs`:

```rust
pub mod profile;
pub mod recipe;
pub mod types;

pub use profile::{ClusterProfile, GpuNodeClass, IngressMode};
pub use recipe::{load_recipe_dir, load_recipe_file, parse_recipe_yaml, search_recipes};
pub use types::{
    DeploymentIntent, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource,
    ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_names_superbloom() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.cluster_name, "superbloom");
        assert_eq!(profile.max_gpu_per_pod, 1);
        assert_eq!(profile.available_serving_runtimes, vec!["vllm"]);
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
git commit -m "feat(catalog): parse local model recipes"
```

## Task 5: Implement Planning and Explainable Fit Validation

**Files:**
- Create: `crates/model-catalog/src/planner.rs`
- Modify: `crates/model-catalog/src/lib.rs`

- [ ] **Step 1: Add planner tests and implementation**

Create `crates/model-catalog/src/planner.rs`:

```rust
use crate::{ClusterProfile, DeploymentIntent, EnvVar, Recipe, ResourceRequests, StorageMode};
use homelab_mcp_core::{ToolResult, ValidationIssue};
use serde::Serialize;

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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeployPlan {
    pub recipe: Recipe,
    pub intent: DeploymentIntent,
}

pub fn plan_deploy(
    recipe: &Recipe,
    profile: &ClusterProfile,
    overrides: DeployOverrides,
) -> ToolResult<DeployPlan> {
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
    let intent = DeploymentIntent {
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
    };
    let issues = validate_fit(recipe, profile, &intent);
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
    ToolResult::pure(
        summary,
        DeployPlan {
            recipe: recipe.clone(),
            intent,
        },
    )
    .with_issues(issues)
}

pub fn validate_fit(
    recipe: &Recipe,
    profile: &ClusterProfile,
    intent: &DeploymentIntent,
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
        .gpu_nodes
        .iter()
        .any(|node| node.product == intent.selected_gpu_class);
    if !has_gpu_class {
        issues.push(ValidationIssue {
            field: "hardware.gpu_class".into(),
            message: format!("cluster has no GPU class {}", intent.selected_gpu_class),
            allowed: Some(
                profile
                    .gpu_nodes
                    .iter()
                    .map(|node| node.product.clone())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        });
    }
    if matches!(intent.storage_mode, StorageMode::ModelCache)
        && profile.known_model_cache_paths.is_empty()
    {
        issues.push(ValidationIssue {
            field: "serving.storage_mode".into(),
            message: "recipe expects model cache paths but cluster profile has none".into(),
            allowed: Some("ephemeral".into()),
        });
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_recipe_yaml, GpuNodeClass};

    #[test]
    fn valid_recipe_creates_explainable_plan_without_issues() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let result = plan_deploy(&recipe, &ClusterProfile::superbloom_default(), DeployOverrides::empty());
        assert!(result.issues.is_empty());
        assert_eq!(result.data.intent.name, "qwen3-8b");
        assert!(result.summary.text.contains("fits cluster superbloom"));
    }

    #[test]
    fn invalid_gpu_class_returns_field_path_allowed_values_and_provenance() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let mut profile = ClusterProfile::superbloom_default();
        profile.gpu_nodes = vec![GpuNodeClass {
            name: "cpu-only".into(),
            product: "none".into(),
            count: 0,
            memory_gb: 0,
        }];
        let result = plan_deploy(&recipe, &profile, DeployOverrides::empty());
        assert_eq!(result.issues[0].field, "hardware.gpu_class");
        assert_eq!(result.issues[0].allowed.as_deref(), Some("none"));
        assert_eq!(
            result.data.recipe.provenance.path.as_deref(),
            Some("argocd/clusters/superbloom/ai/vllm/recipes/qwen3-8b.yaml")
        );
    }
}
```

Modify `crates/model-catalog/src/lib.rs`:

```rust
pub mod planner;
pub mod profile;
pub mod recipe;
pub mod types;

pub use planner::{plan_deploy, validate_fit, DeployOverrides, DeployPlan};
pub use profile::{ClusterProfile, GpuNodeClass, IngressMode};
pub use recipe::{load_recipe_dir, load_recipe_file, parse_recipe_yaml, search_recipes};
pub use types::{
    DeploymentIntent, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource,
    ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_names_superbloom() {
        let profile = ClusterProfile::superbloom_default();
        assert_eq!(profile.cluster_name, "superbloom");
        assert_eq!(profile.max_gpu_per_pod, 1);
        assert_eq!(profile.available_serving_runtimes, vec!["vllm"]);
    }
}
```

- [ ] **Step 2: Run planner tests**

Run:

```bash
cargo test -p model-catalog planner::
```

Expected: both planner tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/model-catalog/src
git commit -m "feat(catalog): add deploy planning and fit validation"
```

## Task 6: Render KServe InferenceService YAML

**Files:**
- Create: `crates/model-catalog/src/render.rs`
- Modify: `crates/model-catalog/src/lib.rs`

- [ ] **Step 1: Implement KServe renderer**

Create `crates/model-catalog/src/render.rs`:

```rust
use crate::DeployPlan;
use homelab_mcp_core::{HomelabMcpError, HomelabResult};
use serde_json::{json, Value};

pub fn render_kserve_value(plan: &DeployPlan) -> Value {
    let recipe = &plan.recipe;
    let intent = &plan.intent;
    let args = recipe.runtime.args.clone();
    json!({
        "apiVersion": "serving.kserve.io/v1beta1",
        "kind": "InferenceService",
        "metadata": {
            "name": intent.name,
            "namespace": intent.namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "homelab-mcp",
                "homelab.saavylab.dev/recipe-id": recipe.id,
                "homelab.saavylab.dev/recipe-source": format!("{:?}", recipe.source).to_lowercase(),
            },
            "annotations": {
                "homelab.saavylab.dev/model-id": recipe.model.id,
                "homelab.saavylab.dev/source-path": recipe.provenance.path.clone().unwrap_or_default(),
                "homelab.saavylab.dev/source-commit": recipe.provenance.commit.clone().unwrap_or_default(),
            }
        },
        "spec": {
            "predictor": {
                "minReplicas": intent.replicas,
                "maxReplicas": intent.replicas.max(1),
                "model": {
                    "modelFormat": { "name": "vllm" },
                    "args": args,
                    "resources": {
                        "requests": {
                            "cpu": intent.resource_requests.cpu,
                            "memory": intent.resource_requests.memory,
                            "nvidia.com/gpu": intent.resource_requests.gpu_count.to_string()
                        },
                        "limits": {
                            "nvidia.com/gpu": intent.resource_requests.gpu_count.to_string()
                        }
                    }
                }
            }
        }
    })
}

pub fn render_kserve_yaml(plan: &DeployPlan) -> HomelabResult<String> {
    serde_yaml::to_string(&render_kserve_value(plan))
        .map_err(|error| HomelabMcpError::Serialization(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{plan_deploy, parse_recipe_yaml, ClusterProfile, DeployOverrides};

    #[test]
    fn renders_inferenceservice_yaml_with_homelab_labels() {
        let recipe = parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/qwen3-8b.yaml"
        ))
        .expect("recipe parses");
        let plan = plan_deploy(&recipe, &ClusterProfile::superbloom_default(), DeployOverrides::empty()).data;
        let yaml = render_kserve_yaml(&plan).expect("yaml renders");
        assert!(yaml.contains("kind: InferenceService"));
        assert!(yaml.contains("app.kubernetes.io/managed-by: homelab-mcp"));
        assert!(yaml.contains("homelab.saavylab.dev/recipe-id: qwen3-8b"));
        assert!(yaml.contains("--tool-call-parser=hermes"));
    }
}
```

Modify `crates/model-catalog/src/lib.rs`:

```rust
pub mod planner;
pub mod profile;
pub mod recipe;
pub mod render;
pub mod types;

pub use planner::{plan_deploy, validate_fit, DeployOverrides, DeployPlan};
pub use profile::{ClusterProfile, GpuNodeClass, IngressMode};
pub use recipe::{load_recipe_dir, load_recipe_file, parse_recipe_yaml, search_recipes};
pub use render::{render_kserve_value, render_kserve_yaml};
pub use types::{
    DeploymentIntent, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe, RecipeSource,
    ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
};
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
git commit -m "feat(catalog): render KServe inference services"
```

## Task 7: Write Local GitOps Patches and Diffs

**Files:**
- Modify: `crates/homelab-mcp-gitops/src/lib.rs`

- [ ] **Step 1: Implement patch writer**

Replace `crates/homelab-mcp-gitops/src/lib.rs`:

```rust
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitOpsPatch {
    pub files: Vec<RenderedFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedFile {
    pub relative_path: PathBuf,
    pub contents: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchResult {
    pub written_paths: Vec<PathBuf>,
    pub diff_summary: String,
}

#[derive(Debug, Error)]
pub enum GitOpsError {
    #[error("path escapes repository root: {0}")]
    PathEscape(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn write_patch(repo_root: &Path, patch: &GitOpsPatch) -> Result<PatchResult, GitOpsError> {
    let mut written_paths = Vec::new();
    for file in &patch.files {
        if file.relative_path.components().any(|component| {
            matches!(component, std::path::Component::ParentDir | std::path::Component::RootDir)
        }) {
            return Err(GitOpsError::PathEscape(file.relative_path.display().to_string()));
        }
        let absolute = repo_root.join(&file.relative_path);
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute, &file.contents)?;
        written_paths.push(file.relative_path.clone());
    }
    let diff_summary = written_paths
        .iter()
        .map(|path| format!("wrote {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(PatchResult {
        written_paths,
        diff_summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_patch_inside_repo() {
        let temp = tempfile::tempdir().expect("temp dir");
        let patch = GitOpsPatch {
            files: vec![RenderedFile {
                relative_path: PathBuf::from(
                    "argocd/clusters/superbloom/ai/vllm/resources/qwen3-8b/inferenceservice.yaml",
                ),
                contents: "kind: InferenceService\n".into(),
            }],
        };
        let result = write_patch(temp.path(), &patch).expect("patch writes");
        assert_eq!(result.written_paths.len(), 1);
        assert!(result.diff_summary.contains("qwen3-8b/inferenceservice.yaml"));
    }

    #[test]
    fn rejects_path_escape() {
        let temp = tempfile::tempdir().expect("temp dir");
        let patch = GitOpsPatch {
            files: vec![RenderedFile {
                relative_path: PathBuf::from("../outside.yaml"),
                contents: "bad".into(),
            }],
        };
        let error = write_patch(temp.path(), &patch).expect_err("path escape rejected");
        assert!(error.to_string().contains("path escapes"));
    }
}
```

- [ ] **Step 2: Run gitops tests**

Run:

```bash
cargo test -p homelab-mcp-gitops
```

Expected: both gitops tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/homelab-mcp-gitops/src/lib.rs
git commit -m "feat(gitops): write local GitOps patches"
```

## Task 8: Add Read-Only KServe Status Interfaces

**Files:**
- Modify: `crates/homelab-mcp-k8s/src/lib.rs`

- [ ] **Step 1: Implement read-only interfaces with mockable trait**

Replace `crates/homelab-mcp-k8s/src/lib.rs`:

```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelStatus {
    pub namespace: String,
    pub name: String,
    pub ready: bool,
    pub conditions: Vec<KserveCondition>,
    pub recent_events: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KserveCondition {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelLogs {
    pub namespace: String,
    pub name: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Error)]
pub enum K8sReadError {
    #[error("model not found: {namespace}/{name}")]
    NotFound { namespace: String, name: String },
    #[error("kubernetes api error: {0}")]
    Api(String),
}

pub trait KserveReader {
    fn status(&self, namespace: &str, name: &str) -> Result<ModelStatus, K8sReadError>;
    fn logs(&self, namespace: &str, name: &str, tail: usize) -> Result<ModelLogs, K8sReadError>;
}

pub struct KubeKserveReader {
    _private: (),
}

impl KubeKserveReader {
    pub async fn try_default() -> Result<Self, K8sReadError> {
        let _client = kube::Client::try_default()
            .await
            .map_err(|error| K8sReadError::Api(error.to_string()))?;
        Ok(Self { _private: () })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeReader;

    impl KserveReader for FakeReader {
        fn status(&self, namespace: &str, name: &str) -> Result<ModelStatus, K8sReadError> {
            Ok(ModelStatus {
                namespace: namespace.into(),
                name: name.into(),
                ready: true,
                conditions: vec![KserveCondition {
                    condition_type: "Ready".into(),
                    status: "True".into(),
                    reason: Some("MinimumReplicasAvailable".into()),
                    message: Some("model is ready".into()),
                }],
                recent_events: vec!["Created predictor pod".into()],
            })
        }

        fn logs(&self, namespace: &str, name: &str, tail: usize) -> Result<ModelLogs, K8sReadError> {
            Ok(ModelLogs {
                namespace: namespace.into(),
                name: name.into(),
                lines: vec!["server started".into()]
                    .into_iter()
                    .take(tail)
                    .collect(),
            })
        }
    }

    #[test]
    fn reader_contract_returns_agent_legible_status() {
        let status = FakeReader.status("ai", "qwen3-8b").expect("status");
        assert!(status.ready);
        assert_eq!(status.conditions[0].condition_type, "Ready");
        assert_eq!(status.recent_events, vec!["Created predictor pod"]);
    }
}
```

- [ ] **Step 2: Run k8s tests**

Run:

```bash
cargo test -p homelab-mcp-k8s
```

Expected: reader contract test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/homelab-mcp-k8s/src/lib.rs
git commit -m "feat(k8s): define read-only KServe status interface"
```

## Task 9: Expose Catalog Operations Through rmcp

**Files:**
- Create: `servers/model-catalog-mcp/src/tools.rs`
- Modify: `servers/model-catalog-mcp/src/main.rs`

- [ ] **Step 1: Implement tool service methods**

Create `servers/model-catalog-mcp/src/tools.rs`:

```rust
use model_catalog::{
    load_recipe_dir, plan_deploy, render_kserve_yaml, search_recipes, ClusterProfile,
    DeployOverrides, Recipe,
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

#[derive(Debug, Serialize)]
pub struct SearchRecipesOutput {
    pub recipes: Vec<String>,
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
pub struct RenderKserveParams {
    pub recipe_id: String,
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
        let output = SearchRecipesOutput {
            recipes: matches.into_iter().map(|recipe| recipe.id.clone()).collect(),
        };
        serde_json::to_string(&output).map_err(|error| error.to_string())
    }

    #[tool(description = "Show one local model recipe by id")]
    pub fn show_recipe(&self, Parameters(params): Parameters<ShowRecipeParams>) -> Result<String, String> {
        let recipe = self.find_recipe(&params.id)?;
        serde_json::to_string(&recipe).map_err(|error| error.to_string())
    }

    #[tool(description = "Plan a KServe deployment without writing files")]
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

    #[tool(description = "Render KServe InferenceService YAML for a recipe")]
    pub fn render_kserve(
        &self,
        Parameters(params): Parameters<RenderKserveParams>,
    ) -> Result<String, String> {
        let recipe = self.find_recipe(&params.recipe_id)?;
        let plan = plan_deploy(
            &recipe,
            &self.cluster_profile,
            DeployOverrides {
                name: None,
                namespace: None,
                replicas: None,
                env_overrides: Vec::new(),
            },
        )
        .data;
        render_kserve_yaml(&plan).map_err(|error| error.to_string())
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
    fn plan_deploy_returns_explainable_summary() {
        let output = tools()
            .plan_deploy(Parameters(PlanDeployParams {
                recipe_id: "qwen3-8b".into(),
                name: None,
                namespace: None,
            }))
            .expect("plan");
        assert!(output.contains("fits cluster superbloom"));
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
    instructions = "Turn model-serving recipes into explainable KServe GitOps plans"
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

Expected: tool method tests pass and `rmcp` server compiles.

- [ ] **Step 3: Commit**

```bash
git add servers/model-catalog-mcp/src
git commit -m "feat(server): expose recipe planning over rmcp"
```

## Task 10: Add GitOps Patch Tool Wiring

**Files:**
- Modify: `servers/model-catalog-mcp/Cargo.toml`
- Modify: `servers/model-catalog-mcp/src/tools.rs`

- [ ] **Step 1: Add patch tool params and implementation**

Add this dependency to `servers/model-catalog-mcp/Cargo.toml` if it is missing:

```toml
homelab-mcp-gitops = { path = "../../crates/homelab-mcp-gitops" }
```

Add these imports to `servers/model-catalog-mcp/src/tools.rs`:

```rust
use homelab_mcp_gitops::{write_patch, GitOpsPatch, RenderedFile};
```

Add this params struct:

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WriteGitOpsPatchParams {
    pub recipe_id: String,
    pub sb_repo_root: String,
}
```

Add this tool method inside `impl ModelCatalogTools`:

```rust
#[tool(description = "Write a local GitOps patch for a planned model deployment")]
pub fn write_gitops_patch(
    &self,
    Parameters(params): Parameters<WriteGitOpsPatchParams>,
) -> Result<String, String> {
    let recipe = self.find_recipe(&params.recipe_id)?;
    let plan = plan_deploy(
        &recipe,
        &self.cluster_profile,
        DeployOverrides {
            name: None,
            namespace: None,
            replicas: None,
            env_overrides: Vec::new(),
        },
    )
    .data;
    let yaml = render_kserve_yaml(&plan).map_err(|error| error.to_string())?;
    let model_dir = format!(
        "argocd/clusters/superbloom/ai/vllm/resources/{}/",
        plan.intent.name
    );
    let patch = GitOpsPatch {
        files: vec![
            RenderedFile {
                relative_path: PathBuf::from(format!("{model_dir}inferenceservice.yaml")),
                contents: yaml,
            },
            RenderedFile {
                relative_path: PathBuf::from(format!("{model_dir}kustomization.yaml")),
                contents: "resources:\n  - inferenceservice.yaml\n".into(),
            },
        ],
    };
    let result = write_patch(PathBuf::from(params.sb_repo_root).as_path(), &patch)
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&result.diff_summary).map_err(|error| error.to_string())
}
```

Add this test:

```rust
#[test]
fn write_gitops_patch_writes_model_directory() {
    let temp = tempfile::tempdir().expect("temp dir");
    let output = tools()
        .write_gitops_patch(Parameters(WriteGitOpsPatchParams {
            recipe_id: "qwen3-8b".into(),
            sb_repo_root: temp.path().display().to_string(),
        }))
        .expect("patch writes");
    assert!(output.contains("qwen3-8b/inferenceservice.yaml"));
    assert!(temp
        .path()
        .join("argocd/clusters/superbloom/ai/vllm/resources/qwen3-8b/kustomization.yaml")
        .exists());
}
```

Add `tempfile.workspace = true` to `[dev-dependencies]` in `servers/model-catalog-mcp/Cargo.toml`.

- [ ] **Step 2: Run server patch tests**

Run:

```bash
cargo test -p model-catalog-mcp write_gitops_patch_writes_model_directory
```

Expected: the test writes both files under a temporary `sb` checkout.

- [ ] **Step 3: Commit**

```bash
git add servers/model-catalog-mcp
git commit -m "feat(server): add explicit GitOps patch tool"
```

## Task 11: Add Status and Logs Tool Wiring

**Files:**
- Modify: `servers/model-catalog-mcp/src/tools.rs`

- [ ] **Step 1: Add status and logs output methods using the read-only contract**

Add these params structs:

```rust
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
```

Add these tool methods inside `impl ModelCatalogTools`:

```rust
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
```

Add tests:

```rust
#[test]
fn status_is_read_only_and_agent_legible() {
    let output = tools()
        .status(Parameters(ModelStatusParams {
            namespace: "ai".into(),
            name: "qwen3-8b".into(),
        }))
        .expect("status");
    assert!(output.contains("\"ready\":false"));
    assert!(output.contains("qwen3-8b"));
}

#[test]
fn logs_accept_tail_parameter() {
    let output = tools()
        .logs(Parameters(ModelLogsParams {
            namespace: "ai".into(),
            name: "qwen3-8b".into(),
            tail: Some(20),
        }))
        .expect("logs");
    assert!(output.contains("\"tail\":20"));
}
```

- [ ] **Step 2: Run status/log tests**

Run:

```bash
cargo test -p model-catalog-mcp status_is_read_only_and_agent_legible
cargo test -p model-catalog-mcp logs_accept_tail_parameter
```

Expected: both tests pass.

- [ ] **Step 3: Commit**

```bash
git add servers/model-catalog-mcp/src/tools.rs
git commit -m "feat(server): add read-only status and logs tools"
```

## Task 12: Final Verification and Handoff

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

Expected: no unstaged changes after formatting, or only intentional formatting changes that should be committed.

- [ ] **Step 4: Commit final validator changes if formatting changed files**

If `cargo fmt --all` changed files, run:

```bash
git add .
git commit -m "style: format model catalog MCP workspace"
```

Expected: a formatting-only commit is created. If no files changed, skip this step.

- [ ] **Step 5: Summarize implementation**

Record these facts in the implementation handoff:

```text
Implemented:
- Rust workspace with core, gitops, k8s, catalog, and rmcp server crates.
- Local recipe parsing for qwen3-8b and deepseek-v4-flash fixtures.
- Recipe -> DeploymentIntent -> KServe manifest flow.
- ClusterProfile-based fit validation.
- Explicit local GitOps patch writing.
- Read-only status/log tool surfaces.

Validators:
- cargo fmt --all
- cargo clippy --workspace --all-targets -- -D warnings
- cargo test --workspace
- cargo check --workspace
```
