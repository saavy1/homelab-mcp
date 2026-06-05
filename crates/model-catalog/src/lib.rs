pub mod arena;
pub mod capacity;
pub mod digest;
pub mod planner;
pub mod policy;
pub mod profile;
pub mod recipe;
pub mod render;
pub mod state;
pub mod types;

pub use arena::{SparkArenaSearchResult, load_spark_arena_recipes, search_spark_arena_recipes};
pub use capacity::{
    ActiveModelCapacity, CapacityReport, FitConfidence, FitEstimate, estimate_fit_from_report,
};
pub use digest::{compute_plan_digest, plan_to_digest_input};
pub use planner::{DeployOverrides, plan_deploy, validate_fit};
pub use policy::validate_plan_policy;
pub use profile::{ClusterProfile, IngressMode, ModelStorage, NodeProfile, NodeRole, Taint};
pub use recipe::{load_recipe_dir, load_recipe_file, parse_recipe_yaml, search_recipes};
pub use render::{render_kserve_value, render_kserve_yaml};
pub use state::{
    DeploymentState, RendererMode, RuntimeDeploymentRecord, RuntimeProfile, RuntimeRecipeRecord,
};
pub use types::{
    ApplyMode, DeploymentPlan, EnvVar, HardwareSpec, IngressPolicy, ModelSpec, Recipe,
    RecipeSource, ResourceRequests, RuntimeSpec, ServingSpec, StorageMode,
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
