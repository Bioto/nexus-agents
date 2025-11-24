-- Meal Plans table
CREATE TABLE IF NOT EXISTS meal_plans (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL,
    description TEXT,
    start_date DATE,
    end_date DATE,
    is_template BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_dates CHECK (start_date IS NULL OR end_date IS NULL OR start_date <= end_date)
);

-- Meal Plan Entries table
CREATE TABLE IF NOT EXISTS meal_plan_entries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    meal_plan_id UUID NOT NULL REFERENCES meal_plans(id) ON DELETE CASCADE,
    day_of_week INTEGER CHECK (day_of_week IS NULL OR (day_of_week >= 0 AND day_of_week <= 6)),
    date DATE,
    meal_type TEXT NOT NULL CHECK (meal_type IN ('breakfast', 'lunch', 'dinner', 'snack')),
    recipe_id UUID NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_template_entry CHECK (
        (day_of_week IS NOT NULL AND date IS NULL) OR
        (day_of_week IS NULL AND date IS NOT NULL)
    )
);

-- Indexes for search performance
CREATE INDEX IF NOT EXISTS idx_meal_plans_name ON meal_plans(name);
CREATE INDEX IF NOT EXISTS idx_meal_plans_is_template ON meal_plans(is_template);
CREATE INDEX IF NOT EXISTS idx_meal_plan_entries_meal_plan_id ON meal_plan_entries(meal_plan_id);
CREATE INDEX IF NOT EXISTS idx_meal_plan_entries_date ON meal_plan_entries(date);
CREATE INDEX IF NOT EXISTS idx_meal_plan_entries_day_of_week ON meal_plan_entries(day_of_week);
CREATE INDEX IF NOT EXISTS idx_meal_plan_entries_meal_type ON meal_plan_entries(meal_type);
CREATE INDEX IF NOT EXISTS idx_meal_plan_entries_recipe_id ON meal_plan_entries(recipe_id);

-- Trigger to automatically update updated_at for meal_plans
DROP TRIGGER IF EXISTS update_meal_plans_updated_at ON meal_plans;
CREATE TRIGGER update_meal_plans_updated_at BEFORE UPDATE ON meal_plans
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();




