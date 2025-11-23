use crate::error::{Result, ToolboxError};
use clap::{Args, Subcommand};
use std::process::Command;
use uuid::Uuid;

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
    /// List ingredients or recipes
    List {
        #[command(subcommand)]
        item: ListItem,
    },
    /// Get details of an ingredient or recipe
    Get {
        #[command(subcommand)]
        item: GetItem,
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
    /// Search for ingredients or recipes
    Search {
        #[command(subcommand)]
        item: SearchItem,
    },
    /// Calculate nutritional information for a recipe
    Calculate {
        /// Recipe ID
        recipe_id: String,
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
pub enum ListItem {
    /// List all ingredients
    Ingredients {
        /// Search term
        #[arg(short, long)]
        search: Option<String>,
    },
    /// List all recipes
    Recipes {
        /// Search term
        #[arg(short, long)]
        search: Option<String>,
        /// Filter by ingredient ID
        #[arg(long)]
        ingredient_id: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum GetItem {
    /// Get ingredient details
    Ingredient {
        /// Ingredient ID
        id: String,
    },
    /// Get recipe details
    Recipe {
        /// Recipe ID
        id: String,
        /// Include full details (ingredients and steps)
        #[arg(long)]
        full: bool,
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
pub enum SearchItem {
    /// Search ingredients
    Ingredients {
        /// Search term
        term: String,
    },
    /// Search recipes
    Recipes {
        /// Search term
        term: String,
        /// Filter by ingredient ID
        #[arg(long)]
        ingredient_id: Option<String>,
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
        NutritionCommands::List { item } => match item {
            ListItem::Ingredients { search } => {
                let ingredients = NutritionService::list_ingredients(pool, search.as_deref()).await?;
                println!("Found {} ingredients:", ingredients.len());
                for ingredient in ingredients {
                    println!("  - {} ({})", ingredient.name, ingredient.id);
                }
            }
            ListItem::Recipes { search, ingredient_id } => {
                let ingredient_uuid = ingredient_id.map(|id| Uuid::parse_str(&id)).transpose()?;
                let recipes = NutritionService::list_recipes(
                    pool,
                    search.as_deref(),
                    ingredient_uuid,
                )
                .await?;
                println!("Found {} recipes:", recipes.len());
                for recipe in recipes {
                    println!("  - {} ({})", recipe.name, recipe.id);
                }
            }
        },
        NutritionCommands::Get { item } => match item {
            GetItem::Ingredient { id } => {
                let ingredient_uuid = Uuid::parse_str(&id)?;
                let ingredient = NutritionService::get_ingredient(pool, ingredient_uuid).await?;
                println!("Ingredient: {}", ingredient.name);
                if let Some(desc) = &ingredient.description {
                    println!("Description: {}", desc);
                }
                println!("ID: {}", ingredient.id);
                println!("Created: {}", ingredient.created_at);
            }
            GetItem::Recipe { id, full } => {
                let recipe_uuid = Uuid::parse_str(&id)?;
                if full {
                    let recipe = NutritionService::get_recipe_with_details(pool, recipe_uuid).await?;
                    println!("Recipe: {}", recipe.recipe.name);
                    if let Some(desc) = &recipe.recipe.description {
                        println!("Description: {}", desc);
                    }
                    println!("Servings: {:?}", recipe.recipe.servings);
                    println!("Prep time: {:?} minutes", recipe.recipe.prep_time_minutes);
                    println!("Cook time: {:?} minutes", recipe.recipe.cook_time_minutes);
                    println!("\nIngredients:");
                    for ing in recipe.ingredients {
                        println!("  - {}: {} {} ({})", 
                            ing.ingredient.name, 
                            ing.recipe_ingredient.quantity, 
                            ing.recipe_ingredient.unit,
                            ing.ingredient.id
                        );
                    }
                    println!("\nSteps:");
                    for step in recipe.steps {
                        println!("  {}. {}", step.step_number, step.instruction);
                    }
                } else {
                    let recipe = NutritionService::get_recipe(pool, recipe_uuid).await?;
                    println!("Recipe: {}", recipe.name);
                    if let Some(desc) = &recipe.description {
                        println!("Description: {}", desc);
                    }
                    println!("ID: {}", recipe.id);
                    println!("Created: {}", recipe.created_at);
                }
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
        NutritionCommands::Search { item } => match item {
            SearchItem::Ingredients { term } => {
                let ingredients = NutritionService::list_ingredients(pool, Some(&term)).await?;
                println!("Found {} ingredients matching '{}':", ingredients.len(), term);
                for ingredient in ingredients {
                    println!("  - {} ({})", ingredient.name, ingredient.id);
                }
            }
            SearchItem::Recipes { term, ingredient_id } => {
                let ingredient_uuid = ingredient_id.map(|id| Uuid::parse_str(&id)).transpose()?;
                let recipes = NutritionService::list_recipes(pool, Some(&term), ingredient_uuid).await?;
                println!("Found {} recipes matching '{}':", recipes.len(), term);
                for recipe in recipes {
                    println!("  - {} ({})", recipe.name, recipe.id);
                }
            }
        },
        NutritionCommands::Calculate { recipe_id } => {
            let recipe_uuid = Uuid::parse_str(&recipe_id)?;
            let nutrition = NutritionService::calculate_recipe_nutrition(pool, recipe_uuid).await?;
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

