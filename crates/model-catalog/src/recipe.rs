use crate::Recipe;
use homelab_mcp_core::{HomelabMcpError, HomelabResult};
use std::{fs, path::Path};

pub fn parse_recipe_yaml(input: &str) -> HomelabResult<Recipe> {
    serde_yaml::from_str(input).map_err(|error| HomelabMcpError::Serialization(error.to_string()))
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
        let recipes = [recipe];
        let results = search_recipes(&recipes, Some("deepseek"));
        assert_eq!(results.len(), 1);
    }
}
