-- Family Members table
CREATE TABLE IF NOT EXISTS family_members (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL,
    preferences JSONB, -- Flexible JSON storage for preferences (dietary restrictions, cuisine preferences, etc.)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Family Member Allergies table (many-to-many with ingredients)
CREATE TABLE IF NOT EXISTS family_member_allergies (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    family_member_id UUID NOT NULL REFERENCES family_members(id) ON DELETE CASCADE,
    ingredient_id UUID NOT NULL REFERENCES ingredients(id) ON DELETE CASCADE,
    severity TEXT CHECK (severity IN ('mild', 'moderate', 'severe')),
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(family_member_id, ingredient_id)
);

-- Recipe Favorites table (many-to-many between family members and recipes)
CREATE TABLE IF NOT EXISTS recipe_favorites (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    family_member_id UUID NOT NULL REFERENCES family_members(id) ON DELETE CASCADE,
    recipe_id UUID NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(family_member_id, recipe_id)
);

-- Indexes for search performance
CREATE INDEX IF NOT EXISTS idx_family_members_name ON family_members(name);
CREATE INDEX IF NOT EXISTS idx_family_member_allergies_family_member_id ON family_member_allergies(family_member_id);
CREATE INDEX IF NOT EXISTS idx_family_member_allergies_ingredient_id ON family_member_allergies(ingredient_id);
CREATE INDEX IF NOT EXISTS idx_recipe_favorites_family_member_id ON recipe_favorites(family_member_id);
CREATE INDEX IF NOT EXISTS idx_recipe_favorites_recipe_id ON recipe_favorites(recipe_id);

-- Trigger to automatically update updated_at for family_members
DROP TRIGGER IF EXISTS update_family_members_updated_at ON family_members;
CREATE TRIGGER update_family_members_updated_at BEFORE UPDATE ON family_members
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();


