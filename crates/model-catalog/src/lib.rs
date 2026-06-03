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
