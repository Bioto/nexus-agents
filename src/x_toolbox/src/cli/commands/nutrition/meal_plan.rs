use crate::error::{Result, ToolboxError};
use crate::nutrition::NutritionService;
use super::commands::{MealPlanCommand, MealPlanBatchOperation};
use super::helpers::parse_ids;
use chrono::NaiveDate;
use futures::future::join_all;
use uuid::Uuid;

/// Handle meal plan commands
pub async fn handle_meal_plan_command(
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

