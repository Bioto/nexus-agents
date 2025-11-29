use crate::error::ToolboxError;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
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
            "/api/nutrition/recipes/:recipe_id/ingredients",
            post(add_ingredient_to_recipe).delete(remove_ingredient_from_recipe),
        )
        .route(
            "/api/nutrition/recipes/:recipe_id/steps",
            post(add_step_to_recipe).delete(remove_step_from_recipe),
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
