use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{types::BigDecimal, FromRow};
use uuid::Uuid;

/// Ingredient model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Ingredient {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Nutritional information model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct NutritionalInfo {
    pub id: Uuid,
    pub ingredient_id: Uuid,
    pub calories_per_100g: BigDecimal,
    pub protein_g: BigDecimal,
    pub carbs_g: BigDecimal,
    pub fat_g: BigDecimal,
    pub fiber_g: Option<BigDecimal>,
    pub sugar_g: Option<BigDecimal>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Recipe model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Recipe {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub servings: Option<i32>,
    pub prep_time_minutes: Option<i32>,
    pub cook_time_minutes: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Recipe ingredient junction model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RecipeIngredient {
    pub id: Uuid,
    pub recipe_id: Uuid,
    pub ingredient_id: Uuid,
    pub quantity: BigDecimal,
    pub unit: String,
    pub created_at: DateTime<Utc>,
}

/// Recipe step model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RecipeStep {
    pub id: Uuid,
    pub recipe_id: Uuid,
    pub step_number: i32,
    pub instruction: String,
    pub created_at: DateTime<Utc>,
}

/// Recipe with full details (ingredients and steps)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeWithDetails {
    #[serde(flatten)]
    pub recipe: Recipe,
    pub ingredients: Vec<RecipeIngredientWithIngredient>,
    pub steps: Vec<RecipeStep>,
}

/// Recipe ingredient with ingredient details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeIngredientWithIngredient {
    #[serde(flatten)]
    pub recipe_ingredient: RecipeIngredient,
    pub ingredient: Ingredient,
    pub nutritional_info: Option<NutritionalInfo>,
}

/// Calculated nutritional information for a recipe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeNutrition {
    pub recipe_id: Uuid,
    pub total_calories: BigDecimal,
    pub total_protein_g: BigDecimal,
    pub total_carbs_g: BigDecimal,
    pub total_fat_g: BigDecimal,
    pub total_fiber_g: Option<BigDecimal>,
    pub total_sugar_g: Option<BigDecimal>,
    pub per_serving_calories: Option<BigDecimal>,
    pub per_serving_protein_g: Option<BigDecimal>,
    pub per_serving_carbs_g: Option<BigDecimal>,
    pub per_serving_fat_g: Option<BigDecimal>,
}

/// Request models for API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateIngredientRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateIngredientRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNutritionalInfoRequest {
    pub ingredient_id: Uuid,
    pub calories_per_100g: BigDecimal,
    pub protein_g: BigDecimal,
    pub carbs_g: BigDecimal,
    pub fat_g: BigDecimal,
    pub fiber_g: Option<BigDecimal>,
    pub sugar_g: Option<BigDecimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRecipeRequest {
    pub name: String,
    pub description: Option<String>,
    pub servings: Option<i32>,
    pub prep_time_minutes: Option<i32>,
    pub cook_time_minutes: Option<i32>,
    pub ingredients: Vec<CreateRecipeIngredientRequest>,
    pub steps: Vec<CreateRecipeStepRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRecipeIngredientRequest {
    pub ingredient_id: Uuid,
    pub quantity: BigDecimal,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRecipeStepRequest {
    pub step_number: i32,
    pub instruction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecipeRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub servings: Option<i32>,
    pub prep_time_minutes: Option<i32>,
    pub cook_time_minutes: Option<i32>,
}

/// Extracted recipe from URL (before database insertion)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRecipe {
    pub name: String,
    pub description: Option<String>,
    pub servings: Option<i32>,
    pub prep_time_minutes: Option<i32>,
    pub cook_time_minutes: Option<i32>,
    pub ingredients: Vec<ExtractedIngredient>,
    pub steps: Vec<String>,
}

/// Extracted ingredient from recipe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedIngredient {
    pub name: String,
    pub quantity: f64,
    pub unit: String,
}

