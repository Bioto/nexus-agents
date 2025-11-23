use crate::error::{Result, ToolboxError};
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
                // Calculate multiplier based on quantity (assuming unit is grams for now)
                // For simplicity, we'll assume unit conversion is handled or all units are grams
                let multiplier = &ri.quantity / &BigDecimal::from(100_i32);
                
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
}

