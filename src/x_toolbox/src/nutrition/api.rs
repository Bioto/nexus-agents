use crate::error::ToolboxError;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{types::BigDecimal, PgPool};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use super::models::*;
use super::NutritionService;

/// Start the API server
pub async fn start_server(
    pool: PgPool,
    host: String,
    port: u16,
) -> std::result::Result<(), ToolboxError> {
    let app = create_router(pool);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| ToolboxError::Configuration(format!("Failed to bind to {}: {}", addr, e)))?;

    println!("Nutrition API server listening on http://{}", addr);

    axum::serve(listener, app)
        .await
        .map_err(|e| ToolboxError::Other(format!("Server error: {}", e)))?;

    Ok(())
}

fn create_router(pool: PgPool) -> Router {
    Router::new()
        // Ingredient endpoints
        .route(
            "/api/nutrition/ingredients",
            get(list_ingredients).post(create_ingredient),
        )
        .route(
            "/api/nutrition/ingredients/:id",
            get(get_ingredient)
                .put(update_ingredient)
                .delete(delete_ingredient),
        )
        .route(
            "/api/nutrition/ingredients/:id/nutrition",
            get(get_ingredient_nutrition),
        )
        .route(
            "/api/nutrition/ingredients/batch",
            post(get_ingredients_batch),
        )
        // Recipe endpoints
        .route(
            "/api/nutrition/recipes",
            get(list_recipes).post(create_recipe),
        )
        .route(
            "/api/nutrition/recipes/:id",
            get(get_recipe).put(update_recipe).delete(delete_recipe),
        )
        .route(
            "/api/nutrition/recipes/:id/nutrition",
            get(calculate_recipe_nutrition),
        )
        .route(
            "/api/nutrition/recipes/batch",
            post(get_recipes_batch),
        )
        .route(
            "/api/nutrition/recipes/:recipe_id/ingredients",
            post(add_ingredient_to_recipe).delete(remove_ingredient_from_recipe),
        )
        .route(
            "/api/nutrition/recipes/:recipe_id/steps",
            post(add_step_to_recipe).delete(remove_step_from_recipe),
        )
        // Meal plan endpoints (read-only for mobile app)
        .route(
            "/api/nutrition/meal-plans",
            get(list_meal_plans),
        )
        .route(
            "/api/nutrition/meal-plans/:id",
            get(get_meal_plan),
        )
        .route(
            "/api/nutrition/meal-plans/:id/entries",
            get(get_meal_plan_with_entries),
        )
        .route(
            "/api/nutrition/meal-plans/:id/nutrition",
            get(calculate_meal_plan_nutrition),
        )
        .route(
            "/api/nutrition/meal-plans/:id/prep-analysis",
            get(get_meal_plan_prep_analysis),
        )
        .route(
            "/api/nutrition/meal-plans/batch",
            post(get_meal_plans_batch),
        )
        // Family member endpoints (read-only for mobile app)
        .route(
            "/api/nutrition/family-members",
            get(list_family_members),
        )
        .route(
            "/api/nutrition/family-members/:id",
            get(get_family_member),
        )
        .route(
            "/api/nutrition/family-members/:id/allergies",
            get(get_family_member_with_allergies),
        )
        .route(
            "/api/nutrition/family-members/:id/favorites",
            get(get_family_member_favorites),
        )
        .route(
            "/api/nutrition/family-members/batch",
            post(get_family_members_batch),
        )
        .route(
            "/api/nutrition/recipes/:recipe_id/favorited-by",
            get(get_recipe_favorited_by),
        )
        .layer(CorsLayer::permissive())
        .with_state(pool)
}

// ========== Ingredient Handlers ==========

#[derive(Deserialize)]
struct ListIngredientsQuery {
    search: Option<String>,
}

async fn list_ingredients(
    Query(params): Query<ListIngredientsQuery>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<Vec<Ingredient>>, ApiError> {
    let ingredients = NutritionService::list_ingredients(&pool, params.search.as_deref())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(ingredients))
}

async fn get_ingredient(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<Ingredient>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let ingredient = NutritionService::get_ingredient(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(ingredient))
}

async fn create_ingredient(
    State(pool): State<PgPool>,
    Json(req): Json<CreateIngredientRequest>,
) -> std::result::Result<Json<Ingredient>, ApiError> {
    let ingredient =
        NutritionService::create_ingredient(&pool, &req.name, req.description.as_deref())
            .await
            .map_err(ApiError::from)?;
    Ok(Json(ingredient))
}

async fn update_ingredient(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
    Json(req): Json<UpdateIngredientRequest>,
) -> std::result::Result<Json<Ingredient>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let ingredient = NutritionService::update_ingredient(
        &pool,
        uuid,
        req.name.as_deref(),
        req.description.as_deref(),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(ingredient))
}

async fn delete_ingredient(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<StatusCode, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    NutritionService::delete_ingredient(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

// ========== Recipe Handlers ==========

#[derive(Deserialize)]
struct ListRecipesQuery {
    search: Option<String>,
    ingredient_id: Option<String>,
}

async fn list_recipes(
    Query(params): Query<ListRecipesQuery>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<Vec<Recipe>>, ApiError> {
    let ingredient_uuid = params
        .ingredient_id
        .map(|id| Uuid::parse_str(&id))
        .transpose()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let recipes = NutritionService::list_recipes(&pool, params.search.as_deref(), ingredient_uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(recipes))
}

async fn get_recipe(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<RecipeWithDetails>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let recipe = NutritionService::get_recipe_with_details(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(recipe))
}

async fn create_recipe(
    State(pool): State<PgPool>,
    Json(req): Json<CreateRecipeRequest>,
) -> std::result::Result<Json<Recipe>, ApiError> {
    let ingredients: Vec<(Uuid, BigDecimal, String)> = req
        .ingredients
        .into_iter()
        .map(|ri| (ri.ingredient_id, ri.quantity, ri.unit))
        .collect();

    let steps: Vec<(i32, String)> = req
        .steps
        .into_iter()
        .map(|rs| (rs.step_number, rs.instruction))
        .collect();

    let recipe = NutritionService::create_recipe(
        &pool,
        &req.name,
        req.description.as_deref(),
        req.servings,
        req.prep_time_minutes,
        req.cook_time_minutes,
        ingredients,
        steps,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(recipe))
}

async fn update_recipe(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
    Json(req): Json<UpdateRecipeRequest>,
) -> std::result::Result<Json<Recipe>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let recipe = NutritionService::update_recipe(
        &pool,
        uuid,
        req.name.as_deref(),
        req.description.as_deref(),
        req.servings,
        req.prep_time_minutes,
        req.cook_time_minutes,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(recipe))
}

async fn delete_recipe(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<StatusCode, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    NutritionService::delete_recipe(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct CalculateNutritionQuery {
    servings: Option<i32>,
}

async fn calculate_recipe_nutrition(
    Path(id): Path<String>,
    Query(query): Query<CalculateNutritionQuery>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<RecipeNutrition>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let nutrition = NutritionService::calculate_recipe_nutrition(&pool, uuid, query.servings)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(nutrition))
}

// ========== Recipe Ingredient Management ==========

#[derive(Deserialize)]
struct AddRecipeIngredientRequest {
    ingredient_id: String,
    quantity: f64,
    unit: String,
}

async fn add_ingredient_to_recipe(
    Path(recipe_id): Path<String>,
    State(pool): State<PgPool>,
    Json(req): Json<AddRecipeIngredientRequest>,
) -> std::result::Result<StatusCode, ApiError> {
    let recipe_uuid =
        Uuid::parse_str(&recipe_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let ingredient_uuid =
        Uuid::parse_str(&req.ingredient_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let quantity: BigDecimal = req
        .quantity
        .to_string()
        .parse()
        .map_err(|e| ApiError::BadRequest(format!("Invalid quantity: {}", e)))?;

    NutritionService::add_recipe_ingredient(
        &pool,
        recipe_uuid,
        ingredient_uuid,
        quantity,
        req.unit,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
struct RemoveRecipeIngredientRequest {
    ingredient_id: String,
}

async fn remove_ingredient_from_recipe(
    Path(recipe_id): Path<String>,
    State(pool): State<PgPool>,
    Json(req): Json<RemoveRecipeIngredientRequest>,
) -> std::result::Result<StatusCode, ApiError> {
    let recipe_uuid =
        Uuid::parse_str(&recipe_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let ingredient_uuid =
        Uuid::parse_str(&req.ingredient_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    NutritionService::remove_recipe_ingredient(&pool, recipe_uuid, ingredient_uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AddRecipeStepRequest {
    step_number: i32,
    instruction: String,
}

async fn add_step_to_recipe(
    Path(recipe_id): Path<String>,
    State(pool): State<PgPool>,
    Json(req): Json<AddRecipeStepRequest>,
) -> std::result::Result<StatusCode, ApiError> {
    let recipe_uuid =
        Uuid::parse_str(&recipe_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    NutritionService::add_recipe_step(&pool, recipe_uuid, req.step_number, req.instruction)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
struct RemoveRecipeStepRequest {
    step_number: i32,
}

async fn remove_step_from_recipe(
    Path(recipe_id): Path<String>,
    State(pool): State<PgPool>,
    Json(req): Json<RemoveRecipeStepRequest>,
) -> std::result::Result<StatusCode, ApiError> {
    let recipe_uuid =
        Uuid::parse_str(&recipe_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    NutritionService::remove_recipe_step(&pool, recipe_uuid, req.step_number)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

// ========== Nutritional Info Handlers ==========

async fn get_ingredient_nutrition(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<Option<NutritionalInfo>>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let nutrition = NutritionService::get_nutritional_info(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(nutrition))
}

// ========== Batch Query Handlers ==========

#[derive(Deserialize)]
struct BatchIdsRequest {
    ids: Vec<String>,
}

async fn get_ingredients_batch(
    State(pool): State<PgPool>,
    Json(req): Json<BatchIdsRequest>,
) -> std::result::Result<Json<Vec<Ingredient>>, ApiError> {
    let uuids: std::result::Result<Vec<Uuid>, _> = req.ids.iter().map(|id| Uuid::parse_str(id)).collect();
    let uuids = uuids.map_err(|e| ApiError::BadRequest(e.to_string()))?;
    
    let mut ingredients = Vec::new();
    for uuid in uuids {
        match NutritionService::get_ingredient(&pool, uuid).await {
            Ok(ingredient) => ingredients.push(ingredient),
            Err(_) => continue, // Skip not found ingredients
        }
    }
    Ok(Json(ingredients))
}

async fn get_recipes_batch(
    State(pool): State<PgPool>,
    Json(req): Json<BatchIdsRequest>,
) -> std::result::Result<Json<Vec<Recipe>>, ApiError> {
    let uuids: std::result::Result<Vec<Uuid>, _> = req.ids.iter().map(|id| Uuid::parse_str(id)).collect();
    let uuids = uuids.map_err(|e| ApiError::BadRequest(e.to_string()))?;
    
    let mut recipes = Vec::new();
    for uuid in uuids {
        match NutritionService::get_recipe(&pool, uuid).await {
            Ok(recipe) => recipes.push(recipe),
            Err(_) => continue, // Skip not found recipes
        }
    }
    Ok(Json(recipes))
}

async fn get_meal_plans_batch(
    State(pool): State<PgPool>,
    Json(req): Json<BatchIdsRequest>,
) -> std::result::Result<Json<Vec<MealPlan>>, ApiError> {
    let uuids: std::result::Result<Vec<Uuid>, _> = req.ids.iter().map(|id| Uuid::parse_str(id)).collect();
    let uuids = uuids.map_err(|e| ApiError::BadRequest(e.to_string()))?;
    
    let mut meal_plans = Vec::new();
    for uuid in uuids {
        match NutritionService::get_meal_plan(&pool, uuid).await {
            Ok(meal_plan) => meal_plans.push(meal_plan),
            Err(_) => continue, // Skip not found meal plans
        }
    }
    Ok(Json(meal_plans))
}

async fn get_family_members_batch(
    State(pool): State<PgPool>,
    Json(req): Json<BatchIdsRequest>,
) -> std::result::Result<Json<Vec<FamilyMember>>, ApiError> {
    let uuids: std::result::Result<Vec<Uuid>, _> = req.ids.iter().map(|id| Uuid::parse_str(id)).collect();
    let uuids = uuids.map_err(|e| ApiError::BadRequest(e.to_string()))?;
    
    let mut family_members = Vec::new();
    for uuid in uuids {
        match NutritionService::get_family_member(&pool, uuid).await {
            Ok(member) => family_members.push(member),
            Err(_) => continue, // Skip not found family members
        }
    }
    Ok(Json(family_members))
}

// ========== Meal Plan Handlers ==========

#[derive(Deserialize)]
struct ListMealPlansQuery {
    search: Option<String>,
    is_template: Option<bool>,
    start_date: Option<String>, // ISO date string
    end_date: Option<String>,   // ISO date string
}

async fn list_meal_plans(
    Query(params): Query<ListMealPlansQuery>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<Vec<MealPlan>>, ApiError> {
    let start_date = params
        .start_date
        .map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| ApiError::BadRequest(format!("Invalid start_date format: {}", e)))?;
    
    let end_date = params
        .end_date
        .map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| ApiError::BadRequest(format!("Invalid end_date format: {}", e)))?;

    let meal_plans = NutritionService::list_meal_plans(
        &pool,
        params.search.as_deref(),
        params.is_template,
        start_date,
        end_date,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(meal_plans))
}

async fn get_meal_plan(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<MealPlan>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let meal_plan = NutritionService::get_meal_plan(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(meal_plan))
}

async fn get_meal_plan_with_entries(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<MealPlanWithEntries>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let meal_plan = NutritionService::get_meal_plan_with_entries(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(meal_plan))
}

async fn calculate_meal_plan_nutrition(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<MealPlanNutrition>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let nutrition = NutritionService::calculate_meal_plan_nutrition(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(nutrition))
}

async fn get_meal_plan_prep_analysis(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<MealPlanPrepData>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let prep_data = NutritionService::get_meal_plan_for_prep_analysis(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(prep_data))
}

// ========== Family Member Handlers ==========

#[derive(Deserialize)]
struct ListFamilyMembersQuery {
    search: Option<String>,
}

async fn list_family_members(
    Query(params): Query<ListFamilyMembersQuery>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<Vec<FamilyMember>>, ApiError> {
    let family_members = NutritionService::list_family_members(&pool, params.search.as_deref())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(family_members))
}

async fn get_family_member(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<FamilyMember>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let family_member = NutritionService::get_family_member(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(family_member))
}

async fn get_family_member_with_allergies(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<FamilyMemberWithAllergies>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let family_member = NutritionService::get_family_member_with_allergies(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(family_member))
}

async fn get_family_member_favorites(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<Vec<RecipeFavoriteWithRecipe>>, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let favorites = NutritionService::get_family_member_favorites(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(favorites))
}

async fn get_recipe_favorited_by(
    Path(recipe_id): Path<String>,
    State(pool): State<PgPool>,
) -> std::result::Result<Json<Vec<FamilyMember>>, ApiError> {
    let uuid = Uuid::parse_str(&recipe_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let family_members = NutritionService::get_recipe_favorited_by(&pool, uuid)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(family_members))
}

// ========== Error Handling ==========

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
}

impl From<ToolboxError> for ApiError {
    fn from(err: ToolboxError) -> Self {
        match err {
            ToolboxError::NotFound(msg) => ApiError::NotFound(msg),
            ToolboxError::Validation(msg) => ApiError::BadRequest(msg),
            ToolboxError::Database(msg) => ApiError::Internal(format!("Database error: {}", msg)),
            ToolboxError::Configuration(msg) => {
                ApiError::Internal(format!("Configuration error: {}", msg))
            }
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(ErrorResponse {
            error: error_message,
        });

        (status, body).into_response()
    }
}
