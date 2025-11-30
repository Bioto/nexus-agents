mod batch;
mod commands;
mod db;
mod helpers;
mod import;
mod meal_plan;

use crate::error::{Result, ToolboxError};
use crate::nutrition::{Database, NutritionService};
use clap::Args;
use sqlx::PgPool;
use uuid::Uuid;

pub use commands::*;

use batch::handle_batch_operation;
use db::handle_db_command;
use import::{
    import_recipes_from_csv, import_usda_branded_ingredients, import_usda_branded_ingredients_json,
    import_usda_ingredients, import_usda_ingredients_json,
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
                let ingredient =
                    NutritionService::create_ingredient(pool, &name, description.as_deref())
                        .await?;
                println!(
                    "Created ingredient: {} ({})",
                    ingredient.name, ingredient.id
                );
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
                    calories
                        .to_string()
                        .parse()
                        .map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    protein
                        .to_string()
                        .parse()
                        .map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    carbs
                        .to_string()
                        .parse()
                        .map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    fat.to_string()
                        .parse()
                        .map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    fiber
                        .map(|f| f.to_string().parse())
                        .transpose()
                        .map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                    sugar
                        .map(|s| s.to_string().parse())
                        .transpose()
                        .map_err(|e| ToolboxError::Validation(format!("Invalid decimal: {}", e)))?,
                )
                .await?;
                println!(
                    "Created/updated nutritional info for ingredient: {}",
                    nutritional_info.ingredient_id
                );
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
            UpdateItem::Ingredient {
                id,
                name,
                description,
            } => {
                let ingredient_uuid = Uuid::parse_str(&id)?;
                let ingredient = NutritionService::update_ingredient(
                    pool,
                    ingredient_uuid,
                    name.as_deref(),
                    description.as_deref(),
                )
                .await?;
                println!(
                    "Updated ingredient: {} ({})",
                    ingredient.name, ingredient.id
                );
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
        NutritionCommands::Calculate {
            recipe_id,
            servings,
        } => {
            let recipe_uuid = Uuid::parse_str(&recipe_id)?;
            let nutrition =
                NutritionService::calculate_recipe_nutrition(pool, recipe_uuid, servings).await?;
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
        NutritionCommands::ImportIngredients {
            directory,
            skip_errors,
        } => {
            import_usda_ingredients(pool, &directory, skip_errors).await?;
        }
        NutritionCommands::ImportBrandedIngredients {
            directory,
            skip_errors,
            limit,
        } => {
            import_usda_branded_ingredients(pool, &directory, skip_errors, limit).await?;
        }
        NutritionCommands::ImportIngredientsJson {
            file,
            skip_errors,
            batch_size,
            concurrent_batches,
        } => {
            import_usda_ingredients_json(pool, &file, skip_errors, batch_size, concurrent_batches)
                .await?;
        }
        NutritionCommands::ImportBrandedIngredientsJson {
            file,
            skip_errors,
            limit,
            batch_size,
            concurrent_batches,
        } => {
            import_usda_branded_ingredients_json(
                pool,
                &file,
                skip_errors,
                limit,
                batch_size,
                concurrent_batches,
            )
            .await?;
        }
        NutritionCommands::Batch { operation } => {
            handle_batch_operation(pool, operation).await?;
        }
        NutritionCommands::MealPlan { command } => {
            handle_meal_plan_command(pool, command).await?;
        }
        NutritionCommands::Family { command } => {
            handle_family_command(pool, command).await?;
        }
        NutritionCommands::Favorite { command } => {
            handle_favorite_command(pool, command).await?;
        }
        NutritionCommands::Db { .. } => {
            // Already handled above
        }
        NutritionCommands::Export {
            recipe_id,
            output,
            include_nutrition,
        } => {
            let recipe_uuid = Uuid::parse_str(&recipe_id)?;
            let recipe = NutritionService::get_recipe_with_details(pool, recipe_uuid).await?;

            // Calculate nutrition if requested
            let nutrition = if include_nutrition {
                Some(
                    NutritionService::calculate_recipe_nutrition(
                        pool,
                        recipe_uuid,
                        recipe.recipe.servings,
                    )
                    .await?,
                )
            } else {
                None
            };

            // Determine output path
            let output_path = if let Some(path) = output {
                std::path::PathBuf::from(path)
            } else {
                // Default: output/recipes/{recipe_name}.pdf
                let output_dir = std::path::PathBuf::from("output/recipes");
                std::fs::create_dir_all(&output_dir)?;
                let sanitized_name = recipe
                    .recipe
                    .name
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
                    .collect::<String>()
                    .replace(' ', "_");
                output_dir.join(format!("{}.pdf", sanitized_name))
            };

            // Ensure parent directory exists
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Export to PDF
            crate::nutrition::pdf_export::RecipePdfExporter::export_recipe(
                &recipe,
                nutrition.as_ref(),
                &output_path,
            )?;

            println!("✅ Recipe exported to PDF: {}", output_path.display());
        }
        NutritionCommands::ExportMealPlan {
            meal_plan_id,
            output,
            include_nutrition,
            html,
        } => {
            let meal_plan_uuid = Uuid::parse_str(&meal_plan_id)?;
            let meal_plan = NutritionService::get_meal_plan_with_entries(pool, meal_plan_uuid).await?;

            // Calculate nutrition if requested
            let nutrition = if include_nutrition {
                Some(
                    NutritionService::calculate_meal_plan_nutrition(pool, meal_plan_uuid).await?,
                )
            } else {
                None
            };

            // Determine output path
            let output_path = if let Some(path) = output {
                std::path::PathBuf::from(path)
            } else {
                // Default: output/meal_plans/{meal_plan_name}.pdf
                let output_dir = std::path::PathBuf::from("output/meal_plans");
                std::fs::create_dir_all(&output_dir)?;
                let sanitized_name = meal_plan
                    .meal_plan
                    .name
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
                    .collect::<String>()
                    .replace(' ', "_");
                output_dir.join(format!("{}.pdf", sanitized_name))
            };

            // Ensure parent directory exists
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Export to PDF
            if html {
                // Use HTML/Chrome-based renderer for better quality
                crate::nutrition::pdf_export::HtmlPdfExporter::export_meal_plan(
                    &meal_plan,
                    nutrition.as_ref(),
                    pool,
                    &output_path,
                )
                .await?;
                println!("✅ Meal plan exported to PDF (HTML): {}", output_path.display());
            } else {
                // Use printpdf-based renderer
                crate::nutrition::pdf_export::RecipePdfExporter::export_meal_plan(
                    &meal_plan,
                    nutrition.as_ref(),
                    pool,
                    &output_path,
                )
                .await?;
                println!("✅ Meal plan exported to PDF: {}", output_path.display());
            }
        }
    }

    Ok(())
}

async fn handle_family_command(
    pool: &PgPool,
    command: crate::cli::commands::nutrition::commands::FamilyCommand,
) -> Result<()> {
    use crate::cli::commands::nutrition::commands::FamilyCommand;
    use serde_json;

    match command {
        FamilyCommand::Add { name, preferences } => {
            let prefs_json = preferences
                .map(|p| serde_json::from_str(&p))
                .transpose()
                .map_err(|e| {
                    ToolboxError::Validation(format!("Invalid JSON preferences: {}", e))
                })?;
            let family_member =
                NutritionService::create_family_member(pool, &name, prefs_json).await?;
            println!(
                "Created family member: {} ({})",
                family_member.name, family_member.id
            );
        }
        FamilyCommand::Get { id, with_allergies } => {
            let uuid = Uuid::parse_str(&id)?;
            if with_allergies {
                let family_member =
                    NutritionService::get_family_member_with_allergies(pool, uuid).await?;
                println!(
                    "Family Member: {} ({})",
                    family_member.family_member.name, family_member.family_member.id
                );
                if let Some(prefs) = family_member.family_member.preferences {
                    println!("Preferences: {}", serde_json::to_string_pretty(&prefs)?);
                }
                println!("\nAllergies ({}):", family_member.allergies.len());
                for allergy in family_member.allergies {
                    println!(
                        "- {} (severity: {})",
                        allergy.ingredient.name,
                        allergy.allergy.severity.as_deref().unwrap_or("unknown")
                    );
                    if let Some(notes) = allergy.allergy.notes {
                        println!("  Notes: {}", notes);
                    }
                }
            } else {
                let family_member = NutritionService::get_family_member(pool, uuid).await?;
                println!(
                    "Family Member: {} ({})",
                    family_member.name, family_member.id
                );
                if let Some(prefs) = family_member.preferences {
                    println!("Preferences: {}", serde_json::to_string_pretty(&prefs)?);
                }
            }
        }
        FamilyCommand::List { search } => {
            let family_members =
                NutritionService::list_family_members(pool, search.as_deref()).await?;
            println!("Found {} family member(s):", family_members.len());
            for fm in family_members {
                println!("- {} ({})", fm.name, fm.id);
            }
        }
        FamilyCommand::Update {
            id,
            name,
            preferences,
        } => {
            let uuid = Uuid::parse_str(&id)?;
            let prefs_json = preferences
                .map(|p| serde_json::from_str(&p))
                .transpose()
                .map_err(|e| {
                    ToolboxError::Validation(format!("Invalid JSON preferences: {}", e))
                })?;
            let family_member =
                NutritionService::update_family_member(pool, uuid, name.as_deref(), prefs_json)
                    .await?;
            println!(
                "Updated family member: {} ({})",
                family_member.name, family_member.id
            );
        }
        FamilyCommand::Delete { id } => {
            let uuid = Uuid::parse_str(&id)?;
            NutritionService::delete_family_member(pool, uuid).await?;
            println!("Deleted family member: {}", id);
        }
        FamilyCommand::AddAllergy {
            family_member_id,
            ingredient_id,
            severity,
            notes,
        } => {
            let fm_uuid = Uuid::parse_str(&family_member_id)?;
            let ing_uuid = Uuid::parse_str(&ingredient_id)?;
            let allergy = NutritionService::add_family_member_allergy(
                pool,
                fm_uuid,
                ing_uuid,
                severity.as_deref(),
                notes.as_deref(),
            )
            .await?;
            println!("Added allergy (ID: {})", allergy.id);
        }
        FamilyCommand::RemoveAllergy {
            family_member_id,
            ingredient_id,
        } => {
            let fm_uuid = Uuid::parse_str(&family_member_id)?;
            let ing_uuid = Uuid::parse_str(&ingredient_id)?;
            NutritionService::remove_family_member_allergy(pool, fm_uuid, ing_uuid).await?;
            println!("Removed allergy");
        }
        FamilyCommand::CheckAllergens {
            family_member_id,
            recipe_id,
        } => {
            let fm_uuid = Uuid::parse_str(&family_member_id)?;
            let recipe_uuid = Uuid::parse_str(&recipe_id)?;
            let allergens =
                NutritionService::check_recipe_allergens(pool, fm_uuid, recipe_uuid).await?;
            if allergens.is_empty() {
                println!("Recipe is safe - no allergens found.");
            } else {
                println!(
                    "⚠️  WARNING: Recipe contains {} allergen(s):",
                    allergens.len()
                );
                for allergen in allergens {
                    println!("- {}", allergen.name);
                }
            }
        }
    }
    Ok(())
}

async fn handle_favorite_command(
    pool: &PgPool,
    command: crate::cli::commands::nutrition::commands::FavoriteCommand,
) -> Result<()> {
    use crate::cli::commands::nutrition::commands::FavoriteCommand;

    match command {
        FavoriteCommand::Add {
            family_member_id,
            recipe_id,
            notes,
        } => {
            let fm_uuid = Uuid::parse_str(&family_member_id)?;
            let recipe_uuid = Uuid::parse_str(&recipe_id)?;
            let favorite =
                NutritionService::add_recipe_favorite(pool, fm_uuid, recipe_uuid, notes.as_deref())
                    .await?;
            println!("Added recipe to favorites (ID: {})", favorite.id);
        }
        FavoriteCommand::Remove {
            family_member_id,
            recipe_id,
        } => {
            let fm_uuid = Uuid::parse_str(&family_member_id)?;
            let recipe_uuid = Uuid::parse_str(&recipe_id)?;
            NutritionService::remove_recipe_favorite(pool, fm_uuid, recipe_uuid).await?;
            println!("Removed recipe from favorites");
        }
        FavoriteCommand::List { id } => {
            let uuid = Uuid::parse_str(&id)?;
            let favorites = NutritionService::get_family_member_favorites(pool, uuid).await?;
            println!("Favorite recipes ({}):", favorites.len());
            for fav in favorites {
                println!("- {} ({})", fav.recipe.name, fav.recipe.id);
                if let Some(notes) = fav.favorite.notes {
                    println!("  Notes: {}", notes);
                }
            }
        }
        FavoriteCommand::FavoritedBy { id } => {
            let uuid = Uuid::parse_str(&id)?;
            let family_members = NutritionService::get_recipe_favorited_by(pool, uuid).await?;
            println!(
                "Family members who favorited this recipe ({}):",
                family_members.len()
            );
            for fm in family_members {
                println!("- {} ({})", fm.name, fm.id);
            }
        }
    }
    Ok(())
}
