use chrono::{DateTime, NaiveDate, Utc};
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
    #[serde(default)]
    pub quantity: Option<f64>,
    #[serde(default)]
    pub unit: Option<String>,
}

// ========== Meal Plan Models ==========

/// Meal type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MealType {
    Breakfast,
    Lunch,
    Dinner,
    Snack,
}

impl MealType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MealType::Breakfast => "breakfast",
            MealType::Lunch => "lunch",
            MealType::Dinner => "dinner",
            MealType::Snack => "snack",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "breakfast" => Some(MealType::Breakfast),
            "lunch" => Some(MealType::Lunch),
            "dinner" => Some(MealType::Dinner),
            "snack" => Some(MealType::Snack),
            _ => None,
        }
    }
}

/// Day of week enum (Monday = 0, Sunday = 6)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayOfWeek {
    Monday = 0,
    Tuesday = 1,
    Wednesday = 2,
    Thursday = 3,
    Friday = 4,
    Saturday = 5,
    Sunday = 6,
}

impl DayOfWeek {
    pub fn from_int(n: i32) -> Option<Self> {
        match n {
            0 => Some(DayOfWeek::Monday),
            1 => Some(DayOfWeek::Tuesday),
            2 => Some(DayOfWeek::Wednesday),
            3 => Some(DayOfWeek::Thursday),
            4 => Some(DayOfWeek::Friday),
            5 => Some(DayOfWeek::Saturday),
            6 => Some(DayOfWeek::Sunday),
            _ => None,
        }
    }

    pub fn as_int(&self) -> i32 {
        *self as i32
    }
}

/// Meal plan model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MealPlan {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub is_template: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Meal plan entry model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MealPlanEntry {
    pub id: Uuid,
    pub meal_plan_id: Uuid,
    pub day_of_week: Option<i32>,
    pub date: Option<NaiveDate>,
    pub meal_type: String,
    pub recipe_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Meal plan entry with recipe details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MealPlanEntryWithRecipe {
    #[serde(flatten)]
    pub entry: MealPlanEntry,
    pub recipe: Recipe,
}

/// Meal plan with all entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MealPlanWithEntries {
    #[serde(flatten)]
    pub meal_plan: MealPlan,
    pub entries: Vec<MealPlanEntryWithRecipe>,
}

/// Daily nutrition totals for a meal plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyNutrition {
    pub date: Option<NaiveDate>,
    pub day_of_week: Option<i32>,
    pub total_calories: BigDecimal,
    pub total_protein_g: BigDecimal,
    pub total_carbs_g: BigDecimal,
    pub total_fat_g: BigDecimal,
    pub total_fiber_g: Option<BigDecimal>,
    pub total_sugar_g: Option<BigDecimal>,
    pub meals: Vec<MealNutrition>,
}

/// Nutrition for a specific meal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MealNutrition {
    pub meal_type: String,
    pub calories: BigDecimal,
    pub protein_g: BigDecimal,
    pub carbs_g: BigDecimal,
    pub fat_g: BigDecimal,
    pub fiber_g: Option<BigDecimal>,
    pub sugar_g: Option<BigDecimal>,
    pub recipes: Vec<RecipeNutrition>,
}

/// Calculated nutritional information for a meal plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MealPlanNutrition {
    pub meal_plan_id: Uuid,
    pub daily_nutrition: Vec<DailyNutrition>,
    pub weekly_totals: Option<WeeklyNutrition>,
}

/// Weekly nutrition totals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyNutrition {
    pub total_calories: BigDecimal,
    pub total_protein_g: BigDecimal,
    pub total_carbs_g: BigDecimal,
    pub total_fat_g: BigDecimal,
    pub total_fiber_g: Option<BigDecimal>,
    pub total_sugar_g: Option<BigDecimal>,
    pub average_daily_calories: BigDecimal,
    pub average_daily_protein_g: BigDecimal,
    pub average_daily_carbs_g: BigDecimal,
    pub average_daily_fat_g: BigDecimal,
}

// ========== Family Member Models ==========

/// Family member model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FamilyMember {
    pub id: Uuid,
    pub name: String,
    pub preferences: Option<serde_json::Value>, // JSONB field for flexible preferences
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Family member allergy model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FamilyMemberAllergy {
    pub id: Uuid,
    pub family_member_id: Uuid,
    pub ingredient_id: Uuid,
    pub severity: Option<String>, // 'mild', 'moderate', 'severe'
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Family member allergy with ingredient details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyMemberAllergyWithIngredient {
    #[serde(flatten)]
    pub allergy: FamilyMemberAllergy,
    pub ingredient: Ingredient,
}

/// Family member with allergies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyMemberWithAllergies {
    #[serde(flatten)]
    pub family_member: FamilyMember,
    pub allergies: Vec<FamilyMemberAllergyWithIngredient>,
}

/// Recipe favorite model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RecipeFavorite {
    pub id: Uuid,
    pub family_member_id: Uuid,
    pub recipe_id: Uuid,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Recipe favorite with recipe details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeFavoriteWithRecipe {
    #[serde(flatten)]
    pub favorite: RecipeFavorite,
    pub recipe: Recipe,
}

/// Family member with favorites
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyMemberWithFavorites {
    #[serde(flatten)]
    pub family_member: FamilyMember,
    pub favorites: Vec<RecipeFavoriteWithRecipe>,
}

/// Request models for family members
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFamilyMemberRequest {
    pub name: String,
    pub preferences: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFamilyMemberRequest {
    pub name: Option<String>,
    pub preferences: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAllergyRequest {
    pub family_member_id: Uuid,
    pub ingredient_id: Uuid,
    pub severity: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddRecipeFavoriteRequest {
    pub family_member_id: Uuid,
    pub recipe_id: Uuid,
    pub notes: Option<String>,
}
