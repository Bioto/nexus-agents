use clap::Subcommand;

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
    /// Calculate nutritional information for a recipe
    Calculate {
        /// Recipe ID
        recipe_id: String,
        /// Optional number of servings to calculate per-serving nutrition for
        #[arg(long)]
        servings: Option<i32>,
    },
    /// Import recipes from a CSV file
    Import {
        /// Path to the CSV file
        file: String,
        /// Skip rows with errors instead of failing
        #[arg(long)]
        skip_errors: bool,
    },
    /// Import ingredients from USDA Foundation Foods dataset
    ImportIngredients {
        /// Path to the directory containing USDA CSV files
        directory: String,
        /// Skip rows with errors instead of failing
        #[arg(long)]
        skip_errors: bool,
    },
    /// Import ingredients from USDA Branded Foods dataset
    ImportBrandedIngredients {
        /// Path to the directory containing USDA CSV files
        directory: String,
        /// Skip rows with errors instead of failing
        #[arg(long)]
        skip_errors: bool,
        /// Limit the number of foods to import (for testing)
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Import ingredients from USDA Foundation Foods JSON file
    ImportIngredientsJson {
        /// Path to the JSON file
        file: String,
        /// Skip rows with errors instead of failing
        #[arg(long)]
        skip_errors: bool,
        /// Batch size for parallel processing (default: 100)
        #[arg(long, default_value = "100")]
        batch_size: usize,
        /// Number of concurrent batches to process (default: auto, based on batch_size)
        #[arg(long)]
        concurrent_batches: Option<usize>,
    },
    /// Import ingredients from USDA Branded Foods JSON file
    ImportBrandedIngredientsJson {
        /// Path to the JSON file
        file: String,
        /// Skip rows with errors instead of failing
        #[arg(long)]
        skip_errors: bool,
        /// Limit the number of foods to import (for testing)
        #[arg(long)]
        limit: Option<usize>,
        /// Batch size for parallel processing (default: 100)
        #[arg(long, default_value = "100")]
        batch_size: usize,
        /// Number of concurrent batches to process (default: auto, based on batch_size)
        #[arg(long)]
        concurrent_batches: Option<usize>,
    },
    /// Batch operations for multiple IDs/queries
    Batch {
        #[command(subcommand)]
        operation: BatchOperation,
    },
    /// Meal plan management commands
    MealPlan {
        #[command(subcommand)]
        command: MealPlanCommand,
    },
    /// Family member management commands
    Family {
        #[command(subcommand)]
        command: FamilyCommand,
    },
    /// Recipe favorite management commands
    Favorite {
        #[command(subcommand)]
        command: FavoriteCommand,
    },
    /// Export recipe to PDF
    Export {
        /// Recipe ID
        recipe_id: String,
        /// Output file path (default: output/recipes/{recipe_name}.pdf)
        #[arg(short, long)]
        output: Option<String>,
        /// Include nutritional information
        #[arg(long)]
        include_nutrition: bool,
    },
    /// Export meal plan to PDF
    ExportMealPlan {
        /// Meal plan ID
        meal_plan_id: String,
        /// Output file path (default: output/meal_plans/{meal_plan_name}.pdf)
        #[arg(short, long)]
        output: Option<String>,
        /// Include nutritional information
        #[arg(long)]
        include_nutrition: bool,
        /// Use HTML/Chrome-based PDF rendering (better quality, requires Chrome)
        #[arg(long)]
        html: bool,
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
pub enum BatchOperation {
    /// Get multiple ingredients by IDs
    GetIngredients {
        /// Ingredient IDs (space-separated or comma-separated)
        ids: Vec<String>,
    },
    /// Get multiple recipes by IDs
    GetRecipes {
        /// Recipe IDs (space-separated or comma-separated)
        ids: Vec<String>,
        /// Include full details (ingredients and steps)
        #[arg(long)]
        full: bool,
    },
    /// Calculate nutrition for multiple recipes
    CalculateNutrition {
        /// Recipe IDs (space-separated or comma-separated)
        ids: Vec<String>,
    },
    /// Search with multiple queries (returns union of results)
    SearchIngredients {
        /// Search terms (space-separated or comma-separated)
        terms: Vec<String>,
    },
    /// Search recipes with multiple queries (returns union of results)
    SearchRecipes {
        /// Search terms (space-separated or comma-separated)
        terms: Vec<String>,
        /// Filter by ingredient ID
        #[arg(long)]
        ingredient_id: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum MealPlanCommand {
    /// Create a new meal plan
    Add {
        /// Meal plan name
        name: String,
        /// Meal plan description
        #[arg(short, long)]
        description: Option<String>,
        /// Start date (YYYY-MM-DD) - null for templates
        #[arg(long)]
        start_date: Option<String>,
        /// End date (YYYY-MM-DD) - null for templates
        #[arg(long)]
        end_date: Option<String>,
        /// Mark as template (day-of-week based)
        #[arg(long)]
        template: bool,
    },
    /// Get meal plan details
    Get {
        /// Meal plan ID
        id: String,
        /// Include full details (entries)
        #[arg(long)]
        full: bool,
    },
    /// Update meal plan metadata
    Update {
        /// Meal plan ID
        id: String,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
        /// New start date (YYYY-MM-DD)
        #[arg(long)]
        start_date: Option<String>,
        /// New end date (YYYY-MM-DD)
        #[arg(long)]
        end_date: Option<String>,
    },
    /// Delete a meal plan
    Delete {
        /// Meal plan ID
        id: String,
    },
    /// Add entry to meal plan
    AddEntry {
        /// Meal plan ID
        #[arg(long)]
        meal_plan_id: String,
        /// Recipe ID
        #[arg(long)]
        recipe_id: String,
        /// Meal type (breakfast, lunch, dinner, snack)
        #[arg(long)]
        meal_type: String,
        /// Day of week (0-6, Monday=0) - for templates
        #[arg(long)]
        day_of_week: Option<i32>,
        /// Date (YYYY-MM-DD) - for date-specific plans
        #[arg(long)]
        date: Option<String>,
    },
    /// Remove entry from meal plan
    RemoveEntry {
        /// Entry ID
        id: String,
    },
    /// List meal plans
    List {
        /// Search term (searches name and description)
        #[arg(short, long)]
        search: Option<String>,
        /// Filter by template status
        #[arg(long)]
        template: Option<bool>,
        /// Filter by start date (YYYY-MM-DD)
        #[arg(long)]
        start_date: Option<String>,
        /// Filter by end date (YYYY-MM-DD)
        #[arg(long)]
        end_date: Option<String>,
    },
    /// Calculate nutrition for meal plan
    CalculateNutrition {
        /// Meal plan ID
        id: String,
    },
    /// Batch operations for meal plans
    Batch {
        #[command(subcommand)]
        operation: MealPlanBatchOperation,
    },
}

#[derive(Subcommand)]
pub enum MealPlanBatchOperation {
    /// Get multiple meal plans by IDs
    GetMealPlans {
        /// Meal plan IDs (space-separated or comma-separated)
        ids: Vec<String>,
        /// Include full details (entries)
        #[arg(long)]
        full: bool,
    },
    /// Calculate nutrition for multiple meal plans
    CalculateNutrition {
        /// Meal plan IDs (space-separated or comma-separated)
        ids: Vec<String>,
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

#[derive(Subcommand)]
pub enum FamilyCommand {
    /// Add a new family member
    Add {
        /// Family member name
        name: String,
        /// Preferences as JSON (optional)
        #[arg(long)]
        preferences: Option<String>,
    },
    /// Get family member details
    Get {
        /// Family member ID
        id: String,
        /// Include allergies
        #[arg(long)]
        with_allergies: bool,
    },
    /// List family members
    List {
        /// Search term (searches by name)
        #[arg(short, long)]
        search: Option<String>,
    },
    /// Update a family member
    Update {
        /// Family member ID
        id: String,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New preferences as JSON
        #[arg(long)]
        preferences: Option<String>,
    },
    /// Delete a family member
    Delete {
        /// Family member ID
        id: String,
    },
    /// Add an allergy to a family member
    AddAllergy {
        /// Family member ID
        #[arg(long)]
        family_member_id: String,
        /// Ingredient ID (allergen)
        #[arg(long)]
        ingredient_id: String,
        /// Severity (mild, moderate, severe)
        #[arg(long)]
        severity: Option<String>,
        /// Notes about the allergy
        #[arg(long)]
        notes: Option<String>,
    },
    /// Remove an allergy from a family member
    RemoveAllergy {
        /// Family member ID
        #[arg(long)]
        family_member_id: String,
        /// Ingredient ID (allergen)
        #[arg(long)]
        ingredient_id: String,
    },
    /// Check if a recipe contains allergens for a family member
    CheckAllergens {
        /// Family member ID
        #[arg(long)]
        family_member_id: String,
        /// Recipe ID
        #[arg(long)]
        recipe_id: String,
    },
}

#[derive(Subcommand)]
pub enum FavoriteCommand {
    /// Add a recipe to a family member's favorites
    Add {
        /// Family member ID
        #[arg(long)]
        family_member_id: String,
        /// Recipe ID
        #[arg(long)]
        recipe_id: String,
        /// Optional notes
        #[arg(long)]
        notes: Option<String>,
    },
    /// Remove a recipe from a family member's favorites
    Remove {
        /// Family member ID
        #[arg(long)]
        family_member_id: String,
        /// Recipe ID
        #[arg(long)]
        recipe_id: String,
    },
    /// List favorite recipes for a family member
    List {
        /// Family member ID
        id: String,
    },
    /// List family members who favorited a recipe
    FavoritedBy {
        /// Recipe ID
        id: String,
    },
}
