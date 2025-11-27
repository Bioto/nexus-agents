mod batch;
mod commands;
mod db;
mod helpers;
mod import;
mod meal_plan;

use crate::error::{Result, ToolboxError};
use crate::nutrition::{Database, NutritionService};
use clap::Args;
use uuid::Uuid;

pub use commands::*;

use batch::handle_batch_operation;
use db::handle_db_command;
use import::{
    import_recipes_from_csv, import_usda_branded_ingredients,
    import_usda_branded_ingredients_json, import_usda_ingredients,
    import_usda_ingredients_json,
};
use meal_plan::handle_meal_plan_command;

/// CLI arguments for the nutrition subcommand
#[derive(Args)]
pub struct NutritionArgs {
    #[command(subcommand)]
    pub command: NutritionCommands,
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
        NutritionCommands::ImportIngredientsJson { file, skip_errors, batch_size, concurrent_batches } => {
            import_usda_ingredients_json(pool, &file, skip_errors, batch_size, concurrent_batches).await?;
        }
        NutritionCommands::ImportBrandedIngredientsJson { file, skip_errors, limit, batch_size, concurrent_batches } => {
            import_usda_branded_ingredients_json(pool, &file, skip_errors, limit, batch_size, concurrent_batches).await?;
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

