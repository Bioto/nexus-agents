use crate::error::{Result, ToolboxError};
use crate::nutrition::NutritionService;
use super::commands::BatchOperation;
use super::helpers::{parse_ids, parse_terms};
use futures::future::join_all;
use uuid::Uuid;

/// Handle batch operations
pub async fn handle_batch_operation(
    pool: &sqlx::PgPool,
    operation: BatchOperation,
) -> Result<()> {
    match operation {
        BatchOperation::GetIngredients { ids } => {
            let parsed_ids: Result<Vec<Uuid>> = parse_ids(&ids)
                .into_iter()
                .map(|id| Uuid::parse_str(&id).map_err(|e| {
                    ToolboxError::Validation(format!("Invalid UUID '{}': {}", id, e))
                }))
                .collect();
            let parsed_ids = parsed_ids?;

            let results: Vec<_> = join_all(
                parsed_ids.iter().map(|&id| {
                    let pool = pool;
                    async move {
                        NutritionService::get_ingredient(pool, id).await
                    }
                })
            )
            .await;

            println!("Batch get ingredients ({} results):", results.len());
            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(ingredient) => {
                        println!("\n[{}] Ingredient: {}", idx + 1, ingredient.name);
                        println!("  ID: {}", ingredient.id);
                        if let Some(desc) = &ingredient.description {
                            println!("  Description: {}", desc);
                        }
                        println!("  Created: {}", ingredient.created_at);
                    }
                    Err(e) => {
                        eprintln!("[{}] Error: {}", idx + 1, e);
                    }
                }
            }
        }
        BatchOperation::GetRecipes { ids, full } => {
            let parsed_ids: Result<Vec<Uuid>> = parse_ids(&ids)
                .into_iter()
                .map(|id| Uuid::parse_str(&id).map_err(|e| {
                    ToolboxError::Validation(format!("Invalid UUID '{}': {}", id, e))
                }))
                .collect();
            let parsed_ids = parsed_ids?;

            if full {
                let results: Vec<_> = join_all(
                    parsed_ids.iter().map(|&id| {
                        let pool = pool;
                        async move {
                            NutritionService::get_recipe_with_details(pool, id).await
                        }
                    })
                )
                .await;

                println!("Batch get recipes with full details ({} results):", results.len());
                for (idx, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(recipe) => {
                            println!("\n[{}] Recipe: {}", idx + 1, recipe.recipe.name);
                            println!("  ID: {}", recipe.recipe.id);
                            if let Some(desc) = &recipe.recipe.description {
                                println!("  Description: {}", desc);
                            }
                            println!("  Servings: {:?}", recipe.recipe.servings);
                            println!("  Prep time: {:?} minutes", recipe.recipe.prep_time_minutes);
                            println!("  Cook time: {:?} minutes", recipe.recipe.cook_time_minutes);
                            println!("  Ingredients:");
                            for ing in &recipe.ingredients {
                                println!("    - {}: {} {} ({})",
                                    ing.ingredient.name,
                                    ing.recipe_ingredient.quantity,
                                    ing.recipe_ingredient.unit,
                                    ing.ingredient.id
                                );
                            }
                            println!("  Steps:");
                            for step in &recipe.steps {
                                println!("    {}. {}", step.step_number, step.instruction);
                            }
                        }
                        Err(e) => {
                            eprintln!("[{}] Error: {}", idx + 1, e);
                        }
                    }
                }
            } else {
                let results: Vec<_> = join_all(
                    parsed_ids.iter().map(|&id| {
                        let pool = pool;
                        async move {
                            NutritionService::get_recipe(pool, id).await
                        }
                    })
                )
                .await;

                println!("Batch get recipes ({} results):", results.len());
                for (idx, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(recipe) => {
                            println!("\n[{}] Recipe: {}", idx + 1, recipe.name);
                            println!("  ID: {}", recipe.id);
                            if let Some(desc) = &recipe.description {
                                println!("  Description: {}", desc);
                            }
                            println!("  Created: {}", recipe.created_at);
                        }
                        Err(e) => {
                            eprintln!("[{}] Error: {}", idx + 1, e);
                        }
                    }
                }
            }
        }
        BatchOperation::CalculateNutrition { ids } => {
            let parsed_ids: Result<Vec<Uuid>> = parse_ids(&ids)
                .into_iter()
                .map(|id| Uuid::parse_str(&id).map_err(|e| {
                    ToolboxError::Validation(format!("Invalid UUID '{}': {}", id, e))
                }))
                .collect();
            let parsed_ids = parsed_ids?;

            let results: Vec<_> = join_all(
                parsed_ids.iter().map(|&id| {
                    let pool = pool;
                    async move {
                        NutritionService::calculate_recipe_nutrition(pool, id, None).await
                    }
                })
            )
            .await;

            println!("Batch calculate nutrition ({} results):", results.len());
            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(nutrition) => {
                        println!("\n[{}] Recipe ID: {}", idx + 1, nutrition.recipe_id);
                        println!("  Total calories: {}", nutrition.total_calories);
                        println!("  Total protein: {}g", nutrition.total_protein_g);
                        println!("  Total carbs: {}g", nutrition.total_carbs_g);
                        println!("  Total fat: {}g", nutrition.total_fat_g);
                        if let Some(fiber) = nutrition.total_fiber_g {
                            println!("  Total fiber: {}g", fiber);
                        }
                        if let Some(sugar) = nutrition.total_sugar_g {
                            println!("  Total sugar: {}g", sugar);
                        }
                        if let Some(cal_per_serving) = nutrition.per_serving_calories {
                            println!("  Per serving:");
                            println!("    Calories: {}", cal_per_serving);
                            if let Some(protein) = nutrition.per_serving_protein_g {
                                println!("    Protein: {}g", protein);
                            }
                            if let Some(carbs) = nutrition.per_serving_carbs_g {
                                println!("    Carbs: {}g", carbs);
                            }
                            if let Some(fat) = nutrition.per_serving_fat_g {
                                println!("    Fat: {}g", fat);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[{}] Error: {}", idx + 1, e);
                    }
                }
            }
        }
        BatchOperation::SearchIngredients { terms } => {
            let parsed_terms = parse_terms(&terms);

            let results: Vec<_> = join_all(
                parsed_terms.iter().map(|term| {
                    let pool = pool;
                    let term = term.clone();
                    async move {
                        NutritionService::list_ingredients(pool, Some(&term)).await
                    }
                })
            )
            .await;

            // Collect unique ingredients (by ID)
            let mut seen_ids = std::collections::HashSet::new();
            let mut all_ingredients = Vec::new();

            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(ingredients) => {
                        println!("Search '{}' found {} ingredients", parsed_terms[idx], ingredients.len());
                        for ingredient in ingredients {
                            if seen_ids.insert(ingredient.id) {
                                all_ingredients.push(ingredient);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Search '{}' error: {}", parsed_terms[idx], e);
                    }
                }
            }

            println!("\nTotal unique ingredients found: {}", all_ingredients.len());
            for ingredient in all_ingredients {
                println!("  - {} ({})", ingredient.name, ingredient.id);
            }
        }
        BatchOperation::SearchRecipes { terms, ingredient_id } => {
            let parsed_terms = parse_terms(&terms);
            let ingredient_uuid = ingredient_id
                .map(|id| Uuid::parse_str(&id))
                .transpose()?;

            let results: Vec<_> = join_all(
                parsed_terms.iter().map(|term| {
                    let pool = pool;
                    let term = term.clone();
                    let ingredient_uuid = ingredient_uuid;
                    async move {
                        NutritionService::list_recipes(pool, Some(&term), ingredient_uuid).await
                    }
                })
            )
            .await;

            // Collect unique recipes (by ID)
            let mut seen_ids = std::collections::HashSet::new();
            let mut all_recipes = Vec::new();

            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(recipes) => {
                        println!("Search '{}' found {} recipes", parsed_terms[idx], recipes.len());
                        for recipe in recipes {
                            if seen_ids.insert(recipe.id) {
                                all_recipes.push(recipe);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Search '{}' error: {}", parsed_terms[idx], e);
                    }
                }
            }

            println!("\nTotal unique recipes found: {}", all_recipes.len());
            for recipe in all_recipes {
                println!("  - {} ({})", recipe.name, recipe.id);
            }
        }
    }

    Ok(())
}

