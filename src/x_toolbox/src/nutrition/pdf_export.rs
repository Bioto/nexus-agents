use crate::error::{Result, ToolboxError};
use crate::nutrition::models::{
    AggregatedIngredient, DayOfWeek, MealPlanEntryWithRecipe, MealPlanNutrition, MealPlanWithEntries,
    RecipeNutrition, RecipeWithDetails,
};
use crate::nutrition::NutritionService;
use chrono::NaiveDate;
use printpdf::*;
use sqlx::types::BigDecimal;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// Chef-themed PDF exporter for recipes
pub struct RecipePdfExporter;

impl RecipePdfExporter {
    /// Export a recipe to a chef-themed PDF
    ///
    /// Creates a beautiful PDF with:
    /// - Opening page with recipe title and description
    /// - Ingredients list with friendly formatting
    /// - Step-by-step instructions
    /// - Nutritional information (if available)
    /// - Closing page with chef's note
    pub fn export_recipe(
        recipe: &RecipeWithDetails,
        nutrition: Option<&RecipeNutrition>,
        output_path: &Path,
    ) -> Result<()> {
        // Create PDF document (A4 size)
        let (mut doc, page1, layer1) = PdfDocument::new(
            &recipe.recipe.name,
            Mm(210.0), // A4 width
            Mm(297.0), // A4 height
            "Layer 1",
        );

        // Set document metadata
        doc = doc.with_title(&recipe.recipe.name);
        doc = doc.with_author("Your Personal Chef");

        // Get fonts
        let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)
            .map_err(|e| ToolboxError::Other(format!("Failed to add bold font: {}", e)))?;
        let font_regular = doc.add_builtin_font(BuiltinFont::Helvetica)
            .map_err(|e| ToolboxError::Other(format!("Failed to add regular font: {}", e)))?;
        let font_italic = doc.add_builtin_font(BuiltinFont::HelveticaOblique)
            .map_err(|e| ToolboxError::Other(format!("Failed to add italic font: {}", e)))?;

        // ========== PAGE 1: Opening Page ==========
        let current_layer = doc.get_page(page1).get_layer(layer1);

        // Title - Large and centered
        let title_y = 250.0;
        current_layer.use_text(
            &recipe.recipe.name,
            32.0,
            Mm(105.0 - (recipe.recipe.name.len() as f32 * 2.0)), // Rough centering
            Mm(title_y),
            &font_bold,
        );

        // Description (if available)
        if let Some(description) = &recipe.recipe.description {
            let desc_y = title_y - 25.0;
            // Wrap description text (simple word wrap)
            let wrapped = Self::wrap_text(description, 80);
            let mut y_offset = 0.0;
            for line in wrapped {
                current_layer.use_text(
                    &line,
                    12.0,
                    Mm(20.0),
                    Mm(desc_y - y_offset),
                    &font_italic,
                );
                y_offset += 15.0;
            }
        }

        // Recipe metadata
        let mut metadata_y = title_y - 60.0;
        if let Some(servings) = recipe.recipe.servings {
            current_layer.use_text(
                &format!("Serves: {}", servings),
                14.0,
                Mm(20.0),
                Mm(metadata_y),
                &font_regular,
            );
            metadata_y -= 18.0;
        }

        if let Some(prep_time) = recipe.recipe.prep_time_minutes {
            current_layer.use_text(
                &format!("Prep time: {} minutes", prep_time),
                14.0,
                Mm(20.0),
                Mm(metadata_y),
                &font_regular,
            );
            metadata_y -= 18.0;
        }

        if let Some(cook_time) = recipe.recipe.cook_time_minutes {
            current_layer.use_text(
                &format!("Cook time: {} minutes", cook_time),
                14.0,
                Mm(20.0),
                Mm(metadata_y),
                &font_regular,
            );
        }

        // Chef's welcome message
        let welcome_y = 100.0;
        current_layer.use_text(
            "Welcome to the kitchen!",
            16.0,
            Mm(20.0),
            Mm(welcome_y),
            &font_bold,
        );

        current_layer.use_text(
            "This recipe has been crafted with care. Follow the steps below and enjoy your delicious creation!",
            11.0,
            Mm(20.0),
            Mm(welcome_y - 20.0),
            &font_regular,
        );

        // ========== PAGE 2: Ingredients ==========
        let (page2, layer2) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
        let current_layer = doc.get_page(page2).get_layer(layer2);

        // Section title
        current_layer.use_text(
            "Ingredients",
            24.0,
            Mm(20.0),
            Mm(270.0),
            &font_bold,
        );

        // Ingredients list
        let mut y_pos = 240.0;
        for ingredient in &recipe.ingredients {
            let quantity_str = format!("{}", ingredient.recipe_ingredient.quantity);
            let ingredient_line = format!(
                "• {} {} {}",
                quantity_str,
                ingredient.recipe_ingredient.unit,
                ingredient.ingredient.name
            );

            current_layer.use_text(
                &ingredient_line,
                12.0,
                Mm(30.0),
                Mm(y_pos),
                &font_regular,
            );

            // Add ingredient description if available
            if let Some(desc) = &ingredient.ingredient.description {
                current_layer.use_text(
                    &format!("  ({})", desc),
                    10.0,
                    Mm(35.0),
                    Mm(y_pos - 12.0),
                    &font_italic,
                );
                y_pos -= 15.0;
            }

            y_pos -= 18.0;

            // Check if we need a new page (note: this is a simple check, 
            // in a real implementation you'd want to track the current page)
            if y_pos < 30.0 {
                // For now, just break - multi-page ingredients would need page tracking
                break;
            }
        }

        // ========== PAGE 3+: Instructions ==========
        let (page3, layer3) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
        let current_layer = doc.get_page(page3).get_layer(layer3);

        // Section title
        current_layer.use_text(
            "Instructions",
            24.0,
            Mm(20.0),
            Mm(270.0),
            &font_bold,
        );

        let mut current_page = page3;
        let mut current_layer_idx = layer3;
        let mut y_pos = 240.0;

        for step in &recipe.steps {
            // Step number and instruction
            let step_text = format!("Step {}: {}", step.step_number, step.instruction);
            let wrapped = Self::wrap_text(&step_text, 70);

            for line in wrapped {
                if y_pos < 30.0 {
                    // New page needed
                    let (new_page, new_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
                    current_page = new_page;
                    current_layer_idx = new_layer;
                    let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                    
                    // Re-add section title on new page
                    current_layer.use_text(
                        "Instructions (continued)",
                        20.0,
                        Mm(20.0),
                        Mm(270.0),
                        &font_bold,
                    );
                    y_pos = 240.0;
                }

                let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                current_layer.use_text(
                    &line,
                    11.0,
                    Mm(30.0),
                    Mm(y_pos),
                    &font_regular,
                );
                y_pos -= 14.0;
            }

            y_pos -= 8.0; // Space between steps
        }

        // ========== Nutrition Page (if available) ==========
        if let Some(nutrition) = nutrition {
            let (nut_page, nut_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
            let current_layer = doc.get_page(nut_page).get_layer(nut_layer);

            current_layer.use_text(
                "Nutritional Information",
                24.0,
                Mm(20.0),
                Mm(270.0),
                &font_bold,
            );

            let mut y_pos = 240.0;

            // Total nutrition
            current_layer.use_text(
                "Per Recipe:",
                16.0,
                Mm(30.0),
                Mm(y_pos),
                &font_bold,
            );
            y_pos -= 20.0;

            current_layer.use_text(
                &format!("Calories: {}", Self::format_decimal(&nutrition.total_calories)),
                12.0,
                Mm(40.0),
                Mm(y_pos),
                &font_regular,
            );
            y_pos -= 16.0;

            current_layer.use_text(
                &format!("Protein: {}g", Self::format_decimal(&nutrition.total_protein_g)),
                12.0,
                Mm(40.0),
                Mm(y_pos),
                &font_regular,
            );
            y_pos -= 16.0;

            current_layer.use_text(
                &format!("Carbohydrates: {}g", Self::format_decimal(&nutrition.total_carbs_g)),
                12.0,
                Mm(40.0),
                Mm(y_pos),
                &font_regular,
            );
            y_pos -= 16.0;

            current_layer.use_text(
                &format!("Fat: {}g", Self::format_decimal(&nutrition.total_fat_g)),
                12.0,
                Mm(40.0),
                Mm(y_pos),
                &font_regular,
            );

            // Per serving (if available)
            if let Some(per_serving_cal) = &nutrition.per_serving_calories {
                y_pos -= 30.0;
                current_layer.use_text(
                    "Per Serving:",
                    16.0,
                    Mm(30.0),
                    Mm(y_pos),
                    &font_bold,
                );
                y_pos -= 20.0;

                current_layer.use_text(
                    &format!("Calories: {}", Self::format_decimal(per_serving_cal)),
                    12.0,
                    Mm(40.0),
                    Mm(y_pos),
                    &font_regular,
                );
            }
        }

        // ========== Final Page: Chef's Note ==========
        let (final_page, final_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
        let current_layer = doc.get_page(final_page).get_layer(final_layer);

        // Centered closing message
        let closing_y = 200.0;
        current_layer.use_text(
            "Bon Appétit!",
            28.0,
            Mm(70.0),
            Mm(closing_y),
            &font_bold,
        );

        current_layer.use_text(
            "Thank you for cooking with us today.",
            14.0,
            Mm(50.0),
            Mm(closing_y - 35.0),
            &font_regular,
        );

        current_layer.use_text(
            "We hope you enjoy this recipe as much as we enjoyed creating it for you.",
            12.0,
            Mm(30.0),
            Mm(closing_y - 55.0),
            &font_italic,
        );

        current_layer.use_text(
            "Happy cooking!",
            16.0,
            Mm(70.0),
            Mm(closing_y - 85.0),
            &font_bold,
        );

        // Save PDF
        let file = File::create(output_path)
            .map_err(|e| ToolboxError::Io(e))?;
        let mut writer = BufWriter::new(file);
        doc.save(&mut writer)
            .map_err(|e| ToolboxError::Other(format!("Failed to save PDF: {}", e)))?;

        Ok(())
    }

    /// Simple text wrapping helper
    fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut lines = Vec::new();
        let mut current_line = String::new();

        for word in words {
            if current_line.is_empty() {
                current_line = word.to_string();
            } else if current_line.len() + word.len() + 1 <= max_width {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                lines.push(current_line);
                current_line = word.to_string();
            }
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }
        lines
    }

    /// Format BigDecimal for display
    fn format_decimal(bd: &BigDecimal) -> String {
        // Convert to f64 for simpler display, or use to_string() for precision
        bd.to_string()
    }

    /// Export a meal plan to a chef-themed PDF
    ///
    /// Creates a beautiful PDF with:
    /// - Opening page with meal plan title and description
    /// - Daily meal schedule organized by day/date
    /// - Recipe listings with full ingredients and instructions for each meal
    /// - Nutritional summary (if available)
    /// - Closing page with chef's note
    pub async fn export_meal_plan(
        meal_plan: &MealPlanWithEntries,
        nutrition: Option<&MealPlanNutrition>,
        pool: &sqlx::PgPool,
        output_path: &Path,
    ) -> Result<()> {
        // Layout constants
        const PAGE_WIDTH: f32 = 210.0; // A4 width in mm
        const PAGE_HEIGHT: f32 = 297.0; // A4 height in mm
        const MARGIN_LEFT: f32 = 20.0;
        const MARGIN_RIGHT: f32 = 20.0;
        const MARGIN_TOP: f32 = 25.0;
        const MARGIN_BOTTOM: f32 = 25.0;
        const CONTENT_WIDTH: f32 = PAGE_WIDTH - MARGIN_LEFT - MARGIN_RIGHT;
        const CONTENT_HEIGHT: f32 = PAGE_HEIGHT - MARGIN_TOP - MARGIN_BOTTOM;
        
        // Spacing constants
        const LINE_HEIGHT_SMALL: f32 = 12.0;
        const LINE_HEIGHT_MEDIUM: f32 = 14.0;
        const LINE_HEIGHT_LARGE: f32 = 18.0;
        const PARAGRAPH_SPACING: f32 = 8.0;
        const SECTION_SPACING: f32 = 20.0;
        
        // Colors (RGB 0.0-1.0)
        let color_primary = Color::Rgb(Rgb::new(0.85, 0.35, 0.15, None)); // Warm orange
        let color_secondary = Color::Rgb(Rgb::new(0.2, 0.5, 0.3, None)); // Green
        let color_text = Color::Rgb(Rgb::new(0.2, 0.2, 0.2, None)); // Dark gray
        let color_light_bg = Color::Rgb(Rgb::new(0.98, 0.96, 0.94, None)); // Cream
        // Create PDF document (A4 size)
        let (mut doc, page1, layer1) = PdfDocument::new(
            &meal_plan.meal_plan.name,
            Mm(PAGE_WIDTH),
            Mm(PAGE_HEIGHT),
            "Layer 1",
        );

        // Set document metadata
        doc = doc.with_title(&meal_plan.meal_plan.name);
        doc = doc.with_author("Your Personal Chef");

        // Get fonts
        let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)
            .map_err(|e| ToolboxError::Other(format!("Failed to add bold font: {}", e)))?;
        let font_regular = doc.add_builtin_font(BuiltinFont::Helvetica)
            .map_err(|e| ToolboxError::Other(format!("Failed to add regular font: {}", e)))?;
        let font_italic = doc.add_builtin_font(BuiltinFont::HelveticaOblique)
            .map_err(|e| ToolboxError::Other(format!("Failed to add italic font: {}", e)))?;

        // ========== PAGE 1: Opening Page ==========
        let current_layer = doc.get_page(page1).get_layer(layer1);
        
        // Helper function to calculate text width for centering
        let center_text = |text: &str, font_size: f32| -> f32 {
            // Rough estimate: 0.6mm per character at 12pt
            let text_width = text.len() as f32 * font_size * 0.05;
            (PAGE_WIDTH - text_width) / 2.0
        };

        // Title - Large and centered with proper spacing
        let mut y_pos = PAGE_HEIGHT - MARGIN_TOP - 40.0;
        let title_font_size = 36.0;
        let title_x = center_text(&meal_plan.meal_plan.name, title_font_size);
        current_layer.set_fill_color(color_primary.clone());
        current_layer.use_text(
            &meal_plan.meal_plan.name,
            title_font_size,
            Mm(title_x.max(MARGIN_LEFT)),
            Mm(y_pos),
            &font_bold,
        );
        y_pos -= title_font_size + SECTION_SPACING;

        // Description (if available) with proper wrapping
        if let Some(description) = &meal_plan.meal_plan.description {
            current_layer.set_fill_color(color_text.clone());
            let wrapped = Self::wrap_text(description, 70);
            for line in wrapped {
                if y_pos < MARGIN_BOTTOM + 50.0 {
                    break; // Don't overflow page
                }
                current_layer.use_text(
                    &line,
                    LINE_HEIGHT_MEDIUM,
                    Mm(MARGIN_LEFT),
                    Mm(y_pos),
                    &font_italic,
                );
                y_pos -= LINE_HEIGHT_MEDIUM + 4.0;
            }
            y_pos -= PARAGRAPH_SPACING;
        }

        // Meal plan metadata with proper spacing
        y_pos -= SECTION_SPACING;
        current_layer.set_fill_color(color_text.clone());
        
        if meal_plan.meal_plan.is_template {
            current_layer.use_text(
                "Template Meal Plan",
                LINE_HEIGHT_MEDIUM,
                Mm(MARGIN_LEFT),
                Mm(y_pos),
                &font_regular,
            );
            y_pos -= LINE_HEIGHT_MEDIUM + PARAGRAPH_SPACING;
        }

        if let Some(start_date) = meal_plan.meal_plan.start_date {
            current_layer.use_text(
                &format!("Start Date: {}", start_date.format("%B %d, %Y")),
                LINE_HEIGHT_MEDIUM,
                Mm(MARGIN_LEFT),
                Mm(y_pos),
                &font_regular,
            );
            y_pos -= LINE_HEIGHT_MEDIUM + PARAGRAPH_SPACING;
        }

        if let Some(end_date) = meal_plan.meal_plan.end_date {
            current_layer.use_text(
                &format!("End Date: {}", end_date.format("%B %d, %Y")),
                LINE_HEIGHT_MEDIUM,
                Mm(MARGIN_LEFT),
                Mm(y_pos),
                &font_regular,
            );
            y_pos -= LINE_HEIGHT_MEDIUM + PARAGRAPH_SPACING;
        }

        current_layer.use_text(
            &format!("Total Meals: {}", meal_plan.entries.len()),
            LINE_HEIGHT_MEDIUM,
            Mm(MARGIN_LEFT),
            Mm(y_pos),
            &font_regular,
        );
        y_pos -= LINE_HEIGHT_MEDIUM + SECTION_SPACING * 2.0;

        // Chef's welcome message
        current_layer.set_fill_color(color_secondary.clone());
        current_layer.use_text(
            "Your Weekly Meal Plan",
            20.0,
            Mm(MARGIN_LEFT),
            Mm(y_pos),
            &font_bold,
        );
        y_pos -= 24.0 + PARAGRAPH_SPACING;

        current_layer.set_fill_color(color_text.clone());
        let welcome_text = "This meal plan has been carefully crafted to help you eat well throughout the week. Enjoy your delicious meals!";
        let wrapped = Self::wrap_text(welcome_text, 70);
        for line in wrapped {
            if y_pos < MARGIN_BOTTOM + 30.0 {
                break;
            }
            current_layer.use_text(
                &line,
                LINE_HEIGHT_SMALL,
                Mm(MARGIN_LEFT),
                Mm(y_pos),
                &font_regular,
            );
            y_pos -= LINE_HEIGHT_SMALL + 4.0;
        }

        // ========== PAGE 2+: Daily Schedule ==========
        // Group entries by day
        let mut daily_entries: HashMap<(Option<NaiveDate>, Option<i32>), Vec<&MealPlanEntryWithRecipe>> = HashMap::new();
        for entry in &meal_plan.entries {
            let key = (entry.entry.date, entry.entry.day_of_week);
            daily_entries.entry(key).or_insert_with(Vec::new).push(entry);
        }

        // Sort days: by date if available, otherwise by day_of_week
        let mut sorted_days: Vec<_> = daily_entries.keys().collect();
        sorted_days.sort_by(|a, b| {
            match (a.0, b.0) {
                (Some(date_a), Some(date_b)) => date_a.cmp(&date_b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => {
                    a.1.cmp(&b.1)
                }
            }
        });

        let mut current_page = page1;
        let mut current_layer_idx = layer1;
        let mut y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
        let mut page_count = 1;

        for day_key in sorted_days {
            let entries = &daily_entries[day_key];
            
            // Check if we need a new page (leave room for day header + at least one meal)
            if y_pos < MARGIN_BOTTOM + 80.0 || page_count == 1 {
                if page_count > 1 {
                    let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                    current_page = new_page;
                    current_layer_idx = new_layer;
                }
                page_count += 1;
                let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                
                // Day header with color
                let day_label = if let Some(date) = day_key.0 {
                    date.format("%A, %B %d, %Y").to_string()
                } else if let Some(day_of_week) = day_key.1 {
                    if let Some(day) = DayOfWeek::from_int(day_of_week) {
                        format!("{}", match day {
                            DayOfWeek::Monday => "Monday",
                            DayOfWeek::Tuesday => "Tuesday",
                            DayOfWeek::Wednesday => "Wednesday",
                            DayOfWeek::Thursday => "Thursday",
                            DayOfWeek::Friday => "Friday",
                            DayOfWeek::Saturday => "Saturday",
                            DayOfWeek::Sunday => "Sunday",
                        })
                    } else {
                        format!("Day {}", day_of_week)
                    }
                } else {
                    "Day".to_string()
                };

                current_layer.set_fill_color(color_primary.clone());
                current_layer.use_text(
                    &day_label,
                    LINE_HEIGHT_LARGE + 4.0,
                    Mm(MARGIN_LEFT),
                    Mm(PAGE_HEIGHT - MARGIN_TOP - 20.0),
                    &font_bold,
                );
                y_pos = PAGE_HEIGHT - MARGIN_TOP - 50.0;
            }

            // Sort entries by meal type (breakfast, lunch, dinner, snack)
            let mut sorted_entries = entries.clone();
            sorted_entries.sort_by(|a, b| {
                let order = |meal_type: &str| -> i32 {
                    match meal_type.to_lowercase().as_str() {
                        "breakfast" => 1,
                        "lunch" => 2,
                        "dinner" => 3,
                        "snack" => 4,
                        _ => 5,
                    }
                };
                order(&a.entry.meal_type).cmp(&order(&b.entry.meal_type))
            });

            // Display meals for this day
            for entry in sorted_entries {
                // Check if we need a new page before adding meal
                if y_pos < MARGIN_BOTTOM + 40.0 {
                    let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                    current_page = new_page;
                    current_layer_idx = new_layer;
                    let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                    // Re-add day header on new page
                    let day_label = if let Some(date) = day_key.0 {
                        date.format("%A, %B %d, %Y").to_string()
                    } else if let Some(day_of_week) = day_key.1 {
                        if let Some(day) = DayOfWeek::from_int(day_of_week) {
                            format!("{} (continued)", match day {
                                DayOfWeek::Monday => "Monday",
                                DayOfWeek::Tuesday => "Tuesday",
                                DayOfWeek::Wednesday => "Wednesday",
                                DayOfWeek::Thursday => "Thursday",
                                DayOfWeek::Friday => "Friday",
                                DayOfWeek::Saturday => "Saturday",
                                DayOfWeek::Sunday => "Sunday",
                            })
                        } else {
                            format!("Day {} (continued)", day_of_week)
                        }
                    } else {
                        "Day (continued)".to_string()
                    };
                    current_layer.set_fill_color(color_primary.clone());
                    current_layer.use_text(
                        &day_label,
                        LINE_HEIGHT_LARGE + 4.0,
                        Mm(MARGIN_LEFT),
                        Mm(PAGE_HEIGHT - MARGIN_TOP - 20.0),
                        &font_bold,
                    );
                    y_pos = PAGE_HEIGHT - MARGIN_TOP - 50.0;
                }

                let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);

                // Meal type (capitalized) with color
                let meal_type = entry.entry.meal_type.clone();
                let meal_type_capitalized = meal_type
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + &meal_type[1..])
                    .unwrap_or(meal_type);

                current_layer.set_fill_color(color_secondary.clone());
                current_layer.use_text(
                    &meal_type_capitalized,
                    LINE_HEIGHT_MEDIUM,
                    Mm(MARGIN_LEFT + 10.0),
                    Mm(y_pos),
                    &font_bold,
                );
                
                // Recipe name
                current_layer.set_fill_color(color_text.clone());
                current_layer.use_text(
                    &entry.recipe.name,
                    LINE_HEIGHT_MEDIUM,
                    Mm(MARGIN_LEFT + 50.0),
                    Mm(y_pos),
                    &font_bold,
                );
                y_pos -= LINE_HEIGHT_MEDIUM + 4.0;

                // Recipe description if available
                if let Some(desc) = &entry.recipe.description {
                    let wrapped = Self::wrap_text(desc, 65);
                    for line in wrapped {
                        if y_pos < MARGIN_BOTTOM + 20.0 {
                            // Would need new page, but skip for now to avoid complexity
                            break;
                        }
                        current_layer.set_fill_color(color_text.clone());
                        current_layer.use_text(
                            &line,
                            LINE_HEIGHT_SMALL,
                            Mm(MARGIN_LEFT + 15.0),
                            Mm(y_pos),
                            &font_italic,
                        );
                        y_pos -= LINE_HEIGHT_SMALL + 3.0;
                    }
                }

                // Fetch full recipe details (ingredients and steps)
                let recipe_details = NutritionService::get_recipe_with_details(pool, entry.recipe.id)
                    .await
                    .map_err(|e| ToolboxError::Other(format!("Failed to fetch recipe details: {}", e)))?;

                // Recipe metadata (servings, prep time, cook time)
                y_pos -= PARAGRAPH_SPACING;
                if recipe_details.recipe.servings.is_some() || 
                   recipe_details.recipe.prep_time_minutes.is_some() || 
                   recipe_details.recipe.cook_time_minutes.is_some() {
                    let mut metadata_parts = Vec::new();
                    if let Some(servings) = recipe_details.recipe.servings {
                        metadata_parts.push(format!("Serves: {}", servings));
                    }
                    if let Some(prep) = recipe_details.recipe.prep_time_minutes {
                        metadata_parts.push(format!("Prep: {} min", prep));
                    }
                    if let Some(cook) = recipe_details.recipe.cook_time_minutes {
                        metadata_parts.push(format!("Cook: {} min", cook));
                    }
                    if !metadata_parts.is_empty() {
                        if y_pos < MARGIN_BOTTOM + 30.0 {
                            let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                            current_page = new_page;
                            current_layer_idx = new_layer;
                            y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
                        }
                        let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                        current_layer.set_fill_color(color_text.clone());
                        current_layer.use_text(
                            &metadata_parts.join(" • "),
                            LINE_HEIGHT_SMALL,
                            Mm(MARGIN_LEFT + 15.0),
                            Mm(y_pos),
                            &font_regular,
                        );
                        y_pos -= LINE_HEIGHT_SMALL + PARAGRAPH_SPACING;
                    }
                }

                // Ingredients section
                if !recipe_details.ingredients.is_empty() {
                    if y_pos < MARGIN_BOTTOM + 50.0 {
                        let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                        current_page = new_page;
                        current_layer_idx = new_layer;
                        y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
                    }
                    let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                    
                    current_layer.set_fill_color(color_secondary.clone());
                    current_layer.use_text(
                        "Ingredients:",
                        LINE_HEIGHT_MEDIUM,
                        Mm(MARGIN_LEFT + 15.0),
                        Mm(y_pos),
                        &font_bold,
                    );
                    y_pos -= LINE_HEIGHT_MEDIUM + 4.0;

                    for ingredient in &recipe_details.ingredients {
                        if y_pos < MARGIN_BOTTOM + 20.0 {
                            let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                            current_page = new_page;
                            current_layer_idx = new_layer;
                            let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                            // Re-add ingredients header
                            current_layer.set_fill_color(color_secondary.clone());
                            current_layer.use_text(
                                "Ingredients (continued):",
                                LINE_HEIGHT_MEDIUM,
                                Mm(MARGIN_LEFT + 15.0),
                                Mm(PAGE_HEIGHT - MARGIN_TOP - 30.0),
                                &font_bold,
                            );
                            y_pos = PAGE_HEIGHT - MARGIN_TOP - 50.0;
                        }
                        let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                        
                        let ingredient_line = format!(
                            "• {} {} {}",
                            ingredient.recipe_ingredient.quantity,
                            ingredient.recipe_ingredient.unit,
                            ingredient.ingredient.name
                        );
                        current_layer.set_fill_color(color_text.clone());
                        current_layer.use_text(
                            &ingredient_line,
                            LINE_HEIGHT_SMALL,
                            Mm(MARGIN_LEFT + 25.0),
                            Mm(y_pos),
                            &font_regular,
                        );
                        y_pos -= LINE_HEIGHT_SMALL + 3.0;
                    }
                    y_pos -= PARAGRAPH_SPACING;
                }

                // Instructions section
                if !recipe_details.steps.is_empty() {
                    if y_pos < MARGIN_BOTTOM + 50.0 {
                        let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                        current_page = new_page;
                        current_layer_idx = new_layer;
                        y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
                    }
                    let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                    
                    current_layer.set_fill_color(color_secondary.clone());
                    current_layer.use_text(
                        "Instructions:",
                        LINE_HEIGHT_MEDIUM,
                        Mm(MARGIN_LEFT + 15.0),
                        Mm(y_pos),
                        &font_bold,
                    );
                    y_pos -= LINE_HEIGHT_MEDIUM + 4.0;

                    for step in &recipe_details.steps {
                        let step_text = format!("{}. {}", step.step_number, step.instruction);
                        let wrapped = Self::wrap_text(&step_text, 60);
                        
                        for line in wrapped {
                            if y_pos < MARGIN_BOTTOM + 20.0 {
                                let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                                current_page = new_page;
                                current_layer_idx = new_layer;
                                let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                                // Re-add instructions header
                                current_layer.set_fill_color(color_secondary.clone());
                                current_layer.use_text(
                                    "Instructions (continued):",
                                    LINE_HEIGHT_MEDIUM,
                                    Mm(MARGIN_LEFT + 15.0),
                                    Mm(PAGE_HEIGHT - MARGIN_TOP - 30.0),
                                    &font_bold,
                                );
                                y_pos = PAGE_HEIGHT - MARGIN_TOP - 50.0;
                            }
                            let current_layer = doc.get_page(current_page).get_layer(current_layer_idx);
                            current_layer.set_fill_color(color_text.clone());
                            current_layer.use_text(
                                &line,
                                LINE_HEIGHT_SMALL,
                                Mm(MARGIN_LEFT + 25.0),
                                Mm(y_pos),
                                &font_regular,
                            );
                            y_pos -= LINE_HEIGHT_SMALL + 3.0;
                        }
                        y_pos -= PARAGRAPH_SPACING;
                    }
                }

                y_pos -= SECTION_SPACING; // Space between meals
            }

            y_pos -= SECTION_SPACING; // Space between days
        }

        // ========== Shopping List Page ==========
        // Get aggregated ingredients for shopping list
        let prep_data = NutritionService::get_meal_plan_for_prep_analysis(pool, meal_plan.meal_plan.id)
            .await
            .map_err(|e| ToolboxError::Other(format!("Failed to get prep data: {}", e)))?;

        let (shop_page, shop_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
        let current_layer = doc.get_page(shop_page).get_layer(shop_layer);
        
        let mut y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
        
        current_layer.set_fill_color(color_primary.clone());
        current_layer.use_text(
            "Shopping List",
            LINE_HEIGHT_LARGE + 8.0,
            Mm(MARGIN_LEFT),
            Mm(y_pos),
            &font_bold,
        );
        y_pos -= LINE_HEIGHT_LARGE + 8.0 + SECTION_SPACING;

        // Group ingredients by category (simple heuristic based on name)
        let categorize_ingredient = |name: &str| -> &str {
            let name_lower = name.to_lowercase();
            if name_lower.contains("chicken") || name_lower.contains("beef") || 
               name_lower.contains("pork") || name_lower.contains("salmon") || 
               name_lower.contains("fish") || name_lower.contains("turkey") ||
               name_lower.contains("meat") || name_lower.contains("sausage") {
                "PROTEINS"
            } else if name_lower.contains("broccoli") || name_lower.contains("spinach") ||
                      name_lower.contains("carrot") || name_lower.contains("onion") ||
                      name_lower.contains("pepper") || name_lower.contains("potato") ||
                      name_lower.contains("tomato") || name_lower.contains("garlic") ||
                      name_lower.contains("lettuce") || name_lower.contains("zucchini") ||
                      name_lower.contains("vegetable") || name_lower.contains("herb") {
                "VEGETABLES & PRODUCE"
            } else if name_lower.contains("rice") || name_lower.contains("pasta") ||
                      name_lower.contains("quinoa") || name_lower.contains("bread") ||
                      name_lower.contains("flour") || name_lower.contains("noodle") {
                "PANTRY STAPLES"
            } else if name_lower.contains("oil") || name_lower.contains("sauce") ||
                      name_lower.contains("seasoning") || name_lower.contains("spice") ||
                      name_lower.contains("salt") || name_lower.contains("pepper") ||
                      name_lower.contains("vinegar") || name_lower.contains("soy") {
                "CONDIMENTS & SEASONINGS"
            } else if name_lower.contains("cheese") || name_lower.contains("milk") ||
                      name_lower.contains("egg") || name_lower.contains("butter") ||
                      name_lower.contains("yogurt") || name_lower.contains("cream") {
                "DAIRY & REFRIGERATED"
            } else {
                "OTHER"
            }
        };

        let mut categorized: HashMap<&str, Vec<&AggregatedIngredient>> = HashMap::new();
        for ingredient in &prep_data.aggregated_ingredients {
            let category = categorize_ingredient(&ingredient.ingredient.name);
            categorized.entry(category).or_insert_with(Vec::new).push(ingredient);
        }

        // Sort categories in a logical order
        let category_order = vec!["PROTEINS", "VEGETABLES & PRODUCE", "PANTRY STAPLES", 
                                   "CONDIMENTS & SEASONINGS", "DAIRY & REFRIGERATED", "OTHER"];
        
        let mut current_shop_page = shop_page;
        let mut current_shop_layer = shop_layer;
        for category in category_order {
            if let Some(ingredients) = categorized.get(category) {
                if y_pos < MARGIN_BOTTOM + 50.0 {
                    let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                    current_shop_page = new_page;
                    current_shop_layer = new_layer;
                    let current_layer = doc.get_page(current_shop_page).get_layer(current_shop_layer);
                    y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
                }
                let current_layer = doc.get_page(current_shop_page).get_layer(current_shop_layer);
                
                // Category header
                current_layer.set_fill_color(color_secondary.clone());
                current_layer.use_text(
                    category,
                    LINE_HEIGHT_LARGE,
                    Mm(MARGIN_LEFT),
                    Mm(y_pos),
                    &font_bold,
                );
                y_pos -= LINE_HEIGHT_LARGE + PARAGRAPH_SPACING;

                // Ingredients in this category
                for ingredient in ingredients {
                    if y_pos < MARGIN_BOTTOM + 20.0 {
                        let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                        current_shop_page = new_page;
                        current_shop_layer = new_layer;
                        let current_layer = doc.get_page(current_shop_page).get_layer(current_shop_layer);
                        // Re-add category header
                        current_layer.set_fill_color(color_secondary.clone());
                        current_layer.use_text(
                            &format!("{} (continued)", category),
                            LINE_HEIGHT_LARGE,
                            Mm(MARGIN_LEFT),
                            Mm(PAGE_HEIGHT - MARGIN_TOP - 30.0),
                            &font_bold,
                        );
                        y_pos = PAGE_HEIGHT - MARGIN_TOP - 50.0;
                    }
                    let current_layer = doc.get_page(current_shop_page).get_layer(current_shop_layer);
                    
                    let ingredient_line = format!(
                        "• {}: {} {}",
                        ingredient.ingredient.name,
                        Self::format_decimal(&ingredient.total_quantity),
                        ingredient.unit
                    );
                    current_layer.set_fill_color(color_text.clone());
                    current_layer.use_text(
                        &ingredient_line,
                        LINE_HEIGHT_MEDIUM,
                        Mm(MARGIN_LEFT + 10.0),
                        Mm(y_pos),
                        &font_regular,
                    );
                    y_pos -= LINE_HEIGHT_MEDIUM + PARAGRAPH_SPACING;
                }
                y_pos -= SECTION_SPACING;
            }
        }

        // ========== Meal Prep Instructions Page ==========
        let (prep_page, prep_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
        let mut current_prep_page = prep_page;
        let mut current_prep_layer = prep_layer;
        let current_layer = doc.get_page(current_prep_page).get_layer(current_prep_layer);
        
        let mut y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
        
        current_layer.set_fill_color(color_primary.clone());
        current_layer.use_text(
            "Meal Prep Guide",
            LINE_HEIGHT_LARGE + 8.0,
            Mm(MARGIN_LEFT),
            Mm(y_pos),
            &font_bold,
        );
        y_pos -= LINE_HEIGHT_LARGE + 8.0 + SECTION_SPACING;

        // Prep timeline overview
        current_layer.set_fill_color(color_secondary.clone());
        current_layer.use_text(
            "Prep Timeline:",
            LINE_HEIGHT_LARGE,
            Mm(MARGIN_LEFT),
            Mm(y_pos),
            &font_bold,
        );
        y_pos -= LINE_HEIGHT_LARGE + PARAGRAPH_SPACING;

        current_layer.set_fill_color(color_text.clone());
        let prep_tips = vec![
            "Sunday: Prep vegetables that last 5-7 days (onions, carrots, potatoes)",
            "Sunday: Portion and marinate proteins if needed",
            "Day Before: Prep fresh vegetables (broccoli, spinach, peppers)",
            "Day Of: Prep delicate items (fresh herbs, lettuce, tomatoes)",
            "Storage: Use airtight containers for prepped vegetables",
            "Storage: Label and date all prepped items",
        ];

        for tip in prep_tips {
            if y_pos < MARGIN_BOTTOM + 20.0 {
                let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                current_prep_page = new_page;
                current_prep_layer = new_layer;
                let current_layer = doc.get_page(current_prep_page).get_layer(current_prep_layer);
                y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
            }
            let current_layer = doc.get_page(current_prep_page).get_layer(current_prep_layer);
            current_layer.use_text(
                &format!("• {}", tip),
                LINE_HEIGHT_SMALL,
                Mm(MARGIN_LEFT + 10.0),
                Mm(y_pos),
                &font_regular,
            );
            y_pos -= LINE_HEIGHT_SMALL + PARAGRAPH_SPACING;
        }

        y_pos -= SECTION_SPACING;

        // Ingredient prep recommendations
        current_layer.set_fill_color(color_secondary.clone());
        current_layer.use_text(
            "Ingredient Prep Recommendations:",
            LINE_HEIGHT_LARGE,
            Mm(MARGIN_LEFT),
            Mm(y_pos),
            &font_bold,
        );
        y_pos -= LINE_HEIGHT_LARGE + PARAGRAPH_SPACING;

        // Show prep recommendations for key ingredients
        let mut current_prep_page = prep_page;
        let mut current_prep_layer = prep_layer;
        for ingredient in &prep_data.aggregated_ingredients {
            if y_pos < MARGIN_BOTTOM + 40.0 {
                let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                current_prep_page = new_page;
                current_prep_layer = new_layer;
                let current_layer = doc.get_page(current_prep_page).get_layer(current_prep_layer);
                y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
            }
            let current_layer = doc.get_page(current_prep_page).get_layer(current_prep_layer);
            
            let name_lower = ingredient.ingredient.name.to_lowercase();
            let prep_note = if name_lower.contains("onion") || name_lower.contains("garlic") {
                "Prep 2-3 days ahead, store in fridge"
            } else if name_lower.contains("carrot") || name_lower.contains("potato") {
                "Prep 3-5 days ahead, store in fridge"
            } else if name_lower.contains("broccoli") || name_lower.contains("pepper") {
                "Prep 1-2 days ahead, store in airtight container"
            } else if name_lower.contains("spinach") || name_lower.contains("lettuce") {
                "Prep day-of, wash and dry thoroughly"
            } else if name_lower.contains("chicken") || name_lower.contains("beef") {
                "Portion and marinate 1 day ahead if needed"
            } else {
                "Check recipe timing"
            };

            current_layer.set_fill_color(color_text.clone());
            let prep_line = format!("• {}: {}", ingredient.ingredient.name, prep_note);
            let wrapped = Self::wrap_text(&prep_line, 65);
            for line in wrapped {
                if y_pos < MARGIN_BOTTOM + 20.0 {
                    let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                    current_prep_page = new_page;
                    current_prep_layer = new_layer;
                    let current_layer = doc.get_page(current_prep_page).get_layer(current_prep_layer);
                    y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
                }
                let current_layer = doc.get_page(current_prep_page).get_layer(current_prep_layer);
                current_layer.use_text(
                    &line,
                    LINE_HEIGHT_SMALL,
                    Mm(MARGIN_LEFT + 10.0),
                    Mm(y_pos),
                    &font_regular,
                );
                y_pos -= LINE_HEIGHT_SMALL + 3.0;
            }
            y_pos -= PARAGRAPH_SPACING;
        }

        // ========== Nutrition Summary Page (if available) ==========
        if let Some(nutrition) = nutrition {
            let (nut_page, nut_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
            let current_layer = doc.get_page(nut_page).get_layer(nut_layer);

            let mut y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
            
            current_layer.set_fill_color(color_primary.clone());
            current_layer.use_text(
                "Nutritional Summary",
                LINE_HEIGHT_LARGE + 8.0,
                Mm(MARGIN_LEFT),
                Mm(y_pos),
                &font_bold,
            );
            y_pos -= LINE_HEIGHT_LARGE + 8.0 + SECTION_SPACING;

            // Weekly totals if available
            if let Some(weekly) = &nutrition.weekly_totals {
                current_layer.set_fill_color(color_secondary.clone());
                current_layer.use_text(
                    "Weekly Totals:",
                    LINE_HEIGHT_LARGE,
                    Mm(MARGIN_LEFT),
                    Mm(y_pos),
                    &font_bold,
                );
                y_pos -= LINE_HEIGHT_LARGE + PARAGRAPH_SPACING;

                current_layer.set_fill_color(color_text.clone());
                let nutrition_items = vec![
                    format!("Total Calories: {}", Self::format_decimal(&weekly.total_calories)),
                    format!("Average Daily Calories: {}", Self::format_decimal(&weekly.average_daily_calories)),
                    format!("Total Protein: {}g", Self::format_decimal(&weekly.total_protein_g)),
                    format!("Total Carbohydrates: {}g", Self::format_decimal(&weekly.total_carbs_g)),
                    format!("Total Fat: {}g", Self::format_decimal(&weekly.total_fat_g)),
                ];
                
                for item in nutrition_items {
                    if y_pos < MARGIN_BOTTOM + 30.0 {
                        break;
                    }
                    current_layer.use_text(
                        &item,
                        LINE_HEIGHT_MEDIUM,
                        Mm(MARGIN_LEFT + 10.0),
                        Mm(y_pos),
                        &font_regular,
                    );
                    y_pos -= LINE_HEIGHT_MEDIUM + PARAGRAPH_SPACING;
                }
                y_pos -= SECTION_SPACING;
            }

            // Daily nutrition
            current_layer.set_fill_color(color_secondary.clone());
            current_layer.use_text(
                "Daily Breakdown:",
                LINE_HEIGHT_LARGE,
                Mm(MARGIN_LEFT),
                Mm(y_pos),
                &font_bold,
            );
            y_pos -= LINE_HEIGHT_LARGE + PARAGRAPH_SPACING;

            for daily in &nutrition.daily_nutrition {
                let day_label = if let Some(date) = daily.date {
                    date.format("%A, %B %d").to_string()
                } else if let Some(day_of_week) = daily.day_of_week {
                    if let Some(day) = DayOfWeek::from_int(day_of_week) {
                        format!("{}", match day {
                            DayOfWeek::Monday => "Monday",
                            DayOfWeek::Tuesday => "Tuesday",
                            DayOfWeek::Wednesday => "Wednesday",
                            DayOfWeek::Thursday => "Thursday",
                            DayOfWeek::Friday => "Friday",
                            DayOfWeek::Saturday => "Saturday",
                            DayOfWeek::Sunday => "Sunday",
                        })
                    } else {
                        format!("Day {}", day_of_week)
                    }
                } else {
                    "Day".to_string()
                };

                if y_pos < MARGIN_BOTTOM + 30.0 {
                    let (_new_page, _new_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
                    // Note: In a full implementation, we'd track the new page and update nut_page/nut_layer
                    y_pos = PAGE_HEIGHT - MARGIN_TOP - 30.0;
                }

                let current_layer = doc.get_page(nut_page).get_layer(nut_layer);
                current_layer.set_fill_color(color_text.clone());
                current_layer.use_text(
                    &format!("{}: {} cal, {}g protein, {}g carbs, {}g fat",
                        day_label,
                        Self::format_decimal(&daily.total_calories),
                        Self::format_decimal(&daily.total_protein_g),
                        Self::format_decimal(&daily.total_carbs_g),
                        Self::format_decimal(&daily.total_fat_g)
                    ),
                    LINE_HEIGHT_SMALL,
                    Mm(MARGIN_LEFT + 10.0),
                    Mm(y_pos),
                    &font_regular,
                );
                y_pos -= LINE_HEIGHT_SMALL + PARAGRAPH_SPACING;
            }
        }

        // ========== Final Page: Chef's Note ==========
        let (final_page, final_layer) = doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
        let current_layer = doc.get_page(final_page).get_layer(final_layer);

        // Centered closing message
        let mut closing_y = PAGE_HEIGHT / 2.0 + 40.0;
        
        current_layer.set_fill_color(color_primary.clone());
        let bon_appetit_x = center_text("Bon Appétit!", 32.0);
        current_layer.use_text(
            "Bon Appétit!",
            32.0,
            Mm(bon_appetit_x.max(MARGIN_LEFT)),
            Mm(closing_y),
            &font_bold,
        );
        closing_y -= 40.0;

        current_layer.set_fill_color(color_text.clone());
        let thank_you_x = center_text("Thank you for choosing this meal plan.", 16.0);
        current_layer.use_text(
            "Thank you for choosing this meal plan.",
            16.0,
            Mm(thank_you_x.max(MARGIN_LEFT)),
            Mm(closing_y),
            &font_regular,
        );
        closing_y -= 30.0;

        let hope_text = "We hope these recipes bring joy to your kitchen and nourishment to your table.";
        let wrapped = Self::wrap_text(hope_text, 70);
        for line in wrapped {
            let line_x = center_text(&line, 14.0);
            current_layer.use_text(
                &line,
                14.0,
                Mm(line_x.max(MARGIN_LEFT)),
                Mm(closing_y),
                &font_italic,
            );
            closing_y -= 18.0;
        }
        closing_y -= 10.0;

        current_layer.set_fill_color(color_secondary.clone());
        let happy_x = center_text("Happy cooking and eating!", 18.0);
        current_layer.use_text(
            "Happy cooking and eating!",
            18.0,
            Mm(happy_x.max(MARGIN_LEFT)),
            Mm(closing_y),
            &font_bold,
        );

        // Save PDF
        let file = File::create(output_path)
            .map_err(|e| ToolboxError::Io(e))?;
        let mut writer = BufWriter::new(file);
        doc.save(&mut writer)
            .map_err(|e| ToolboxError::Other(format!("Failed to save PDF: {}", e)))?;

        Ok(())
    }
}

