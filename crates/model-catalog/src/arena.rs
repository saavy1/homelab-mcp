use std::path::Path;

use homelab_mcp_core::HomelabResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Recipe, RecipeSource, load_recipe_dir, search_recipes};

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct SparkArenaSearchResult {
    pub id: String,
    pub model_id: String,
    pub quantization: Option<String>,
    pub estimated_vram_gb: Option<u32>,
    pub required_args: Vec<String>,
    pub source: String,
}

pub fn load_spark_arena_recipes(path: impl AsRef<Path>) -> HomelabResult<Vec<Recipe>> {
    let mut recipes = load_recipe_dir(path)?;
    for recipe in &mut recipes {
        recipe.source = RecipeSource::SparkArena;
    }
    Ok(recipes)
}

pub fn search_spark_arena_recipes(
    recipes: &[Recipe],
    query: Option<&str>,
) -> Vec<SparkArenaSearchResult> {
    search_recipes(recipes, query)
        .into_iter()
        .map(|recipe| SparkArenaSearchResult {
            id: recipe.id.clone(),
            model_id: recipe.model.id.clone(),
            quantization: recipe.model.quantization.clone(),
            estimated_vram_gb: recipe.hardware.estimated_vram_gb,
            required_args: recipe.runtime.args.clone(),
            source: recipe.provenance.source.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_result_exposes_required_args() {
        let recipe = crate::parse_recipe_yaml(include_str!(
            "../tests/fixtures/local-recipes/lfm25-350m.yaml"
        ))
        .expect("recipe parses");
        let results = search_spark_arena_recipes(&[recipe], Some("lfm"));
        assert_eq!(results.len(), 1);
        assert!(
            results[0]
                .required_args
                .contains(&"--language-model-only".into())
        );
    }

    #[test]
    fn load_spark_arena_forces_source() {
        let recipes =
            load_spark_arena_recipes("tests/fixtures/local-recipes").expect("load recipes");
        let recipe = recipes
            .iter()
            .find(|r| r.id == "lfm25-350m")
            .expect("lfm25-350m loaded");
        assert_eq!(recipe.source, RecipeSource::SparkArena);
    }

    #[test]
    fn search_spark_arena_returns_empty_for_no_match() {
        let recipes =
            load_spark_arena_recipes("tests/fixtures/local-recipes").expect("load recipes");
        let results = search_spark_arena_recipes(&recipes, Some("nonexistent-model"));
        assert!(results.is_empty());
    }

    #[test]
    fn search_spark_arena_returns_all_when_no_query() {
        let recipes =
            load_spark_arena_recipes("tests/fixtures/local-recipes").expect("load recipes");
        let results = search_spark_arena_recipes(&recipes, None);
        assert!(!results.is_empty());
    }
}
