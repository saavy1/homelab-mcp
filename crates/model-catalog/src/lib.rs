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
