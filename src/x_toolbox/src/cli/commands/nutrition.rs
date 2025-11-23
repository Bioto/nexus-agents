use crate::error::{Result, ToolboxError};
use chrono::NaiveDate;
use clap::{Args, Subcommand};
use std::process::Command;
use uuid::Uuid;
use csv::ReaderBuilder;
use std::fs::File;
use std::path::Path;
use bigdecimal::BigDecimal;
use regex::Regex;
use futures::future::join_all;

use crate::nutrition::{Database, NutritionService};

/// CLI arguments for the nutrition subcommand
#[derive(Args)]
pub struct NutritionArgs {
    #[command(subcommand)]
    pub command: NutritionCommands,
}

#[derive(Subcommand)]
pub enum NutritionCommands {
    /// Start the API server
    Server {
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind to
        #[arg(long, default_value = "8080")]
        port: u16,
    },
    /// Database management commands
    Db {
        #[command(subcommand)]
        command: DbCommand,
    },
    /// Add a new ingredient or recipe
    Add {
        #[command(subcommand)]
        item: AddItem,
    },
    /// Update an ingredient or recipe
    Update {
        #[command(subcommand)]
        item: UpdateItem,
    },
    /// Delete an ingredient or recipe
    Delete {
        #[command(subcommand)]
        item: DeleteItem,
    },
    /// Calculate nutritional information for a recipe
    Calculate {
        /// Recipe ID
        recipe_id: String,
        /// Optional number of servings to calculate per-serving nutrition for
        #[arg(long)]
        servings: Option<i32>,
    },
    /// Import recipes from a CSV file
    Import {
        /// Path to the CSV file
        file: String,
        /// Skip rows with errors instead of failing
        #[arg(long)]
        skip_errors: bool,
    },
    /// Import ingredients from USDA Foundation Foods dataset
    ImportIngredients {
        /// Path to the directory containing USDA CSV files
        directory: String,
        /// Skip rows with errors instead of failing
        #[arg(long)]
        skip_errors: bool,
    },
    /// Import ingredients from USDA Branded Foods dataset
    ImportBrandedIngredients {
        /// Path to the directory containing USDA CSV files
        directory: String,
        /// Skip rows with errors instead of failing
        #[arg(long)]
        skip_errors: bool,
        /// Limit the number of foods to import (for testing)
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Batch operations for multiple IDs/queries
    Batch {
        #[command(subcommand)]
        operation: BatchOperation,
    },
    /// Meal plan management commands
    MealPlan {
        #[command(subcommand)]
        command: MealPlanCommand,
    },
}

#[derive(Subcommand)]
pub enum AddItem {
    /// Add a new ingredient
    Ingredient {
        /// Ingredient name
        name: String,
        /// Ingredient description
        #[arg(short, long)]
        description: Option<String>,
    },
    /// Add nutritional info for an ingredient
    NutritionalInfo {
        /// Ingredient ID
        #[arg(long)]
        ingredient_id: String,
        /// Calories per 100g
        #[arg(long)]
        calories: f64,
        /// Protein in grams per 100g
        #[arg(long)]
        protein: f64,
        /// Carbs in grams per 100g
        #[arg(long)]
        carbs: f64,
        /// Fat in grams per 100g
        #[arg(long)]
        fat: f64,
        /// Fiber in grams per 100g
        #[arg(long)]
        fiber: Option<f64>,
        /// Sugar in grams per 100g
        #[arg(long)]
        sugar: Option<f64>,
    },
    /// Add a new recipe
    Recipe {
        /// Recipe name
        name: String,
        /// Recipe description
        #[arg(short, long)]
        description: Option<String>,
        /// Number of servings
        #[arg(short, long)]
        servings: Option<i32>,
        /// Prep time in minutes
        #[arg(long)]
        prep_time: Option<i32>,
        /// Cook time in minutes
        #[arg(long)]
        cook_time: Option<i32>,
    },
}

#[derive(Subcommand)]
pub enum UpdateItem {
    /// Update an ingredient
    Ingredient {
        /// Ingredient ID
        id: String,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
    },
    /// Update a recipe
    Recipe {
        /// Recipe ID
        id: String,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
        /// New servings count
        #[arg(short, long)]
        servings: Option<i32>,
        /// New prep time in minutes
        #[arg(long)]
        prep_time: Option<i32>,
        /// New cook time in minutes
        #[arg(long)]
        cook_time: Option<i32>,
    },
}

#[derive(Subcommand)]
pub enum DeleteItem {
    /// Delete an ingredient
    Ingredient {
        /// Ingredient ID
        id: String,
    },
    /// Delete a recipe
    Recipe {
        /// Recipe ID
        id: String,
    },
}

#[derive(Subcommand)]
pub enum BatchOperation {
    /// Get multiple ingredients by IDs
    GetIngredients {
        /// Ingredient IDs (space-separated or comma-separated)
        ids: Vec<String>,
    },
    /// Get multiple recipes by IDs
    GetRecipes {
        /// Recipe IDs (space-separated or comma-separated)
        ids: Vec<String>,
        /// Include full details (ingredients and steps)
        #[arg(long)]
        full: bool,
    },
    /// Calculate nutrition for multiple recipes
    CalculateNutrition {
        /// Recipe IDs (space-separated or comma-separated)
        ids: Vec<String>,
    },
    /// Search with multiple queries (returns union of results)
    SearchIngredients {
        /// Search terms (space-separated or comma-separated)
        terms: Vec<String>,
    },
    /// Search recipes with multiple queries (returns union of results)
    SearchRecipes {
        /// Search terms (space-separated or comma-separated)
        terms: Vec<String>,
        /// Filter by ingredient ID
        #[arg(long)]
        ingredient_id: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum MealPlanCommand {
    /// Create a new meal plan
    Add {
        /// Meal plan name
        name: String,
        /// Meal plan description
        #[arg(short, long)]
        description: Option<String>,
        /// Start date (YYYY-MM-DD) - null for templates
        #[arg(long)]
        start_date: Option<String>,
        /// End date (YYYY-MM-DD) - null for templates
        #[arg(long)]
        end_date: Option<String>,
        /// Mark as template (day-of-week based)
        #[arg(long)]
        template: bool,
    },
    /// Get meal plan details
    Get {
        /// Meal plan ID
        id: String,
        /// Include full details (entries)
        #[arg(long)]
        full: bool,
    },
    /// Update meal plan metadata
    Update {
        /// Meal plan ID
        id: String,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
        /// New start date (YYYY-MM-DD)
        #[arg(long)]
        start_date: Option<String>,
        /// New end date (YYYY-MM-DD)
        #[arg(long)]
        end_date: Option<String>,
    },
    /// Delete a meal plan
    Delete {
        /// Meal plan ID
        id: String,
    },
    /// Add entry to meal plan
    AddEntry {
        /// Meal plan ID
        #[arg(long)]
        meal_plan_id: String,
        /// Recipe ID
        #[arg(long)]
        recipe_id: String,
        /// Meal type (breakfast, lunch, dinner, snack)
        #[arg(long)]
        meal_type: String,
        /// Day of week (0-6, Monday=0) - for templates
        #[arg(long)]
        day_of_week: Option<i32>,
        /// Date (YYYY-MM-DD) - for date-specific plans
        #[arg(long)]
        date: Option<String>,
    },
    /// Remove entry from meal plan
    RemoveEntry {
        /// Entry ID
        id: String,
    },
        /// List meal plans
        List {
            /// Search term (searches name and description)
            #[arg(short, long)]
            search: Option<String>,
            /// Filter by template status
            #[arg(long)]
            template: Option<bool>,
            /// Filter by start date (YYYY-MM-DD)
            #[arg(long)]
            start_date: Option<String>,
            /// Filter by end date (YYYY-MM-DD)
            #[arg(long)]
            end_date: Option<String>,
        },
    /// Calculate nutrition for meal plan
    CalculateNutrition {
        /// Meal plan ID
        id: String,
    },
    /// Batch operations for meal plans
    Batch {
        #[command(subcommand)]
        operation: MealPlanBatchOperation,
    },
}

#[derive(Subcommand)]
pub enum MealPlanBatchOperation {
    /// Get multiple meal plans by IDs
    GetMealPlans {
        /// Meal plan IDs (space-separated or comma-separated)
        ids: Vec<String>,
        /// Include full details (entries)
        #[arg(long)]
        full: bool,
    },
    /// Calculate nutrition for multiple meal plans
    CalculateNutrition {
        /// Meal plan IDs (space-separated or comma-separated)
        ids: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum DbCommand {
    /// Start the Postgres database server using Docker Compose
    Start,
    /// Stop the Postgres database server
    Stop,
    /// Show database status
    Status,
}

/// Runs the nutrition command based on args
pub async fn run_nutrition(args: NutritionArgs) -> Result<()> {
    match args.command {
        NutritionCommands::Db { command } => {
            handle_db_command(command).await?;
            return Ok(());
        }
        _ => {}
    }

    let db = Database::new().await?;
    let pool = db.pool();

    match args.command {
        NutritionCommands::Server { host, port } => {
            crate::nutrition::api::start_server(pool.clone(), host, port).await?;
        }
        NutritionCommands::Add { item } => match item {
            AddItem::Ingredient { name, description } => {
                let ingredient = NutritionService::create_ingredient(pool, &name, description.as_deref()).await?;
                println!("Created ingredient: {} ({})", ingredient.name, ingredient.id);
            }
            AddItem::NutritionalInfo {
                ingredient_id,
                calories,
                protein,
                carbs,
                fat,
                fiber,
                sugar,
            } => {
                let ingredient_uuid = Uuid::parse_str(&ingredient_id)?;
                let nutritional_info = NutritionService::upsert_nutritional_info(
                    pool,
                    ingredient_uuid,
                    calories.to_string().parse().map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    protein.to_string().parse().map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    carbs.to_string().parse().map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    fat.to_string().parse().map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    fiber.map(|f| f.to_string().parse()).transpose().map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    sugar.map(|s| s.to_string().parse()).transpose().map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                )
                .await?;
                println!("Created/updated nutritional info for ingredient: {}", nutritional_info.ingredient_id);
            }
            AddItem::Recipe {
                name,
                description,
                servings,
                prep_time,
                cook_time,
            } => {
                let recipe = NutritionService::create_recipe(
                    pool,
                    &name,
                    description.as_deref(),
                    servings,
                    prep_time,
                    cook_time,
                    vec![], // Ingredients would need to be added separately
                    vec![], // Steps would need to be added separately
                )
                .await?;
                println!("Created recipe: {} ({})", recipe.name, recipe.id);
            }
        },
        NutritionCommands::Update { item } => match item {
            UpdateItem::Ingredient { id, name, description } => {
                let ingredient_uuid = Uuid::parse_str(&id)?;
                let ingredient = NutritionService::update_ingredient(
                    pool,
                    ingredient_uuid,
                    name.as_deref(),
                    description.as_deref(),
                )
                .await?;
                println!("Updated ingredient: {} ({})", ingredient.name, ingredient.id);
            }
            UpdateItem::Recipe {
                id,
                name,
                description,
                servings,
                prep_time,
                cook_time,
            } => {
                let recipe_uuid = Uuid::parse_str(&id)?;
                let recipe = NutritionService::update_recipe(
                    pool,
                    recipe_uuid,
                    name.as_deref(),
                    description.as_deref(),
                    servings,
                    prep_time,
                    cook_time,
                )
                .await?;
                println!("Updated recipe: {} ({})", recipe.name, recipe.id);
            }
        },
        NutritionCommands::Delete { item } => match item {
            DeleteItem::Ingredient { id } => {
                let ingredient_uuid = Uuid::parse_str(&id)?;
                NutritionService::delete_ingredient(pool, ingredient_uuid).await?;
                println!("Deleted ingredient: {}", id);
            }
            DeleteItem::Recipe { id } => {
                let recipe_uuid = Uuid::parse_str(&id)?;
                NutritionService::delete_recipe(pool, recipe_uuid).await?;
                println!("Deleted recipe: {}", id);
            }
        },
        NutritionCommands::Calculate { recipe_id, servings } => {
            let recipe_uuid = Uuid::parse_str(&recipe_id)?;
            let nutrition = NutritionService::calculate_recipe_nutrition(pool, recipe_uuid, servings).await?;
            println!("Nutritional information for recipe {}:", recipe_id);
            println!("Total calories: {}", nutrition.total_calories);
            println!("Total protein: {}g", nutrition.total_protein_g);
            println!("Total carbs: {}g", nutrition.total_carbs_g);
            println!("Total fat: {}g", nutrition.total_fat_g);
            if let Some(fiber) = nutrition.total_fiber_g {
                println!("Total fiber: {}g", fiber);
            }
            if let Some(sugar) = nutrition.total_sugar_g {
                println!("Total sugar: {}g", sugar);
            }
            if let Some(servings) = nutrition.per_serving_calories {
                println!("\nPer serving:");
                println!("  Calories: {}", servings);
                if let Some(protein) = nutrition.per_serving_protein_g {
                    println!("  Protein: {}g", protein);
                }
                if let Some(carbs) = nutrition.per_serving_carbs_g {
                    println!("  Carbs: {}g", carbs);
                }
                if let Some(fat) = nutrition.per_serving_fat_g {
                    println!("  Fat: {}g", fat);
                }
            }
        }
        NutritionCommands::Import { file, skip_errors } => {
            import_recipes_from_csv(pool, &file, skip_errors).await?;
        }
        NutritionCommands::ImportIngredients { directory, skip_errors } => {
            import_usda_ingredients(pool, &directory, skip_errors).await?;
        }
        NutritionCommands::ImportBrandedIngredients { directory, skip_errors, limit } => {
            import_usda_branded_ingredients(pool, &directory, skip_errors, limit).await?;
        }
        NutritionCommands::Batch { operation } => {
            handle_batch_operation(pool, operation).await?;
        }
        NutritionCommands::MealPlan { command } => {
            handle_meal_plan_command(pool, command).await?;
        }
        NutritionCommands::Db { .. } => {
            // Already handled above
        }
    }

    Ok(())
}

/// Handle database management commands
async fn handle_db_command(command: DbCommand) -> Result<()> {
    let compose_file = std::env::current_dir()
        .map_err(|e| ToolboxError::Io(e))?
        .join("src/x_toolbox/docker-compose.yml");

    let compose_file_str = compose_file
        .to_str()
        .ok_or_else(|| ToolboxError::Other("Invalid docker-compose.yml path".to_string()))?;

    match command {
        DbCommand::Start => {
            println!("Starting Postgres database...");
            let output = Command::new("docker")
                .args(&["compose", "-f", compose_file_str, "up", "-d"])
                .output()
                .map_err(|e| {
                    ToolboxError::Other(format!(
                        "Failed to execute docker compose: {}. Make sure Docker is installed and running.",
                        e
                    ))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ToolboxError::Other(format!(
                    "Failed to start database: {}",
                    stderr
                )));
            }

            println!("Postgres database started successfully!");
            println!("Connection details:");
            println!("  Host: localhost");
            println!("  Port: 5432");
            println!("  Database: nutrition");
            println!("  User: postgres");
            println!("  Password: postgres");
            println!("\nSet these environment variables:");
            println!("  export POSTGRES_HOST=localhost");
            println!("  export POSTGRES_PORT=5432");
            println!("  export POSTGRES_DATABASE=nutrition");
            println!("  export POSTGRES_USER=postgres");
            println!("  export POSTGRES_PASSWORD=postgres");
        }
        DbCommand::Stop => {
            println!("Stopping Postgres database...");
            let output = Command::new("docker")
                .args(&["compose", "-f", compose_file_str, "down"])
                .output()
                .map_err(|e| {
                    ToolboxError::Other(format!(
                        "Failed to execute docker compose: {}. Make sure Docker is installed and running.",
                        e
                    ))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ToolboxError::Other(format!(
                    "Failed to stop database: {}",
                    stderr
                )));
            }

            println!("Postgres database stopped successfully!");
        }
        DbCommand::Status => {
            let output = Command::new("docker")
                .args(&["compose", "-f", compose_file_str, "ps"])
                .output()
                .map_err(|e| {
                    ToolboxError::Other(format!(
                        "Failed to execute docker compose: {}. Make sure Docker is installed and running.",
                        e
                    ))
                })?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                print!("{}", stdout);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ToolboxError::Other(format!(
                    "Failed to get database status: {}",
                    stderr
                )));
            }
        }
    }

    Ok(())
}

/// Handle batch operations
async fn handle_batch_operation(
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

/// Parse IDs from a vector, handling both space and comma-separated values
fn parse_ids(input: &[String]) -> Vec<String> {
    input
        .iter()
        .flat_map(|s| {
            // Split by comma first, then by space
            s.split(',')
                .flat_map(|part| part.split_whitespace())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .collect()
}

/// Parse search terms from a vector, handling both space and comma-separated values
fn parse_terms(input: &[String]) -> Vec<String> {
    input
        .iter()
        .flat_map(|s| {
            // Split by comma first, then by space
            s.split(',')
                .flat_map(|part| part.split_whitespace())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .collect()
}

/// Handle meal plan commands
async fn handle_meal_plan_command(
    pool: &sqlx::PgPool,
    command: MealPlanCommand,
) -> Result<()> {
    match command {
        MealPlanCommand::Add {
            name,
            description,
            start_date,
            end_date,
            template,
        } => {
            let start = start_date
                .as_ref()
                .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| ToolboxError::Validation(format!("Invalid start_date format: {}", e)))?;
            let end = end_date
                .as_ref()
                .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| ToolboxError::Validation(format!("Invalid end_date format: {}", e)))?;

            let meal_plan = NutritionService::create_meal_plan(
                pool,
                &name,
                description.as_deref(),
                start,
                end,
                template,
            )
            .await?;
            println!("Created meal plan: {} ({})", meal_plan.name, meal_plan.id);
        }
        MealPlanCommand::Get { id, full } => {
            let meal_plan_uuid = Uuid::parse_str(&id)?;
            if full {
                let meal_plan = NutritionService::get_meal_plan_with_entries(pool, meal_plan_uuid).await?;
                println!("Meal Plan: {}", meal_plan.meal_plan.name);
                if let Some(desc) = &meal_plan.meal_plan.description {
                    println!("Description: {}", desc);
                }
                println!("Template: {}", meal_plan.meal_plan.is_template);
                if let Some(start) = meal_plan.meal_plan.start_date {
                    println!("Start date: {}", start);
                }
                if let Some(end) = meal_plan.meal_plan.end_date {
                    println!("End date: {}", end);
                }
                println!("\nEntries ({}):", meal_plan.entries.len());
                for entry in meal_plan.entries {
                    if let Some(date) = entry.entry.date {
                        println!("  Date: {}, Meal: {}, Recipe: {} ({})",
                            date,
                            entry.entry.meal_type,
                            entry.recipe.name,
                            entry.entry.recipe_id
                        );
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
                        println!("  Day: {}, Meal: {}, Recipe: {} ({})",
                            day_name,
                            entry.entry.meal_type,
                            entry.recipe.name,
                            entry.entry.recipe_id
                        );
                    }
                }
            } else {
                let meal_plan = NutritionService::get_meal_plan(pool, meal_plan_uuid).await?;
                println!("Meal Plan: {}", meal_plan.name);
                if let Some(desc) = &meal_plan.description {
                    println!("Description: {}", desc);
                }
                println!("ID: {}", meal_plan.id);
                println!("Template: {}", meal_plan.is_template);
                println!("Created: {}", meal_plan.created_at);
            }
        }
        MealPlanCommand::Update {
            id,
            name,
            description,
            start_date,
            end_date,
        } => {
            let meal_plan_uuid = Uuid::parse_str(&id)?;
            let start = start_date
                .as_ref()
                .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| ToolboxError::Validation(format!("Invalid start_date format: {}", e)))?;
            let end = end_date
                .as_ref()
                .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| ToolboxError::Validation(format!("Invalid end_date format: {}", e)))?;

            let meal_plan = NutritionService::update_meal_plan(
                pool,
                meal_plan_uuid,
                name.as_deref(),
                description.as_deref(),
                start,
                end,
            )
            .await?;
            println!("Updated meal plan: {} ({})", meal_plan.name, meal_plan.id);
        }
        MealPlanCommand::Delete { id } => {
            let meal_plan_uuid = Uuid::parse_str(&id)?;
            NutritionService::delete_meal_plan(pool, meal_plan_uuid).await?;
            println!("Deleted meal plan: {}", id);
        }
        MealPlanCommand::AddEntry {
            meal_plan_id,
            recipe_id,
            meal_type,
            day_of_week,
            date,
        } => {
            let meal_plan_uuid = Uuid::parse_str(&meal_plan_id)?;
            let recipe_uuid = Uuid::parse_str(&recipe_id)?;
            let date_parsed = date
                .as_ref()
                .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| ToolboxError::Validation(format!("Invalid date format: {}", e)))?;

            let entry = NutritionService::add_meal_plan_entry(
                pool,
                meal_plan_uuid,
                recipe_uuid,
                &meal_type,
                day_of_week,
                date_parsed,
            )
            .await?;
            println!("Added entry to meal plan: {} ({})", entry.id, meal_plan_id);
        }
        MealPlanCommand::RemoveEntry { id } => {
            let entry_uuid = Uuid::parse_str(&id)?;
            NutritionService::remove_meal_plan_entry(pool, entry_uuid).await?;
            println!("Removed entry: {}", id);
        }
        MealPlanCommand::List {
            search,
            template,
            start_date,
            end_date,
        } => {
            let start = start_date
                .as_ref()
                .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| ToolboxError::Validation(format!("Invalid start_date format: {}", e)))?;
            let end = end_date
                .as_ref()
                .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| ToolboxError::Validation(format!("Invalid end_date format: {}", e)))?;

            let meal_plans = NutritionService::list_meal_plans(pool, search.as_deref(), template, start, end).await?;
            println!("Found {} meal plans:", meal_plans.len());
            for meal_plan in meal_plans {
                println!("  - {} ({})", meal_plan.name, meal_plan.id);
                if meal_plan.is_template {
                    println!("    Template");
                } else {
                    if let Some(start) = meal_plan.start_date {
                        print!("    {} - ", start);
                    }
                    if let Some(end) = meal_plan.end_date {
                        println!("{}", end);
                    } else {
                        println!();
                    }
                }
            }
        }
        MealPlanCommand::CalculateNutrition { id } => {
            let meal_plan_uuid = Uuid::parse_str(&id)?;
            let nutrition = NutritionService::calculate_meal_plan_nutrition(pool, meal_plan_uuid).await?;
            println!("Nutritional information for meal plan {}:", id);
            println!("\nDaily Nutrition:");
            for daily in nutrition.daily_nutrition {
                if let Some(date) = daily.date {
                    println!("\nDate: {}", date);
                } else if let Some(dow) = daily.day_of_week {
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
                    println!("\nDay: {}", day_name);
                }
                println!("  Total calories: {}", daily.total_calories);
                println!("  Total protein: {}g", daily.total_protein_g);
                println!("  Total carbs: {}g", daily.total_carbs_g);
                println!("  Total fat: {}g", daily.total_fat_g);
                if let Some(fiber) = daily.total_fiber_g {
                    println!("  Total fiber: {}g", fiber);
                }
                if let Some(sugar) = daily.total_sugar_g {
                    println!("  Total sugar: {}g", sugar);
                }
                println!("  Meals:");
                for meal in daily.meals {
                    println!("    {}: {} calories, {}g protein, {}g carbs, {}g fat",
                        meal.meal_type,
                        meal.calories,
                        meal.protein_g,
                        meal.carbs_g,
                        meal.fat_g
                    );
                }
            }
            if let Some(weekly) = nutrition.weekly_totals {
                println!("\nWeekly Totals:");
                println!("  Total calories: {}", weekly.total_calories);
                println!("  Total protein: {}g", weekly.total_protein_g);
                println!("  Total carbs: {}g", weekly.total_carbs_g);
                println!("  Total fat: {}g", weekly.total_fat_g);
                if let Some(fiber) = weekly.total_fiber_g {
                    println!("  Total fiber: {}g", fiber);
                }
                if let Some(sugar) = weekly.total_sugar_g {
                    println!("  Total sugar: {}g", sugar);
                }
                println!("\nDaily Averages:");
                println!("  Calories: {}", weekly.average_daily_calories);
                println!("  Protein: {}g", weekly.average_daily_protein_g);
                println!("  Carbs: {}g", weekly.average_daily_carbs_g);
                println!("  Fat: {}g", weekly.average_daily_fat_g);
            }
        }
        MealPlanCommand::Batch { operation } => {
            handle_meal_plan_batch_operation(pool, operation).await?;
        }
    }

    Ok(())
}

/// Handle meal plan batch operations
async fn handle_meal_plan_batch_operation(
    pool: &sqlx::PgPool,
    operation: MealPlanBatchOperation,
) -> Result<()> {
    match operation {
        MealPlanBatchOperation::GetMealPlans { ids, full } => {
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
                            NutritionService::get_meal_plan_with_entries(pool, id).await
                        }
                    })
                )
                .await;

                println!("Batch get meal plans with entries ({} results):", results.len());
                for (idx, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(meal_plan) => {
                            println!("\n[{}] Meal Plan: {}", idx + 1, meal_plan.meal_plan.name);
                            println!("  ID: {}", meal_plan.meal_plan.id);
                            println!("  Entries: {}", meal_plan.entries.len());
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
                            NutritionService::get_meal_plan(pool, id).await
                        }
                    })
                )
                .await;

                println!("Batch get meal plans ({} results):", results.len());
                for (idx, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(meal_plan) => {
                            println!("\n[{}] Meal Plan: {}", idx + 1, meal_plan.name);
                            println!("  ID: {}", meal_plan.id);
                            println!("  Template: {}", meal_plan.is_template);
                        }
                        Err(e) => {
                            eprintln!("[{}] Error: {}", idx + 1, e);
                        }
                    }
                }
            }
        }
        MealPlanBatchOperation::CalculateNutrition { ids } => {
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
                        NutritionService::calculate_meal_plan_nutrition(pool, id).await
                    }
                })
            )
            .await;

            println!("Batch calculate nutrition ({} results):", results.len());
            for (idx, result) in results.into_iter().enumerate() {
                match result {
                    Ok(nutrition) => {
                        println!("\n[{}] Meal Plan ID: {}", idx + 1, nutrition.meal_plan_id);
                        println!("  Days: {}", nutrition.daily_nutrition.len());
                        if let Some(weekly) = nutrition.weekly_totals {
                            println!("  Weekly total calories: {}", weekly.total_calories);
                            println!("  Average daily calories: {}", weekly.average_daily_calories);
                        }
                    }
                    Err(e) => {
                        eprintln!("[{}] Error: {}", idx + 1, e);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Import recipes from a CSV file
async fn import_recipes_from_csv(
    pool: &sqlx::PgPool,
    file_path: &str,
    skip_errors: bool,
) -> Result<()> {
    let path = Path::new(file_path);
    let file = File::open(path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open CSV file {}: {}", file_path, e),
        ))
    })?;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(file);

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = 0;

    // Regex to parse ingredient strings like "3 tablespoons butter" or "2 pounds Granny Smith apples"
    let ingredient_re = Regex::new(r"^(\d+(?:\.\d+)?)\s+([a-zA-Z]+(?:\s+[a-zA-Z]+)?)\s+(.+)$")
        .map_err(|e| ToolboxError::Other(format!("Failed to compile regex: {}", e)))?;

    // Regex to parse time strings like "30 mins", "1 hrs", "1 hrs 30 mins"
    let time_re = Regex::new(r"(?:(\d+)\s*(?:hrs?|hours?))?\s*(?:(\d+)\s*(?:mins?|minutes?))?")
        .map_err(|e| ToolboxError::Other(format!("Failed to compile regex: {}", e)))?;

    for (row_num, result) in reader.records().enumerate() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                if skip_errors {
                    eprintln!("Error reading row {}: {}", row_num + 2, e);
                    skipped += 1;
                    continue;
                } else {
                    return Err(ToolboxError::Other(format!(
                        "Error reading row {}: {}",
                        row_num + 2, e
                    )));
                }
            }
        };

        let recipe_name = record.get(1).ok_or_else(|| {
            ToolboxError::Validation(format!("Row {}: missing recipe_name", row_num + 2))
        })?;

        if recipe_name.is_empty() {
            if skip_errors {
                skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "Row {}: recipe_name is empty",
                    row_num + 2
                )));
            }
        }

        // Parse times
        let prep_time = record.get(2).and_then(|t| parse_time(&time_re, t));
        let cook_time = record.get(3).and_then(|t| parse_time(&time_re, t));

        // Parse servings
        let servings = record.get(5).and_then(|s| s.parse::<i32>().ok());

        // Parse ingredients
        let ingredients_str = record.get(7).unwrap_or("");
        let ingredients = parse_ingredients(pool, ingredients_str, &ingredient_re).await;

        let ingredients = match ingredients {
            Ok(ing) => ing,
            Err(e) => {
                if skip_errors {
                    eprintln!("Row {}: Error parsing ingredients: {}", row_num + 2, e);
                    errors += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        // Parse directions into steps
        let directions_str = record.get(8).unwrap_or("");
        let steps = parse_directions(directions_str);

        // Create recipe
        match NutritionService::create_recipe(
            pool,
            recipe_name,
            None, // description
            servings,
            prep_time,
            cook_time,
            ingredients,
            steps,
        )
        .await
        {
            Ok(_) => {
                imported += 1;
                if imported % 100 == 0 {
                    println!("Imported {} recipes...", imported);
                }
            }
            Err(e) => {
                if skip_errors {
                    eprintln!("Row {}: Error creating recipe: {}", row_num + 2, e);
                    errors += 1;
                } else {
                    return Err(e);
                }
            }
        }
    }

    println!("\nImport complete:");
    println!("  Imported: {}", imported);
    if skipped > 0 {
        println!("  Skipped: {}", skipped);
    }
    if errors > 0 {
        println!("  Errors: {}", errors);
    }

    Ok(())
}

/// Parse time string like "30 mins", "1 hrs", "1 hrs 30 mins" into minutes
fn parse_time(re: &Regex, time_str: &str) -> Option<i32> {
    if time_str.trim().is_empty() {
        return None;
    }

    re.captures(time_str).and_then(|caps| {
        let hours: i32 = caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        let minutes: i32 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        Some(hours * 60 + minutes)
    })
}

/// Parse ingredients string and create/find ingredients in database
async fn parse_ingredients(
    pool: &sqlx::PgPool,
    ingredients_str: &str,
    ingredient_re: &Regex,
) -> Result<Vec<(Uuid, BigDecimal, String)>> {
    if ingredients_str.trim().is_empty() {
        return Ok(vec![]);
    }

    let mut result = Vec::new();

    // Split by comma, but be careful with commas inside parentheses
    let parts: Vec<&str> = ingredients_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    for part in parts {
        // Try to match the pattern: quantity unit name
        if let Some(caps) = ingredient_re.captures(part) {
            let quantity_str = caps.get(1).unwrap().as_str();
            let unit = caps.get(2).unwrap().as_str().to_string();
            let name = caps.get(3).unwrap().as_str().trim().to_string();

            // Parse quantity
            let quantity: BigDecimal = quantity_str
                .parse()
                .map_err(|e| {
                    ToolboxError::Validation(format!("Invalid quantity '{}': {}", quantity_str, e))
                })?;

            // Find or create ingredient
            let ingredient = NutritionService::find_or_create_ingredient(pool, &name).await?;

            result.push((ingredient.id, quantity, unit));
        } else {
            // If regex doesn't match, try to extract just the name (everything after the first number and unit)
            // This is a fallback for complex ingredient strings
            let name = part
                .trim()
                .split_whitespace()
                .skip(2) // Skip quantity and unit
                .collect::<Vec<_>>()
                .join(" ");

            if !name.is_empty() {
                // Default to quantity 1 and unit "piece" if we can't parse
                let ingredient = NutritionService::find_or_create_ingredient(pool, &name).await?;
                result.push((ingredient.id, BigDecimal::from(1), "piece".to_string()));
            }
        }
    }

    Ok(result)
}

/// Parse directions string into numbered steps
fn parse_directions(directions_str: &str) -> Vec<(i32, String)> {
    if directions_str.trim().is_empty() {
        return vec![];
    }

    // Split by newlines and filter out empty lines
    let lines: Vec<&str> = directions_str
        .split('\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    lines
        .into_iter()
        .enumerate()
        .map(|(idx, line)| ((idx + 1) as i32, line.to_string()))
        .collect()
}

/// Import ingredients from USDA Foundation Foods dataset
async fn import_usda_ingredients(
    pool: &sqlx::PgPool,
    directory: &str,
    skip_errors: bool,
) -> Result<()> {
    use std::collections::HashMap;

    let dir_path = Path::new(directory);
    
    // USDA nutrient IDs we care about
    const NUTRIENT_ENERGY: i32 = 1008; // Energy (KCAL)
    const NUTRIENT_PROTEIN: i32 = 1003; // Protein (G)
    const NUTRIENT_FAT: i32 = 1004; // Total lipid (fat) (G)
    const NUTRIENT_CARBS: i32 = 1005; // Carbohydrate, by difference (G)
    const NUTRIENT_FIBER: i32 = 1079; // Fiber, total dietary (G)
    const NUTRIENT_SUGAR: i32 = 1063; // Sugars, Total (G)

    println!("Loading USDA Foundation Foods dataset from: {}", directory);

    // Step 1: Load foundation_food.csv to get fdc_ids
    let foundation_food_path = dir_path.join("foundation_food.csv");
    let foundation_food_file = File::open(&foundation_food_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open foundation_food.csv: {}", e),
        ))
    })?;

    let mut foundation_fdc_ids = Vec::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(foundation_food_file);

    for result in reader.records() {
        let record = result.map_err(|e| {
            ToolboxError::Other(format!("Error reading foundation_food.csv: {}", e))
        })?;
        if let Some(fdc_id_str) = record.get(0) {
            if let Ok(fdc_id) = fdc_id_str.parse::<i32>() {
                foundation_fdc_ids.push(fdc_id);
            }
        }
    }

    println!("Found {} foundation foods", foundation_fdc_ids.len());

    // Step 2: Load food.csv to get descriptions (map fdc_id -> description)
    let food_path = dir_path.join("food.csv");
    let food_file = File::open(&food_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open food.csv: {}", e),
        ))
    })?;

    let mut food_descriptions: HashMap<i32, String> = HashMap::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(food_file);

    for result in reader.records() {
        let record = result.map_err(|e| {
            ToolboxError::Other(format!("Error reading food.csv: {}", e))
        })?;
        if let (Some(fdc_id_str), Some(description)) = (record.get(0), record.get(2)) {
            if let Ok(fdc_id) = fdc_id_str.parse::<i32>() {
                food_descriptions.insert(fdc_id, description.to_string());
            }
        }
    }

    println!("Loaded {} food descriptions", food_descriptions.len());

    // Step 3: Load food_nutrient.csv to get nutrient values (map (fdc_id, nutrient_id) -> amount)
    let food_nutrient_path = dir_path.join("food_nutrient.csv");
    let food_nutrient_file = File::open(&food_nutrient_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open food_nutrient.csv: {}", e),
        ))
    })?;

    let mut food_nutrients: HashMap<(i32, i32), f64> = HashMap::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(food_nutrient_file);

    for result in reader.records() {
        let record = result.map_err(|e| {
            ToolboxError::Other(format!("Error reading food_nutrient.csv: {}", e))
        })?;
        if let (Some(fdc_id_str), Some(nutrient_id_str), Some(amount_str)) = 
            (record.get(1), record.get(2), record.get(3)) {
            if let (Ok(fdc_id), Ok(nutrient_id)) = 
                (fdc_id_str.parse::<i32>(), nutrient_id_str.parse::<i32>()) {
                if let Ok(amount) = amount_str.parse::<f64>() {
                    food_nutrients.insert((fdc_id, nutrient_id), amount);
                }
            }
        }
    }

    println!("Loaded {} nutrient values", food_nutrients.len());

    // Step 4: Process each foundation food
    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = 0;

    for fdc_id in foundation_fdc_ids.iter() {
        // Get food description
        let food_name = match food_descriptions.get(fdc_id) {
            Some(name) => name.trim().to_string(),
            None => {
                if skip_errors {
                    eprintln!("FDC ID {}: No description found", fdc_id);
                    skipped += 1;
                    continue;
                } else {
                    return Err(ToolboxError::Validation(format!(
                        "FDC ID {}: No description found",
                        fdc_id
                    )));
                }
            }
        };

        if food_name.is_empty() {
            if skip_errors {
                skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "FDC ID {}: Empty description",
                    fdc_id
                )));
            }
        }

        // Extract nutrient values
        let calories = food_nutrients
            .get(&(*fdc_id, NUTRIENT_ENERGY))
            .copied()
            .unwrap_or(0.0);
        let protein = food_nutrients
            .get(&(*fdc_id, NUTRIENT_PROTEIN))
            .copied()
            .unwrap_or(0.0);
        let fat = food_nutrients
            .get(&(*fdc_id, NUTRIENT_FAT))
            .copied()
            .unwrap_or(0.0);
        let carbs = food_nutrients
            .get(&(*fdc_id, NUTRIENT_CARBS))
            .copied()
            .unwrap_or(0.0);
        let fiber = food_nutrients
            .get(&(*fdc_id, NUTRIENT_FIBER))
            .copied();
        let sugar = food_nutrients
            .get(&(*fdc_id, NUTRIENT_SUGAR))
            .copied();

        // Validate required nutrients
        if calories == 0.0 && protein == 0.0 && carbs == 0.0 && fat == 0.0 {
            if skip_errors {
                eprintln!("FDC ID {} ({}): No nutritional data found", fdc_id, food_name);
                skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "FDC ID {} ({}): No nutritional data found",
                    fdc_id, food_name
                )));
            }
        }

        // Find or create ingredient
        let ingredient = match NutritionService::find_or_create_ingredient(pool, &food_name).await {
            Ok(ing) => ing,
            Err(e) => {
                if skip_errors {
                    eprintln!("FDC ID {} ({}): Error creating ingredient: {}", fdc_id, food_name, e);
                    errors += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        // Create or update nutritional info
        // Convert f64 to BigDecimal via string parsing
        let calories_bd = calories.to_string().parse::<BigDecimal>().map_err(|e| {
            ToolboxError::Validation(format!("Invalid calories value: {}", e))
        })?;
        let protein_bd = protein.to_string().parse::<BigDecimal>().map_err(|e| {
            ToolboxError::Validation(format!("Invalid protein value: {}", e))
        })?;
        let carbs_bd = carbs.to_string().parse::<BigDecimal>().map_err(|e| {
            ToolboxError::Validation(format!("Invalid carbs value: {}", e))
        })?;
        let fat_bd = fat.to_string().parse::<BigDecimal>().map_err(|e| {
            ToolboxError::Validation(format!("Invalid fat value: {}", e))
        })?;
        let fiber_bd = fiber.and_then(|f| f.to_string().parse::<BigDecimal>().ok());
        let sugar_bd = sugar.and_then(|s| s.to_string().parse::<BigDecimal>().ok());

        match NutritionService::upsert_nutritional_info(
            pool,
            ingredient.id,
            calories_bd,
            protein_bd,
            carbs_bd,
            fat_bd,
            fiber_bd,
            sugar_bd,
        )
        .await
        {
            Ok(_) => {
                imported += 1;
                if imported % 100 == 0 {
                    println!("Imported {} ingredients...", imported);
                }
            }
            Err(e) => {
                if skip_errors {
                    eprintln!("FDC ID {} ({}): Error creating nutritional info: {}", fdc_id, food_name, e);
                    errors += 1;
                } else {
                    return Err(e);
                }
            }
        }
    }

    println!("\nImport complete:");
    println!("  Imported: {}", imported);
    if skipped > 0 {
        println!("  Skipped: {}", skipped);
    }
    if errors > 0 {
        println!("  Errors: {}", errors);
    }

    Ok(())
}

/// Import ingredients from USDA Branded Foods dataset
async fn import_usda_branded_ingredients(
    pool: &sqlx::PgPool,
    directory: &str,
    skip_errors: bool,
    limit: Option<usize>,
) -> Result<()> {
    use std::collections::{HashMap, HashSet};

    let dir_path = Path::new(directory);
    
    // USDA nutrient IDs we care about
    const NUTRIENT_ENERGY: i32 = 1008; // Energy (KCAL)
    const NUTRIENT_PROTEIN: i32 = 1003; // Protein (G)
    const NUTRIENT_FAT: i32 = 1004; // Total lipid (fat) (G)
    const NUTRIENT_CARBS: i32 = 1005; // Carbohydrate, by difference (G)
    const NUTRIENT_FIBER: i32 = 1079; // Fiber, total dietary (G)
    const NUTRIENT_SUGAR: i32 = 1063; // Sugars, Total (G)

    println!("Loading USDA Branded Foods dataset from: {}", directory);

    // Step 1: Load branded_food.csv to get fdc_ids
    let branded_food_path = dir_path.join("branded_food.csv");
    let branded_food_file = File::open(&branded_food_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open branded_food.csv: {}", e),
        ))
    })?;

    let mut branded_fdc_ids = Vec::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(branded_food_file);

    for result in reader.records() {
        let record = result.map_err(|e| {
            ToolboxError::Other(format!("Error reading branded_food.csv: {}", e))
        })?;
        if let Some(fdc_id_str) = record.get(0) {
            if let Ok(fdc_id) = fdc_id_str.parse::<i32>() {
                branded_fdc_ids.push(fdc_id);
            }
        }
    }

    // Apply limit if specified
    if let Some(limit_val) = limit {
        branded_fdc_ids.truncate(limit_val);
        println!("Limited to {} foods for import", limit_val);
    }

    // Create HashSet for efficient lookup
    let branded_fdc_set: HashSet<i32> = branded_fdc_ids.iter().copied().collect();

    println!("Found {} branded foods to import", branded_fdc_ids.len());

    // Step 2: Load food.csv to get descriptions (only for branded foods)
    let food_path = dir_path.join("food.csv");
    let food_file = File::open(&food_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open food.csv: {}", e),
        ))
    })?;

    let mut food_descriptions: HashMap<i32, String> = HashMap::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(food_file);

    let mut loaded_count = 0;
    for result in reader.records() {
        let record = result.map_err(|e| {
            ToolboxError::Other(format!("Error reading food.csv: {}", e))
        })?;
        if let (Some(fdc_id_str), Some(data_type), Some(description)) = 
            (record.get(0), record.get(1), record.get(2)) {
            // Only process branded_food entries
            if data_type == "branded_food" {
                if let Ok(fdc_id) = fdc_id_str.parse::<i32>() {
                    if branded_fdc_set.contains(&fdc_id) {
                        food_descriptions.insert(fdc_id, description.to_string());
                        loaded_count += 1;
                    }
                }
            }
        }
    }

    println!("Loaded {} branded food descriptions", loaded_count);

    // Step 3: Stream through food_nutrient.csv to get nutrient values
    // Only process rows for branded foods we care about
    let food_nutrient_path = dir_path.join("food_nutrient.csv");
    let food_nutrient_file = File::open(&food_nutrient_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open food_nutrient.csv: {}", e),
        ))
    })?;

    let mut food_nutrients: HashMap<(i32, i32), f64> = HashMap::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(food_nutrient_file);

    let mut processed_count = 0;
    let mut relevant_rows = 0;
    println!("Processing food_nutrient.csv (this may take a while for large datasets)...");

    for result in reader.records() {
        let record = result.map_err(|e| {
            ToolboxError::Other(format!("Error reading food_nutrient.csv: {}", e))
        })?;
        
        processed_count += 1;
        if processed_count % 1_000_000 == 0 {
            println!("  Processed {} million nutrient rows, matched {} relevant rows...", 
                processed_count / 1_000_000, relevant_rows);
        }

        if let (Some(fdc_id_str), Some(nutrient_id_str), Some(amount_str)) = 
            (record.get(1), record.get(2), record.get(3)) {
            if let (Ok(fdc_id), Ok(nutrient_id)) = 
                (fdc_id_str.parse::<i32>(), nutrient_id_str.parse::<i32>()) {
                // Only process if this is a branded food we care about
                if branded_fdc_set.contains(&fdc_id) {
                    if let Ok(amount) = amount_str.parse::<f64>() {
                        food_nutrients.insert((fdc_id, nutrient_id), amount);
                        relevant_rows += 1;
                    }
                }
            }
        }
    }

    println!("Loaded {} unique nutrient values for branded foods", food_nutrients.len());

    // Step 4: Process each branded food
    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = 0;

    for fdc_id in branded_fdc_ids.iter() {
        // Get food description
        let food_name = match food_descriptions.get(fdc_id) {
            Some(name) => name.trim().to_string(),
            None => {
                if skip_errors {
                    eprintln!("FDC ID {}: No description found", fdc_id);
                    skipped += 1;
                    continue;
                } else {
                    return Err(ToolboxError::Validation(format!(
                        "FDC ID {}: No description found",
                        fdc_id
                    )));
                }
            }
        };

        if food_name.is_empty() {
            if skip_errors {
                skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "FDC ID {}: Empty description",
                    fdc_id
                )));
            }
        }

        // Extract nutrient values
        let calories = food_nutrients
            .get(&(*fdc_id, NUTRIENT_ENERGY))
            .copied()
            .unwrap_or(0.0);
        let protein = food_nutrients
            .get(&(*fdc_id, NUTRIENT_PROTEIN))
            .copied()
            .unwrap_or(0.0);
        let fat = food_nutrients
            .get(&(*fdc_id, NUTRIENT_FAT))
            .copied()
            .unwrap_or(0.0);
        let carbs = food_nutrients
            .get(&(*fdc_id, NUTRIENT_CARBS))
            .copied()
            .unwrap_or(0.0);
        let fiber = food_nutrients
            .get(&(*fdc_id, NUTRIENT_FIBER))
            .copied();
        let sugar = food_nutrients
            .get(&(*fdc_id, NUTRIENT_SUGAR))
            .copied();

        // Validate required nutrients
        if calories == 0.0 && protein == 0.0 && carbs == 0.0 && fat == 0.0 {
            if skip_errors {
                eprintln!("FDC ID {} ({}): No nutritional data found", fdc_id, food_name);
                skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "FDC ID {} ({}): No nutritional data found",
                    fdc_id, food_name
                )));
            }
        }

        // Find or create ingredient
        let ingredient = match NutritionService::find_or_create_ingredient(pool, &food_name).await {
            Ok(ing) => ing,
            Err(e) => {
                if skip_errors {
                    eprintln!("FDC ID {} ({}): Error creating ingredient: {}", fdc_id, food_name, e);
                    errors += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        // Create or update nutritional info
        // Convert f64 to BigDecimal via string parsing
        let calories_bd = calories.to_string().parse::<BigDecimal>().map_err(|e| {
            ToolboxError::Validation(format!("Invalid calories value: {}", e))
        })?;
        let protein_bd = protein.to_string().parse::<BigDecimal>().map_err(|e| {
            ToolboxError::Validation(format!("Invalid protein value: {}", e))
        })?;
        let carbs_bd = carbs.to_string().parse::<BigDecimal>().map_err(|e| {
            ToolboxError::Validation(format!("Invalid carbs value: {}", e))
        })?;
        let fat_bd = fat.to_string().parse::<BigDecimal>().map_err(|e| {
            ToolboxError::Validation(format!("Invalid fat value: {}", e))
        })?;
        let fiber_bd = fiber.and_then(|f| f.to_string().parse::<BigDecimal>().ok());
        let sugar_bd = sugar.and_then(|s| s.to_string().parse::<BigDecimal>().ok());

        match NutritionService::upsert_nutritional_info(
            pool,
            ingredient.id,
            calories_bd,
            protein_bd,
            carbs_bd,
            fat_bd,
            fiber_bd,
            sugar_bd,
        )
        .await
        {
            Ok(_) => {
                imported += 1;
                if imported % 100 == 0 {
                    println!("Imported {} ingredients...", imported);
                }
            }
            Err(e) => {
                if skip_errors {
                    eprintln!("FDC ID {} ({}): Error creating nutritional info: {}", fdc_id, food_name, e);
                    errors += 1;
                } else {
                    return Err(e);
                }
            }
        }
    }

    println!("\nImport complete:");
    println!("  Imported: {}", imported);
    if skipped > 0 {
        println!("  Skipped: {}", skipped);
    }
    if errors > 0 {
        println!("  Errors: {}", errors);
    }

    Ok(())
}

