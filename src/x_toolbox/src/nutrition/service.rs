use crate::error::{Result, ToolboxError};
use chrono::NaiveDate;
use sqlx::{types::BigDecimal, PgPool};
use uuid::Uuid;

use super::models::*;

/// Service layer for nutrition database operations
pub struct NutritionService;

impl NutritionService {
    // ========== Ingredient Operations ==========

    /// Create a new ingredient
    pub async fn create_ingredient(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
    ) -> Result<Ingredient> {
        let ingredient = sqlx::query_as!(
            Ingredient,
            r#"
            INSERT INTO ingredients (name, description)
            VALUES ($1, $2)
            RETURNING id, name, description, created_at, updated_at
            "#,
            name,
            description
        )
        .fetch_one(pool)
        .await?;

        Ok(ingredient)
    }

    /// Get ingredient by ID
    pub async fn get_ingredient(pool: &PgPool, id: Uuid) -> Result<Ingredient> {
        let ingredient = sqlx::query_as!(
            Ingredient,
            r#"
            SELECT id, name, description, created_at, updated_at
            FROM ingredients
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Ingredient with id {} not found", id)))?;

        Ok(ingredient)
    }

    /// Find or create an ingredient by name (case-insensitive)
    pub async fn find_or_create_ingredient(
        pool: &PgPool,
        name: &str,
    ) -> Result<Ingredient> {
        // Try to find existing ingredient by exact name match (case-insensitive)
        let ingredient = sqlx::query_as!(
            Ingredient,
            r#"
            SELECT id, name, description, created_at, updated_at
            FROM ingredients
            WHERE LOWER(name) = LOWER($1)
            LIMIT 1
            "#,
            name
        )
        .fetch_optional(pool)
        .await?;

        if let Some(ingredient) = ingredient {
            Ok(ingredient)
        } else {
            // Create new ingredient if not found
            Self::create_ingredient(pool, name, None).await
        }
    }

    /// List all ingredients with optional search
    pub async fn list_ingredients(
        pool: &PgPool,
        search: Option<&str>,
    ) -> Result<Vec<Ingredient>> {
        let ingredients = if let Some(search_term) = search {
            sqlx::query_as!(
                Ingredient,
                r#"
                SELECT id, name, description, created_at, updated_at
                FROM ingredients
                WHERE name ILIKE $1
                ORDER BY name
                "#,
                format!("%{}%", search_term)
            )
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as!(
                Ingredient,
                r#"
                SELECT id, name, description, created_at, updated_at
                FROM ingredients
                ORDER BY name
                "#
            )
            .fetch_all(pool)
            .await?
        };

        Ok(ingredients)
    }

    /// Update ingredient
    pub async fn update_ingredient(
        pool: &PgPool,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<Ingredient> {
        if name.is_none() && description.is_none() {
            return Self::get_ingredient(pool, id).await;
        }

        // For simplicity, use a prepared approach
        let ingredient = if name.is_some() && description.is_some() {
            sqlx::query_as!(
                Ingredient,
                r#"
                UPDATE ingredients
                SET name = $1, description = $2
                WHERE id = $3
                RETURNING id, name, description, created_at, updated_at
                "#,
                name.unwrap(),
                description,
                id
            )
            .fetch_optional(pool)
            .await?
        } else if name.is_some() {
            sqlx::query_as!(
                Ingredient,
                r#"
                UPDATE ingredients
                SET name = $1
                WHERE id = $2
                RETURNING id, name, description, created_at, updated_at
                "#,
                name.unwrap(),
                id
            )
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query_as!(
                Ingredient,
                r#"
                UPDATE ingredients
                SET description = $1
                WHERE id = $2
                RETURNING id, name, description, created_at, updated_at
                "#,
                description,
                id
            )
            .fetch_optional(pool)
            .await?
        }
        .ok_or_else(|| ToolboxError::NotFound(format!("Ingredient with id {} not found", id)))?;

        Ok(ingredient)
    }

    /// Delete ingredient
    pub async fn delete_ingredient(pool: &PgPool, id: Uuid) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM ingredients
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Ingredient with id {} not found",
                id
            )));
        }

        Ok(())
    }

    // ========== Nutritional Info Operations ==========

    /// Create or update nutritional info for an ingredient
    pub async fn upsert_nutritional_info(
        pool: &PgPool,
        ingredient_id: Uuid,
        calories_per_100g: BigDecimal,
        protein_g: BigDecimal,
        carbs_g: BigDecimal,
        fat_g: BigDecimal,
        fiber_g: Option<BigDecimal>,
        sugar_g: Option<BigDecimal>,
    ) -> Result<NutritionalInfo> {
        let nutritional_info = sqlx::query_as!(
            NutritionalInfo,
            r#"
            INSERT INTO nutritional_info (
                ingredient_id, calories_per_100g, protein_g, carbs_g, fat_g, fiber_g, sugar_g
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (ingredient_id)
            DO UPDATE SET
                calories_per_100g = $2,
                protein_g = $3,
                carbs_g = $4,
                fat_g = $5,
                fiber_g = $6,
                sugar_g = $7,
                updated_at = CURRENT_TIMESTAMP
            RETURNING id, ingredient_id, calories_per_100g, protein_g, carbs_g, fat_g, fiber_g, sugar_g, created_at, updated_at
            "#,
            ingredient_id,
            calories_per_100g,
            protein_g,
            carbs_g,
            fat_g,
            fiber_g,
            sugar_g
        )
        .fetch_one(pool)
        .await?;

        Ok(nutritional_info)
    }

    /// Get nutritional info by ingredient ID
    pub async fn get_nutritional_info(
        pool: &PgPool,
        ingredient_id: Uuid,
    ) -> Result<Option<NutritionalInfo>> {
        let nutritional_info = sqlx::query_as!(
            NutritionalInfo,
            r#"
            SELECT id, ingredient_id, calories_per_100g, protein_g, carbs_g, fat_g, fiber_g, sugar_g, created_at, updated_at
            FROM nutritional_info
            WHERE ingredient_id = $1
            "#,
            ingredient_id
        )
        .fetch_optional(pool)
        .await?;

        Ok(nutritional_info)
    }

    // ========== Recipe Operations ==========

    /// Create a new recipe with ingredients and steps
    pub async fn create_recipe(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
        servings: Option<i32>,
        prep_time_minutes: Option<i32>,
        cook_time_minutes: Option<i32>,
        ingredients: Vec<(Uuid, BigDecimal, String)>,
        steps: Vec<(i32, String)>,
    ) -> Result<Recipe> {
        let mut tx = pool.begin().await?;

        // Create recipe
        let recipe = sqlx::query_as!(
            Recipe,
            r#"
            INSERT INTO recipes (name, description, servings, prep_time_minutes, cook_time_minutes)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, name, description, servings, prep_time_minutes, cook_time_minutes, created_at, updated_at
            "#,
            name,
            description,
            servings,
            prep_time_minutes,
            cook_time_minutes
        )
        .fetch_one(&mut *tx)
        .await?;

        // Add ingredients
        for (ingredient_id, quantity, unit) in ingredients {
            sqlx::query!(
                r#"
                INSERT INTO recipe_ingredients (recipe_id, ingredient_id, quantity, unit)
                VALUES ($1, $2, $3, $4)
                "#,
                recipe.id,
                ingredient_id,
                quantity,
                unit
            )
            .execute(&mut *tx)
            .await?;
        }

        // Add steps
        for (step_number, instruction) in steps {
            sqlx::query!(
                r#"
                INSERT INTO recipe_steps (recipe_id, step_number, instruction)
                VALUES ($1, $2, $3)
                "#,
                recipe.id,
                step_number,
                instruction
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(recipe)
    }

    /// Get recipe by ID
    pub async fn get_recipe(pool: &PgPool, id: Uuid) -> Result<Recipe> {
        let recipe = sqlx::query_as!(
            Recipe,
            r#"
            SELECT id, name, description, servings, prep_time_minutes, cook_time_minutes, created_at, updated_at
            FROM recipes
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Recipe with id {} not found", id)))?;

        Ok(recipe)
    }

    /// Get recipe with full details (ingredients and steps)
    pub async fn get_recipe_with_details(
        pool: &PgPool,
        id: Uuid,
    ) -> Result<RecipeWithDetails> {
        let recipe = Self::get_recipe(pool, id).await?;

        // Get ingredients with ingredient details
        let recipe_ingredients = sqlx::query_as!(
            RecipeIngredient,
            r#"
            SELECT id, recipe_id, ingredient_id, quantity, unit, created_at
            FROM recipe_ingredients
            WHERE recipe_id = $1
            ORDER BY created_at
            "#,
            id
        )
        .fetch_all(pool)
        .await?;

        let mut ingredients_with_details = Vec::new();
        for ri in recipe_ingredients {
            let ingredient = Self::get_ingredient(pool, ri.ingredient_id).await?;
            let nutritional_info = Self::get_nutritional_info(pool, ri.ingredient_id).await?;
            ingredients_with_details.push(RecipeIngredientWithIngredient {
                recipe_ingredient: ri,
                ingredient,
                nutritional_info,
            });
        }

        // Get steps
        let steps = sqlx::query_as!(
            RecipeStep,
            r#"
            SELECT id, recipe_id, step_number, instruction, created_at
            FROM recipe_steps
            WHERE recipe_id = $1
            ORDER BY step_number
            "#,
            id
        )
        .fetch_all(pool)
        .await?;

        Ok(RecipeWithDetails {
            recipe,
            ingredients: ingredients_with_details,
            steps,
        })
    }

    /// List all recipes with optional search
    pub async fn list_recipes(
        pool: &PgPool,
        search: Option<&str>,
        ingredient_filter: Option<Uuid>,
    ) -> Result<Vec<Recipe>> {
        let recipes = if let Some(search_term) = search {
            if let Some(ingredient_id) = ingredient_filter {
                sqlx::query_as!(
                    Recipe,
                    r#"
                    SELECT DISTINCT r.id, r.name, r.description, r.servings, r.prep_time_minutes, r.cook_time_minutes, r.created_at, r.updated_at
                    FROM recipes r
                    INNER JOIN recipe_ingredients ri ON r.id = ri.recipe_id
                    WHERE (r.name ILIKE $1 OR r.description ILIKE $1)
                    AND ri.ingredient_id = $2
                    ORDER BY r.name
                    "#,
                    format!("%{}%", search_term),
                    ingredient_id
                )
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as!(
                    Recipe,
                    r#"
                    SELECT id, name, description, servings, prep_time_minutes, cook_time_minutes, created_at, updated_at
                    FROM recipes
                    WHERE name ILIKE $1 OR description ILIKE $1
                    ORDER BY name
                    "#,
                    format!("%{}%", search_term)
                )
                .fetch_all(pool)
                .await?
            }
        } else if let Some(ingredient_id) = ingredient_filter {
            sqlx::query_as!(
                Recipe,
                r#"
                SELECT DISTINCT r.id, r.name, r.description, r.servings, r.prep_time_minutes, r.cook_time_minutes, r.created_at, r.updated_at
                FROM recipes r
                INNER JOIN recipe_ingredients ri ON r.id = ri.recipe_id
                WHERE ri.ingredient_id = $1
                ORDER BY r.name
                "#,
                ingredient_id
            )
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as!(
                Recipe,
                r#"
                SELECT id, name, description, servings, prep_time_minutes, cook_time_minutes, created_at, updated_at
                FROM recipes
                ORDER BY name
                "#
            )
            .fetch_all(pool)
            .await?
        };

        Ok(recipes)
    }

    /// Update recipe
    pub async fn update_recipe(
        pool: &PgPool,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        servings: Option<i32>,
        prep_time_minutes: Option<i32>,
        cook_time_minutes: Option<i32>,
    ) -> Result<Recipe> {
        // Build update query dynamically
        let recipe = if name.is_some() || description.is_some() || servings.is_some()
            || prep_time_minutes.is_some() || cook_time_minutes.is_some()
        {
            // For simplicity, update all fields
            sqlx::query_as!(
                Recipe,
                r#"
                UPDATE recipes
                SET
                    name = COALESCE($1, name),
                    description = COALESCE($2, description),
                    servings = COALESCE($3, servings),
                    prep_time_minutes = COALESCE($4, prep_time_minutes),
                    cook_time_minutes = COALESCE($5, cook_time_minutes)
                WHERE id = $6
                RETURNING id, name, description, servings, prep_time_minutes, cook_time_minutes, created_at, updated_at
                "#,
                name,
                description,
                servings,
                prep_time_minutes,
                cook_time_minutes,
                id
            )
            .fetch_optional(pool)
            .await?
        } else {
            None
        }
        .ok_or_else(|| ToolboxError::NotFound(format!("Recipe with id {} not found", id)))?;

        Ok(recipe)
    }

    /// Delete recipe
    pub async fn delete_recipe(pool: &PgPool, id: Uuid) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM recipes
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Recipe with id {} not found",
                id
            )));
        }

        Ok(())
    }

    // ========== Recipe Ingredient Operations ==========

    /// Add an ingredient to a recipe
    pub async fn add_recipe_ingredient(
        pool: &PgPool,
        recipe_id: Uuid,
        ingredient_id: Uuid,
        quantity: BigDecimal,
        unit: String,
    ) -> Result<RecipeIngredient> {
        let recipe_ingredient = sqlx::query_as!(
            RecipeIngredient,
            r#"
            INSERT INTO recipe_ingredients (recipe_id, ingredient_id, quantity, unit)
            VALUES ($1, $2, $3, $4)
            RETURNING id, recipe_id, ingredient_id, quantity, unit, created_at
            "#,
            recipe_id,
            ingredient_id,
            quantity,
            unit
        )
        .fetch_one(pool)
        .await?;

        Ok(recipe_ingredient)
    }

    /// Remove an ingredient from a recipe
    pub async fn remove_recipe_ingredient(
        pool: &PgPool,
        recipe_id: Uuid,
        ingredient_id: Uuid,
    ) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM recipe_ingredients
            WHERE recipe_id = $1 AND ingredient_id = $2
            "#,
            recipe_id,
            ingredient_id
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Ingredient {} not found in recipe {}",
                ingredient_id, recipe_id
            )));
        }

        Ok(())
    }

    // ========== Recipe Step Operations ==========

    /// Add a step to a recipe
    pub async fn add_recipe_step(
        pool: &PgPool,
        recipe_id: Uuid,
        step_number: i32,
        instruction: String,
    ) -> Result<RecipeStep> {
        let step = sqlx::query_as!(
            RecipeStep,
            r#"
            INSERT INTO recipe_steps (recipe_id, step_number, instruction)
            VALUES ($1, $2, $3)
            RETURNING id, recipe_id, step_number, instruction, created_at
            "#,
            recipe_id,
            step_number,
            instruction
        )
        .fetch_one(pool)
        .await?;

        Ok(step)
    }

    /// Remove a step from a recipe
    pub async fn remove_recipe_step(pool: &PgPool, recipe_id: Uuid, step_number: i32) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM recipe_steps
            WHERE recipe_id = $1 AND step_number = $2
            "#,
            recipe_id,
            step_number
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Step {} not found in recipe {}",
                step_number, recipe_id
            )));
        }

        Ok(())
    }

    // ========== Nutritional Calculation ==========

    /// Convert quantity from various units to grams
    /// This function handles common cooking units and ingredient-specific conversions
    fn convert_to_grams(quantity: &BigDecimal, unit: &str, ingredient_name: &str) -> BigDecimal {
        let unit_lower = unit.to_lowercase();
        let name_lower = ingredient_name.to_lowercase();
        
        match unit_lower.as_str() {
            "g" | "gram" | "grams" => quantity.clone(),
            "kg" | "kilogram" | "kilograms" => quantity * BigDecimal::from(1000_i32),
            "oz" | "ounce" | "ounces" => quantity * "28.3495".parse::<BigDecimal>().unwrap(),
            "lb" | "lbs" | "pound" | "pounds" => quantity * "453.592".parse::<BigDecimal>().unwrap(),
            "cup" | "cups" => {
                // Ingredient-specific conversions for cups
                if name_lower.contains("rice") {
                    quantity * BigDecimal::from(200_i32) // ~200g per cup of uncooked rice
                } else if name_lower.contains("oil") || name_lower.contains("olive") {
                    quantity * "218".parse::<BigDecimal>().unwrap() // ~218g per cup of olive oil
                } else if name_lower.contains("water") || name_lower.contains("broth") || name_lower.contains("stock") {
                    quantity * "236.588".parse::<BigDecimal>().unwrap() // ~237g per cup of liquid
                } else {
                    // Default: assume similar density to water
                    quantity * "236.588".parse::<BigDecimal>().unwrap()
                }
            },
            "tbsp" | "tablespoon" | "tablespoons" => {
                if name_lower.contains("oil") || name_lower.contains("olive") {
                    quantity * "13.6".parse::<BigDecimal>().unwrap() // ~13.6g per tbsp of oil
                } else {
                    quantity * "15".parse::<BigDecimal>().unwrap() // ~15g per tbsp (general)
                }
            },
            "tsp" | "teaspoon" | "teaspoons" => {
                if name_lower.contains("oil") || name_lower.contains("olive") {
                    quantity * "4.5".parse::<BigDecimal>().unwrap() // ~4.5g per tsp of oil
                } else {
                    quantity * "5".parse::<BigDecimal>().unwrap() // ~5g per tsp (general)
                }
            },
            "piece" | "pieces" | "whole" | "item" | "items" => {
                // Ingredient-specific conversions for pieces
                if name_lower.contains("lemon") {
                    quantity * BigDecimal::from(100_i32) // ~100g per lemon
                } else if name_lower.contains("garlic") && (name_lower.contains("clove") || name_lower.contains("cloves")) {
                    quantity * BigDecimal::from(3_i32) // ~3g per garlic clove
                } else if name_lower.contains("potato") || name_lower.contains("potatoes") {
                    quantity * BigDecimal::from(150_i32) // ~150g per medium potato
                } else {
                    // Default: assume 100g per piece
                    quantity * BigDecimal::from(100_i32)
                }
            },
            "clove" | "cloves" => {
                quantity * BigDecimal::from(3_i32) // ~3g per garlic clove
            },
            _ => {
                // Unknown unit - assume it's already in grams or log a warning
                // In production, you might want to log this
                quantity.clone()
            }
        }
    }

    /// Calculate nutritional information for a recipe
    pub async fn calculate_recipe_nutrition(
        pool: &PgPool,
        recipe_id: Uuid,
    ) -> Result<RecipeNutrition> {
        let recipe = Self::get_recipe(pool, recipe_id).await?;

        // Get all ingredients with their nutritional info
        let recipe_ingredients = sqlx::query_as!(
            RecipeIngredient,
            r#"
            SELECT id, recipe_id, ingredient_id, quantity, unit, created_at
            FROM recipe_ingredients
            WHERE recipe_id = $1
            "#,
            recipe_id
        )
        .fetch_all(pool)
        .await?;

        let mut total_calories = BigDecimal::from(0_i32);
        let mut total_protein = BigDecimal::from(0_i32);
        let mut total_carbs = BigDecimal::from(0_i32);
        let mut total_fat = BigDecimal::from(0_i32);
        let mut total_fiber = BigDecimal::from(0_i32);
        let mut total_sugar = BigDecimal::from(0_i32);

        for ri in recipe_ingredients {
            if let Some(nutritional_info) = Self::get_nutritional_info(pool, ri.ingredient_id).await? {
                // Get ingredient name for unit conversion
                let ingredient = Self::get_ingredient(pool, ri.ingredient_id).await?;
                
                // Convert quantity to grams
                let quantity_grams = Self::convert_to_grams(&ri.quantity, &ri.unit, &ingredient.name);
                
                // Calculate multiplier: quantity in grams / 100g (since nutritional info is per 100g)
                let multiplier = &quantity_grams / &BigDecimal::from(100_i32);
                
                total_calories += &nutritional_info.calories_per_100g * &multiplier;
                total_protein += &nutritional_info.protein_g * &multiplier;
                total_carbs += &nutritional_info.carbs_g * &multiplier;
                total_fat += &nutritional_info.fat_g * &multiplier;
                if let Some(ref fiber) = nutritional_info.fiber_g {
                    total_fiber += fiber * &multiplier;
                }
                if let Some(ref sugar) = nutritional_info.sugar_g {
                    total_sugar += sugar * &multiplier;
                }
            }
        }

        let per_serving_calories = recipe.servings.map(|s| total_calories.clone() / BigDecimal::from(s));
        let per_serving_protein = recipe.servings.map(|s| total_protein.clone() / BigDecimal::from(s));
        let per_serving_carbs = recipe.servings.map(|s| total_carbs.clone() / BigDecimal::from(s));
        let per_serving_fat = recipe.servings.map(|s| total_fat.clone() / BigDecimal::from(s));

        Ok(RecipeNutrition {
            recipe_id,
            total_calories,
            total_protein_g: total_protein,
            total_carbs_g: total_carbs,
            total_fat_g: total_fat,
            total_fiber_g: if total_fiber > BigDecimal::from(0_i32) {
                Some(total_fiber)
            } else {
                None
            },
            total_sugar_g: if total_sugar > BigDecimal::from(0_i32) {
                Some(total_sugar)
            } else {
                None
            },
            per_serving_calories,
            per_serving_protein_g: per_serving_protein,
            per_serving_carbs_g: per_serving_carbs,
            per_serving_fat_g: per_serving_fat,
        })
    }

    // ========== Recipe Extraction from URL ==========

    /// Extract recipe information from a URL using LLM
    pub async fn extract_recipe_from_url(url: &str) -> Result<ExtractedRecipe> {
        use reqwest::Client;
        use scraper::{Html, Selector};
        use std::env;

        // Fetch HTML content
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .map_err(|e| ToolboxError::Other(format!("Failed to create HTTP client: {}", e)))?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| ToolboxError::Other(format!("Failed to fetch URL: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolboxError::Other(format!(
                "Failed to fetch URL: HTTP {}",
                response.status()
            )));
        }

        let html_content = response
            .text()
            .await
            .map_err(|e| ToolboxError::Other(format!("Failed to read response: {}", e)))?;

        // Extract text from HTML (do this synchronously before any await)
        let text_content = {
            let document = Html::parse_document(&html_content);
            let body_selector = Selector::parse("body").unwrap();
            document
                .select(&body_selector)
                .next()
                .map(|body| {
                    // Extract text from body, removing script and style tags
                    let mut text = String::new();
                    for text_node in body.text() {
                        let trimmed = text_node.trim();
                        if !trimmed.is_empty() {
                            text.push_str(trimmed);
                            text.push(' ');
                        }
                    }
                    text
                })
                .unwrap_or_else(|| {
                    // Fallback: use first 10000 chars of HTML if body extraction fails
                    html_content.chars().take(10000).collect()
                })
        };

        // Truncate to reasonable size for LLM (keep first 50000 chars)
        let text_for_llm = if text_content.len() > 50000 {
            text_content.chars().take(50000).collect::<String>()
                + "\n[... content truncated ...]"
        } else {
            text_content
        };

        // Call LLM API to extract recipe
        let llm_api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| ToolboxError::Configuration(
                "OPENAI_API_KEY environment variable not set".to_string()
            ))?;

        let llm_base_url = env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        let model = env::var("DEFAULT_MODEL")
            .unwrap_or_else(|_| "gpt-4o-mini".to_string());

        // Create prompt for recipe extraction
        let prompt = format!(
            r#"Extract recipe information from the following HTML content. Return a JSON object with the following structure:
{{
  "name": "Recipe name",
  "description": "Optional description",
  "servings": optional_number,
  "prep_time_minutes": optional_number,
  "cook_time_minutes": optional_number,
  "ingredients": [
    {{"name": "ingredient name", "quantity": number, "unit": "unit string"}}
  ],
  "steps": ["step 1", "step 2", ...]
}}

HTML content:
{}

Return only valid JSON, no markdown formatting."#,
            text_for_llm
        );

        // Make LLM API call
        let llm_request = serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.1,
            "response_format": {
                "type": "json_object"
            }
        });

        let llm_response = client
            .post(&format!("{}/chat/completions", llm_base_url))
            .header("Authorization", format!("Bearer {}", llm_api_key))
            .header("Content-Type", "application/json")
            .json(&llm_request)
            .send()
            .await
            .map_err(|e| ToolboxError::Other(format!("Failed to call LLM API: {}", e)))?;

        let status = llm_response.status();
        if !status.is_success() {
            let error_text = llm_response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToolboxError::Other(format!(
                "LLM API error: HTTP {} - {}",
                status,
                error_text
            )));
        }

        let llm_json: serde_json::Value = llm_response
            .json()
            .await
            .map_err(|e| ToolboxError::Other(format!("Failed to parse LLM response: {}", e)))?;

        // Extract the recipe JSON from LLM response
        let recipe_json_str = llm_json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| ToolboxError::Other("Invalid LLM response format".to_string()))?;

        // Parse the extracted recipe
        let extracted: ExtractedRecipe = serde_json::from_str(recipe_json_str)
            .map_err(|e| ToolboxError::Other(format!("Failed to parse extracted recipe: {}", e)))?;

        Ok(extracted)
    }

    // ========== Meal Plan Operations ==========

    /// Create a new meal plan
    pub async fn create_meal_plan(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        is_template: bool,
    ) -> Result<MealPlan> {
        // Validate dates
        if let (Some(start), Some(end)) = (start_date, end_date) {
            if start > end {
                return Err(ToolboxError::Validation(
                    "start_date must be less than or equal to end_date".to_string(),
                ));
            }
        }

        let meal_plan = sqlx::query_as!(
            MealPlan,
            r#"
            INSERT INTO meal_plans (name, description, start_date, end_date, is_template)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, name, description, start_date, end_date, is_template, created_at, updated_at
            "#,
            name,
            description,
            start_date,
            end_date,
            is_template
        )
        .fetch_one(pool)
        .await?;

        Ok(meal_plan)
    }

    /// Get meal plan by ID
    pub async fn get_meal_plan(pool: &PgPool, id: Uuid) -> Result<MealPlan> {
        let meal_plan = sqlx::query_as!(
            MealPlan,
            r#"
            SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
            FROM meal_plans
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Meal plan with id {} not found", id)))?;

        Ok(meal_plan)
    }

    /// Get meal plan with all entries
    pub async fn get_meal_plan_with_entries(
        pool: &PgPool,
        id: Uuid,
    ) -> Result<MealPlanWithEntries> {
        let meal_plan = Self::get_meal_plan(pool, id).await?;

        // Get all entries with recipe details
        let entries = sqlx::query_as!(
            MealPlanEntry,
            r#"
            SELECT id, meal_plan_id, day_of_week, date, meal_type, recipe_id, created_at
            FROM meal_plan_entries
            WHERE meal_plan_id = $1
            ORDER BY 
                CASE 
                    WHEN date IS NOT NULL THEN date
                    ELSE NULL
                END,
                day_of_week,
                CASE meal_type
                    WHEN 'breakfast' THEN 1
                    WHEN 'lunch' THEN 2
                    WHEN 'dinner' THEN 3
                    WHEN 'snack' THEN 4
                    ELSE 5
                END
            "#,
            id
        )
        .fetch_all(pool)
        .await?;

        let mut entries_with_recipes = Vec::new();
        for entry in entries {
            let recipe = Self::get_recipe(pool, entry.recipe_id).await?;
            entries_with_recipes.push(MealPlanEntryWithRecipe {
                entry,
                recipe,
            });
        }

        Ok(MealPlanWithEntries {
            meal_plan,
            entries: entries_with_recipes,
        })
    }

    /// Update meal plan
    pub async fn update_meal_plan(
        pool: &PgPool,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<MealPlan> {
        // Validate dates if both provided
        if let (Some(start), Some(end)) = (start_date, end_date) {
            if start > end {
                return Err(ToolboxError::Validation(
                    "start_date must be less than or equal to end_date".to_string(),
                ));
            }
        }

        let meal_plan = sqlx::query_as!(
            MealPlan,
            r#"
            UPDATE meal_plans
            SET
                name = COALESCE($1, name),
                description = COALESCE($2, description),
                start_date = COALESCE($3, start_date),
                end_date = COALESCE($4, end_date)
            WHERE id = $5
            RETURNING id, name, description, start_date, end_date, is_template, created_at, updated_at
            "#,
            name,
            description,
            start_date,
            end_date,
            id
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Meal plan with id {} not found", id)))?;

        Ok(meal_plan)
    }

    /// Delete meal plan
    pub async fn delete_meal_plan(pool: &PgPool, id: Uuid) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM meal_plans
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Meal plan with id {} not found",
                id
            )));
        }

        Ok(())
    }

    /// Add entry to meal plan
    pub async fn add_meal_plan_entry(
        pool: &PgPool,
        meal_plan_id: Uuid,
        recipe_id: Uuid,
        meal_type: &str,
        day_of_week: Option<i32>,
        date: Option<NaiveDate>,
    ) -> Result<MealPlanEntry> {
        // Validate meal type
        if !matches!(meal_type, "breakfast" | "lunch" | "dinner" | "snack") {
            return Err(ToolboxError::Validation(format!(
                "Invalid meal_type: {}. Must be one of: breakfast, lunch, dinner, snack",
                meal_type
            )));
        }

        // Validate that either day_of_week or date is provided, but not both
        match (day_of_week, date) {
            (Some(_dow), Some(_)) => {
                return Err(ToolboxError::Validation(
                    "Cannot specify both day_of_week and date".to_string(),
                ));
            }
            (None, None) => {
                return Err(ToolboxError::Validation(
                    "Must specify either day_of_week or date".to_string(),
                ));
            }
            (Some(dow), None) if dow < 0 || dow > 6 => {
                return Err(ToolboxError::Validation(
                    "day_of_week must be between 0 (Monday) and 6 (Sunday)".to_string(),
                ));
            }
            _ => {}
        }

        // Verify meal plan exists
        Self::get_meal_plan(pool, meal_plan_id).await?;

        // Verify recipe exists
        Self::get_recipe(pool, recipe_id).await?;

        let entry = sqlx::query_as!(
            MealPlanEntry,
            r#"
            INSERT INTO meal_plan_entries (meal_plan_id, recipe_id, meal_type, day_of_week, date)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, meal_plan_id, day_of_week, date, meal_type, recipe_id, created_at
            "#,
            meal_plan_id,
            recipe_id,
            meal_type,
            day_of_week,
            date
        )
        .fetch_one(pool)
        .await?;

        Ok(entry)
    }

    /// Remove entry from meal plan
    pub async fn remove_meal_plan_entry(pool: &PgPool, entry_id: Uuid) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM meal_plan_entries
            WHERE id = $1
            "#,
            entry_id
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Meal plan entry with id {} not found",
                entry_id
            )));
        }

        Ok(())
    }

    /// List meal plans with optional filters
    pub async fn list_meal_plans(
        pool: &PgPool,
        search: Option<&str>,
        is_template: Option<bool>,
        start_date_filter: Option<NaiveDate>,
        end_date_filter: Option<NaiveDate>,
    ) -> Result<Vec<MealPlan>> {
        let search_term = search.map(|s| format!("%{}%", s));
        
        let meal_plans = if let Some(search_str) = search_term {
            if let Some(template) = is_template {
                if let (Some(start), Some(end)) = (start_date_filter, end_date_filter) {
                    sqlx::query_as!(
                        MealPlan,
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                        FROM meal_plans
                        WHERE is_template = $1
                        AND (name ILIKE $2 OR description ILIKE $2)
                        AND (start_date IS NULL OR start_date <= $3)
                        AND (end_date IS NULL OR end_date >= $4)
                        ORDER BY name
                        "#,
                        template,
                        search_str,
                        start,
                        end
                    )
                    .fetch_all(pool)
                    .await?
                } else if let Some(start) = start_date_filter {
                    sqlx::query_as!(
                        MealPlan,
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                        FROM meal_plans
                        WHERE is_template = $1
                        AND (name ILIKE $2 OR description ILIKE $2)
                        AND (start_date IS NULL OR start_date <= $3)
                        ORDER BY name
                        "#,
                        template,
                        search_str,
                        start
                    )
                    .fetch_all(pool)
                    .await?
                } else if let Some(end) = end_date_filter {
                    sqlx::query_as!(
                        MealPlan,
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                        FROM meal_plans
                        WHERE is_template = $1
                        AND (name ILIKE $2 OR description ILIKE $2)
                        AND (end_date IS NULL OR end_date >= $3)
                        ORDER BY name
                        "#,
                        template,
                        search_str,
                        end
                    )
                    .fetch_all(pool)
                    .await?
                } else {
                    sqlx::query_as!(
                        MealPlan,
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                        FROM meal_plans
                        WHERE is_template = $1
                        AND (name ILIKE $2 OR description ILIKE $2)
                        ORDER BY name
                        "#,
                        template,
                        search_str
                    )
                    .fetch_all(pool)
                    .await?
                }
            } else {
                if let (Some(start), Some(end)) = (start_date_filter, end_date_filter) {
                    sqlx::query_as!(
                        MealPlan,
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                        FROM meal_plans
                        WHERE (name ILIKE $1 OR description ILIKE $1)
                        AND (start_date IS NULL OR start_date <= $2)
                        AND (end_date IS NULL OR end_date >= $3)
                        ORDER BY name
                        "#,
                        search_str,
                        start,
                        end
                    )
                    .fetch_all(pool)
                    .await?
                } else if let Some(start) = start_date_filter {
                    sqlx::query_as!(
                        MealPlan,
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                        FROM meal_plans
                        WHERE (name ILIKE $1 OR description ILIKE $1)
                        AND (start_date IS NULL OR start_date <= $2)
                        ORDER BY name
                        "#,
                        search_str,
                        start
                    )
                    .fetch_all(pool)
                    .await?
                } else if let Some(end) = end_date_filter {
                    sqlx::query_as!(
                        MealPlan,
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                        FROM meal_plans
                        WHERE (name ILIKE $1 OR description ILIKE $1)
                        AND (end_date IS NULL OR end_date >= $2)
                        ORDER BY name
                        "#,
                        search_str,
                        end
                    )
                    .fetch_all(pool)
                    .await?
                } else {
                    sqlx::query_as!(
                        MealPlan,
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                        FROM meal_plans
                        WHERE (name ILIKE $1 OR description ILIKE $1)
                        ORDER BY name
                        "#,
                        search_str
                    )
                    .fetch_all(pool)
                    .await?
                }
            }
        } else if let Some(template) = is_template {
            if let (Some(start), Some(end)) = (start_date_filter, end_date_filter) {
                sqlx::query_as!(
                    MealPlan,
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                    FROM meal_plans
                    WHERE is_template = $1
                    AND (start_date IS NULL OR start_date <= $2)
                    AND (end_date IS NULL OR end_date >= $3)
                    ORDER BY name
                    "#,
                    template,
                    start,
                    end
                )
                .fetch_all(pool)
                .await?
            } else if let Some(start) = start_date_filter {
                sqlx::query_as!(
                    MealPlan,
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                    FROM meal_plans
                    WHERE is_template = $1
                    AND (start_date IS NULL OR start_date <= $2)
                    ORDER BY name
                    "#,
                    template,
                    start
                )
                .fetch_all(pool)
                .await?
            } else if let Some(end) = end_date_filter {
                sqlx::query_as!(
                    MealPlan,
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                    FROM meal_plans
                    WHERE is_template = $1
                    AND (end_date IS NULL OR end_date >= $2)
                    ORDER BY name
                    "#,
                    template,
                    end
                )
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as!(
                    MealPlan,
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                    FROM meal_plans
                    WHERE is_template = $1
                    ORDER BY name
                    "#,
                    template
                )
                .fetch_all(pool)
                .await?
            }
        } else {
            if let (Some(start), Some(end)) = (start_date_filter, end_date_filter) {
                sqlx::query_as!(
                    MealPlan,
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                    FROM meal_plans
                    WHERE (start_date IS NULL OR start_date <= $1)
                    AND (end_date IS NULL OR end_date >= $2)
                    ORDER BY name
                    "#,
                    start,
                    end
                )
                .fetch_all(pool)
                .await?
            } else if let Some(start) = start_date_filter {
                sqlx::query_as!(
                    MealPlan,
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                    FROM meal_plans
                    WHERE (start_date IS NULL OR start_date <= $1)
                    ORDER BY name
                    "#,
                    start
                )
                .fetch_all(pool)
                .await?
            } else if let Some(end) = end_date_filter {
                sqlx::query_as!(
                    MealPlan,
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                    FROM meal_plans
                    WHERE (end_date IS NULL OR end_date >= $1)
                    ORDER BY name
                    "#,
                    end
                )
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as!(
                    MealPlan,
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, created_at, updated_at
                    FROM meal_plans
                    ORDER BY name
                    "#
                )
                .fetch_all(pool)
                .await?
            }
        };

        Ok(meal_plans)
    }

    /// Calculate nutritional information for a meal plan
    pub async fn calculate_meal_plan_nutrition(
        pool: &PgPool,
        meal_plan_id: Uuid,
    ) -> Result<MealPlanNutrition> {
        let meal_plan = Self::get_meal_plan(pool, meal_plan_id).await?;

        // Get all entries
        let entries = sqlx::query_as!(
            MealPlanEntry,
            r#"
            SELECT id, meal_plan_id, day_of_week, date, meal_type, recipe_id, created_at
            FROM meal_plan_entries
            WHERE meal_plan_id = $1
            "#,
            meal_plan_id
        )
        .fetch_all(pool)
        .await?;

        // Group entries by date or day_of_week
        use std::collections::HashMap;
        let mut daily_map: HashMap<(Option<NaiveDate>, Option<i32>), Vec<MealNutrition>> =
            HashMap::new();

        for entry in entries {
            let key = (entry.date, entry.day_of_week);
            let recipe_nutrition =
                Self::calculate_recipe_nutrition(pool, entry.recipe_id).await?;

            let meal_nutrition = daily_map.entry(key).or_insert_with(Vec::new);

            // Find or create meal nutrition for this meal type
            let meal_nut = meal_nutrition
                .iter_mut()
                .find(|m| m.meal_type == entry.meal_type);

            if let Some(meal) = meal_nut {
                // Add to existing meal
                meal.calories += &recipe_nutrition.total_calories;
                meal.protein_g += &recipe_nutrition.total_protein_g;
                meal.carbs_g += &recipe_nutrition.total_carbs_g;
                meal.fat_g += &recipe_nutrition.total_fat_g;
                if let Some(ref fiber) = recipe_nutrition.total_fiber_g {
                    meal.fiber_g = Some(
                        meal.fiber_g
                            .as_ref()
                            .map(|f| f.clone())
                            .unwrap_or_else(|| BigDecimal::from(0))
                            + fiber,
                    );
                }
                if let Some(ref sugar) = recipe_nutrition.total_sugar_g {
                    meal.sugar_g = Some(
                        meal.sugar_g
                            .as_ref()
                            .map(|s| s.clone())
                            .unwrap_or_else(|| BigDecimal::from(0))
                            + sugar,
                    );
                }
                meal.recipes.push(recipe_nutrition);
            } else {
                // Create new meal nutrition
                meal_nutrition.push(MealNutrition {
                    meal_type: entry.meal_type.clone(),
                    calories: recipe_nutrition.total_calories.clone(),
                    protein_g: recipe_nutrition.total_protein_g.clone(),
                    carbs_g: recipe_nutrition.total_carbs_g.clone(),
                    fat_g: recipe_nutrition.total_fat_g.clone(),
                    fiber_g: recipe_nutrition.total_fiber_g.clone(),
                    sugar_g: recipe_nutrition.total_sugar_g.clone(),
                    recipes: vec![recipe_nutrition],
                });
            }
        }

        // Convert to DailyNutrition
        let mut daily_nutrition: Vec<DailyNutrition> = daily_map
            .into_iter()
            .map(|((date, day_of_week), meals)| {
                let total_calories = meals
                    .iter()
                    .fold(BigDecimal::from(0), |acc, m| acc + &m.calories);
                let total_protein = meals
                    .iter()
                    .fold(BigDecimal::from(0), |acc, m| acc + &m.protein_g);
                let total_carbs = meals
                    .iter()
                    .fold(BigDecimal::from(0), |acc, m| acc + &m.carbs_g);
                let total_fat = meals
                    .iter()
                    .fold(BigDecimal::from(0), |acc, m| acc + &m.fat_g);
                let total_fiber: Option<BigDecimal> = {
                    let sum: BigDecimal = meals
                        .iter()
                        .filter_map(|m| m.fiber_g.as_ref())
                        .fold(BigDecimal::from(0), |acc, f| acc + f);
                    if sum > BigDecimal::from(0) {
                        Some(sum)
                    } else {
                        None
                    }
                };
                let total_sugar: Option<BigDecimal> = {
                    let sum: BigDecimal = meals
                        .iter()
                        .filter_map(|m| m.sugar_g.as_ref())
                        .fold(BigDecimal::from(0), |acc, s| acc + s);
                    if sum > BigDecimal::from(0) {
                        Some(sum)
                    } else {
                        None
                    }
                };

                DailyNutrition {
                    date,
                    day_of_week,
                    total_calories,
                    total_protein_g: total_protein,
                    total_carbs_g: total_carbs,
                    total_fat_g: total_fat,
                    total_fiber_g: total_fiber,
                    total_sugar_g: total_sugar,
                    meals,
                }
            })
            .collect();

        // Sort by date or day_of_week
        daily_nutrition.sort_by(|a, b| {
            match (a.date, b.date) {
                (Some(ad), Some(bd)) => ad.cmp(&bd),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => {
                    a.day_of_week
                        .unwrap_or(7)
                        .cmp(&b.day_of_week.unwrap_or(7))
                }
            }
        });

        // Calculate weekly totals if applicable
        let weekly_totals = if meal_plan.is_template || daily_nutrition.len() >= 7 {
            let total_calories: BigDecimal = daily_nutrition
                .iter()
                .fold(BigDecimal::from(0), |acc, d| acc + &d.total_calories);
            let total_protein: BigDecimal = daily_nutrition
                .iter()
                .fold(BigDecimal::from(0), |acc, d| acc + &d.total_protein_g);
            let total_carbs: BigDecimal = daily_nutrition
                .iter()
                .fold(BigDecimal::from(0), |acc, d| acc + &d.total_carbs_g);
            let total_fat: BigDecimal = daily_nutrition
                .iter()
                .fold(BigDecimal::from(0), |acc, d| acc + &d.total_fat_g);
            let total_fiber: Option<BigDecimal> = {
                let sum: BigDecimal = daily_nutrition
                    .iter()
                    .filter_map(|d| d.total_fiber_g.as_ref())
                    .fold(BigDecimal::from(0), |acc, f| acc + f);
                if sum > BigDecimal::from(0) {
                    Some(sum)
                } else {
                    None
                }
            };
            let total_sugar: Option<BigDecimal> = {
                let sum: BigDecimal = daily_nutrition
                    .iter()
                    .filter_map(|d| d.total_sugar_g.as_ref())
                    .fold(BigDecimal::from(0), |acc, s| acc + s);
                if sum > BigDecimal::from(0) {
                    Some(sum)
                } else {
                    None
                }
            };

            let day_count = BigDecimal::from(daily_nutrition.len() as i32);
            let total_calories_clone = total_calories.clone();
            let total_protein_clone = total_protein.clone();
            let total_carbs_clone = total_carbs.clone();
            let total_fat_clone = total_fat.clone();
            Some(WeeklyNutrition {
                total_calories,
                total_protein_g: total_protein,
                total_carbs_g: total_carbs,
                total_fat_g: total_fat,
                total_fiber_g: total_fiber,
                total_sugar_g: total_sugar,
                average_daily_calories: total_calories_clone / day_count.clone(),
                average_daily_protein_g: total_protein_clone / day_count.clone(),
                average_daily_carbs_g: total_carbs_clone / day_count.clone(),
                average_daily_fat_g: total_fat_clone / day_count,
            })
        } else {
            None
        };

        Ok(MealPlanNutrition {
            meal_plan_id,
            daily_nutrition,
            weekly_totals,
        })
    }
}

