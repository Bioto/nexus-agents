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

    /// Get ingredient by ID
    #[tool(description = "Get ingredient details by UUID")]
    async fn get_ingredient(
        &self,
        params: Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        let ingredient = NutritionService::get_ingredient(&self.pool, uuid)
            .await
            .map_err(convert_error)?;

        let nutritional_info = NutritionService::get_nutritional_info(&self.pool, uuid)
            .await
            .map_err(convert_error)?;

        let mut output = format!(
            "Ingredient: {}\nID: {}\nDescription: {}\nCreated: {}",
            ingredient.name,
            ingredient.id,
            ingredient.description.unwrap_or_else(|| "None".to_string()),
            ingredient.created_at
        );

        if let Some(info) = nutritional_info {
            output.push_str(&format!(
                "\n\nNutritional Info (per 100g):\n  Calories: {}\n  Protein: {}g\n  Carbs: {}g\n  Fat: {}g",
                info.calories_per_100g, info.protein_g, info.carbs_g, info.fat_g
            ));
            if let Some(fiber) = info.fiber_g {
                output.push_str(&format!("\n  Fiber: {}g", fiber));
            }
            if let Some(sugar) = info.sugar_g {
                output.push_str(&format!("\n  Sugar: {}g", sugar));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// List all ingredients
    #[tool(description = "List all ingredients with optional search term")]
    async fn list_ingredients(
        &self,
        params: Parameters<ListIngredientsParams>,
    ) -> Result<CallToolResult, McpError> {
        let ingredients = NutritionService::list_ingredients(&self.pool, params.0.search.as_deref())
            .await
            .map_err(convert_error)?;

        let mut output = format!("Found {} ingredients:\n", ingredients.len());
        for ingredient in ingredients {
            output.push_str(&format!(
                "\n- {} ({})\n  Description: {}",
                ingredient.name,
                ingredient.id,
                ingredient.description.unwrap_or_else(|| "None".to_string())
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
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

    /// Get recipe by ID with full details
    #[tool(description = "Get recipe details by UUID, including all ingredients and steps")]
    async fn get_recipe(
        &self,
        params: Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        let recipe = NutritionService::get_recipe_with_details(&self.pool, uuid)
            .await
            .map_err(convert_error)?;

        let mut output = format!(
            "Recipe: {}\nID: {}\nDescription: {}\nServings: {:?}\nPrep time: {:?} minutes\nCook time: {:?} minutes\n",
            recipe.recipe.name,
            recipe.recipe.id,
            recipe.recipe.description.unwrap_or_else(|| "None".to_string()),
            recipe.recipe.servings,
            recipe.recipe.prep_time_minutes,
            recipe.recipe.cook_time_minutes
        );

        output.push_str("\nIngredients:\n");
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
            output.push_str(&format!("  {}. {}\n", step.step_number, step.instruction));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// List all recipes
    #[tool(description = "List all recipes with optional search term and ingredient filter")]
    async fn list_recipes(
        &self,
        params: Parameters<ListRecipesParams>,
    ) -> Result<CallToolResult, McpError> {
        let ingredient_uuid = params
            .0
            .ingredient_id
            .map(|id| Uuid::parse_str(&id))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None))?;

        let recipes = NutritionService::list_recipes(
            &self.pool,
            params.0.search.as_deref(),
            ingredient_uuid,
        )
        .await
        .map_err(convert_error)?;

        let mut output = format!("Found {} recipes:\n", recipes.len());
        for recipe in recipes {
            output.push_str(&format!(
                "\n- {} ({})\n  Servings: {:?}, Prep: {:?} min, Cook: {:?} min\n  Description: {}",
                recipe.name,
                recipe.id,
                recipe.servings,
                recipe.prep_time_minutes,
                recipe.cook_time_minutes,
                recipe.description.unwrap_or_else(|| "None".to_string())
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
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
    #[tool(description = "Calculate total and per-serving nutritional information for a recipe by UUID")]
    async fn calculate_recipe_nutrition(
        &self,
        params: Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let uuid = Uuid::parse_str(&params.0.id).map_err(|e| {
            McpError::invalid_params(format!("Invalid UUID: {}", e), None)
        })?;

        let nutrition = NutritionService::calculate_recipe_nutrition(&self.pool, uuid)
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
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Search ingredients
    #[tool(description = "Search for ingredients by name or description")]
    async fn search_ingredients(
        &self,
        params: Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let ingredients = NutritionService::list_ingredients(&self.pool, Some(&params.0.query))
            .await
            .map_err(convert_error)?;

        let mut output = format!("Found {} ingredients matching '{}':\n", ingredients.len(), params.0.query);
        for ingredient in ingredients {
            output.push_str(&format!(
                "\n- {} ({})\n  Description: {}",
                ingredient.name,
                ingredient.id,
                ingredient.description.unwrap_or_else(|| "None".to_string())
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Search recipes
    #[tool(description = "Search for recipes by name or description, optionally filter by ingredient")]
    async fn search_recipes(
        &self,
        params: Parameters<SearchRecipesParams>,
    ) -> Result<CallToolResult, McpError> {
        let ingredient_uuid = params
            .0
            .ingredient_id
            .map(|id| Uuid::parse_str(&id))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid ingredient UUID: {}", e), None))?;

        let recipes = NutritionService::list_recipes(
            &self.pool,
            Some(&params.0.query),
            ingredient_uuid,
        )
        .await
        .map_err(convert_error)?;

        let mut output = format!("Found {} recipes matching '{}':\n", recipes.len(), params.0.query);
        for recipe in recipes {
            output.push_str(&format!(
                "\n- {} ({})\n  Description: {}",
                recipe.name,
                recipe.id,
                recipe.description.unwrap_or_else(|| "None".to_string())
            ));
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
struct ListIngredientsParams {
    /// Optional search term to filter ingredients
    search: Option<String>,
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
struct ListRecipesParams {
    /// Optional search term to filter recipes
    search: Option<String>,
    /// Optional ingredient UUID to filter recipes containing this ingredient
    ingredient_id: Option<String>,
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
struct SearchParams {
    /// Search query
    query: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct SearchRecipesParams {
    /// Search query
    query: String,
    /// Optional ingredient UUID to filter by
    ingredient_id: Option<String>,
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

