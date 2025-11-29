use crate::error::{Result, ToolboxError};
use crate::nutrition::NutritionService;
use bigdecimal::BigDecimal;
use regex::Regex;
use uuid::Uuid;

/// Parse IDs from a vector, handling both space and comma-separated values
pub fn parse_ids(input: &[String]) -> Vec<String> {
    input
        .iter()
        .flat_map(|s| {
            // Split by comma first, then by space
            s.split(',')
                .flat_map(|part| part.split_whitespace())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .collect()
}

/// Parse search terms from a vector, handling both space and comma-separated values
pub fn parse_terms(input: &[String]) -> Vec<String> {
    input
        .iter()
        .flat_map(|s| {
            // Split by comma first, then by space
            s.split(',')
                .flat_map(|part| part.split_whitespace())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .collect()
}

/// Parse time string like "30 mins", "1 hrs", "1 hrs 30 mins" into minutes
pub fn parse_time(re: &regex::Regex, time_str: &str) -> Option<i32> {
    if time_str.trim().is_empty() {
        return None;
    }

    re.captures(time_str).and_then(|caps| {
        let hours: i32 = caps
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let minutes: i32 = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        Some(hours * 60 + minutes)
    })
}

/// Parse directions string into numbered steps
pub fn parse_directions(directions_str: &str) -> Vec<(i32, String)> {
    if directions_str.trim().is_empty() {
        return vec![];
    }

    // Split by newlines and filter out empty lines
    let lines: Vec<&str> = directions_str
        .split('\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    lines
        .into_iter()
        .enumerate()
        .map(|(idx, line)| ((idx + 1) as i32, line.to_string()))
        .collect()
}

/// Parse ingredients string and create/find ingredients in database
pub async fn parse_ingredients(
    pool: &sqlx::PgPool,
    ingredients_str: &str,
    ingredient_re: &Regex,
) -> Result<Vec<(Uuid, BigDecimal, String)>> {
    if ingredients_str.trim().is_empty() {
        return Ok(vec![]);
    }

    let mut result = Vec::new();

    // Split by comma, but be careful with commas inside parentheses
    let parts: Vec<&str> = ingredients_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    for part in parts {
        // Try to match the pattern: quantity unit name
        if let Some(caps) = ingredient_re.captures(part) {
            let quantity_str = caps.get(1).unwrap().as_str();
            let unit = caps.get(2).unwrap().as_str().to_string();
            let name = caps.get(3).unwrap().as_str().trim().to_string();

            // Parse quantity
            let quantity: BigDecimal = quantity_str.parse().map_err(|e| {
                ToolboxError::Validation(format!("Invalid quantity '{}': {}", quantity_str, e))
            })?;

            // Find or create ingredient
            let ingredient = NutritionService::find_or_create_ingredient(pool, &name).await?;

            result.push((ingredient.id, quantity, unit));
        } else {
            // If regex doesn't match, try to extract just the name (everything after the first number and unit)
            // This is a fallback for complex ingredient strings
            let name = part
                .trim()
                .split_whitespace()
                .skip(2) // Skip quantity and unit
                .collect::<Vec<_>>()
                .join(" ");

            if !name.is_empty() {
                // Default to quantity 1 and unit "piece" if we can't parse
                let ingredient = NutritionService::find_or_create_ingredient(pool, &name).await?;
                result.push((ingredient.id, BigDecimal::from(1), "piece".to_string()));
            }
        }
    }

    Ok(result)
}
