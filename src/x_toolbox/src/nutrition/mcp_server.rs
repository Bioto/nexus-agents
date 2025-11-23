use crate::error::ToolboxError;
use rmcp::{
    handler::server::{
        router::tool::ToolRouter,
        wrapper::Parameters,
        ServerHandler,
    },
    model::*,
    schemars, tool, tool_router, ErrorData as McpError,
};
use serde::{Deserialize, Serialize};
use sqlx::{types::BigDecimal, PgPool};
use std::sync::Arc;
use uuid::Uuid;



use super::{Database, NutritionService};

/// MCP Server for nutrition module
#[derive(Clone)]
pub struct NutritionMcpServer {
    pool: Arc<PgPool>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

// Tool definitions
#[tool_router]
impl NutritionMcpServer {
    pub async fn new() -> Result<Self, ToolboxError> {
        // Load environment variables from .env file if it exists
        let _ = dotenvy::dotenv();

        
        
        let db = Database::new().await?;
        Ok(Self {
            pool: Arc::new(db.pool().clone()),
            tool_router: Self::tool_router(),
        })
    }

    pub async fn with_database(db: Database) -> Result<Self, ToolboxError> {
        Ok(Self {
            pool: Arc::new(db.pool().clone()),
            tool_router: Self::tool_router(),
        })
    }

    // ========== Ingredient Tools ==========

    /// Create a new ingredient
    #[tool(description = "Create a new ingredient with optional description. Returns the ingredient ID.")]
    async fn create_ingredient(
        &self,
        params: Parameters<CreateIngredientParams>,
    ) -> Result<CallToolResult, McpError> {
        let ingredient = NutritionService::create_ingredient(
            &self.pool,
            &params.0.name,
            params.0.description.as_deref(),
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Created ingredient: {} (ID: {})\nDescription: {}",
            ingredient.name,
            ingredient.id,
            ingredient.description.unwrap_or_else(|| "None".to_string())
        ))]))
    }

    /// Update an ingredient
    #[tool(description = "Update ingredient name and/or description by UUID")]
    async fn update_ingredient(
        &self,
        params: Parameters<UpdateIngredientParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        let ingredient = NutritionService::update_ingredient(
            &self.pool,
            uuid,
            params.0.name.as_deref(),
            params.0.description.as_deref(),
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updated ingredient: {} ({})",
            ingredient.name, ingredient.id
        ))]))
    }

    /// Delete an ingredient
    #[tool(description = "Delete an ingredient by UUID. This will also delete associated nutritional info and remove it from recipes.")]
    async fn delete_ingredient(
        &self,
        params: Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        NutritionService::delete_ingredient(&self.pool, uuid)
            .await
            .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Deleted ingredient: {}",
            params.0.id
        ))]))
    }

    // ========== Nutritional Info Tools ==========

    /// Add or update nutritional information for an ingredient
    #[tool(description = "Add or update nutritional information for an ingredient (per 100g basis). All values are required except fiber and sugar.")]
    async fn add_nutritional_info(
        &self,
        params: Parameters<AddNutritionalInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.ingredient_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        let nutritional_info = NutritionService::upsert_nutritional_info(
            &self.pool,
            uuid,
            params.0.calories_per_100g.to_string().parse().map_err(|e| {
                McpError::invalid_params(format!("Invalid calories value: {}", e), None)
            })?,
            params.0.protein_g.to_string().parse().map_err(|e| {
                McpError::invalid_params(format!("Invalid protein value: {}", e), None)
            })?,
            params.0.carbs_g.to_string().parse().map_err(|e| {
                McpError::invalid_params(format!("Invalid carbs value: {}", e), None)
            })?,
            params.0.fat_g.to_string().parse().map_err(|e| {
                McpError::invalid_params(format!("Invalid fat value: {}", e), None)
            })?,
            params.0.fiber_g.map(|f| f.to_string().parse()).transpose().map_err(|e| {
                McpError::invalid_params(format!("Invalid fiber value: {}", e), None)
            })?,
            params.0.sugar_g.map(|s| s.to_string().parse()).transpose().map_err(|e| {
                McpError::invalid_params(format!("Invalid sugar value: {}", e), None)
            })?,
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Added/updated nutritional info for ingredient {}\nCalories: {} per 100g\nProtein: {}g\nCarbs: {}g\nFat: {}g",
            nutritional_info.ingredient_id,
            nutritional_info.calories_per_100g,
            nutritional_info.protein_g,
            nutritional_info.carbs_g,
            nutritional_info.fat_g
        ))]))
    }

    // ========== Recipe Tools ==========

    /// Create a new recipe
    #[tool(description = "Create a new recipe with ingredients and steps. Ingredient IDs must already exist. Steps are numbered automatically.")]
    async fn create_recipe(
        &self,
        params: Parameters<CreateRecipeParams>,
    ) -> Result<CallToolResult, McpError> {
        let ingredients: Result<Vec<(Uuid, BigDecimal, String)>, McpError> = params
            .0
            .ingredients
            .into_iter()
            .map(|ri| {
                let uuid = Uuid::parse_str(&ri.ingredient_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None)
                })?;
                let quantity: BigDecimal = ri.quantity.to_string().parse().map_err(|e| {
                    McpError::invalid_params(format!("Invalid quantity: {}", e), None)
                })?;
                Ok((uuid, quantity, ri.unit))
            })
            .collect();

        let ingredients = ingredients?;

        let steps: Vec<(i32, String)> = params
            .0
            .steps
            .into_iter()
            .enumerate()
            .map(|(i, instruction)| ((i + 1) as i32, instruction))
            .collect();

        let recipe = NutritionService::create_recipe(
            &self.pool,
            &params.0.name,
            params.0.description.as_deref(),
            params.0.servings,
            params.0.prep_time_minutes,
            params.0.cook_time_minutes,
            ingredients,
            steps,
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Created recipe: {} (ID: {})\nServings: {:?}\nPrep time: {:?} min\nCook time: {:?} min",
            recipe.name,
            recipe.id,
            recipe.servings,
            recipe.prep_time_minutes,
            recipe.cook_time_minutes
        ))]))
    }

    /// Update a recipe
    #[tool(description = "Update recipe metadata (name, description, servings, times). Does not modify ingredients or steps.")]
    async fn update_recipe(
        &self,
        params: Parameters<UpdateRecipeParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        let recipe = NutritionService::update_recipe(
            &self.pool,
            uuid,
            params.0.name.as_deref(),
            params.0.description.as_deref(),
            params.0.servings,
            params.0.prep_time_minutes,
            params.0.cook_time_minutes,
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updated recipe: {} ({})",
            recipe.name, recipe.id
        ))]))
    }

    /// Delete a recipe
    #[tool(description = "Delete a recipe by UUID. This will also delete associated ingredients and steps.")]
    async fn delete_recipe(
        &self,
        params: Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        NutritionService::delete_recipe(&self.pool, uuid)
            .await
            .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Deleted recipe: {}",
            params.0.id
        ))]))
    }

    /// Calculate nutritional information for a recipe
    #[tool(description = "Calculate total and per-serving nutritional information for a recipe by UUID. Optionally specify servings to calculate per-serving nutrition for a different number of servings than the recipe's default.")]
    async fn calculate_recipe_nutrition(
        &self,
        params: Parameters<CalculateNutritionParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        let nutrition = NutritionService::calculate_recipe_nutrition(&self.pool, uuid, params.0.servings)
            .await
            .map_err(convert_error)?;

        let mut output = format!(
            "Nutritional Information for Recipe {}\n\nTotal:\n  Calories: {}\n  Protein: {}g\n  Carbs: {}g\n  Fat: {}g",
            params.0.id,
            nutrition.total_calories,
            nutrition.total_protein_g,
            nutrition.total_carbs_g,
            nutrition.total_fat_g
        );

        if let Some(fiber) = nutrition.total_fiber_g {
            output.push_str(&format!("\n  Fiber: {}g", fiber));
        }
        if let Some(sugar) = nutrition.total_sugar_g {
            output.push_str(&format!("\n  Sugar: {}g", sugar));
        }

        if let Some(cal_per_serving) = nutrition.per_serving_calories {
            output.push_str(&format!(
                "\n\nPer Serving:\n  Calories: {}\n  Protein: {:?}g\n  Carbs: {:?}g\n  Fat: {:?}g",
                cal_per_serving,
                nutrition.per_serving_protein_g,
                nutrition.per_serving_carbs_g,
                nutrition.per_serving_fat_g
            ));
            
            // If servings were specified, show total for that number of servings
            if let Some(s) = params.0.servings {
                let total_cal = &cal_per_serving * BigDecimal::from(s);
                let total_protein = nutrition.per_serving_protein_g.as_ref().map(|p| p * BigDecimal::from(s));
                let total_carbs = nutrition.per_serving_carbs_g.as_ref().map(|c| c * BigDecimal::from(s));
                let total_fat = nutrition.per_serving_fat_g.as_ref().map(|f| f * BigDecimal::from(s));
                
                output.push_str(&format!(
                    "\n\nTotal for {} servings:\n  Calories: {}\n  Protein: {:?}g\n  Carbs: {:?}g\n  Fat: {:?}g",
                    s,
                    total_cal,
                    total_protein,
                    total_carbs,
                    total_fat
                ));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    // ========== Recipe Ingredient Management ==========

    /// Add an ingredient to an existing recipe
    #[tool(description = "Add an ingredient to an existing recipe with quantity and unit")]
    async fn add_recipe_ingredient(
        &self,
        params: Parameters<AddRecipeIngredientParams>,
    ) -> Result<CallToolResult, McpError> {
        let recipe_uuid = Uuid::parse_str(&params.0.recipe_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None)
        })?;
        let ingredient_uuid = Uuid::parse_str(&params.0.ingredient_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None)
        })?;
        let quantity: BigDecimal = params.0.quantity.to_string().parse().map_err(|e| {
            McpError::invalid_params(format!("Invalid quantity: {}", e), None)
        })?;

        let recipe_ingredient = NutritionService::add_recipe_ingredient(
            &self.pool,
            recipe_uuid,
            ingredient_uuid,
            quantity,
            params.0.unit,
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Added ingredient {} to recipe {}\nQuantity: {} {}",
            params.0.ingredient_id, params.0.recipe_id, recipe_ingredient.quantity, recipe_ingredient.unit
        ))]))
    }

    /// Remove an ingredient from a recipe
    #[tool(description = "Remove an ingredient from a recipe")]
    async fn remove_recipe_ingredient(
        &self,
        params: Parameters<RemoveRecipeIngredientParams>,
    ) -> Result<CallToolResult, McpError> {
        let recipe_uuid = Uuid::parse_str(&params.0.recipe_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None)
        })?;
        let ingredient_uuid = Uuid::parse_str(&params.0.ingredient_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None)
        })?;

        NutritionService::remove_recipe_ingredient(&self.pool, recipe_uuid, ingredient_uuid)
            .await
            .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Removed ingredient {} from recipe {}",
            params.0.ingredient_id, params.0.recipe_id
        ))]))
    }

    /// Add a step to a recipe
    #[tool(description = "Add a cooking step to a recipe")]
    async fn add_recipe_step(
        &self,
        params: Parameters<AddRecipeStepParams>,
    ) -> Result<CallToolResult, McpError> {
        let recipe_uuid = Uuid::parse_str(&params.0.recipe_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None)
        })?;

        let step = NutritionService::add_recipe_step(
            &self.pool,
            recipe_uuid,
            params.0.step_number,
            params.0.instruction,
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Added step {} to recipe {}: {}",
            step.step_number, params.0.recipe_id, step.instruction
        ))]))
    }

    /// Remove a step from a recipe
    #[tool(description = "Remove a cooking step from a recipe")]
    async fn remove_recipe_step(
        &self,
        params: Parameters<RemoveRecipeStepParams>,
    ) -> Result<CallToolResult, McpError> {
        let recipe_uuid = Uuid::parse_str(&params.0.recipe_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None)
        })?;

        NutritionService::remove_recipe_step(&self.pool, recipe_uuid, params.0.step_number)
            .await
            .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Removed step {} from recipe {}",
            params.0.step_number, params.0.recipe_id
        ))]))
    }

    /// Extract recipe from URL
    #[tool(description = "Extract recipe information from a URL by fetching the HTML content and using an LLM to parse the recipe. Returns the extracted recipe data.")]
    async fn extract_recipe_from_url(
        &self,
        params: Parameters<ExtractRecipeFromUrlParams>,
    ) -> Result<CallToolResult, McpError> {
        let extracted = NutritionService::extract_recipe_from_url(&params.0.url)
            .await
            .map_err(convert_error)?;

        let mut output = format!(
            "Extracted Recipe: {}\n",
            extracted.name
        );

        if let Some(desc) = &extracted.description {
            output.push_str(&format!("Description: {}\n", desc));
        }

        if let Some(servings) = extracted.servings {
            output.push_str(&format!("Servings: {}\n", servings));
        }

        if let Some(prep) = extracted.prep_time_minutes {
            output.push_str(&format!("Prep time: {} minutes\n", prep));
        }

        if let Some(cook) = extracted.cook_time_minutes {
            output.push_str(&format!("Cook time: {} minutes\n", cook));
        }

        output.push_str("\nIngredients:\n");
        for ing in &extracted.ingredients {
            output.push_str(&format!("  - {} {} {}\n", ing.quantity, ing.unit, ing.name));
        }

        output.push_str("\nSteps:\n");
        for (i, step) in extracted.steps.iter().enumerate() {
            output.push_str(&format!("  {}. {}\n", i + 1, step));
        }

        // Also return JSON for programmatic use
        let json_output = serde_json::to_string_pretty(&extracted)
            .unwrap_or_else(|_| "Failed to serialize recipe".to_string());

        Ok(CallToolResult::success(vec![
            Content::text(output),
            Content::text(format!("\nJSON representation:\n{}", json_output)),
        ]))
    }

    // ========== Batch Operations ==========

    /// Get multiple ingredients by IDs
    #[tool(description = "Get multiple ingredients by their UUIDs. Returns details for all found ingredients.")]
    async fn batch_get_ingredients(
        &self,
        params: Parameters<BatchGetByIdsParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        let uuids: Result<Vec<Uuid>, McpError> = params
            .0
            .ids
            .into_iter()
            .map(|id| {
                Uuid::parse_str(&id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                })
            })
            .collect();

        let uuids = uuids?;

        let results: Vec<_> = join_all(
            uuids.iter().map(|&id| {
                let pool = &self.pool;
                async move {
                    NutritionService::get_ingredient(pool, id).await.map_err(convert_error)
                }
            })
        )
        .await;

        let mut output = format!("Batch get ingredients ({} requested):\n\n", results.len());
        let mut success_count = 0;
        let mut error_count = 0;

        for (idx, result) in results.into_iter().enumerate() {
            match result {
                Ok(ingredient) => {
                    success_count += 1;
                    output.push_str(&format!(
                        "[{}] Ingredient: {}\n  ID: {}\n  Description: {}\n  Created: {}\n\n",
                        idx + 1,
                        ingredient.name,
                        ingredient.id,
                        ingredient.description.unwrap_or_else(|| "None".to_string()),
                        ingredient.created_at
                    ));
                }
                Err(e) => {
                    error_count += 1;
                    output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                }
            }
        }

        output.push_str(&format!("Summary: {} succeeded, {} failed", success_count, error_count));

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Get multiple recipes by IDs
    #[tool(description = "Get multiple recipes by their UUIDs. Returns details for all found recipes. Set full=true to include ingredients and steps.")]
    async fn batch_get_recipes(
        &self,
        params: Parameters<BatchGetRecipesParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        let uuids: Result<Vec<Uuid>, McpError> = params
            .0
            .ids
            .into_iter()
            .map(|id| {
                Uuid::parse_str(&id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                })
            })
            .collect();

        let uuids = uuids?;

        if params.0.full {
            let results: Vec<_> = join_all(
                uuids.iter().map(|&id| {
                    let pool = &self.pool;
                    async move {
                        NutritionService::get_recipe_with_details(pool, id).await.map_err(convert_error)
                    }
                })
            )
            .await;

            let mut output = format!("Batch get recipes with full details ({} requested):\n\n", results.len());
            let mut success_count = 0;
            let mut error_count = 0;

            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(recipe) => {
                        success_count += 1;
                        output.push_str(&format!(
                            "[{}] Recipe: {}\n  ID: {}\n  Description: {}\n  Servings: {:?}\n  Prep: {:?} min, Cook: {:?} min\n",
                            idx + 1,
                            recipe.recipe.name,
                            recipe.recipe.id,
                            recipe.recipe.description.unwrap_or_else(|| "None".to_string()),
                            recipe.recipe.servings,
                            recipe.recipe.prep_time_minutes,
                            recipe.recipe.cook_time_minutes
                        ));
                        output.push_str("  Ingredients:\n");
                        for ing in &recipe.ingredients {
                            output.push_str(&format!(
                                "    - {} {} of {} ({})\n",
                                ing.recipe_ingredient.quantity,
                                ing.recipe_ingredient.unit,
                                ing.ingredient.name,
                                ing.ingredient.id
                            ));
                        }
                        output.push_str("  Steps:\n");
                        for step in &recipe.steps {
                            output.push_str(&format!("    {}. {}\n", step.step_number, step.instruction));
                        }
                        output.push('\n');
                    }
                    Err(e) => {
                        error_count += 1;
                        output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                    }
                }
            }

            output.push_str(&format!("Summary: {} succeeded, {} failed", success_count, error_count));
            Ok(CallToolResult::success(vec![Content::text(output)]))
        } else {
            let results: Vec<_> = join_all(
                uuids.iter().map(|&id| {
                    let pool = &self.pool;
                    async move {
                        NutritionService::get_recipe(pool, id).await.map_err(convert_error)
                    }
                })
            )
            .await;

            let mut output = format!("Batch get recipes ({} requested):\n\n", results.len());
            let mut success_count = 0;
            let mut error_count = 0;

            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(recipe) => {
                        success_count += 1;
                        output.push_str(&format!(
                            "[{}] Recipe: {}\n  ID: {}\n  Description: {}\n  Created: {}\n\n",
                            idx + 1,
                            recipe.name,
                            recipe.id,
                            recipe.description.unwrap_or_else(|| "None".to_string()),
                            recipe.created_at
                        ));
                    }
                    Err(e) => {
                        error_count += 1;
                        output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                    }
                }
            }

            output.push_str(&format!("Summary: {} succeeded, {} failed", success_count, error_count));
            Ok(CallToolResult::success(vec![Content::text(output)]))
        }
    }

    /// Calculate nutrition for multiple recipes
    #[tool(description = "Calculate nutritional information for multiple recipes by their UUIDs. Returns nutrition data for all recipes.")]
    async fn batch_calculate_nutrition(
        &self,
        params: Parameters<BatchGetByIdsParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        let uuids: Result<Vec<Uuid>, McpError> = params
            .0
            .ids
            .into_iter()
            .map(|id| {
                Uuid::parse_str(&id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                })
            })
            .collect();

        let uuids = uuids?;

        let results: Vec<_> = join_all(
            uuids.iter().map(|&id| {
                let pool = &self.pool;
                async move {
                    NutritionService::calculate_recipe_nutrition(pool, id, None).await.map_err(convert_error)
                }
            })
        )
        .await;

        let mut output = format!("Batch calculate nutrition ({} requested):\n\n", results.len());
        let mut success_count = 0;
        let mut error_count = 0;

        for (idx, result) in results.into_iter().enumerate() {
            match result {
                Ok(nutrition) => {
                    success_count += 1;
                    output.push_str(&format!(
                        "[{}] Recipe ID: {}\n  Total calories: {}\n  Total protein: {}g\n  Total carbs: {}g\n  Total fat: {}g\n",
                        idx + 1,
                        nutrition.recipe_id,
                        nutrition.total_calories,
                        nutrition.total_protein_g,
                        nutrition.total_carbs_g,
                        nutrition.total_fat_g
                    ));
                    if let Some(fiber) = nutrition.total_fiber_g {
                        output.push_str(&format!("  Total fiber: {}g\n", fiber));
                    }
                    if let Some(sugar) = nutrition.total_sugar_g {
                        output.push_str(&format!("  Total sugar: {}g\n", sugar));
                    }
                    if let Some(cal_per_serving) = nutrition.per_serving_calories {
                        output.push_str(&format!("  Per serving: {} calories", cal_per_serving));
                        if let Some(protein) = nutrition.per_serving_protein_g {
                            output.push_str(&format!(", {}g protein", protein));
                        }
                        if let Some(carbs) = nutrition.per_serving_carbs_g {
                            output.push_str(&format!(", {}g carbs", carbs));
                        }
                        if let Some(fat) = nutrition.per_serving_fat_g {
                            output.push_str(&format!(", {}g fat", fat));
                        }
                        output.push('\n');
                    }
                    output.push('\n');
                }
                Err(e) => {
                    error_count += 1;
                    output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                }
            }
        }

        output.push_str(&format!("Summary: {} succeeded, {} failed", success_count, error_count));

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Search ingredients with multiple queries
    #[tool(description = "Search for ingredients using multiple search terms. Returns union of all results (unique ingredients).")]
    async fn batch_search_ingredients(
        &self,
        params: Parameters<BatchSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;
        use std::collections::HashSet;

        let results: Vec<_> = join_all(
            params.0.queries.iter().map(|term| {
                let pool = &self.pool;
                let term = term.clone();
                async move {
                    NutritionService::list_ingredients(pool, Some(&term)).await.map_err(convert_error)
                }
            })
        )
        .await;

        let mut seen_ids = HashSet::new();
        let mut all_ingredients = Vec::new();
        let mut output = String::new();

        for (idx, result) in results.into_iter().enumerate() {
            match result {
                Ok(ingredients) => {
                    output.push_str(&format!(
                        "Search '{}' found {} ingredients\n",
                        params.0.queries[idx],
                        ingredients.len()
                    ));
                    for ingredient in ingredients {
                        if seen_ids.insert(ingredient.id) {
                            all_ingredients.push(ingredient);
                        }
                    }
                }
                Err(e) => {
                    output.push_str(&format!("Search '{}' error: {}\n", params.0.queries[idx], e));
                }
            }
        }

        output.push_str(&format!("\nTotal unique ingredients found: {}\n\n", all_ingredients.len()));
        for ingredient in all_ingredients {
            output.push_str(&format!(
                "- {} ({})\n  Description: {}\n",
                ingredient.name,
                ingredient.id,
                ingredient.description.unwrap_or_else(|| "None".to_string())
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Search recipes with multiple queries
    #[tool(description = "Search for recipes using multiple search terms. Returns union of all results (unique recipes). Optionally filter by ingredient ID.")]
    async fn batch_search_recipes(
        &self,
        params: Parameters<BatchSearchRecipesParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;
        use std::collections::HashSet;

        let ingredient_uuid = params
            .0
            .ingredient_id
            .map(|id| Uuid::parse_str(&id))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None))?;

        let results: Vec<_> = join_all(
            params.0.queries.iter().map(|term| {
                let pool = &self.pool;
                let term = term.clone();
                let ingredient_uuid = ingredient_uuid;
                async move {
                    NutritionService::list_recipes(pool, Some(&term), ingredient_uuid).await.map_err(convert_error)
                }
            })
        )
        .await;

        let mut seen_ids = HashSet::new();
        let mut all_recipes = Vec::new();
        let mut output = String::new();

        for (idx, result) in results.into_iter().enumerate() {
            match result {
                Ok(recipes) => {
                    output.push_str(&format!(
                        "Search '{}' found {} recipes\n",
                        params.0.queries[idx],
                        recipes.len()
                    ));
                    for recipe in recipes {
                        if seen_ids.insert(recipe.id) {
                            all_recipes.push(recipe);
                        }
                    }
                }
                Err(e) => {
                    output.push_str(&format!("Search '{}' error: {}\n", params.0.queries[idx], e));
                }
            }
        }

        output.push_str(&format!("\nTotal unique recipes found: {}\n\n", all_recipes.len()));
        for recipe in all_recipes {
            output.push_str(&format!(
                "- {} ({})\n  Description: {}\n  Servings: {:?}, Prep: {:?} min, Cook: {:?} min\n",
                recipe.name,
                recipe.id,
                recipe.description.unwrap_or_else(|| "None".to_string()),
                recipe.servings,
                recipe.prep_time_minutes,
                recipe.cook_time_minutes
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    // ========== Meal Plan Tools ==========

    /// Create a new meal plan
    #[tool(description = "Create a new meal plan with optional name, description, dates, and template flag")]
    async fn create_meal_plan(
        &self,
        params: Parameters<CreateMealPlanParams>,
    ) -> Result<CallToolResult, McpError> {
        use chrono::NaiveDate;

        let start_date = params.0.start_date
            .as_ref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid start_date format: {}", e), None))?;
        let end_date = params.0.end_date
            .as_ref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid end_date format: {}", e), None))?;

        let meal_plan = NutritionService::create_meal_plan(
            &self.pool,
            &params.0.name,
            params.0.description.as_deref(),
            start_date,
            end_date,
            params.0.is_template,
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Created meal plan: {} (ID: {})\nTemplate: {}\nStart: {:?}\nEnd: {:?}",
            meal_plan.name,
            meal_plan.id,
            meal_plan.is_template,
            meal_plan.start_date,
            meal_plan.end_date
        ))]))
    }

    /// Get multiple meal plans by IDs
    #[tool(description = "Get multiple meal plans by their UUIDs. Returns details for all found meal plans. Set full=true to include entries.")]
    async fn batch_get_meal_plans(
        &self,
        params: Parameters<BatchGetMealPlansParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        let uuids: Result<Vec<Uuid>, McpError> = params
            .0
            .ids
            .into_iter()
            .map(|id| {
                Uuid::parse_str(&id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                })
            })
            .collect();

        let uuids = uuids?;

        if params.0.full {
            let results: Vec<_> = join_all(
                uuids.iter().map(|&id| {
                    let pool = &self.pool;
                    async move {
                        NutritionService::get_meal_plan_with_entries(pool, id).await.map_err(convert_error)
                    }
                })
            )
            .await;

            let mut output = format!("Batch get meal plans with entries ({} requested):\n\n", results.len());
            let mut success_count = 0;
            let mut error_count = 0;

            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(meal_plan) => {
                        success_count += 1;
                        output.push_str(&format!(
                            "[{}] Meal Plan: {}\n  ID: {}\n  Template: {}\n  Entries: {}\n\n",
                            idx + 1,
                            meal_plan.meal_plan.name,
                            meal_plan.meal_plan.id,
                            meal_plan.meal_plan.is_template,
                            meal_plan.entries.len()
                        ));
                    }
                    Err(e) => {
                        error_count += 1;
                        output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                    }
                }
            }

            output.push_str(&format!("Summary: {} succeeded, {} failed", success_count, error_count));
            Ok(CallToolResult::success(vec![Content::text(output)]))
        } else {
            let results: Vec<_> = join_all(
                uuids.iter().map(|&id| {
                    let pool = &self.pool;
                    async move {
                        NutritionService::get_meal_plan(pool, id).await.map_err(convert_error)
                    }
                })
            )
            .await;

            let mut output = format!("Batch get meal plans ({} requested):\n\n", results.len());
            let mut success_count = 0;
            let mut error_count = 0;

            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(meal_plan) => {
                        success_count += 1;
                        output.push_str(&format!(
                            "[{}] Meal Plan: {}\n  ID: {}\n  Template: {}\n\n",
                            idx + 1,
                            meal_plan.name,
                            meal_plan.id,
                            meal_plan.is_template
                        ));
                    }
                    Err(e) => {
                        error_count += 1;
                        output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                    }
                }
            }

            output.push_str(&format!("Summary: {} succeeded, {} failed", success_count, error_count));
            Ok(CallToolResult::success(vec![Content::text(output)]))
        }
    }

    /// Update meal plan metadata
    #[tool(description = "Update meal plan metadata (name, description, dates). Does not modify entries.")]
    async fn update_meal_plan(
        &self,
        params: Parameters<UpdateMealPlanParams>,
    ) -> Result<CallToolResult, McpError> {
        use chrono::NaiveDate;

        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        let start_date = params.0.start_date
            .as_ref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid start_date format: {}", e), None))?;
        let end_date = params.0.end_date
            .as_ref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid end_date format: {}", e), None))?;

        let meal_plan = NutritionService::update_meal_plan(
            &self.pool,
            uuid,
            params.0.name.as_deref(),
            params.0.description.as_deref(),
            start_date,
            end_date,
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updated meal plan: {} ({})",
            meal_plan.name, meal_plan.id
        ))]))
    }

    /// Delete a meal plan
    #[tool(description = "Delete a meal plan by UUID. This will also delete all associated entries.")]
    async fn delete_meal_plan(
        &self,
        params: Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        NutritionService::delete_meal_plan(&self.pool, uuid)
            .await
            .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Deleted meal plan: {}",
            params.0.id
        ))]))
    }

    /// Add an entry to a meal plan
    #[tool(description = "Add a recipe entry to a meal plan. For templates, use day_of_week (0-6, Monday=0). For date-specific plans, use date (YYYY-MM-DD).")]
    async fn add_meal_plan_entry(
        &self,
        params: Parameters<AddMealPlanEntryParams>,
    ) -> Result<CallToolResult, McpError> {
        use chrono::NaiveDate;

        let meal_plan_uuid = Uuid::parse_str(&params.0.meal_plan_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid meal plan UUID: {}", e), None)
        })?;
        let recipe_uuid = Uuid::parse_str(&params.0.recipe_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None)
        })?;

        let date = params.0.date
            .as_ref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid date format: {}", e), None))?;

        let entry = NutritionService::add_meal_plan_entry(
            &self.pool,
            meal_plan_uuid,
            recipe_uuid,
            &params.0.meal_type,
            params.0.day_of_week,
            date,
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Added entry to meal plan {}\nEntry ID: {}\nMeal type: {}\nRecipe ID: {}",
            params.0.meal_plan_id,
            entry.id,
            entry.meal_type,
            entry.recipe_id
        ))]))
    }

    /// Remove an entry from a meal plan
    #[tool(description = "Remove an entry from a meal plan by entry UUID")]
    async fn remove_meal_plan_entry(
        &self,
        params: Parameters<RemoveMealPlanEntryParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.entry_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        NutritionService::remove_meal_plan_entry(&self.pool, uuid)
            .await
            .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Removed meal plan entry: {}",
            params.0.entry_id
        ))]))
    }

    /// List or search meal plans
    #[tool(description = "List meal plans with optional search term and filters (template status, date range)")]
    async fn list_meal_plans(
        &self,
        params: Parameters<ListMealPlansParams>,
    ) -> Result<CallToolResult, McpError> {
        use chrono::NaiveDate;

        let start_date = params.0.start_date
            .as_ref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid start_date format: {}", e), None))?;
        let end_date = params.0.end_date
            .as_ref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid end_date format: {}", e), None))?;

        let meal_plans = NutritionService::list_meal_plans(
            &self.pool,
            params.0.search.as_deref(),
            params.0.is_template,
            start_date,
            end_date,
        )
        .await
        .map_err(convert_error)?;

        let mut output = format!("Found {} meal plans:\n\n", meal_plans.len());
        for meal_plan in meal_plans {
            output.push_str(&format!(
                "- {} ({})\n  Template: {}\n",
                meal_plan.name,
                meal_plan.id,
                meal_plan.is_template
            ));
            if let Some(desc) = &meal_plan.description {
                output.push_str(&format!("  Description: {}\n", desc));
            }
            if let Some(start) = meal_plan.start_date {
                output.push_str(&format!("  Start date: {}\n", start));
            }
            if let Some(end) = meal_plan.end_date {
                output.push_str(&format!("  End date: {}\n", end));
            }
            output.push('\n');
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Calculate nutrition for multiple meal plans
    #[tool(description = "Calculate nutritional information for multiple meal plans by their UUIDs. Returns nutrition data for all meal plans.")]
    async fn batch_calculate_meal_plan_nutrition(
        &self,
        params: Parameters<BatchCalculateMealPlanNutritionParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        let uuids: Result<Vec<Uuid>, McpError> = params
            .0
            .ids
            .into_iter()
            .map(|id| {
                Uuid::parse_str(&id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                })
            })
            .collect();

        let uuids = uuids?;

        let results: Vec<_> = join_all(
            uuids.iter().map(|&id| {
                let pool = &self.pool;
                async move {
                    NutritionService::calculate_meal_plan_nutrition(pool, id).await.map_err(convert_error)
                }
            })
        )
        .await;

        let mut output = format!("Batch calculate meal plan nutrition ({} requested):\n\n", results.len());
        let mut success_count = 0;
        let mut error_count = 0;

        for (idx, result) in results.into_iter().enumerate() {
            match result {
                Ok(nutrition) => {
                    success_count += 1;
                    output.push_str(&format!(
                        "[{}] Meal Plan ID: {}\n  Days: {}\n",
                        idx + 1,
                        nutrition.meal_plan_id,
                        nutrition.daily_nutrition.len()
                    ));
                    if let Some(weekly) = nutrition.weekly_totals {
                        output.push_str(&format!(
                            "  Weekly total calories: {}\n  Average daily calories: {}\n",
                            weekly.total_calories,
                            weekly.average_daily_calories
                        ));
                    }
                    output.push('\n');
                }
                Err(e) => {
                    error_count += 1;
                    output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                }
            }
        }

        output.push_str(&format!("Summary: {} succeeded, {} failed", success_count, error_count));

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
}

// Parameter structs
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct CreateIngredientParams {
    /// Name of the ingredient
    name: String,
    /// Optional description
    description: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct GetByIdParams {
    /// UUID of the item
    id: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct CalculateNutritionParams {
    /// UUID of the recipe
    id: String,
    /// Optional number of servings to calculate per-serving nutrition for. If not provided, uses the recipe's default servings.
    servings: Option<i32>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct UpdateIngredientParams {
    /// UUID of the ingredient
    id: String,
    /// New name (optional)
    name: Option<String>,
    /// New description (optional)
    description: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct AddNutritionalInfoParams {
    /// UUID of the ingredient
    ingredient_id: String,
    /// Calories per 100g
    calories_per_100g: f64,
    /// Protein in grams per 100g
    protein_g: f64,
    /// Carbohydrates in grams per 100g
    carbs_g: f64,
    /// Fat in grams per 100g
    fat_g: f64,
    /// Fiber in grams per 100g (optional)
    fiber_g: Option<f64>,
    /// Sugar in grams per 100g (optional)
    sugar_g: Option<f64>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct RecipeIngredientInput {
    /// UUID of the ingredient
    ingredient_id: String,
    /// Quantity of the ingredient
    quantity: f64,
    /// Unit of measurement (e.g., "g", "ml", "cups", "pieces")
    unit: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct CreateRecipeParams {
    /// Name of the recipe
    name: String,
    /// Optional description
    description: Option<String>,
    /// Number of servings
    servings: Option<i32>,
    /// Preparation time in minutes
    prep_time_minutes: Option<i32>,
    /// Cooking time in minutes
    cook_time_minutes: Option<i32>,
    /// List of ingredients with quantities
    ingredients: Vec<RecipeIngredientInput>,
    /// List of cooking instructions (will be numbered automatically)
    steps: Vec<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct UpdateRecipeParams {
    /// UUID of the recipe
    id: String,
    /// New name (optional)
    name: Option<String>,
    /// New description (optional)
    description: Option<String>,
    /// New servings count (optional)
    servings: Option<i32>,
    /// New prep time in minutes (optional)
    prep_time_minutes: Option<i32>,
    /// New cook time in minutes (optional)
    cook_time_minutes: Option<i32>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct AddRecipeIngredientParams {
    /// Recipe UUID
    recipe_id: String,
    /// Ingredient UUID
    ingredient_id: String,
    /// Quantity of the ingredient
    quantity: f64,
    /// Unit of measurement (e.g., "g", "ml", "cups")
    unit: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct RemoveRecipeIngredientParams {
    /// Recipe UUID
    recipe_id: String,
    /// Ingredient UUID
    ingredient_id: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct AddRecipeStepParams {
    /// Recipe UUID
    recipe_id: String,
    /// Step number (for ordering)
    step_number: i32,
    /// Cooking instruction
    instruction: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct RemoveRecipeStepParams {
    /// Recipe UUID
    recipe_id: String,
    /// Step number to remove
    step_number: i32,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ExtractRecipeFromUrlParams {
    /// URL of the recipe page to extract
    url: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct BatchGetByIdsParams {
    /// List of UUIDs to fetch
    ids: Vec<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct BatchGetRecipesParams {
    /// List of recipe UUIDs to fetch
    ids: Vec<String>,
    /// Include full details (ingredients and steps)
    full: bool,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct BatchSearchParams {
    /// List of search queries
    queries: Vec<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct BatchSearchRecipesParams {
    /// List of search queries
    queries: Vec<String>,
    /// Optional ingredient UUID to filter by
    ingredient_id: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct CreateMealPlanParams {
    /// Name of the meal plan
    name: String,
    /// Optional description
    description: Option<String>,
    /// Start date (YYYY-MM-DD) - null for templates
    start_date: Option<String>,
    /// End date (YYYY-MM-DD) - null for templates
    end_date: Option<String>,
    /// Mark as template (day-of-week based)
    is_template: bool,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct BatchGetMealPlansParams {
    /// List of meal plan UUIDs to fetch
    ids: Vec<String>,
    /// Include full details (entries)
    full: bool,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct UpdateMealPlanParams {
    /// UUID of the meal plan
    id: String,
    /// New name (optional)
    name: Option<String>,
    /// New description (optional)
    description: Option<String>,
    /// New start date (YYYY-MM-DD, optional)
    start_date: Option<String>,
    /// New end date (YYYY-MM-DD, optional)
    end_date: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct AddMealPlanEntryParams {
    /// Meal plan UUID
    meal_plan_id: String,
    /// Recipe UUID
    recipe_id: String,
    /// Meal type (breakfast, lunch, dinner, snack)
    meal_type: String,
    /// Day of week (0-6, Monday=0) - for templates
    day_of_week: Option<i32>,
    /// Date (YYYY-MM-DD) - for date-specific plans
    date: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct RemoveMealPlanEntryParams {
    /// Entry UUID
    entry_id: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct BatchCalculateMealPlanNutritionParams {
    /// List of meal plan UUIDs
    ids: Vec<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ListMealPlansParams {
    /// Optional search term to filter by name or description
    search: Option<String>,
    /// Filter by template status
    is_template: Option<bool>,
    /// Filter by start date (YYYY-MM-DD)
    start_date: Option<String>,
    /// Filter by end date (YYYY-MM-DD)
    end_date: Option<String>,
}

// Error conversion helper
fn convert_error(err: ToolboxError) -> McpError {
    match err {
        ToolboxError::NotFound(msg) => McpError::invalid_params(msg, None),
        ToolboxError::Validation(msg) => McpError::invalid_params(msg, None),
        ToolboxError::Database(msg) => McpError::internal_error(msg, None),
        ToolboxError::Configuration(msg) => McpError::internal_error(msg, None),
        _ => McpError::internal_error(err.to_string(), None),
    }
}

// Implement ServerHandler
impl ServerHandler for NutritionMcpServer {
    fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<InitializeResult, McpError>> + Send + '_ {
        async move {
            Ok(InitializeResult {
                protocol_version: ProtocolVersion::V_2024_11_05,
                capabilities: ServerCapabilities {
                    tools: Some(ToolsCapability { list_changed: None }),
                    ..Default::default()
                },
                server_info: Implementation {
                    name: "nutrition-mcp-server".into(),
                    version: "0.1.0".into(),
                    icons: None,
                    title: Some("Nutrition & Recipe Management".into()),
                    website_url: None,
                },
                instructions: Some("Nutrition database management system. Manage ingredients, recipes, and nutritional information.".into()),
            })
        }
    }

    fn on_initialized(
        &self,
        _context: rmcp::service::NotificationContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        async move {}
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        let tool_router = self.tool_router.clone();
        let server = self.clone();
        async move {
            let tool_call_context =
                rmcp::handler::server::tool::ToolCallContext::new(&server, request, context);
            tool_router.call(tool_call_context).await
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = self.tool_router.list_all();
        async move {
            Ok(ListToolsResult {
                tools,
                next_cursor: None,
            })
        }
    }
}

