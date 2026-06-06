use kube::CustomResourceExt;
use model_catalog::{ModelDeployment, ModelRecipe};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_yaml::to_string(&ModelRecipe::crd())?);
    println!("---");
    println!("{}", serde_yaml::to_string(&ModelDeployment::crd())?);
    Ok(())
}
