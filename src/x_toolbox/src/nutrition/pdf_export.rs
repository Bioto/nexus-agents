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

// ============================================================================
// Layout Constants
// ============================================================================

const PAGE_WIDTH: f32 = 210.0;
const PAGE_HEIGHT: f32 = 297.0;
const MARGIN: f32 = 20.0;
#[allow(dead_code)]
const CONTENT_WIDTH: f32 = PAGE_WIDTH - (MARGIN * 2.0);

// Font sizes
const FONT_TITLE: f32 = 28.0;
const FONT_HEADING: f32 = 18.0;
const FONT_SUBHEADING: f32 = 14.0;
const FONT_BODY: f32 = 11.0;
const FONT_SMALL: f32 = 9.0;

// Spacing
const LINE_HEIGHT: f32 = 1.4;
const SECTION_GAP: f32 = 16.0;
const PARAGRAPH_GAP: f32 = 8.0;

// Colors
const COLOR_PRIMARY: (f32, f32, f32) = (0.15, 0.45, 0.35);      // Deep teal
const COLOR_SECONDARY: (f32, f32, f32) = (0.85, 0.55, 0.25);    // Warm amber  
const COLOR_TEXT: (f32, f32, f32) = (0.15, 0.15, 0.15);         // Near black
const COLOR_LIGHT: (f32, f32, f32) = (0.55, 0.55, 0.55);        // Gray
const COLOR_ACCENT_LINE: (f32, f32, f32) = (0.85, 0.85, 0.85);  // Light gray

// ============================================================================
// PDF Builder - Clean abstraction over printpdf
// ============================================================================

struct PdfBuilder {
    doc: PdfDocumentReference,
    font_bold: IndirectFontRef,
    font_regular: IndirectFontRef,
    font_italic: IndirectFontRef,
    current_page: PdfPageIndex,
    current_layer: PdfLayerIndex,
    y_pos: f32,
    page_number: u32,
}

impl PdfBuilder {
    fn new(title: &str) -> Result<Self> {
        let (doc, page1, layer1) = PdfDocument::new(
            title,
            Mm(PAGE_WIDTH),
            Mm(PAGE_HEIGHT),
            "Layer 1",
        );

        let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)
            .map_err(|e| ToolboxError::Other(format!("Failed to add bold font: {}", e)))?;
        let font_regular = doc.add_builtin_font(BuiltinFont::Helvetica)
            .map_err(|e| ToolboxError::Other(format!("Failed to add regular font: {}", e)))?;
        let font_italic = doc.add_builtin_font(BuiltinFont::HelveticaOblique)
            .map_err(|e| ToolboxError::Other(format!("Failed to add italic font: {}", e)))?;

        Ok(Self {
            doc,
            font_bold,
            font_regular,
            font_italic,
            current_page: page1,
            current_layer: layer1,
            y_pos: PAGE_HEIGHT - MARGIN,
            page_number: 1,
        })
    }

    fn layer(&self) -> PdfLayerReference {
        self.doc.get_page(self.current_page).get_layer(self.current_layer)
    }

    fn new_page(&mut self) {
        let (page, layer) = self.doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Layer 1");
        self.current_page = page;
        self.current_layer = layer;
        self.y_pos = PAGE_HEIGHT - MARGIN;
        self.page_number += 1;
    }

    fn ensure_space(&mut self, needed: f32) {
        if self.y_pos - needed < MARGIN + 20.0 {
            self.new_page();
        }
    }

    fn set_color(&self, rgb: (f32, f32, f32)) {
        self.layer().set_fill_color(Color::Rgb(Rgb::new(rgb.0, rgb.1, rgb.2, None)));
    }

    fn set_stroke_color(&self, rgb: (f32, f32, f32)) {
        self.layer().set_outline_color(Color::Rgb(Rgb::new(rgb.0, rgb.1, rgb.2, None)));
    }

    // Text rendering
    fn text(&mut self, content: &str, font_size: f32, bold: bool, x: f32) {
        let font = if bold { &self.font_bold } else { &self.font_regular };
        self.layer().use_text(content, font_size, Mm(x), Mm(self.y_pos), font);
    }

    fn text_italic(&mut self, content: &str, font_size: f32, x: f32) {
        self.layer().use_text(content, font_size, Mm(x), Mm(self.y_pos), &self.font_italic);
    }

    fn advance(&mut self, amount: f32) {
        self.y_pos -= amount;
    }

    // Wrapped text
    fn wrapped_text(&mut self, content: &str, font_size: f32, max_chars: usize, x: f32, bold: bool) {
        let lines = Self::wrap_text(content, max_chars);
        let line_spacing = font_size * LINE_HEIGHT * 0.35;
        
        for line in lines {
            self.ensure_space(line_spacing);
            if bold {
                self.text(&line, font_size, true, x);
            } else {
                self.text(&line, font_size, false, x);
            }
            self.advance(line_spacing);
        }
    }

    fn wrapped_text_italic(&mut self, content: &str, font_size: f32, max_chars: usize, x: f32) {
        let lines = Self::wrap_text(content, max_chars);
        let line_spacing = font_size * LINE_HEIGHT * 0.35;
        
        for line in lines {
            self.ensure_space(line_spacing);
            self.text_italic(&line, font_size, x);
            self.advance(line_spacing);
        }
    }

    // Drawing
    fn horizontal_line(&mut self, thickness: f32) {
        self.set_stroke_color(COLOR_ACCENT_LINE);
        let layer = self.layer();
        layer.set_outline_thickness(thickness);
        
        let line = Line {
            points: vec![
                (Point::new(Mm(MARGIN), Mm(self.y_pos)), false),
                (Point::new(Mm(PAGE_WIDTH - MARGIN), Mm(self.y_pos)), false),
            ],
            is_closed: false,
        };
        layer.add_line(line);
    }

    fn thick_line(&mut self) {
        self.set_stroke_color(COLOR_PRIMARY);
        let layer = self.layer();
        layer.set_outline_thickness(2.0);
        
        let line = Line {
            points: vec![
                (Point::new(Mm(MARGIN), Mm(self.y_pos)), false),
                (Point::new(Mm(MARGIN + 50.0), Mm(self.y_pos)), false),
            ],
            is_closed: false,
        };
        layer.add_line(line);
    }

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

    fn format_decimal(bd: &BigDecimal) -> String {
        let s = bd.to_string();
        // Truncate to 1 decimal place for readability
        if let Some(dot_pos) = s.find('.') {
            let end = (dot_pos + 2).min(s.len());
            s[..end].to_string()
        } else {
            s
        }
    }

    /// Convert metric units to American/imperial units for shopping lists
    fn to_american_units(quantity: &BigDecimal, unit: &str) -> (String, String) {
        let qty: f64 = quantity.to_string().parse().unwrap_or(0.0);
        let unit_lower = unit.to_lowercase();
        
        match unit_lower.as_str() {
            // Grams to pounds/ounces
            "g" | "gram" | "grams" => {
                if qty >= 453.6 {
                    // Convert to pounds
                    let lbs = qty / 453.6;
                    (format!("{:.1}", lbs), "lb".to_string())
                } else {
                    // Convert to ounces
                    let oz = qty / 28.35;
                    (format!("{:.1}", oz), "oz".to_string())
                }
            }
            // Milliliters to cups/tablespoons
            "ml" | "milliliter" | "milliliters" => {
                if qty >= 240.0 {
                    // Convert to cups
                    let cups = qty / 240.0;
                    (format!("{:.1}", cups), "cups".to_string())
                } else if qty >= 60.0 {
                    // Convert to 1/4 cups
                    let quarter_cups = qty / 60.0;
                    (format!("{:.1}", quarter_cups * 0.25), "cups".to_string())
                } else if qty >= 15.0 {
                    // Convert to tablespoons
                    let tbsp = qty / 15.0;
                    (format!("{:.0}", tbsp), "tbsp".to_string())
                } else {
                    // Convert to teaspoons
                    let tsp = qty / 5.0;
                    (format!("{:.0}", tsp), "tsp".to_string())
                }
            }
            // Kilograms to pounds
            "kg" | "kilogram" | "kilograms" => {
                let lbs = qty * 2.205;
                (format!("{:.1}", lbs), "lb".to_string())
            }
            // Liters to quarts/cups
            "l" | "liter" | "liters" => {
                if qty >= 1.0 {
                    let quarts = qty * 1.057;
                    (format!("{:.1}", quarts), "qt".to_string())
                } else {
                    let cups = qty * 4.227;
                    (format!("{:.1}", cups), "cups".to_string())
                }
            }
            // Already American or count-based - pass through
            "oz" | "ounce" | "ounces" | "lb" | "lbs" | "pound" | "pounds" 
            | "cup" | "cups" | "tbsp" | "tablespoon" | "tsp" | "teaspoon"
            | "piece" | "pieces" | "whole" | "clove" | "cloves" => {
                (format!("{:.0}", qty), unit.to_string())
            }
            // Unknown unit - pass through with original formatting
            _ => (Self::format_decimal(quantity), unit.to_string())
        }
    }

    fn save(self, output_path: &Path) -> Result<()> {
        let file = File::create(output_path).map_err(ToolboxError::Io)?;
        let mut writer = BufWriter::new(file);
        self.doc.save(&mut writer)
            .map_err(|e| ToolboxError::Other(format!("Failed to save PDF: {}", e)))?;
        Ok(())
    }
}

// ============================================================================
// Recipe PDF Exporter (single recipe)
// ============================================================================

pub struct RecipePdfExporter;

impl RecipePdfExporter {
    pub fn export_recipe(
        recipe: &RecipeWithDetails,
        nutrition: Option<&RecipeNutrition>,
        output_path: &Path,
    ) -> Result<()> {
        let mut pdf = PdfBuilder::new(&recipe.recipe.name)?;
        
        // Cover page
        Self::render_cover(&mut pdf, recipe);
        
        // Ingredients
        pdf.new_page();
        Self::render_ingredients(&mut pdf, recipe);
        
        // Instructions
        pdf.new_page();
        Self::render_instructions(&mut pdf, recipe);
        
        // Nutrition
        if let Some(nutrition) = nutrition {
            pdf.new_page();
            Self::render_nutrition(&mut pdf, nutrition);
        }
        
        // Closing
        pdf.new_page();
        Self::render_closing(&mut pdf);
        
        pdf.save(output_path)
    }

    fn render_cover(pdf: &mut PdfBuilder, recipe: &RecipeWithDetails) {
        pdf.advance(60.0);
        
        // Title
        pdf.set_color(COLOR_PRIMARY);
        pdf.text(&recipe.recipe.name, FONT_TITLE, true, MARGIN);
        pdf.advance(FONT_TITLE * 0.5);
        
        pdf.thick_line();
        pdf.advance(SECTION_GAP);
        
        // Description
        if let Some(desc) = &recipe.recipe.description {
            pdf.set_color(COLOR_TEXT);
            pdf.wrapped_text_italic(desc, FONT_BODY, 80, MARGIN);
            pdf.advance(SECTION_GAP);
        }
        
        // Metadata
        pdf.set_color(COLOR_LIGHT);
        let mut meta_parts = Vec::new();
        if let Some(servings) = recipe.recipe.servings {
            meta_parts.push(format!("Serves {}", servings));
        }
        if let Some(prep) = recipe.recipe.prep_time_minutes {
            meta_parts.push(format!("{} min prep", prep));
        }
        if let Some(cook) = recipe.recipe.cook_time_minutes {
            meta_parts.push(format!("{} min cook", cook));
        }
        if !meta_parts.is_empty() {
            pdf.text(&meta_parts.join("  •  "), FONT_BODY, false, MARGIN);
        }
    }

    fn render_ingredients(pdf: &mut PdfBuilder, recipe: &RecipeWithDetails) {
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Ingredients", FONT_HEADING, true, MARGIN);
        pdf.advance(FONT_HEADING * 0.4);
        pdf.thick_line();
        pdf.advance(SECTION_GAP);
        
        for ing in &recipe.ingredients {
            pdf.ensure_space(FONT_BODY * 2.0);
            pdf.set_color(COLOR_TEXT);
            let line = format!(
                "{}  {} {}",
                ing.recipe_ingredient.quantity,
                ing.recipe_ingredient.unit,
                ing.ingredient.name
            );
            pdf.text(&line, FONT_BODY, false, MARGIN + 5.0);
            pdf.advance(FONT_BODY * LINE_HEIGHT * 0.4);
        }
    }

    fn render_instructions(pdf: &mut PdfBuilder, recipe: &RecipeWithDetails) {
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Instructions", FONT_HEADING, true, MARGIN);
        pdf.advance(FONT_HEADING * 0.4);
        pdf.thick_line();
        pdf.advance(SECTION_GAP);
        
        for step in &recipe.steps {
            pdf.ensure_space(FONT_BODY * 4.0);
            
            // Step number
            pdf.set_color(COLOR_SECONDARY);
            pdf.text(&format!("Step {}", step.step_number), FONT_SUBHEADING, true, MARGIN);
            pdf.advance(FONT_SUBHEADING * 0.5);
            
            // Instruction
            pdf.set_color(COLOR_TEXT);
            pdf.wrapped_text(&step.instruction, FONT_BODY, 75, MARGIN + 5.0, false);
            pdf.advance(PARAGRAPH_GAP);
        }
    }

    fn render_nutrition(pdf: &mut PdfBuilder, nutrition: &RecipeNutrition) {
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Nutrition Facts", FONT_HEADING, true, MARGIN);
        pdf.advance(FONT_HEADING * 0.4);
        pdf.thick_line();
        pdf.advance(SECTION_GAP);
        
        let items = [
            ("Calories", PdfBuilder::format_decimal(&nutrition.total_calories)),
            ("Protein", format!("{}g", PdfBuilder::format_decimal(&nutrition.total_protein_g))),
            ("Carbs", format!("{}g", PdfBuilder::format_decimal(&nutrition.total_carbs_g))),
            ("Fat", format!("{}g", PdfBuilder::format_decimal(&nutrition.total_fat_g))),
        ];
        
        for (label, value) in items {
            pdf.set_color(COLOR_TEXT);
            pdf.text(&format!("{}:", label), FONT_BODY, true, MARGIN + 5.0);
            pdf.text(&value, FONT_BODY, false, MARGIN + 50.0);
            pdf.advance(FONT_BODY * LINE_HEIGHT * 0.4);
        }
    }

    fn render_closing(pdf: &mut PdfBuilder) {
        pdf.advance(80.0);
        
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Bon Appétit!", FONT_TITLE, true, MARGIN + 40.0);
        pdf.advance(SECTION_GAP * 2.0);
        
        pdf.set_color(COLOR_TEXT);
        pdf.text("Enjoy your meal!", FONT_BODY, false, MARGIN + 55.0);
    }
}

// ============================================================================
// Meal Plan PDF Exporter
// ============================================================================

impl RecipePdfExporter {
    pub async fn export_meal_plan(
        meal_plan: &MealPlanWithEntries,
        nutrition: Option<&MealPlanNutrition>,
        pool: &sqlx::PgPool,
        output_path: &Path,
    ) -> Result<()> {
        let mut pdf = PdfBuilder::new(&meal_plan.meal_plan.name)?;
        
        // Cover page
        Self::render_meal_plan_cover(&mut pdf, meal_plan);
        
        // Daily recipes
        let daily_entries = Self::group_by_day(&meal_plan.entries);
        for (day_key, entries) in &daily_entries {
            pdf.new_page();
            Self::render_day_page(&mut pdf, day_key, entries, pool).await?;
        }
        
        // Shopping list
        let prep_data = NutritionService::get_meal_plan_for_prep_analysis(pool, meal_plan.meal_plan.id)
            .await
            .map_err(|e| ToolboxError::Other(format!("Failed to get prep data: {}", e)))?;
        
        pdf.new_page();
        Self::render_shopping_list(&mut pdf, &prep_data.aggregated_ingredients);
        
        // Meal prep guide
        pdf.new_page();
        Self::render_prep_guide(&mut pdf, &prep_data.aggregated_ingredients);
        
        // Nutrition summary
        if let Some(nutrition) = nutrition {
            pdf.new_page();
            Self::render_nutrition_summary(&mut pdf, nutrition);
        }
        
        // Closing
        pdf.new_page();
        Self::render_meal_plan_closing(&mut pdf);
        
        pdf.save(output_path)
    }

    fn render_meal_plan_cover(pdf: &mut PdfBuilder, meal_plan: &MealPlanWithEntries) {
        pdf.advance(40.0);
        
        // Title
        pdf.set_color(COLOR_PRIMARY);
        pdf.text(&meal_plan.meal_plan.name, FONT_TITLE, true, MARGIN);
        pdf.advance(FONT_TITLE * 0.5);
        pdf.thick_line();
        pdf.advance(PARAGRAPH_GAP);
        
        // Description
        if let Some(desc) = &meal_plan.meal_plan.description {
            pdf.set_color(COLOR_TEXT);
            pdf.wrapped_text_italic(desc, FONT_BODY, 80, MARGIN);
            pdf.advance(PARAGRAPH_GAP);
        }
        
        // Dates and metadata in a tight block
        pdf.set_color(COLOR_LIGHT);
        if let Some(start) = meal_plan.meal_plan.start_date {
            pdf.text(&format!("From: {}", start.format("%B %d, %Y")), FONT_BODY, false, MARGIN);
            pdf.advance(FONT_BODY + 2.0);
        }
        if let Some(end) = meal_plan.meal_plan.end_date {
            pdf.text(&format!("To: {}", end.format("%B %d, %Y")), FONT_BODY, false, MARGIN);
            pdf.advance(FONT_BODY + 2.0);
        }
        pdf.text(&format!("Total Meals: {}", meal_plan.entries.len()), FONT_BODY, true, MARGIN);
        
        // Overview section
        pdf.advance(SECTION_GAP);
        pdf.set_color(COLOR_SECONDARY);
        pdf.text("Your Week at a Glance", FONT_HEADING, true, MARGIN);
        pdf.advance(PARAGRAPH_GAP);
        
        pdf.horizontal_line(0.5);
        pdf.advance(PARAGRAPH_GAP);
        
        // Quick meal list - clean and compact
        let daily = Self::group_by_day(&meal_plan.entries);
        
        for (day_key, entries) in daily.iter().take(7) {
            pdf.ensure_space(FONT_SMALL * 2.0);
            
            // Day name
            pdf.set_color(COLOR_PRIMARY);
            let day_name = Self::format_day_key(day_key);
            pdf.text(&day_name, FONT_SMALL, true, MARGIN);
            
            // Recipes on same line
            pdf.set_color(COLOR_TEXT);
            let recipes: Vec<_> = entries.iter().map(|e| e.recipe.name.as_str()).collect();
            let recipe_text = recipes.join(", ");
            if recipe_text.len() > 50 {
                pdf.text(&format!("{}...", &recipe_text[..47]), FONT_SMALL, false, MARGIN + 55.0);
            } else {
                pdf.text(&recipe_text, FONT_SMALL, false, MARGIN + 55.0);
            }
            pdf.advance(FONT_SMALL + 3.0);
        }
    }

    fn group_by_day(entries: &[MealPlanEntryWithRecipe]) -> Vec<((Option<NaiveDate>, Option<i32>), Vec<&MealPlanEntryWithRecipe>)> {
        let mut map: HashMap<(Option<NaiveDate>, Option<i32>), Vec<&MealPlanEntryWithRecipe>> = HashMap::new();
        
        for entry in entries {
            let key = (entry.entry.date, entry.entry.day_of_week);
            map.entry(key).or_default().push(entry);
        }
        
        let mut result: Vec<_> = map.into_iter().collect();
        result.sort_by(|a, b| {
            match (a.0.0, b.0.0) {
                (Some(da), Some(db)) => da.cmp(&db),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.0.1.cmp(&b.0.1),
            }
        });
        
        // Sort entries within each day by meal type
        for (_, entries) in &mut result {
            entries.sort_by(|a, b| {
                let order = |mt: &str| match mt.to_lowercase().as_str() {
                    "breakfast" => 0,
                    "lunch" => 1,
                    "dinner" => 2,
                    "snack" => 3,
                    _ => 4,
                };
                order(&a.entry.meal_type).cmp(&order(&b.entry.meal_type))
            });
        }
        
        result
    }

    fn format_day_key(key: &(Option<NaiveDate>, Option<i32>)) -> String {
        if let Some(date) = key.0 {
            date.format("%A, %B %d").to_string()
        } else if let Some(dow) = key.1 {
            DayOfWeek::from_int(dow)
                .map(|d| format!("{:?}", d))
                .unwrap_or_else(|| format!("Day {}", dow))
        } else {
            "Day".to_string()
        }
    }

    async fn render_day_page(
        pdf: &mut PdfBuilder,
        day_key: &(Option<NaiveDate>, Option<i32>),
        entries: &[&MealPlanEntryWithRecipe],
        pool: &sqlx::PgPool,
    ) -> Result<()> {
        let day_name = Self::format_day_key(day_key);
        
        // Day header
        pdf.set_color(COLOR_PRIMARY);
        pdf.text(&day_name, FONT_HEADING, true, MARGIN);
        pdf.advance(FONT_HEADING * 0.4);
        pdf.thick_line();
        pdf.advance(SECTION_GAP);
        
        for entry in entries {
            // Fetch full recipe details
            let recipe = NutritionService::get_recipe_with_details(pool, entry.recipe.id)
                .await
                .map_err(|e| ToolboxError::Other(format!("Failed to fetch recipe: {}", e)))?;
            
            pdf.ensure_space(80.0);
            Self::render_recipe_card(pdf, &entry.entry.meal_type, &recipe);
            pdf.advance(SECTION_GAP);
            pdf.horizontal_line(0.3);
            pdf.advance(SECTION_GAP);
        }
        
        Ok(())
    }

    fn render_recipe_card(pdf: &mut PdfBuilder, meal_type: &str, recipe: &RecipeWithDetails) {
        // Meal type badge
        pdf.set_color(COLOR_SECONDARY);
        let meal_label = meal_type.chars().next()
            .map(|c| c.to_uppercase().collect::<String>() + &meal_type[1..])
            .unwrap_or_else(|| meal_type.to_string());
        pdf.text(&meal_label, FONT_SMALL, true, MARGIN);
        pdf.advance(FONT_SMALL + 2.0);
        
        // Recipe name
        pdf.set_color(COLOR_PRIMARY);
        pdf.text(&recipe.recipe.name, FONT_SUBHEADING, true, MARGIN);
        pdf.advance(FONT_SUBHEADING + 2.0);
        
        // Description
        if let Some(desc) = &recipe.recipe.description {
            pdf.set_color(COLOR_TEXT);
            pdf.wrapped_text_italic(desc, FONT_SMALL, 85, MARGIN);
        }
        
        // Metadata line
        let mut meta = Vec::new();
        if let Some(s) = recipe.recipe.servings { meta.push(format!("Serves {}", s)); }
        if let Some(p) = recipe.recipe.prep_time_minutes { meta.push(format!("{} min prep", p)); }
        if let Some(c) = recipe.recipe.cook_time_minutes { meta.push(format!("{} min cook", c)); }
        
        if !meta.is_empty() {
            pdf.advance(PARAGRAPH_GAP * 0.5);
            pdf.set_color(COLOR_LIGHT);
            pdf.text(&meta.join("  •  "), FONT_SMALL, false, MARGIN);
            pdf.advance(FONT_SMALL * LINE_HEIGHT * 0.4);
        }
        
        pdf.advance(PARAGRAPH_GAP);
        
        // Ingredients (compact)
        if !recipe.ingredients.is_empty() {
            pdf.set_color(COLOR_PRIMARY);
            pdf.text("Ingredients", FONT_SMALL, true, MARGIN);
            pdf.advance(FONT_SMALL * 0.5);
            
            for ing in &recipe.ingredients {
                pdf.ensure_space(FONT_SMALL * 1.5);
                pdf.set_color(COLOR_TEXT);
                // Convert to American units
                let (qty_str, unit_str) = PdfBuilder::to_american_units(
                    &ing.recipe_ingredient.quantity,
                    &ing.recipe_ingredient.unit
                );
                let line = format!("• {} {} {}", qty_str, unit_str, ing.ingredient.name);
                pdf.text(&line, FONT_SMALL, false, MARGIN + 3.0);
                pdf.advance(FONT_SMALL * LINE_HEIGHT * 0.35);
            }
            pdf.advance(PARAGRAPH_GAP);
        }
        
        // Instructions (compact)
        if !recipe.steps.is_empty() {
            pdf.set_color(COLOR_PRIMARY);
            pdf.text("Instructions", FONT_SMALL, true, MARGIN);
            pdf.advance(FONT_SMALL * 0.5);
            
            for step in &recipe.steps {
                pdf.ensure_space(FONT_SMALL * 3.0);
                pdf.set_color(COLOR_TEXT);
                let text = format!("{}. {}", step.step_number, step.instruction);
                pdf.wrapped_text(&text, FONT_SMALL, 85, MARGIN + 3.0, false);
            }
        }
    }

    fn render_shopping_list(pdf: &mut PdfBuilder, ingredients: &[AggregatedIngredient]) {
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Shopping List", FONT_HEADING, true, MARGIN);
        pdf.advance(FONT_HEADING * 0.4);
        pdf.thick_line();
        pdf.advance(PARAGRAPH_GAP);
        
        // Categorize ingredients
        let categories = Self::categorize_ingredients(ingredients);
        let category_order = [
            "Proteins",
            "Vegetables & Produce",
            "Dairy & Refrigerated",
            "Pantry Staples",
            "Condiments & Seasonings",
            "Other",
        ];
        
        // Table columns: Checkbox | Qty | Unit | Item
        let col_check = MARGIN;
        let col_qty = MARGIN + 12.0;
        let col_unit = MARGIN + 35.0;
        let col_item = MARGIN + 55.0;
        
        for cat_name in category_order {
            if let Some(items) = categories.get(cat_name) {
                if items.is_empty() { continue; }
                
                pdf.ensure_space(FONT_BODY * 4.0);
                
                // Category header with underline
                pdf.set_color(COLOR_PRIMARY);
                pdf.text(cat_name, FONT_BODY, true, MARGIN);
                pdf.advance(FONT_BODY + 1.0);
                pdf.horizontal_line(0.3);
                pdf.advance(3.0);
                
                // Table rows
                for ing in items {
                    pdf.ensure_space(FONT_BODY + 3.0);
                    pdf.set_color(COLOR_TEXT);
                    
                    // Convert to American units
                    let (qty_str, unit_str) = PdfBuilder::to_american_units(&ing.total_quantity, &ing.unit);
                    
                    // Checkbox
                    pdf.text("[  ]", FONT_BODY, false, col_check);
                    // Quantity
                    pdf.text(&qty_str, FONT_BODY, false, col_qty);
                    // Unit
                    pdf.text(&unit_str, FONT_BODY, false, col_unit);
                    // Item name
                    let name = if ing.ingredient.name.len() > 35 {
                        format!("{}...", &ing.ingredient.name[..32])
                    } else {
                        ing.ingredient.name.clone()
                    };
                    pdf.text(&name, FONT_BODY, false, col_item);
                    
                    pdf.advance(FONT_BODY + 2.0);
                }
                pdf.advance(PARAGRAPH_GAP);
            }
        }
    }

    fn categorize_ingredients(ingredients: &[AggregatedIngredient]) -> HashMap<&'static str, Vec<&AggregatedIngredient>> {
        let mut result: HashMap<&'static str, Vec<&AggregatedIngredient>> = HashMap::new();
        
        for ing in ingredients {
            let name = ing.ingredient.name.to_lowercase();
            let cat = if name.contains("chicken") || name.contains("beef") || name.contains("pork") 
                || name.contains("salmon") || name.contains("fish") || name.contains("turkey")
                || name.contains("lamb") || name.contains("meat") || name.contains("sausage") {
                "Proteins"
            } else if name.contains("broccoli") || name.contains("spinach") || name.contains("carrot")
                || name.contains("onion") || name.contains("pepper") || name.contains("potato")
                || name.contains("tomato") || name.contains("garlic") || name.contains("lettuce")
                || name.contains("vegetable") || name.contains("herb") || name.contains("beans") {
                "Vegetables & Produce"
            } else if name.contains("cheese") || name.contains("milk") || name.contains("egg")
                || name.contains("butter") || name.contains("yogurt") || name.contains("cream") {
                "Dairy & Refrigerated"
            } else if name.contains("rice") || name.contains("pasta") || name.contains("flour")
                || name.contains("bread") || name.contains("noodle") || name.contains("macaroni")
                || name.contains("quinoa") || name.contains("tortilla") {
                "Pantry Staples"
            } else if name.contains("oil") || name.contains("sauce") || name.contains("seasoning")
                || name.contains("spice") || name.contains("vinegar") || name.contains("soy") {
                "Condiments & Seasonings"
            } else {
                "Other"
            };
            result.entry(cat).or_default().push(ing);
        }
        result
    }

    fn render_prep_guide(pdf: &mut PdfBuilder, ingredients: &[AggregatedIngredient]) {
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Meal Prep Guide", FONT_HEADING, true, MARGIN);
        pdf.advance(FONT_HEADING * 0.4);
        pdf.thick_line();
        pdf.advance(PARAGRAPH_GAP);
        
        // General tips - section header
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Prep Timeline", FONT_BODY, true, MARGIN);
        pdf.advance(FONT_BODY + 1.0);
        pdf.horizontal_line(0.3);
        pdf.advance(3.0);
        
        let tips = [
            ("Sunday", "Prep vegetables that last 5-7 days (onions, carrots, potatoes)"),
            ("Sunday", "Portion and marinate proteins if needed"),
            ("Day Before", "Prep fresh vegetables (broccoli, peppers)"),
            ("Day Of", "Prep delicate items (fresh herbs, lettuce)"),
        ];
        
        for (when, what) in tips {
            pdf.ensure_space(FONT_BODY + 3.0);
            pdf.set_color(COLOR_PRIMARY);
            pdf.text(when, FONT_BODY, true, MARGIN + 5.0);
            pdf.set_color(COLOR_TEXT);
            pdf.text(what, FONT_BODY, false, MARGIN + 40.0);
            pdf.advance(FONT_BODY + 2.0);
        }
        
        pdf.advance(PARAGRAPH_GAP);
        
        // Storage notes - section header
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Storage Notes", FONT_BODY, true, MARGIN);
        pdf.advance(FONT_BODY + 1.0);
        pdf.horizontal_line(0.3);
        pdf.advance(3.0);
        
        for ing in ingredients.iter().take(12) {
            let name = ing.ingredient.name.to_lowercase();
            let tip = if name.contains("onion") || name.contains("garlic") {
                "2-3 days ahead, refrigerate"
            } else if name.contains("broccoli") || name.contains("pepper") {
                "1-2 days ahead, airtight container"
            } else if name.contains("lettuce") || name.contains("spinach") {
                "Day-of, wash and dry"
            } else if name.contains("chicken") || name.contains("beef") || name.contains("pork") {
                "1 day ahead, marinate refrigerated"
            } else {
                continue;
            };
            
            pdf.ensure_space(FONT_BODY + 3.0);
            pdf.set_color(COLOR_TEXT);
            pdf.text(&ing.ingredient.name, FONT_BODY, false, MARGIN + 5.0);
            pdf.set_color(COLOR_LIGHT);
            pdf.text(tip, FONT_BODY, false, MARGIN + 80.0);
            pdf.advance(FONT_BODY + 2.0);
        }
    }

    fn render_nutrition_summary(pdf: &mut PdfBuilder, nutrition: &MealPlanNutrition) {
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Nutrition Summary", FONT_HEADING, true, MARGIN);
        pdf.advance(FONT_HEADING * 0.4);
        pdf.thick_line();
        pdf.advance(PARAGRAPH_GAP);
        
        if let Some(weekly) = &nutrition.weekly_totals {
            // Weekly totals header
            pdf.set_color(COLOR_PRIMARY);
            pdf.text("Weekly Totals", FONT_BODY, true, MARGIN);
            pdf.advance(FONT_BODY + 1.0);
            pdf.horizontal_line(0.3);
            pdf.advance(3.0);
            
            let items = [
                ("Total Calories", PdfBuilder::format_decimal(&weekly.total_calories)),
                ("Avg Daily Calories", PdfBuilder::format_decimal(&weekly.average_daily_calories)),
                ("Total Protein", format!("{}g", PdfBuilder::format_decimal(&weekly.total_protein_g))),
                ("Total Carbs", format!("{}g", PdfBuilder::format_decimal(&weekly.total_carbs_g))),
                ("Total Fat", format!("{}g", PdfBuilder::format_decimal(&weekly.total_fat_g))),
            ];
            
            for (label, value) in items {
                pdf.set_color(COLOR_TEXT);
                pdf.text(label, FONT_BODY, false, MARGIN + 5.0);
                pdf.text(&value, FONT_BODY, true, MARGIN + 70.0);
                pdf.advance(FONT_BODY + 2.0);
            }
            
            pdf.advance(PARAGRAPH_GAP);
        }
        
        // Daily breakdown header
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Daily Breakdown", FONT_BODY, true, MARGIN);
        pdf.advance(FONT_BODY + 1.0);
        pdf.horizontal_line(0.3);
        pdf.advance(3.0);
        
        // Table header row
        pdf.set_color(COLOR_LIGHT);
        pdf.text("Day", FONT_BODY, true, MARGIN + 5.0);
        pdf.text("Calories", FONT_BODY, true, MARGIN + 45.0);
        pdf.text("Protein", FONT_BODY, true, MARGIN + 80.0);
        pdf.text("Carbs", FONT_BODY, true, MARGIN + 110.0);
        pdf.text("Fat", FONT_BODY, true, MARGIN + 140.0);
        pdf.advance(FONT_BODY + 2.0);
        
        for daily in &nutrition.daily_nutrition {
            let day_label = if let Some(date) = daily.date {
                date.format("%a").to_string()
            } else if let Some(dow) = daily.day_of_week {
                DayOfWeek::from_int(dow)
                    .map(|d| format!("{:?}", d).chars().take(3).collect())
                    .unwrap_or_else(|| format!("D{}", dow))
            } else {
                continue;
            };
            
            pdf.ensure_space(FONT_BODY + 3.0);
            pdf.set_color(COLOR_TEXT);
            pdf.text(&day_label, FONT_BODY, false, MARGIN + 5.0);
            pdf.text(&PdfBuilder::format_decimal(&daily.total_calories), FONT_BODY, false, MARGIN + 45.0);
            pdf.text(&format!("{}g", PdfBuilder::format_decimal(&daily.total_protein_g)), FONT_BODY, false, MARGIN + 80.0);
            pdf.text(&format!("{}g", PdfBuilder::format_decimal(&daily.total_carbs_g)), FONT_BODY, false, MARGIN + 110.0);
            pdf.text(&format!("{}g", PdfBuilder::format_decimal(&daily.total_fat_g)), FONT_BODY, false, MARGIN + 140.0);
            pdf.advance(FONT_BODY + 2.0);
        }
    }

    fn render_meal_plan_closing(pdf: &mut PdfBuilder) {
        // Center content vertically on page
        pdf.y_pos = PAGE_HEIGHT / 2.0 + 30.0;
        
        // All text centered horizontally (page center = 105mm)
        let center = PAGE_WIDTH / 2.0;
        
        pdf.set_color(COLOR_PRIMARY);
        pdf.text("Bon Appetit!", FONT_TITLE, true, center - 40.0);
        pdf.advance(SECTION_GAP * 1.5);
        
        pdf.set_color(COLOR_TEXT);
        pdf.text("Thank you for using this meal plan.", FONT_BODY, false, center - 55.0);
        pdf.advance(FONT_BODY + 4.0);
        pdf.text("We hope these recipes bring joy to your kitchen", FONT_BODY, false, center - 70.0);
        pdf.advance(FONT_BODY + 4.0);
        pdf.text("and nourishment to your table.", FONT_BODY, false, center - 45.0);
        pdf.advance(SECTION_GAP * 1.5);
        
        pdf.set_color(COLOR_SECONDARY);
        pdf.text("Happy cooking!", FONT_BODY, true, center - 22.0);
    }
}
