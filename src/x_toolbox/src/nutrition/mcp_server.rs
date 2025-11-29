use crate::error::ToolboxError;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters, ServerHandler},
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
    pub tool_router: ToolRouter<Self>,
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

    /// Get all available tools (public API for OpenAPI server)
    pub fn list_all_tools(&self) -> Vec<Tool> {
        self.tool_router.list_all()
    }

    // ========== Consolidated Ingredient Management ==========

    /// Manage ingredients: create, update, delete, or add nutritional info
    #[tool(
        description = "Manage ingredients. Action: 'create' (name, description), 'update' (id, name?, description?), 'delete' (id), or 'add_nutrition' (ingredient_id, calories_per_100g, protein_g, carbs_g, fat_g, fiber_g?, sugar_g?)."
    )]
    async fn manage_ingredient(
        &self,
        params: Parameters<ManageIngredientParams>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.action.as_str() {
            "create" => {
                let ingredient = NutritionService::create_ingredient(
                    &self.pool,
                    &params.0.name.ok_or_else(|| {
                        McpError::invalid_params("name is required for create action", None)
                    })?,
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
            "update" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for update action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

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
            "delete" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for delete action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                NutritionService::delete_ingredient(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted ingredient: {}",
                    id
                ))]))
            }
            "add_nutrition" => {
                let ingredient_id = params.0.ingredient_id.ok_or_else(|| {
                    McpError::invalid_params("ingredient_id is required for add_nutrition action", None)
                })?;
                let uuid = Uuid::parse_str(&ingredient_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let nutritional_info = NutritionService::upsert_nutritional_info(
                    &self.pool,
                    uuid,
                    params.0.calories_per_100g.ok_or_else(|| {
                        McpError::invalid_params("calories_per_100g is required", None)
                    })?
                    .to_string()
                    .parse()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid calories value: {}", e), None)
                    })?,
                    params.0.protein_g.ok_or_else(|| {
                        McpError::invalid_params("protein_g is required", None)
                    })?
                    .to_string()
                    .parse()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid protein value: {}", e), None)
                    })?,
                    params.0.carbs_g.ok_or_else(|| {
                        McpError::invalid_params("carbs_g is required", None)
                    })?
                    .to_string()
                    .parse()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid carbs value: {}", e), None)
                    })?,
                    params.0.fat_g.ok_or_else(|| {
                        McpError::invalid_params("fat_g is required", None)
                    })?
                    .to_string()
                    .parse()
                    .map_err(|e| {
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
            _ => Err(McpError::invalid_params(
                format!("Unknown action: {}. Valid actions: create, update, delete, add_nutrition", params.0.action),
                None,
            )),
        }
    }

    /// Query ingredients: search, get by ID, or batch get
    #[tool(
        description = "Query ingredients. Use 'search' with search_term, 'get' with id, or 'batch' with ids array."
    )]
    async fn query_ingredients(
        &self,
        params: Parameters<QueryIngredientsParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        match params.0.query_type.as_str() {
            "search" => {
                let ingredients = NutritionService::list_ingredients(
                    &self.pool,
                    params.0.search_term.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                let mut output = format!("Found {} ingredient(s):\n\n", ingredients.len());
                for ing in ingredients {
                    output.push_str(&format!(
                        "- {} ({})\n  Description: {}\n",
                        ing.name,
                        ing.id,
                        ing.description.unwrap_or_else(|| "None".to_string())
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "get" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for get query", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let ingredient = NutritionService::get_ingredient(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Ingredient: {}\nID: {}\nDescription: {}\nCreated: {}",
                    ingredient.name,
                    ingredient.id,
                    ingredient.description.unwrap_or_else(|| "None".to_string()),
                    ingredient.created_at
                ))]))
            }
            "batch" => {
                let ids = params.0.ids.ok_or_else(|| {
                    McpError::invalid_params("ids array is required for batch query", None)
                })?;

                let uuids: Result<Vec<Uuid>, McpError> = ids
                    .into_iter()
                    .map(|id| {
                        Uuid::parse_str(&id).map_err(|e| {
                            McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                        })
                    })
                    .collect();

                let uuids = uuids?;
                let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                    let pool = &self.pool;
                    async move {
                        NutritionService::get_ingredient(pool, id)
                            .await
                            .map_err(convert_error)
                    }
                }))
                .await;

                let mut output = format!("Batch get ingredients ({} requested):\n\n", results.len());
                let mut success_count = 0;
                let mut error_count = 0;

                for (idx, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(ingredient) => {
                            success_count += 1;
                            output.push_str(&format!(
                                "[{}] Ingredient: {}\n  ID: {}\n  Description: {}\n\n",
                                idx + 1,
                                ingredient.name,
                                ingredient.id,
                                ingredient.description.unwrap_or_else(|| "None".to_string())
                            ));
                        }
                        Err(e) => {
                            error_count += 1;
                            output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                        }
                    }
                }

                output.push_str(&format!(
                    "Summary: {} succeeded, {} failed",
                    success_count, error_count
                ));
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown query_type: {}. Valid types: search, get, batch", params.0.query_type),
                None,
            )),
        }
    }

    // ========== Consolidated Recipe Management ==========

    /// Manage recipes: create, update, or delete
    #[tool(
        description = "Manage recipes. Action: 'create' (name, description?, servings?, prep_time_minutes?, cook_time_minutes?, ingredients[], steps[]), 'update' (id, name?, description?, servings?, prep_time_minutes?, cook_time_minutes?), or 'delete' (id)."
    )]
    async fn manage_recipe(
        &self,
        params: Parameters<ManageRecipeParams>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.action.as_str() {
            "create" => {
                let name = params.0.name.ok_or_else(|| {
                    McpError::invalid_params("name is required for create action", None)
                })?;

                let ingredients: Result<Vec<(Uuid, BigDecimal, String)>, McpError> = params
                    .0
                    .ingredients
                    .unwrap_or_default()
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
                    .unwrap_or_default()
                    .into_iter()
                    .enumerate()
                    .map(|(i, instruction)| ((i + 1) as i32, instruction))
                    .collect();

                let recipe = NutritionService::create_recipe(
                    &self.pool,
                    &name,
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
            "update" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for update action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

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
            "delete" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for delete action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                NutritionService::delete_recipe(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted recipe: {}",
                    id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown action: {}. Valid actions: create, update, delete", params.0.action),
                None,
            )),
        }
    }

    /// Manage recipe content: add/remove ingredients or steps
    #[tool(
        description = "Manage recipe content. Action: 'add_ingredient' (recipe_id, ingredient_id, quantity, unit), 'remove_ingredient' (recipe_id, ingredient_id), 'add_step' (recipe_id, step_number, instruction), or 'remove_step' (recipe_id, step_number)."
    )]
    async fn manage_recipe_content(
        &self,
        params: Parameters<ManageRecipeContentParams>,
    ) -> Result<CallToolResult, McpError> {
        let recipe_id = params.0.recipe_id.ok_or_else(|| {
            McpError::invalid_params("recipe_id is required", None)
        })?;
        let recipe_uuid = Uuid::parse_str(&recipe_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None))?;

        match params.0.action.as_str() {
            "add_ingredient" => {
                let ingredient_id = params.0.ingredient_id.ok_or_else(|| {
                    McpError::invalid_params("ingredient_id is required for add_ingredient", None)
                })?;
                let ingredient_uuid = Uuid::parse_str(&ingredient_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None)
                })?;
                let quantity: BigDecimal = params.0.quantity.ok_or_else(|| {
                    McpError::invalid_params("quantity is required for add_ingredient", None)
                })?
                .to_string()
                .parse()
                .map_err(|e| McpError::invalid_params(format!("Invalid quantity: {}", e), None))?;
                let unit = params.0.unit.ok_or_else(|| {
                    McpError::invalid_params("unit is required for add_ingredient", None)
                })?;

                let recipe_ingredient = NutritionService::add_recipe_ingredient(
                    &self.pool,
                    recipe_uuid,
                    ingredient_uuid,
                    quantity,
                    unit,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Added ingredient {} to recipe {}\nQuantity: {} {}",
                    ingredient_id, recipe_id, recipe_ingredient.quantity, recipe_ingredient.unit
                ))]))
            }
            "remove_ingredient" => {
                let ingredient_id = params.0.ingredient_id.ok_or_else(|| {
                    McpError::invalid_params("ingredient_id is required for remove_ingredient", None)
                })?;
                let ingredient_uuid = Uuid::parse_str(&ingredient_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None)
                })?;

                NutritionService::remove_recipe_ingredient(&self.pool, recipe_uuid, ingredient_uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Removed ingredient {} from recipe {}",
                    ingredient_id, recipe_id
                ))]))
            }
            "add_step" => {
                let step_number = params.0.step_number.ok_or_else(|| {
                    McpError::invalid_params("step_number is required for add_step", None)
                })?;
                let instruction = params.0.instruction.ok_or_else(|| {
                    McpError::invalid_params("instruction is required for add_step", None)
                })?;

                let step = NutritionService::add_recipe_step(
                    &self.pool,
                    recipe_uuid,
                    step_number,
                    instruction,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Added step {} to recipe {}: {}",
                    step.step_number, recipe_id, step.instruction
                ))]))
            }
            "remove_step" => {
                let step_number = params.0.step_number.ok_or_else(|| {
                    McpError::invalid_params("step_number is required for remove_step", None)
                })?;

                NutritionService::remove_recipe_step(&self.pool, recipe_uuid, step_number)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Removed step {} from recipe {}",
                    step_number, recipe_id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown action: {}. Valid actions: add_ingredient, remove_ingredient, add_step, remove_step", params.0.action),
                None,
            )),
        }
    }

    /// Query recipes: search, get by ID, batch get, or extract from URL
    #[tool(
        description = "Query recipes. Use 'search' with search_term and optional ingredient_id, 'get' with id and optional full=true, 'batch' with ids array and optional full=true, or 'extract' with url."
    )]
    async fn query_recipes(
        &self,
        params: Parameters<QueryRecipesParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        match params.0.query_type.as_str() {
            "search" => {
                let ingredient_uuid = params
                    .0
                    .ingredient_id
                    .map(|id| Uuid::parse_str(&id))
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None)
                    })?;

                let recipes = NutritionService::list_recipes(
                    &self.pool,
                    params.0.search_term.as_deref(),
                    ingredient_uuid,
                )
                .await
                .map_err(convert_error)?;

                let mut output = format!("Found {} recipe(s):\n\n", recipes.len());
                for recipe in recipes {
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
            "get" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for get query", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                if params.0.full.unwrap_or(false) {
                    let recipe = NutritionService::get_recipe_with_details(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    let mut output = format!(
                        "Recipe: {}\nID: {}\nDescription: {}\nServings: {:?}\nPrep: {:?} min, Cook: {:?} min\n\nIngredients:\n",
                        recipe.recipe.name,
                        recipe.recipe.id,
                        recipe.recipe.description.unwrap_or_else(|| "None".to_string()),
                        recipe.recipe.servings,
                        recipe.recipe.prep_time_minutes,
                        recipe.recipe.cook_time_minutes
                    );
                    for ing in &recipe.ingredients {
                        output.push_str(&format!(
                            "  - {} {} of {} ({})\n",
                            ing.recipe_ingredient.quantity,
                            ing.recipe_ingredient.unit,
                            ing.ingredient.name,
                            ing.ingredient.id
                        ));
                    }
                    output.push_str("\nSteps:\n");
                    for step in &recipe.steps {
                        output.push_str(&format!(
                            "  {}. {}\n",
                            step.step_number, step.instruction
                        ));
                    }
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    let recipe = NutritionService::get_recipe(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    Ok(CallToolResult::success(vec![Content::text(format!(
                        "Recipe: {}\nID: {}\nDescription: {}\nCreated: {}",
                        recipe.name,
                        recipe.id,
                        recipe.description.unwrap_or_else(|| "None".to_string()),
                        recipe.created_at
                    ))]))
                }
            }
            "batch" => {
                let ids = params.0.ids.ok_or_else(|| {
                    McpError::invalid_params("ids array is required for batch query", None)
                })?;

                let uuids: Result<Vec<Uuid>, McpError> = ids
                    .into_iter()
                    .map(|id| {
                        Uuid::parse_str(&id).map_err(|e| {
                            McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                        })
                    })
                    .collect();

                let uuids = uuids?;
                let full = params.0.full.unwrap_or(false);

                if full {
                    let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                        let pool = &self.pool;
                        async move {
                            NutritionService::get_recipe_with_details(pool, id)
                                .await
                                .map_err(convert_error)
                        }
                    }))
                    .await;

                    let mut output = format!(
                        "Batch get recipes with full details ({} requested):\n\n",
                        results.len()
                    );
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
                                    output.push_str(&format!(
                                        "    {}. {}\n",
                                        step.step_number, step.instruction
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

                    output.push_str(&format!(
                        "Summary: {} succeeded, {} failed",
                        success_count, error_count
                    ));
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                        let pool = &self.pool;
                        async move {
                            NutritionService::get_recipe(pool, id)
                                .await
                                .map_err(convert_error)
                        }
                    }))
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

                    output.push_str(&format!(
                        "Summary: {} succeeded, {} failed",
                        success_count, error_count
                    ));
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                }
            }
            "extract" => {
                let url = params.0.url.ok_or_else(|| {
                    McpError::invalid_params("url is required for extract query", None)
                })?;

                let extracted = NutritionService::extract_recipe_from_url(&url)
                    .await
                    .map_err(convert_error)?;

                let mut output = format!("Extracted Recipe: {}\n", extracted.name);

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
                    let qty_str = ing.quantity
                        .map(|q| q.to_string())
                        .unwrap_or_else(|| "?".to_string());
                    let unit_str = ing.unit.as_deref().unwrap_or("?");
                    output.push_str(&format!("  - {} {} {}\n", qty_str, unit_str, ing.name));
                }

                output.push_str("\nSteps:\n");
                for (i, step) in extracted.steps.iter().enumerate() {
                    output.push_str(&format!("  {}. {}\n", i + 1, step));
                }

                let json_output = serde_json::to_string_pretty(&extracted)
                    .unwrap_or_else(|_| "Failed to serialize recipe".to_string());

                Ok(CallToolResult::success(vec![
                    Content::text(output),
                    Content::text(format!("\nJSON representation:\n{}", json_output)),
                ]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown query_type: {}. Valid types: search, get, batch, extract", params.0.query_type),
                None,
            )),
        }
    }

    // ========== Nutrition Calculation ==========

    /// Calculate nutritional information for recipes or meal plans
    #[tool(
        description = "Calculate nutritional information. Type: 'recipe' (id, servings?) or 'meal_plan' (id). For batch calculations, use ids array."
    )]
    async fn calculate_nutrition(
        &self,
        params: Parameters<CalculateNutritionParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        match params.0.calc_type.as_str() {
            "recipe" => {
                if let Some(ids) = params.0.ids {
                    // Batch calculation
                    let uuids: Result<Vec<Uuid>, McpError> = ids
                        .into_iter()
                        .map(|id| {
                            Uuid::parse_str(&id).map_err(|e| {
                                McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                            })
                        })
                        .collect();

                    let uuids = uuids?;
                    let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                        let pool = &self.pool;
                        async move {
                            NutritionService::calculate_recipe_nutrition(pool, id, params.0.servings)
                                .await
                                .map_err(convert_error)
                        }
                    }))
                    .await;

                    let mut output = format!(
                        "Batch calculate nutrition ({} requested):\n\n",
                        results.len()
                    );
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

                    output.push_str(&format!(
                        "Summary: {} succeeded, {} failed",
                        success_count, error_count
                    ));
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    // Single calculation
                    let id = params.0.id.ok_or_else(|| {
                        McpError::invalid_params("id is required for single recipe calculation", None)
                    })?;
                    let uuid = Uuid::parse_str(&id)
                        .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                    let nutrition =
                        NutritionService::calculate_recipe_nutrition(&self.pool, uuid, params.0.servings)
                            .await
                            .map_err(convert_error)?;

                    let mut output = format!(
                        "Nutritional Information for Recipe {}\n\nTotal:\n  Calories: {}\n  Protein: {}g\n  Carbs: {}g\n  Fat: {}g",
                        id,
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

                        if let Some(s) = params.0.servings {
                            let total_cal = &cal_per_serving * BigDecimal::from(s);
                            let total_protein = nutrition
                                .per_serving_protein_g
                                .as_ref()
                                .map(|p| p * BigDecimal::from(s));
                            let total_carbs = nutrition
                                .per_serving_carbs_g
                                .as_ref()
                                .map(|c| c * BigDecimal::from(s));
                            let total_fat = nutrition
                                .per_serving_fat_g
                                .as_ref()
                                .map(|f| f * BigDecimal::from(s));

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
            }
            "meal_plan" => {
                let ids = params.0.ids.or_else(|| params.0.id.map(|id| vec![id]));

                if let Some(ids) = ids {
                    // Batch calculation
                    let uuids: Result<Vec<Uuid>, McpError> = ids
                        .into_iter()
                        .map(|id| {
                            Uuid::parse_str(&id).map_err(|e| {
                                McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                            })
                        })
                        .collect();

                    let uuids = uuids?;
                    let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                        let pool = &self.pool;
                        async move {
                            NutritionService::calculate_meal_plan_nutrition(pool, id)
                                .await
                                .map_err(convert_error)
                        }
                    }))
                    .await;

                    let mut output = format!(
                        "Batch calculate meal plan nutrition ({} requested):\n\n",
                        results.len()
                    );
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
                                        weekly.total_calories, weekly.average_daily_calories
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

                    output.push_str(&format!(
                        "Summary: {} succeeded, {} failed",
                        success_count, error_count
                    ));
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    Err(McpError::invalid_params(
                        "id or ids is required for meal_plan calculation",
                        None,
                    ))
                }
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown calc_type: {}. Valid types: recipe, meal_plan", params.0.calc_type),
                None,
            )),
        }
    }

    // ========== Consolidated Meal Plan Management ==========

    /// Manage meal plans: create, update, delete, or manage entries
    #[tool(
        description = "Manage meal plans. Action: 'create' (name, description?, start_date?, end_date?, is_template), 'update' (id, name?, description?, start_date?, end_date?), 'delete' (id), 'add_entry' (meal_plan_id, recipe_id, meal_type, day_of_week?, date?), or 'remove_entry' (entry_id)."
    )]
    async fn manage_meal_plan(
        &self,
        params: Parameters<ManageMealPlanParams>,
    ) -> Result<CallToolResult, McpError> {
        use chrono::NaiveDate;

        match params.0.action.as_str() {
            "create" => {
                let name = params.0.name.ok_or_else(|| {
                    McpError::invalid_params("name is required for create action", None)
                })?;

                let start_date = params
                    .0
                    .start_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid start_date format: {}", e), None)
                    })?;
                let end_date = params
                    .0
                    .end_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid end_date format: {}", e), None)
                    })?;

                let meal_plan = NutritionService::create_meal_plan(
                    &self.pool,
                    &name,
                    params.0.description.as_deref(),
                    start_date,
                    end_date,
                    params.0.is_template.unwrap_or(false),
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
            "update" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for update action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let start_date = params
                    .0
                    .start_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid start_date format: {}", e), None)
                    })?;
                let end_date = params
                    .0
                    .end_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid end_date format: {}", e), None)
                    })?;

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
            "delete" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for delete action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                NutritionService::delete_meal_plan(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted meal plan: {}",
                    id
                ))]))
            }
            "add_entry" => {
                let meal_plan_id = params.0.meal_plan_id.ok_or_else(|| {
                    McpError::invalid_params("meal_plan_id is required for add_entry", None)
                })?;
                let meal_plan_uuid = Uuid::parse_str(&meal_plan_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid meal plan UUID: {}", e), None)
                })?;
                let recipe_id = params.0.recipe_id.ok_or_else(|| {
                    McpError::invalid_params("recipe_id is required for add_entry", None)
                })?;
                let recipe_uuid = Uuid::parse_str(&recipe_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None))?;

                let date = params
                    .0
                    .date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| McpError::invalid_params(format!("Invalid date format: {}", e), None))?;

                let meal_type = params.0.meal_type.ok_or_else(|| {
                    McpError::invalid_params("meal_type is required for add_entry", None)
                })?;

                let entry = NutritionService::add_meal_plan_entry(
                    &self.pool,
                    meal_plan_uuid,
                    recipe_uuid,
                    &meal_type,
                    params.0.day_of_week,
                    date,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Added entry to meal plan {}\nEntry ID: {}\nMeal type: {}\nRecipe ID: {}",
                    meal_plan_id, entry.id, entry.meal_type, entry.recipe_id
                ))]))
            }
            "remove_entry" => {
                let entry_id = params.0.entry_id.ok_or_else(|| {
                    McpError::invalid_params("entry_id is required for remove_entry", None)
                })?;
                let uuid = Uuid::parse_str(&entry_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                NutritionService::remove_meal_plan_entry(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Removed meal plan entry: {}",
                    entry_id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown action: {}. Valid actions: create, update, delete, add_entry, remove_entry", params.0.action),
                None,
            )),
        }
    }

    /// Query meal plans: search, get by ID, or batch get
    #[tool(
        description = "Query meal plans. Use 'search' with optional search_term, is_template, start_date, end_date, or 'get'/'batch' with id/ids and optional full=true."
    )]
    async fn query_meal_plans(
        &self,
        params: Parameters<QueryMealPlansParams>,
    ) -> Result<CallToolResult, McpError> {
        use chrono::NaiveDate;
        use futures::future::join_all;

        match params.0.query_type.as_str() {
            "search" => {
                let start_date = params
                    .0
                    .start_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid start_date format: {}", e), None)
                    })?;
                let end_date = params
                    .0
                    .end_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid end_date format: {}", e), None)
                    })?;

                let meal_plans = NutritionService::list_meal_plans(
                    &self.pool,
                    params.0.search_term.as_deref(),
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
                        meal_plan.name, meal_plan.id, meal_plan.is_template
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
            "get" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for get query", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                if params.0.full.unwrap_or(false) {
                    let meal_plan = NutritionService::get_meal_plan_with_entries(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    let mut output = format!(
                        "Meal Plan: {}\nID: {}\nTemplate: {}\nEntries: {}\n\n",
                        meal_plan.meal_plan.name,
                        meal_plan.meal_plan.id,
                        meal_plan.meal_plan.is_template,
                        meal_plan.entries.len()
                    );

                    for entry in &meal_plan.entries {
                        let day_info = if let Some(date) = entry.entry.date {
                            format!("{}", date)
                        } else if let Some(dow) = entry.entry.day_of_week {
                            let day_name = match dow {
                                0 => "Monday",
                                1 => "Tuesday",
                                2 => "Wednesday",
                                3 => "Thursday",
                                4 => "Friday",
                                5 => "Saturday",
                                6 => "Sunday",
                                _ => "Unknown",
                            };
                            day_name.to_string()
                        } else {
                            "No date/day".to_string()
                        };

                        output.push_str(&format!(
                            "  - {}: {} - {}\n",
                            day_info, entry.entry.meal_type, entry.recipe.name
                        ));
                    }

                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    let meal_plan = NutritionService::get_meal_plan(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    Ok(CallToolResult::success(vec![Content::text(format!(
                        "Meal Plan: {}\nID: {}\nTemplate: {}\n",
                        meal_plan.name, meal_plan.id, meal_plan.is_template
                    ))]))
                }
            }
            "batch" => {
                let ids = params.0.ids.ok_or_else(|| {
                    McpError::invalid_params("ids array is required for batch query", None)
                })?;

                let uuids: Result<Vec<Uuid>, McpError> = ids
                    .into_iter()
                    .map(|id| {
                        Uuid::parse_str(&id).map_err(|e| {
                            McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                        })
                    })
                    .collect();

                let uuids = uuids?;
                let full = params.0.full.unwrap_or(false);

                if full {
                    let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                        let pool = &self.pool;
                        async move {
                            NutritionService::get_meal_plan_with_entries(pool, id)
                                .await
                                .map_err(convert_error)
                        }
                    }))
                    .await;

                    let mut output = format!(
                        "Batch get meal plans with entries ({} requested):\n\n",
                        results.len()
                    );
                    let mut success_count = 0;
                    let mut error_count = 0;

                    for (idx, result) in results.into_iter().enumerate() {
                        match result {
                            Ok(meal_plan) => {
                                success_count += 1;
                                output.push_str(&format!(
                                    "[{}] Meal Plan: {}\n  ID: {}\n  Template: {}\n  Entries: {}\n",
                                    idx + 1,
                                    meal_plan.meal_plan.name,
                                    meal_plan.meal_plan.id,
                                    meal_plan.meal_plan.is_template,
                                    meal_plan.entries.len()
                                ));

                                for entry in &meal_plan.entries {
                                    let day_info = if let Some(date) = entry.entry.date {
                                        format!("{}", date)
                                    } else if let Some(dow) = entry.entry.day_of_week {
                                        let day_name = match dow {
                                            0 => "Monday",
                                            1 => "Tuesday",
                                            2 => "Wednesday",
                                            3 => "Thursday",
                                            4 => "Friday",
                                            5 => "Saturday",
                                            6 => "Sunday",
                                            _ => "Unknown",
                                        };
                                        day_name.to_string()
                                    } else {
                                        "No date/day".to_string()
                                    };

                                    output.push_str(&format!(
                                        "    - {}: {} - {}\n",
                                        day_info, entry.entry.meal_type, entry.recipe.name
                                    ));
                                }
                                output.push_str("\n");
                            }
                            Err(e) => {
                                error_count += 1;
                                output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                            }
                        }
                    }

                    output.push_str(&format!(
                        "Summary: {} succeeded, {} failed",
                        success_count, error_count
                    ));
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                        let pool = &self.pool;
                        async move {
                            NutritionService::get_meal_plan(pool, id)
                                .await
                                .map_err(convert_error)
                        }
                    }))
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

                    output.push_str(&format!(
                        "Summary: {} succeeded, {} failed",
                        success_count, error_count
                    ));
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                }
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown query_type: {}. Valid types: search, get, batch", params.0.query_type),
                None,
            )),
        }
    }

    // ========== Consolidated Family Member Management ==========

    /// Manage family members: create, update, or delete
    #[tool(
        description = "Manage family members. Action: 'create' (name, preferences?), 'update' (id, name?, preferences?), or 'delete' (id)."
    )]
    async fn manage_family_member(
        &self,
        params: Parameters<ManageFamilyMemberParams>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.action.as_str() {
            "create" => {
                let name = params.0.name.ok_or_else(|| {
                    McpError::invalid_params("name is required for create action", None)
                })?;

                let preferences = params.0.preferences.map(|p| serde_json::json!(p));
                let family_member =
                    NutritionService::create_family_member(&self.pool, &name, preferences)
                        .await
                        .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Created family member: {} (ID: {})",
                    family_member.name, family_member.id
                ))]))
            }
            "update" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for update action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let preferences = params.0.preferences.map(|p| serde_json::json!(p));
                let family_member = NutritionService::update_family_member(
                    &self.pool,
                    uuid,
                    params.0.name.as_deref(),
                    preferences,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Updated family member: {} ({})",
                    family_member.name, family_member.id
                ))]))
            }
            "delete" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for delete action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                NutritionService::delete_family_member(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted family member: {}",
                    id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown action: {}. Valid actions: create, update, delete", params.0.action),
                None,
            )),
        }
    }

    /// Query family members: search, get by ID, batch get, or list all
    #[tool(
        description = "Query family members. Use 'search' with optional search_term to search by name, 'get' with id for a single family member (optionally with_allergies=true), 'batch' with ids array for multiple family members (optionally with_allergies=true), or 'list' to get all. IMPORTANT: If you need to get multiple family members by their IDs, use 'batch' with an ids array rather than calling 'get' multiple times."
    )]
    async fn query_family_members(
        &self,
        params: Parameters<QueryFamilyMembersParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        match params.0.query_type.as_str() {
            "search" | "list" => {
                let family_members =
                    NutritionService::list_family_members(&self.pool, params.0.search_term.as_deref())
                        .await
                        .map_err(convert_error)?;

                if family_members.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "No family members found.",
                    )]));
                }

                let mut output = format!("Found {} family member(s):\n\n", family_members.len());
                for fm in family_members {
                    output.push_str(&format!("- {} (ID: {})\n", fm.name, fm.id));
                }

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "get" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for get query", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                if params.0.with_allergies.unwrap_or(false) {
                    let family_member = NutritionService::get_family_member_with_allergies(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    let mut output = format!(
                        "Family Member: {}\nID: {}\n\n",
                        family_member.family_member.name, family_member.family_member.id
                    );

                    if family_member.allergies.is_empty() {
                        output.push_str("No allergies recorded.\n");
                    } else {
                        output.push_str(&format!("Allergies ({}):\n", family_member.allergies.len()));
                        for allergy in family_member.allergies {
                            output.push_str(&format!(
                                "- {} (severity: {})\n",
                                allergy.ingredient.name,
                                allergy.allergy.severity.as_deref().unwrap_or("unknown")
                            ));
                            if let Some(notes) = allergy.allergy.notes {
                                output.push_str(&format!("  Notes: {}\n", notes));
                            }
                        }
                    }

                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    let family_member = NutritionService::get_family_member(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    let mut output = format!(
                        "Family Member: {}\nID: {}\n",
                        family_member.name, family_member.id
                    );
                    if let Some(prefs) = family_member.preferences {
                        output.push_str(&format!(
                            "Preferences: {}\n",
                            serde_json::to_string_pretty(&prefs).unwrap_or_default()
                        ));
                    }

                    Ok(CallToolResult::success(vec![Content::text(output)]))
                }
            }
            "batch" => {
                let ids = params.0.ids.ok_or_else(|| {
                    McpError::invalid_params("ids array is required for batch query", None)
                })?;

                let uuids: Result<Vec<Uuid>, McpError> = ids
                    .into_iter()
                    .map(|id| {
                        Uuid::parse_str(&id).map_err(|e| {
                            McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                        })
                    })
                    .collect();

                let uuids = uuids?;
                let with_allergies = params.0.with_allergies.unwrap_or(false);

                if with_allergies {
                    let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                        let pool = &self.pool;
                        async move {
                            NutritionService::get_family_member_with_allergies(pool, id)
                                .await
                                .map_err(convert_error)
                        }
                    }))
                    .await;

                    let mut output = format!(
                        "Batch get family members with allergies ({} requested):\n\n",
                        results.len()
                    );
                    let mut success_count = 0;
                    let mut error_count = 0;

                    for (idx, result) in results.into_iter().enumerate() {
                        match result {
                            Ok(family_member) => {
                                success_count += 1;
                                output.push_str(&format!(
                                    "[{}] Family Member: {}\n  ID: {}\n",
                                    idx + 1,
                                    family_member.family_member.name,
                                    family_member.family_member.id
                                ));

                                if family_member.allergies.is_empty() {
                                    output.push_str("  No allergies recorded.\n");
                                } else {
                                    output.push_str(&format!("  Allergies ({}):\n", family_member.allergies.len()));
                                    for allergy in family_member.allergies {
                                        output.push_str(&format!(
                                            "    - {} (severity: {})\n",
                                            allergy.ingredient.name,
                                            allergy.allergy.severity.as_deref().unwrap_or("unknown")
                                        ));
                                        if let Some(notes) = allergy.allergy.notes {
                                            output.push_str(&format!("      Notes: {}\n", notes));
                                        }
                                    }
                                }
                                output.push('\n');
                            }
                            Err(e) => {
                                error_count += 1;
                                output.push_str(&format!("[{}] Error: {}\n\n", idx + 1, e));
                            }
                        }
                    }

                    output.push_str(&format!(
                        "Summary: {} succeeded, {} failed",
                        success_count, error_count
                    ));
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                        let pool = &self.pool;
                        async move {
                            NutritionService::get_family_member(pool, id)
                                .await
                                .map_err(convert_error)
                        }
                    }))
                    .await;

                    let mut output = format!(
                        "Batch get family members ({} requested):\n\n",
                        results.len()
                    );
                    let mut success_count = 0;
                    let mut error_count = 0;

                    for (idx, result) in results.into_iter().enumerate() {
                        match result {
                            Ok(family_member) => {
                                success_count += 1;
                                output.push_str(&format!(
                                    "[{}] Family Member: {}\n  ID: {}\n",
                                    idx + 1,
                                    family_member.name,
                                    family_member.id
                                ));
                                if let Some(prefs) = family_member.preferences {
                                    output.push_str(&format!(
                                        "  Preferences: {}\n",
                                        serde_json::to_string_pretty(&prefs).unwrap_or_default()
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

                    output.push_str(&format!(
                        "Summary: {} succeeded, {} failed",
                        success_count, error_count
                    ));
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                }
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown query_type: {}. Valid types: search, list, get, batch", params.0.query_type),
                None,
            )),
        }
    }

    /// Manage family member data: allergies and favorites
    #[tool(
        description = "Manage family member data. Action: 'add_allergy' (family_member_id, ingredient_id, severity?, notes?), 'remove_allergy' (family_member_id, ingredient_id), 'add_favorite' (family_member_id, recipe_id, notes?), 'remove_favorite' (family_member_id, recipe_id), 'get_favorites' (family_member_id), or 'get_favorited_by' (recipe_id)."
    )]
    async fn manage_family_member_data(
        &self,
        params: Parameters<ManageFamilyMemberDataParams>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.action.as_str() {
            "add_allergy" => {
                let family_member_id = params.0.family_member_id.ok_or_else(|| {
                    McpError::invalid_params("family_member_id is required for add_allergy", None)
                })?;
                let family_member_uuid = Uuid::parse_str(&family_member_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid family member UUID: {}", e), None)
                })?;
                let ingredient_id = params.0.ingredient_id.ok_or_else(|| {
                    McpError::invalid_params("ingredient_id is required for add_allergy", None)
                })?;
                let ingredient_uuid = Uuid::parse_str(&ingredient_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None)
                })?;

                let allergy = NutritionService::add_family_member_allergy(
                    &self.pool,
                    family_member_uuid,
                    ingredient_uuid,
                    params.0.severity.as_deref(),
                    params.0.notes.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Added allergy for family member {} (ID: {})",
                    family_member_id, allergy.id
                ))]))
            }
            "remove_allergy" => {
                let family_member_id = params.0.family_member_id.ok_or_else(|| {
                    McpError::invalid_params("family_member_id is required for remove_allergy", None)
                })?;
                let family_member_uuid = Uuid::parse_str(&family_member_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid family member UUID: {}", e), None)
                })?;
                let ingredient_id = params.0.ingredient_id.ok_or_else(|| {
                    McpError::invalid_params("ingredient_id is required for remove_allergy", None)
                })?;
                let ingredient_uuid = Uuid::parse_str(&ingredient_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None)
                })?;

                NutritionService::remove_family_member_allergy(&self.pool, family_member_uuid, ingredient_uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text("Removed allergy")]))
            }
            "add_favorite" => {
                let family_member_id = params.0.family_member_id.ok_or_else(|| {
                    McpError::invalid_params("family_member_id is required for add_favorite", None)
                })?;
                let family_member_uuid = Uuid::parse_str(&family_member_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid family member UUID: {}", e), None)
                })?;
                let recipe_id = params.0.recipe_id.ok_or_else(|| {
                    McpError::invalid_params("recipe_id is required for add_favorite", None)
                })?;
                let recipe_uuid = Uuid::parse_str(&recipe_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None))?;

                let favorite = NutritionService::add_recipe_favorite(
                    &self.pool,
                    family_member_uuid,
                    recipe_uuid,
                    params.0.notes.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Added recipe to favorites (ID: {})",
                    favorite.id
                ))]))
            }
            "remove_favorite" => {
                let family_member_id = params.0.family_member_id.ok_or_else(|| {
                    McpError::invalid_params("family_member_id is required for remove_favorite", None)
                })?;
                let family_member_uuid = Uuid::parse_str(&family_member_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid family member UUID: {}", e), None)
                })?;
                let recipe_id = params.0.recipe_id.ok_or_else(|| {
                    McpError::invalid_params("recipe_id is required for remove_favorite", None)
                })?;
                let recipe_uuid = Uuid::parse_str(&recipe_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None))?;

                NutritionService::remove_recipe_favorite(&self.pool, family_member_uuid, recipe_uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text("Removed recipe from favorites")]))
            }
            "get_favorites" => {
                let family_member_id = params.0.family_member_id.ok_or_else(|| {
                    McpError::invalid_params("family_member_id is required for get_favorites", None)
                })?;
                let family_member_uuid = Uuid::parse_str(&family_member_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid family member UUID: {}", e), None)
                })?;

                let favorites = NutritionService::get_family_member_favorites(&self.pool, family_member_uuid)
                    .await
                    .map_err(convert_error)?;

                if favorites.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "No favorite recipes found.",
                    )]));
                }

                let mut output = format!("Favorite recipes ({}):\n\n", favorites.len());
                for fav in favorites {
                    output.push_str(&format!("- {} (ID: {})\n", fav.recipe.name, fav.recipe.id));
                    if let Some(notes) = fav.favorite.notes {
                        output.push_str(&format!("  Notes: {}\n", notes));
                    }
                }

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "get_favorited_by" => {
                let recipe_id = params.0.recipe_id.ok_or_else(|| {
                    McpError::invalid_params("recipe_id is required for get_favorited_by", None)
                })?;
                let recipe_uuid = Uuid::parse_str(&recipe_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None))?;

                let family_members = NutritionService::get_recipe_favorited_by(&self.pool, recipe_uuid)
                    .await
                    .map_err(convert_error)?;

                if family_members.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "No family members have favorited this recipe.",
                    )]));
                }

                let mut output = format!(
                    "Family members who favorited this recipe ({}):\n\n",
                    family_members.len()
                );
                for fm in family_members {
                    output.push_str(&format!("- {} (ID: {})\n", fm.name, fm.id));
                }

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown action: {}. Valid actions: add_allergy, remove_allergy, add_favorite, remove_favorite, get_favorites, get_favorited_by", params.0.action),
                None,
            )),
        }
    }

    /// Check if a recipe contains allergens for a family member
    #[tool(description = "Check if a recipe contains any allergens for a specific family member")]
    async fn check_allergens(
        &self,
        params: Parameters<CheckAllergensParams>,
    ) -> Result<CallToolResult, McpError> {
        let family_member_id = params.0.family_member_id;
        let family_member_uuid = Uuid::parse_str(&family_member_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid family member UUID: {}", e), None)
        })?;
        let recipe_id = params.0.recipe_id;
        let recipe_uuid = Uuid::parse_str(&recipe_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid recipe UUID: {}", e), None))?;

        let allergens =
            NutritionService::check_recipe_allergens(&self.pool, family_member_uuid, recipe_uuid)
                .await
                .map_err(convert_error)?;

        if allergens.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                "Recipe is safe - no allergens found.",
            )]))
        } else {
            let mut output = format!(
                "⚠️  WARNING: Recipe contains {} allergen(s):\n\n",
                allergens.len()
            );
            for allergen in allergens {
                output.push_str(&format!("- {}\n", allergen.name));
            }
            Ok(CallToolResult::success(vec![Content::text(output)]))
        }
    }
}

// ========== Parameter structs ==========

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageIngredientParams {
    action: String,
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    ingredient_id: Option<String>,
    calories_per_100g: Option<f64>,
    protein_g: Option<f64>,
    carbs_g: Option<f64>,
    fat_g: Option<f64>,
    fiber_g: Option<f64>,
    sugar_g: Option<f64>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryIngredientsParams {
    query_type: String,
    search_term: Option<String>,
    id: Option<String>,
    ids: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct RecipeIngredientInput {
    ingredient_id: String,
    quantity: f64,
    unit: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageRecipeParams {
    action: String,
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    servings: Option<i32>,
    prep_time_minutes: Option<i32>,
    cook_time_minutes: Option<i32>,
    ingredients: Option<Vec<RecipeIngredientInput>>,
    steps: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageRecipeContentParams {
    action: String,
    recipe_id: Option<String>,
    ingredient_id: Option<String>,
    quantity: Option<f64>,
    unit: Option<String>,
    step_number: Option<i32>,
    instruction: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryRecipesParams {
    query_type: String,
    search_term: Option<String>,
    ingredient_id: Option<String>,
    id: Option<String>,
    ids: Option<Vec<String>>,
    full: Option<bool>,
    url: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct CalculateNutritionParams {
    calc_type: String,
    id: Option<String>,
    ids: Option<Vec<String>>,
    servings: Option<i32>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageMealPlanParams {
    action: String,
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    is_template: Option<bool>,
    meal_plan_id: Option<String>,
    recipe_id: Option<String>,
    meal_type: Option<String>,
    day_of_week: Option<i32>,
    date: Option<String>,
    entry_id: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryMealPlansParams {
    query_type: String,
    search_term: Option<String>,
    is_template: Option<bool>,
    start_date: Option<String>,
    end_date: Option<String>,
    id: Option<String>,
    ids: Option<Vec<String>>,
    full: Option<bool>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageFamilyMemberParams {
    action: String,
    id: Option<String>,
    name: Option<String>,
    preferences: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryFamilyMembersParams {
    query_type: String,
    search_term: Option<String>,
    id: Option<String>,
    ids: Option<Vec<String>>,
    with_allergies: Option<bool>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageFamilyMemberDataParams {
    action: String,
    family_member_id: Option<String>,
    ingredient_id: Option<String>,
    recipe_id: Option<String>,
    severity: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct CheckAllergensParams {
    family_member_id: String,
    recipe_id: String,
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
