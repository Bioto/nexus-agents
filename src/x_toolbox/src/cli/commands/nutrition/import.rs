use crate::error::{Result, ToolboxError};
use crate::nutrition::NutritionService;
use bigdecimal::BigDecimal;
use csv::ReaderBuilder;
use futures::future::join_all;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tokio::sync::mpsc;

use super::helpers::{parse_directions, parse_ingredients, parse_time};

/// Import recipes from a CSV file
pub async fn import_recipes_from_csv(
    pool: &sqlx::PgPool,
    file_path: &str,
    skip_errors: bool,
) -> Result<()> {
    let path = Path::new(file_path);
    let file = File::open(path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open CSV file {}: {}", file_path, e),
        ))
    })?;

    let mut reader = ReaderBuilder::new().has_headers(true).from_reader(file);

    let mut imported = 0;
    let mut _skipped = 0;
    let mut errors = 0;

    // Regex to parse ingredient strings like "3 tablespoons butter" or "2 pounds Granny Smith apples"
    let ingredient_re = Regex::new(r"^(\d+(?:\.\d+)?)\s+([a-zA-Z]+(?:\s+[a-zA-Z]+)?)\s+(.+)$")
        .map_err(|e| ToolboxError::Other(format!("Failed to compile regex: {}", e)))?;

    // Regex to parse time strings like "30 mins", "1 hrs", "1 hrs 30 mins"
    let time_re = Regex::new(r"(?:(\d+)\s*(?:hrs?|hours?))?\s*(?:(\d+)\s*(?:mins?|minutes?))?")
        .map_err(|e| ToolboxError::Other(format!("Failed to compile regex: {}", e)))?;

    for (row_num, result) in reader.records().enumerate() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                if skip_errors {
                    eprintln!("Error reading row {}: {}", row_num + 2, e);
                    _skipped += 1;
                    continue;
                } else {
                    return Err(ToolboxError::Other(format!(
                        "Error reading row {}: {}",
                        row_num + 2,
                        e
                    )));
                }
            }
        };

        let recipe_name = record.get(1).ok_or_else(|| {
            ToolboxError::Validation(format!("Row {}: missing recipe_name", row_num + 2))
        })?;

        if recipe_name.is_empty() {
            if skip_errors {
                _skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "Row {}: recipe_name is empty",
                    row_num + 2
                )));
            }
        }

        // Parse times
        let prep_time = record.get(2).and_then(|t| parse_time(&time_re, t));
        let cook_time = record.get(3).and_then(|t| parse_time(&time_re, t));

        // Parse servings
        let servings = record.get(5).and_then(|s| s.parse::<i32>().ok());

        // Parse ingredients
        let ingredients_str = record.get(7).unwrap_or("");
        let ingredients = parse_ingredients(pool, ingredients_str, &ingredient_re).await;

        let ingredients = match ingredients {
            Ok(ing) => ing,
            Err(e) => {
                if skip_errors {
                    eprintln!("Row {}: Error parsing ingredients: {}", row_num + 2, e);
                    errors += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        // Parse directions into steps
        let directions_str = record.get(8).unwrap_or("");
        let steps = parse_directions(directions_str);

        // Create recipe
        match NutritionService::create_recipe(
            pool,
            recipe_name,
            None, // description
            servings,
            prep_time,
            cook_time,
            ingredients,
            steps,
        )
        .await
        {
            Ok(_) => {
                imported += 1;
                if imported % 100 == 0 {
                    println!("Imported {} recipes...", imported);
                }
            }
            Err(e) => {
                if skip_errors {
                    eprintln!("Row {}: Error creating recipe: {}", row_num + 2, e);
                    errors += 1;
                } else {
                    return Err(e);
                }
            }
        }
    }

    println!("\nImport complete:");
    println!("  Imported: {}", imported);
    if errors > 0 {
        println!("  Errors: {}", errors);
    }

    Ok(())
}

/// Import ingredients from USDA Foundation Foods dataset
pub async fn import_usda_ingredients(
    pool: &sqlx::PgPool,
    directory: &str,
    skip_errors: bool,
) -> Result<()> {
    let dir_path = Path::new(directory);

    // USDA nutrient IDs we care about
    const NUTRIENT_ENERGY: i32 = 1008; // Energy (KCAL)
    const NUTRIENT_PROTEIN: i32 = 1003; // Protein (G)
    const NUTRIENT_FAT: i32 = 1004; // Total lipid (fat) (G)
    const NUTRIENT_CARBS: i32 = 1005; // Carbohydrate, by difference (G)
    const NUTRIENT_FIBER: i32 = 1079; // Fiber, total dietary (G)
    const NUTRIENT_SUGAR: i32 = 1063; // Sugars, Total (G)

    println!("Loading USDA Foundation Foods dataset from: {}", directory);

    // Step 1: Load foundation_food.csv to get fdc_ids
    let foundation_food_path = dir_path.join("foundation_food.csv");
    let foundation_food_file = File::open(&foundation_food_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open foundation_food.csv: {}", e),
        ))
    })?;

    let mut foundation_fdc_ids = Vec::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(foundation_food_file);

    for result in reader.records() {
        let record = result.map_err(|e| {
            ToolboxError::Other(format!("Error reading foundation_food.csv: {}", e))
        })?;
        if let Some(fdc_id_str) = record.get(0) {
            if let Ok(fdc_id) = fdc_id_str.parse::<i32>() {
                foundation_fdc_ids.push(fdc_id);
            }
        }
    }

    println!("Found {} foundation foods", foundation_fdc_ids.len());

    // Step 2: Load food.csv to get descriptions (map fdc_id -> description)
    let food_path = dir_path.join("food.csv");
    let food_file = File::open(&food_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open food.csv: {}", e),
        ))
    })?;

    let mut food_descriptions: HashMap<i32, String> = HashMap::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(food_file);

    for result in reader.records() {
        let record =
            result.map_err(|e| ToolboxError::Other(format!("Error reading food.csv: {}", e)))?;
        if let (Some(fdc_id_str), Some(description)) = (record.get(0), record.get(2)) {
            if let Ok(fdc_id) = fdc_id_str.parse::<i32>() {
                food_descriptions.insert(fdc_id, description.to_string());
            }
        }
    }

    println!("Loaded {} food descriptions", food_descriptions.len());

    // Step 3: Load food_nutrient.csv to get nutrient values (map (fdc_id, nutrient_id) -> amount)
    let food_nutrient_path = dir_path.join("food_nutrient.csv");
    let food_nutrient_file = File::open(&food_nutrient_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open food_nutrient.csv: {}", e),
        ))
    })?;

    let mut food_nutrients: HashMap<(i32, i32), f64> = HashMap::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(food_nutrient_file);

    for result in reader.records() {
        let record = result
            .map_err(|e| ToolboxError::Other(format!("Error reading food_nutrient.csv: {}", e)))?;
        if let (Some(fdc_id_str), Some(nutrient_id_str), Some(amount_str)) =
            (record.get(1), record.get(2), record.get(3))
        {
            if let (Ok(fdc_id), Ok(nutrient_id)) =
                (fdc_id_str.parse::<i32>(), nutrient_id_str.parse::<i32>())
            {
                if let Ok(amount) = amount_str.parse::<f64>() {
                    food_nutrients.insert((fdc_id, nutrient_id), amount);
                }
            }
        }
    }

    println!("Loaded {} nutrient values", food_nutrients.len());

    // Step 4: Process each foundation food
    let mut imported = 0;
    let mut _skipped = 0;
    let mut errors = 0;

    for fdc_id in foundation_fdc_ids.iter() {
        // Get food description
        let food_name = match food_descriptions.get(fdc_id) {
            Some(name) => name.trim().to_string(),
            None => {
                if skip_errors {
                    eprintln!("FDC ID {}: No description found", fdc_id);
                    _skipped += 1;
                    continue;
                } else {
                    return Err(ToolboxError::Validation(format!(
                        "FDC ID {}: No description found",
                        fdc_id
                    )));
                }
            }
        };

        if food_name.is_empty() {
            if skip_errors {
                _skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "FDC ID {}: Empty description",
                    fdc_id
                )));
            }
        }

        // Extract nutrient values
        let calories = food_nutrients
            .get(&(*fdc_id, NUTRIENT_ENERGY))
            .copied()
            .unwrap_or(0.0);
        let protein = food_nutrients
            .get(&(*fdc_id, NUTRIENT_PROTEIN))
            .copied()
            .unwrap_or(0.0);
        let fat = food_nutrients
            .get(&(*fdc_id, NUTRIENT_FAT))
            .copied()
            .unwrap_or(0.0);
        let carbs = food_nutrients
            .get(&(*fdc_id, NUTRIENT_CARBS))
            .copied()
            .unwrap_or(0.0);
        let fiber = food_nutrients.get(&(*fdc_id, NUTRIENT_FIBER)).copied();
        let sugar = food_nutrients.get(&(*fdc_id, NUTRIENT_SUGAR)).copied();

        // Validate required nutrients
        if calories == 0.0 && protein == 0.0 && carbs == 0.0 && fat == 0.0 {
            if skip_errors {
                eprintln!(
                    "FDC ID {} ({}): No nutritional data found",
                    fdc_id, food_name
                );
                _skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "FDC ID {} ({}): No nutritional data found",
                    fdc_id, food_name
                )));
            }
        }

        // Find or create ingredient
        let ingredient = match NutritionService::find_or_create_ingredient(pool, &food_name).await {
            Ok(ing) => ing,
            Err(e) => {
                if skip_errors {
                    eprintln!(
                        "FDC ID {} ({}): Error creating ingredient: {}",
                        fdc_id, food_name, e
                    );
                    errors += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        // Create or update nutritional info
        // Convert f64 to BigDecimal via string parsing
        let calories_bd = calories
            .to_string()
            .parse::<BigDecimal>()
            .map_err(|e| ToolboxError::Validation(format!("Invalid calories value: {}", e)))?;
        let protein_bd = protein
            .to_string()
            .parse::<BigDecimal>()
            .map_err(|e| ToolboxError::Validation(format!("Invalid protein value: {}", e)))?;
        let carbs_bd = carbs
            .to_string()
            .parse::<BigDecimal>()
            .map_err(|e| ToolboxError::Validation(format!("Invalid carbs value: {}", e)))?;
        let fat_bd = fat
            .to_string()
            .parse::<BigDecimal>()
            .map_err(|e| ToolboxError::Validation(format!("Invalid fat value: {}", e)))?;
        let fiber_bd = fiber.and_then(|f| f.to_string().parse::<BigDecimal>().ok());
        let sugar_bd = sugar.and_then(|s| s.to_string().parse::<BigDecimal>().ok());

        match NutritionService::upsert_nutritional_info(
            pool,
            ingredient.id,
            calories_bd,
            protein_bd,
            carbs_bd,
            fat_bd,
            fiber_bd,
            sugar_bd,
        )
        .await
        {
            Ok(_) => {
                imported += 1;
                if imported % 100 == 0 {
                    println!("Imported {} ingredients...", imported);
                }
            }
            Err(e) => {
                if skip_errors {
                    eprintln!(
                        "FDC ID {} ({}): Error creating nutritional info: {}",
                        fdc_id, food_name, e
                    );
                    errors += 1;
                } else {
                    return Err(e);
                }
            }
        }
    }

    println!("\nImport complete:");
    println!("  Imported: {}", imported);
    if errors > 0 {
        println!("  Errors: {}", errors);
    }

    Ok(())
}

/// Import ingredients from USDA Branded Foods dataset
pub async fn import_usda_branded_ingredients(
    pool: &sqlx::PgPool,
    directory: &str,
    skip_errors: bool,
    limit: Option<usize>,
) -> Result<()> {
    let dir_path = Path::new(directory);

    // USDA nutrient IDs we care about
    const NUTRIENT_ENERGY: i32 = 1008; // Energy (KCAL)
    const NUTRIENT_PROTEIN: i32 = 1003; // Protein (G)
    const NUTRIENT_FAT: i32 = 1004; // Total lipid (fat) (G)
    const NUTRIENT_CARBS: i32 = 1005; // Carbohydrate, by difference (G)
    const NUTRIENT_FIBER: i32 = 1079; // Fiber, total dietary (G)
    const NUTRIENT_SUGAR: i32 = 1063; // Sugars, Total (G)

    println!("Loading USDA Branded Foods dataset from: {}", directory);

    // Step 1: Load branded_food.csv to get fdc_ids
    let branded_food_path = dir_path.join("branded_food.csv");
    let branded_food_file = File::open(&branded_food_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open branded_food.csv: {}", e),
        ))
    })?;

    let mut branded_fdc_ids = Vec::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(branded_food_file);

    for result in reader.records() {
        let record = result
            .map_err(|e| ToolboxError::Other(format!("Error reading branded_food.csv: {}", e)))?;
        if let Some(fdc_id_str) = record.get(0) {
            if let Ok(fdc_id) = fdc_id_str.parse::<i32>() {
                branded_fdc_ids.push(fdc_id);
            }
        }
    }

    // Apply limit if specified
    if let Some(limit_val) = limit {
        branded_fdc_ids.truncate(limit_val);
        println!("Limited to {} foods for import", limit_val);
    }

    // Create HashSet for efficient lookup
    let branded_fdc_set: HashSet<i32> = branded_fdc_ids.iter().copied().collect();

    println!("Found {} branded foods to import", branded_fdc_ids.len());

    // Step 2: Load food.csv to get descriptions (only for branded foods)
    let food_path = dir_path.join("food.csv");
    let food_file = File::open(&food_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open food.csv: {}", e),
        ))
    })?;

    let mut food_descriptions: HashMap<i32, String> = HashMap::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(food_file);

    let mut loaded_count = 0;
    for result in reader.records() {
        let record =
            result.map_err(|e| ToolboxError::Other(format!("Error reading food.csv: {}", e)))?;
        if let (Some(fdc_id_str), Some(data_type), Some(description)) =
            (record.get(0), record.get(1), record.get(2))
        {
            // Only process branded_food entries
            if data_type == "branded_food" {
                if let Ok(fdc_id) = fdc_id_str.parse::<i32>() {
                    if branded_fdc_set.contains(&fdc_id) {
                        food_descriptions.insert(fdc_id, description.to_string());
                        loaded_count += 1;
                    }
                }
            }
        }
    }

    println!("Loaded {} branded food descriptions", loaded_count);

    // Step 3: Stream through food_nutrient.csv to get nutrient values
    // Only process rows for branded foods we care about
    let food_nutrient_path = dir_path.join("food_nutrient.csv");
    let food_nutrient_file = File::open(&food_nutrient_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open food_nutrient.csv: {}", e),
        ))
    })?;

    let mut food_nutrients: HashMap<(i32, i32), f64> = HashMap::new();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(food_nutrient_file);

    let mut processed_count = 0;
    let mut relevant_rows = 0;
    println!("Processing food_nutrient.csv (this may take a while for large datasets)...");

    for result in reader.records() {
        let record = result
            .map_err(|e| ToolboxError::Other(format!("Error reading food_nutrient.csv: {}", e)))?;

        processed_count += 1;
        if processed_count % 1_000_000 == 0 {
            println!(
                "  Processed {} million nutrient rows, matched {} relevant rows...",
                processed_count / 1_000_000,
                relevant_rows
            );
        }

        if let (Some(fdc_id_str), Some(nutrient_id_str), Some(amount_str)) =
            (record.get(1), record.get(2), record.get(3))
        {
            if let (Ok(fdc_id), Ok(nutrient_id)) =
                (fdc_id_str.parse::<i32>(), nutrient_id_str.parse::<i32>())
            {
                // Only process if this is a branded food we care about
                if branded_fdc_set.contains(&fdc_id) {
                    if let Ok(amount) = amount_str.parse::<f64>() {
                        food_nutrients.insert((fdc_id, nutrient_id), amount);
                        relevant_rows += 1;
                    }
                }
            }
        }
    }

    println!(
        "Loaded {} unique nutrient values for branded foods",
        food_nutrients.len()
    );

    // Step 4: Process each branded food
    let mut imported = 0;
    let mut _skipped = 0;
    let mut errors = 0;

    for fdc_id in branded_fdc_ids.iter() {
        // Get food description
        let food_name = match food_descriptions.get(fdc_id) {
            Some(name) => name.trim().to_string(),
            None => {
                if skip_errors {
                    eprintln!("FDC ID {}: No description found", fdc_id);
                    _skipped += 1;
                    continue;
                } else {
                    return Err(ToolboxError::Validation(format!(
                        "FDC ID {}: No description found",
                        fdc_id
                    )));
                }
            }
        };

        if food_name.is_empty() {
            if skip_errors {
                _skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "FDC ID {}: Empty description",
                    fdc_id
                )));
            }
        }

        // Extract nutrient values
        let calories = food_nutrients
            .get(&(*fdc_id, NUTRIENT_ENERGY))
            .copied()
            .unwrap_or(0.0);
        let protein = food_nutrients
            .get(&(*fdc_id, NUTRIENT_PROTEIN))
            .copied()
            .unwrap_or(0.0);
        let fat = food_nutrients
            .get(&(*fdc_id, NUTRIENT_FAT))
            .copied()
            .unwrap_or(0.0);
        let carbs = food_nutrients
            .get(&(*fdc_id, NUTRIENT_CARBS))
            .copied()
            .unwrap_or(0.0);
        let fiber = food_nutrients.get(&(*fdc_id, NUTRIENT_FIBER)).copied();
        let sugar = food_nutrients.get(&(*fdc_id, NUTRIENT_SUGAR)).copied();

        // Validate required nutrients
        if calories == 0.0 && protein == 0.0 && carbs == 0.0 && fat == 0.0 {
            if skip_errors {
                eprintln!(
                    "FDC ID {} ({}): No nutritional data found",
                    fdc_id, food_name
                );
                _skipped += 1;
                continue;
            } else {
                return Err(ToolboxError::Validation(format!(
                    "FDC ID {} ({}): No nutritional data found",
                    fdc_id, food_name
                )));
            }
        }

        // Find or create ingredient
        let ingredient = match NutritionService::find_or_create_ingredient(pool, &food_name).await {
            Ok(ing) => ing,
            Err(e) => {
                if skip_errors {
                    eprintln!(
                        "FDC ID {} ({}): Error creating ingredient: {}",
                        fdc_id, food_name, e
                    );
                    errors += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        // Create or update nutritional info
        // Convert f64 to BigDecimal via string parsing
        let calories_bd = calories
            .to_string()
            .parse::<BigDecimal>()
            .map_err(|e| ToolboxError::Validation(format!("Invalid calories value: {}", e)))?;
        let protein_bd = protein
            .to_string()
            .parse::<BigDecimal>()
            .map_err(|e| ToolboxError::Validation(format!("Invalid protein value: {}", e)))?;
        let carbs_bd = carbs
            .to_string()
            .parse::<BigDecimal>()
            .map_err(|e| ToolboxError::Validation(format!("Invalid carbs value: {}", e)))?;
        let fat_bd = fat
            .to_string()
            .parse::<BigDecimal>()
            .map_err(|e| ToolboxError::Validation(format!("Invalid fat value: {}", e)))?;
        let fiber_bd = fiber.and_then(|f| f.to_string().parse::<BigDecimal>().ok());
        let sugar_bd = sugar.and_then(|s| s.to_string().parse::<BigDecimal>().ok());

        match NutritionService::upsert_nutritional_info(
            pool,
            ingredient.id,
            calories_bd,
            protein_bd,
            carbs_bd,
            fat_bd,
            fiber_bd,
            sugar_bd,
        )
        .await
        {
            Ok(_) => {
                imported += 1;
                if imported % 100 == 0 {
                    println!("Imported {} ingredients...", imported);
                }
            }
            Err(e) => {
                if skip_errors {
                    eprintln!(
                        "FDC ID {} ({}): Error creating nutritional info: {}",
                        fdc_id, food_name, e
                    );
                    errors += 1;
                } else {
                    return Err(e);
                }
            }
        }
    }

    println!("\nImport complete:");
    println!("  Imported: {}", imported);
    if errors > 0 {
        println!("  Errors: {}", errors);
    }

    Ok(())
}

/// Import ingredients from USDA Foundation Foods JSON file
/// Uses streaming parser to avoid loading entire file into memory
pub async fn import_usda_ingredients_json(
    pool: &sqlx::PgPool,
    file: &str,
    skip_errors: bool,
    batch_size: usize,
    concurrent_batches: Option<usize>,
) -> Result<()> {
    // USDA nutrient IDs we care about
    // Note: Energy can be 1008 (Energy KCAL) or 2047/2048 (Energy Atwater factors)
    const NUTRIENT_ENERGY: &[i32] = &[2047, 2048, 1008]; // Prefer Atwater General, then Specific, then KCAL
    const NUTRIENT_PROTEIN: i32 = 1003; // Protein (G)
    const NUTRIENT_FAT: i32 = 1004; // Total lipid (fat) (G)
    const NUTRIENT_CARBS: i32 = 1005; // Carbohydrate, by difference (G)
    const NUTRIENT_FIBER: i32 = 1079; // Fiber, total dietary (G)
    const NUTRIENT_SUGAR: i32 = 1063; // Sugars, Total (G)

    println!(
        "Loading USDA Foundation Foods JSON from: {} (streaming)",
        file
    );

    let file_path = Path::new(file);
    let file_handle = File::open(file_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open JSON file {}: {}", file, e),
        ))
    })?;

    stream_parse_json_array(
        pool,
        BufReader::new(file_handle),
        "FoundationFoods",
        NUTRIENT_ENERGY,
        NUTRIENT_PROTEIN,
        NUTRIENT_FAT,
        NUTRIENT_CARBS,
        NUTRIENT_FIBER,
        NUTRIENT_SUGAR,
        skip_errors,
        None,
        batch_size,
        concurrent_batches,
    )
    .await
}

/// Import ingredients from USDA Branded Foods JSON file
/// Uses streaming parser to avoid loading entire file into memory
pub async fn import_usda_branded_ingredients_json(
    pool: &sqlx::PgPool,
    file: &str,
    skip_errors: bool,
    limit: Option<usize>,
    batch_size: usize,
    concurrent_batches: Option<usize>,
) -> Result<()> {
    // USDA nutrient IDs we care about
    // Note: Energy can be 1008 (Energy KCAL) or 2047/2048 (Energy Atwater factors)
    const NUTRIENT_ENERGY: &[i32] = &[2047, 2048, 1008]; // Prefer Atwater General, then Specific, then KCAL
    const NUTRIENT_PROTEIN: i32 = 1003; // Protein (G)
    const NUTRIENT_FAT: i32 = 1004; // Total lipid (fat) (G)
    const NUTRIENT_CARBS: i32 = 1005; // Carbohydrate, by difference (G)
    const NUTRIENT_FIBER: i32 = 1079; // Fiber, total dietary (G)
    const NUTRIENT_SUGAR: i32 = 1063; // Sugars, Total (G)

    if let Some(limit_val) = limit {
        println!(
            "Loading USDA Branded Foods JSON from: {} (streaming, limited to {})",
            file, limit_val
        );
    } else {
        println!("Loading USDA Branded Foods JSON from: {} (streaming)", file);
    }

    let file_path = Path::new(file);
    let file_handle = File::open(file_path).map_err(|e| {
        ToolboxError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to open JSON file {}: {}", file, e),
        ))
    })?;

    stream_parse_json_array(
        pool,
        BufReader::new(file_handle),
        "BrandedFoods",
        NUTRIENT_ENERGY,
        NUTRIENT_PROTEIN,
        NUTRIENT_FAT,
        NUTRIENT_CARBS,
        NUTRIENT_FIBER,
        NUTRIENT_SUGAR,
        skip_errors,
        limit,
        batch_size,
        concurrent_batches,
    )
    .await
}

/// Stream parse a JSON file with structure: {"ArrayName": [ ... ]}
/// This avoids loading the entire file into memory
/// Processes multiple batches concurrently using worker tasks
async fn stream_parse_json_array<R: BufRead>(
    pool: &sqlx::PgPool,
    mut reader: R,
    array_key: &str,
    energy_nutrient_ids: &[i32],
    protein_nutrient_id: i32,
    fat_nutrient_id: i32,
    carbs_nutrient_id: i32,
    fiber_nutrient_id: i32,
    sugar_nutrient_id: i32,
    skip_errors: bool,
    limit: Option<usize>,
    batch_size: usize,
    concurrent_batches: Option<usize>,
) -> Result<()> {
    // Determine number of concurrent batches
    let num_concurrent_batches = concurrent_batches
        .unwrap_or_else(|| std::cmp::max(2, std::cmp::min(10, 1000 / batch_size.max(1))));

    // Read until we find the array start: {"ArrayName": [
    let mut buffer = String::new();
    let found_array_start;
    let array_start_pattern = format!("\"{}\": [", array_key);

    // Read in chunks to find the array start
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).map_err(|e| {
            ToolboxError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Error reading file: {}", e),
            ))
        })?;

        if bytes_read == 0 {
            return Err(ToolboxError::Validation(format!(
                "Could not find '{}' array in JSON file",
                array_key
            )));
        }

        buffer.push_str(&line);

        // Check if we found the array start
        if let Some(_pos) = buffer.find(&array_start_pattern) {
            // We found the array start, so we can discard the buffer prefix
            // The remaining buffer content will be processed in the main loop
            found_array_start = true;
            // Clear buffer since we'll start fresh from array_start position
            buffer.clear();
            break;
        }

        // Keep a reasonable buffer size (don't accumulate too much)
        if buffer.len() > 10000 {
            return Err(ToolboxError::Validation(format!(
                "Could not find '{}' array start in first 10KB of file",
                array_key
            )));
        }
    }

    if !found_array_start {
        return Err(ToolboxError::Validation(format!(
            "Could not find '{}' array in JSON file",
            array_key
        )));
    }

    // Create broadcast channel for sending batches to workers
    // Broadcast allows multiple receivers, but each message is received by all
    // So we need a different approach - use a work-stealing queue pattern
    // Instead, use mpsc with a dispatcher that round-robins to workers
    let (batch_tx, mut batch_rx) = mpsc::unbounded_channel::<(Vec<Value>, usize)>();
    let (result_tx, mut result_rx) = mpsc::unbounded_channel::<(usize, usize, usize)>(); // (imported, errors, processed)

    // Spawn worker tasks to process batches concurrently
    let pool_clone = pool.clone();
    let energy_nutrient_ids_clone = energy_nutrient_ids.to_vec();
    let skip_errors_clone = skip_errors;
    let num_workers = num_concurrent_batches;

    // Create worker channels - one per worker for true parallelism
    let mut worker_channels: Vec<mpsc::UnboundedSender<(Vec<Value>, usize)>> = Vec::new();
    let mut worker_handles = Vec::new();

    for _worker_id in 0..num_workers {
        let (worker_tx, mut worker_rx) = mpsc::unbounded_channel::<(Vec<Value>, usize)>();
        worker_channels.push(worker_tx);

        let pool_worker = pool_clone.clone();
        let tx = result_tx.clone();
        let energy_ids = energy_nutrient_ids_clone.clone();

        let handle = tokio::spawn(async move {
            while let Some((batch, start_line_num)) = worker_rx.recv().await {
                let batch_results = process_batch(
                    &pool_worker,
                    &batch,
                    &energy_ids,
                    protein_nutrient_id,
                    fat_nutrient_id,
                    carbs_nutrient_id,
                    fiber_nutrient_id,
                    sugar_nutrient_id,
                    skip_errors_clone,
                    start_line_num,
                )
                .await;

                let mut imported_count = 0;
                let mut error_count = 0;

                for result in batch_results {
                    match result {
                        Ok(()) => {
                            imported_count += 1;
                        }
                        Err(_) => {
                            error_count += 1;
                        }
                    }
                }

                let _ = tx.send((imported_count, error_count, batch.len()));
            }
        });
        worker_handles.push(handle);
    }

    // Spawn dispatcher that round-robins batches to workers
    // Use a work-stealing approach: send to the worker with the shortest queue
    let dispatcher_handle = tokio::spawn(async move {
        let mut worker_index = 0;
        while let Some(batch_data) = batch_rx.recv().await {
            // Round-robin distribution to workers
            // This ensures even distribution and prevents one worker from getting all batches
            if let Some(worker_tx) = worker_channels.get(worker_index % worker_channels.len()) {
                if worker_tx.send(batch_data).is_err() {
                    // Worker channel closed, continue to next
                }
            }
            worker_index += 1;
        }
        // Close all worker channels
        for worker_tx in worker_channels {
            drop(worker_tx);
        }
    });

    // Give workers a moment to start up and be ready
    // This ensures they're actively polling before we start sending batches
    tokio::task::yield_now().await;

    // Now stream parse each object in the array
    // We'll use a manual approach: track braces to find complete JSON objects
    // Collect items into batches and send to workers
    let mut processed = 0;
    let mut object_buffer = String::new();
    let mut brace_depth = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut object_start_pos = 0;
    let mut current_batch: Vec<Value> = Vec::new();
    let mut batch_num = 0;

    // Spawn task to collect results
    let result_handle = tokio::spawn(async move {
        let mut total_imported = 0;
        let mut total_errors = 0;
        let mut total_processed = 0;

        while let Some((imported, errors, batch_size)) = result_rx.recv().await {
            total_imported += imported;
            total_errors += errors;
            total_processed += batch_size;

            if total_processed % 1000 == 0 {
                println!(
                    "Processed {} foods, imported {}...",
                    total_processed, total_imported
                );
            }
            if total_imported % 100 == 0 && total_imported > 0 {
                println!("Imported {} ingredients...", total_imported);
            }
        }

        (total_imported, total_errors)
    });

    // Stream parse objects from the array
    loop {
        // Apply limit if specified
        if let Some(limit_val) = limit {
            if processed >= limit_val {
                break;
            }
        }

        // Use fill_buf to get available data
        let bytes_to_process = {
            let buf = reader.fill_buf().map_err(|e| {
                ToolboxError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Error reading file: {}", e),
                ))
            })?;

            if buf.is_empty() {
                // End of file - process any remaining items
                if !object_buffer.trim().is_empty() && brace_depth == 0 {
                    let object_str = object_buffer.trim().trim_start_matches(',').trim();
                    if object_str.starts_with('{') {
                        if let Ok(food) = serde_json::from_str::<Value>(object_str) {
                            processed += 1;

                            // For branded foods, verify dataType
                            if array_key == "BrandedFoods" {
                                if let Some(data_type) =
                                    food.get("dataType").and_then(|v| v.as_str())
                                {
                                    if data_type != "Branded" {
                                        // Skip non-branded foods
                                    } else {
                                        current_batch.push(food);
                                    }
                                } else {
                                    current_batch.push(food);
                                }
                            } else {
                                current_batch.push(food);
                            }
                        }
                    }
                }

                // Send final batch
                if !current_batch.is_empty() {
                    let start_line = processed - current_batch.len() + 1;
                    let batch_to_send: Vec<Value> = current_batch.drain(..).collect();
                    let _ = batch_tx.send((batch_to_send, start_line));
                }

                // Close the channel to signal workers to finish
                drop(batch_tx);

                // Wait for all workers to finish
                for handle in worker_handles {
                    let _ = handle.await;
                }

                // Wait for dispatcher to finish
                let _ = dispatcher_handle.await;

                // Close result channel and get final counts
                drop(result_tx);
                let (imported, errors) = result_handle.await.map_err(|e| {
                    ToolboxError::Validation(format!("Error collecting results: {}", e))
                })?;

                println!("\nImport complete:");
                println!("  Imported: {}", imported);
                if errors > 0 {
                    println!("  Errors: {}", errors);
                }

                return Ok(());
            }

            // Process characters from buffer
            for &byte in buf {
                let ch = byte as char;
                let pos = object_buffer.len();
                object_buffer.push(ch);

                if escape_next {
                    escape_next = false;
                    continue;
                }

                match ch {
                    '\\' if in_string => {
                        escape_next = true;
                    }
                    '"' => {
                        in_string = !in_string;
                    }
                    '{' if !in_string => {
                        if brace_depth == 0 {
                            // Start of a new object
                            object_start_pos = pos;
                        }
                        brace_depth += 1;
                    }
                    '}' if !in_string => {
                        brace_depth -= 1;
                        // When we close all braces, we have a complete object
                        if brace_depth == 0 {
                            // Extract just the object (from object_start_pos to current end)
                            let object_str = object_buffer[object_start_pos..].trim();
                            match serde_json::from_str::<Value>(object_str) {
                                Ok(food) => {
                                    processed += 1;

                                    // For branded foods, verify dataType
                                    if array_key == "BrandedFoods" {
                                        if let Some(data_type) =
                                            food.get("dataType").and_then(|v| v.as_str())
                                        {
                                            if data_type != "Branded" {
                                                // Clear buffer up to this point and continue
                                                object_buffer.clear();
                                                continue;
                                            }
                                        }
                                    }

                                    // Add to current batch
                                    current_batch.push(food);

                                    // Send batch to workers when it reaches batch_size
                                    if current_batch.len() >= batch_size {
                                        let start_line = processed - current_batch.len() + 1;
                                        let batch_to_send: Vec<Value> =
                                            current_batch.drain(..).collect();

                                        // Send batch - unbounded channel never blocks, but yield occasionally
                                        // to let workers process and maintain parallelism
                                        if let Err(_) = batch_tx.send((batch_to_send, start_line)) {
                                            // Channel closed, workers are done
                                            break;
                                        }
                                        batch_num += 1;

                                        // Yield every few batches to ensure workers get a chance to process
                                        // This helps maintain parallelism, especially at the start
                                        if batch_num % num_concurrent_batches == 0 {
                                            tokio::task::yield_now().await;
                                        }
                                    }

                                    // Clear buffer - we've processed this object
                                    object_buffer.clear();
                                }
                                Err(e) => {
                                    if skip_errors {
                                        eprintln!(
                                            "Error parsing JSON object at position {}: {}",
                                            processed + 1,
                                            e
                                        );
                                        // Try to recover by clearing buffer
                                        object_buffer.clear();
                                    } else {
                                        return Err(ToolboxError::Validation(format!(
                                            "Error parsing JSON object at position {}: {}",
                                            processed + 1,
                                            e
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            buf.len()
        };

        // Consume the bytes we processed (after the borrow is released)
        reader.consume(bytes_to_process);
    }

    // Should not reach here, but handle it just in case
    drop(batch_tx);
    let _ = dispatcher_handle.await;
    for handle in worker_handles {
        let _ = handle.await;
    }
    drop(result_tx);
    let (imported, errors) = result_handle
        .await
        .map_err(|e| ToolboxError::Validation(format!("Error collecting results: {}", e)))?;

    println!("\nImport complete:");
    println!("  Imported: {}", imported);
    if errors > 0 {
        println!("  Errors: {}", errors);
    }

    Ok(())
}

/// Process a batch of foods in parallel
async fn process_batch(
    pool: &sqlx::PgPool,
    foods: &[Value],
    energy_nutrient_ids: &[i32],
    protein_nutrient_id: i32,
    fat_nutrient_id: i32,
    carbs_nutrient_id: i32,
    fiber_nutrient_id: i32,
    sugar_nutrient_id: i32,
    skip_errors: bool,
    start_line_num: usize,
) -> Vec<Result<()>> {
    let tasks: Vec<_> = foods
        .iter()
        .enumerate()
        .map(|(idx, food)| {
            let pool = pool;
            let food = food;
            async move {
                process_food_json(
                    pool,
                    food,
                    energy_nutrient_ids,
                    protein_nutrient_id,
                    fat_nutrient_id,
                    carbs_nutrient_id,
                    fiber_nutrient_id,
                    sugar_nutrient_id,
                    skip_errors,
                    start_line_num + idx,
                )
                .await
            }
        })
        .collect();

    join_all(tasks).await
}

/// Helper function to process a single food JSON object
async fn process_food_json(
    pool: &sqlx::PgPool,
    food: &Value,
    energy_nutrient_ids: &[i32],
    protein_nutrient_id: i32,
    fat_nutrient_id: i32,
    carbs_nutrient_id: i32,
    fiber_nutrient_id: i32,
    sugar_nutrient_id: i32,
    skip_errors: bool,
    line_num: usize,
) -> Result<()> {
    // Extract food description
    let food_name = food
        .get("description")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolboxError::Validation(format!("Line {}: Missing description", line_num))
        })?;

    if food_name.trim().is_empty() {
        if skip_errors {
            return Ok(()); // Skip empty names
        } else {
            return Err(ToolboxError::Validation(format!(
                "Line {}: Empty description",
                line_num
            )));
        }
    }

    // Extract foodNutrients array
    let food_nutrients = food
        .get("foodNutrients")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            ToolboxError::Validation(format!("Line {}: Missing foodNutrients array", line_num))
        })?;

    // Build a map of nutrient_id -> amount
    // For energy, we'll collect all and pick the best one
    let mut nutrient_map: HashMap<i32, f64> = HashMap::new();
    let mut energy_values: Vec<(i32, f64)> = Vec::new();

    for nutrient_obj in food_nutrients {
        if let (Some(nutrient), Some(amount)) = (
            nutrient_obj
                .get("nutrient")
                .and_then(|n| n.get("id").and_then(|v| v.as_i64())),
            nutrient_obj.get("amount").and_then(|v| v.as_f64()),
        ) {
            let nutrient_id = nutrient as i32;
            if energy_nutrient_ids.contains(&nutrient_id) {
                energy_values.push((nutrient_id, amount));
            } else {
                nutrient_map.insert(nutrient_id, amount);
            }
        }
    }

    // Pick the best energy value (earliest in priority list)
    if let Some((best_energy_id, best_energy_value)) = energy_values.iter().min_by_key(|(id, _)| {
        energy_nutrient_ids
            .iter()
            .position(|&eid| eid == *id)
            .unwrap_or(usize::MAX)
    }) {
        nutrient_map.insert(*best_energy_id, *best_energy_value);
    }

    // Extract nutrient values, trying multiple energy IDs in priority order
    let mut calories = 0.0;
    for &energy_id in energy_nutrient_ids {
        if let Some(cal) = nutrient_map.get(&energy_id) {
            calories = *cal;
            break;
        }
    }

    let protein = nutrient_map
        .get(&protein_nutrient_id)
        .copied()
        .unwrap_or(0.0);
    let fat = nutrient_map.get(&fat_nutrient_id).copied().unwrap_or(0.0);
    let carbs = nutrient_map.get(&carbs_nutrient_id).copied().unwrap_or(0.0);
    let fiber = nutrient_map.get(&fiber_nutrient_id).copied();
    let sugar = nutrient_map.get(&sugar_nutrient_id).copied();

    // Validate required nutrients
    if calories == 0.0 && protein == 0.0 && carbs == 0.0 && fat == 0.0 {
        if skip_errors {
            return Ok(()); // Skip foods with no nutritional data
        } else {
            return Err(ToolboxError::Validation(format!(
                "Line {} ({}): No nutritional data found",
                line_num, food_name
            )));
        }
    }

    // Find or create ingredient
    let ingredient = NutritionService::find_or_create_ingredient(pool, food_name.trim())
        .await
        .map_err(|e| {
            ToolboxError::Other(format!(
                "Line {} ({}): Error creating ingredient: {}",
                line_num, food_name, e
            ))
        })?;

    // Convert f64 to BigDecimal via string parsing
    let calories_bd = calories
        .to_string()
        .parse::<BigDecimal>()
        .map_err(|e| ToolboxError::Validation(format!("Invalid calories value: {}", e)))?;
    let protein_bd = protein
        .to_string()
        .parse::<BigDecimal>()
        .map_err(|e| ToolboxError::Validation(format!("Invalid protein value: {}", e)))?;
    let carbs_bd = carbs
        .to_string()
        .parse::<BigDecimal>()
        .map_err(|e| ToolboxError::Validation(format!("Invalid carbs value: {}", e)))?;
    let fat_bd = fat
        .to_string()
        .parse::<BigDecimal>()
        .map_err(|e| ToolboxError::Validation(format!("Invalid fat value: {}", e)))?;
    let fiber_bd = fiber.and_then(|f| f.to_string().parse::<BigDecimal>().ok());
    let sugar_bd = sugar.and_then(|s| s.to_string().parse::<BigDecimal>().ok());

    NutritionService::upsert_nutritional_info(
        pool,
        ingredient.id,
        calories_bd,
        protein_bd,
        carbs_bd,
        fat_bd,
        fiber_bd,
        sugar_bd,
    )
    .await
    .map_err(|e| {
        ToolboxError::Other(format!(
            "Line {} ({}): Error creating nutritional info: {}",
            line_num, food_name, e
        ))
    })?;

    Ok(())
}
