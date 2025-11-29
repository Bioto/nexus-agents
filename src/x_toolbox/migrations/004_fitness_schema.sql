-- Fitness Module Schema
-- Personal trainer functionality with exercises, workouts, training programs, and progress tracking

-- ============================================================================
-- CORE EXERCISE TABLES
-- ============================================================================

-- Exercises table (analogous to ingredients)
CREATE TABLE IF NOT EXISTS exercises (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL,
    description TEXT,
    muscle_groups JSONB DEFAULT '[]'::jsonb,  -- ["chest", "triceps", "shoulders"]
    equipment JSONB DEFAULT '[]'::jsonb,       -- ["barbell", "bench"]
    exercise_type TEXT NOT NULL DEFAULT 'strength',  -- strength, cardio, flexibility, plyometric, balance
    difficulty_level TEXT NOT NULL DEFAULT 'intermediate',  -- beginner, intermediate, advanced
    instructions TEXT,  -- Step-by-step instructions
    video_url TEXT,     -- Optional video demonstration
    calories_per_minute DECIMAL(10, 2),  -- Estimated calorie burn
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create unique index on exercise name (case-insensitive)
CREATE UNIQUE INDEX IF NOT EXISTS idx_exercises_name_unique ON exercises(LOWER(name));

-- ============================================================================
-- WORKOUT PLAN TABLES
-- ============================================================================

-- Workout plans table (analogous to recipes)
CREATE TABLE IF NOT EXISTS workout_plans (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL,
    description TEXT,
    workout_type TEXT NOT NULL DEFAULT 'strength',  -- strength, cardio, hiit, flexibility, full_body, upper_body, lower_body, core
    difficulty_level TEXT NOT NULL DEFAULT 'intermediate',
    estimated_duration_minutes INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Workout exercises junction table (analogous to recipe_ingredients)
CREATE TABLE IF NOT EXISTS workout_exercises (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    workout_id UUID NOT NULL REFERENCES workout_plans(id) ON DELETE CASCADE,
    exercise_id UUID NOT NULL REFERENCES exercises(id) ON DELETE CASCADE,
    order_index INTEGER NOT NULL DEFAULT 0,
    sets INTEGER,                    -- Number of sets (null for timed exercises)
    reps INTEGER,                    -- Reps per set (null for timed exercises)
    duration_seconds INTEGER,        -- For timed exercises (planks, cardio, etc.)
    rest_seconds INTEGER DEFAULT 60, -- Rest between sets
    notes TEXT,                      -- Special instructions for this exercise in this workout
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(workout_id, order_index)
);

-- ============================================================================
-- TRAINING PROGRAM TABLES (analogous to meal_plans)
-- ============================================================================

-- Training programs table
CREATE TABLE IF NOT EXISTS training_programs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL,
    description TEXT,
    start_date DATE,
    end_date DATE,
    is_template BOOLEAN NOT NULL DEFAULT false,
    goal TEXT,  -- weight_loss, muscle_gain, endurance, strength, flexibility, general_fitness
    weeks INTEGER,  -- Program duration in weeks (for templates)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Program entries table (scheduled workouts)
CREATE TABLE IF NOT EXISTS program_entries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    program_id UUID NOT NULL REFERENCES training_programs(id) ON DELETE CASCADE,
    workout_id UUID NOT NULL REFERENCES workout_plans(id) ON DELETE CASCADE,
    day_of_week INTEGER,  -- 0=Monday, 6=Sunday (for templates)
    week_number INTEGER,  -- Week within program (for multi-week templates)
    date DATE,            -- Specific date (for non-templates)
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- FITNESS PROFILE TABLES (linked to nutrition family_members)
-- ============================================================================

-- Fitness profiles table (linked to family_members from nutrition)
CREATE TABLE IF NOT EXISTS fitness_profiles (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    family_member_id UUID NOT NULL REFERENCES family_members(id) ON DELETE CASCADE,
    current_weight_kg DECIMAL(10, 2),
    target_weight_kg DECIMAL(10, 2),
    height_cm DECIMAL(10, 2),
    fitness_level TEXT NOT NULL DEFAULT 'beginner',  -- beginner, intermediate, advanced
    goals JSONB DEFAULT '[]'::jsonb,  -- ["weight_loss", "muscle_gain", "endurance"]
    restrictions JSONB DEFAULT '[]'::jsonb,  -- ["knee_injury", "back_pain", "no_running"]
    activity_level TEXT DEFAULT 'moderate',  -- sedentary, light, moderate, active, very_active
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(family_member_id)  -- One fitness profile per family member
);

-- ============================================================================
-- PROGRESS TRACKING TABLES
-- ============================================================================

-- Body measurements table
CREATE TABLE IF NOT EXISTS body_measurements (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    fitness_profile_id UUID NOT NULL REFERENCES fitness_profiles(id) ON DELETE CASCADE,
    measured_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    weight_kg DECIMAL(10, 2),
    body_fat_percentage DECIMAL(5, 2),
    waist_cm DECIMAL(10, 2),
    chest_cm DECIMAL(10, 2),
    hips_cm DECIMAL(10, 2),
    left_arm_cm DECIMAL(10, 2),
    right_arm_cm DECIMAL(10, 2),
    left_thigh_cm DECIMAL(10, 2),
    right_thigh_cm DECIMAL(10, 2),
    neck_cm DECIMAL(10, 2),
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Workout logs table (completed workout sessions)
CREATE TABLE IF NOT EXISTS workout_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    fitness_profile_id UUID NOT NULL REFERENCES fitness_profiles(id) ON DELETE CASCADE,
    workout_id UUID REFERENCES workout_plans(id) ON DELETE SET NULL,  -- Can be null for ad-hoc workouts
    workout_name TEXT,  -- Store name in case workout is deleted
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    duration_minutes INTEGER,
    calories_burned INTEGER,
    notes TEXT,
    rating INTEGER CHECK (rating >= 1 AND rating <= 5),  -- How the workout felt (1-5)
    perceived_difficulty INTEGER CHECK (perceived_difficulty >= 1 AND perceived_difficulty <= 10),  -- RPE scale
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Exercise logs table (detailed performance per exercise in a workout)
CREATE TABLE IF NOT EXISTS exercise_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    workout_log_id UUID NOT NULL REFERENCES workout_logs(id) ON DELETE CASCADE,
    exercise_id UUID REFERENCES exercises(id) ON DELETE SET NULL,
    exercise_name TEXT,  -- Store name in case exercise is deleted
    order_index INTEGER NOT NULL DEFAULT 0,
    sets_completed INTEGER,
    reps_per_set JSONB,  -- [12, 10, 8] for varying reps per set
    weight_kg DECIMAL(10, 2),  -- Weight used (for weighted exercises)
    duration_seconds INTEGER,  -- For timed exercises
    distance_meters DECIMAL(10, 2),  -- For cardio exercises
    notes TEXT,
    is_personal_record BOOLEAN DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Personal records table (PRs tracked separately for easy querying)
CREATE TABLE IF NOT EXISTS personal_records (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    fitness_profile_id UUID NOT NULL REFERENCES fitness_profiles(id) ON DELETE CASCADE,
    exercise_id UUID NOT NULL REFERENCES exercises(id) ON DELETE CASCADE,
    record_type TEXT NOT NULL,  -- max_weight, max_reps, max_duration, fastest_time, longest_distance
    value DECIMAL(10, 2) NOT NULL,
    achieved_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    workout_log_id UUID REFERENCES workout_logs(id) ON DELETE SET NULL,
    exercise_log_id UUID REFERENCES exercise_logs(id) ON DELETE SET NULL,
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(fitness_profile_id, exercise_id, record_type)  -- One PR per type per exercise per profile
);

-- ============================================================================
-- INDEXES FOR PERFORMANCE
-- ============================================================================

-- Exercise indexes
CREATE INDEX IF NOT EXISTS idx_exercises_name ON exercises(name);
CREATE INDEX IF NOT EXISTS idx_exercises_type ON exercises(exercise_type);
CREATE INDEX IF NOT EXISTS idx_exercises_difficulty ON exercises(difficulty_level);
CREATE INDEX IF NOT EXISTS idx_exercises_muscle_groups ON exercises USING GIN(muscle_groups);
CREATE INDEX IF NOT EXISTS idx_exercises_equipment ON exercises USING GIN(equipment);

-- Workout plan indexes
CREATE INDEX IF NOT EXISTS idx_workout_plans_name ON workout_plans(name);
CREATE INDEX IF NOT EXISTS idx_workout_plans_type ON workout_plans(workout_type);
CREATE INDEX IF NOT EXISTS idx_workout_exercises_workout_id ON workout_exercises(workout_id);
CREATE INDEX IF NOT EXISTS idx_workout_exercises_exercise_id ON workout_exercises(exercise_id);

-- Training program indexes
CREATE INDEX IF NOT EXISTS idx_training_programs_name ON training_programs(name);
CREATE INDEX IF NOT EXISTS idx_training_programs_template ON training_programs(is_template);
CREATE INDEX IF NOT EXISTS idx_training_programs_goal ON training_programs(goal);
CREATE INDEX IF NOT EXISTS idx_program_entries_program_id ON program_entries(program_id);
CREATE INDEX IF NOT EXISTS idx_program_entries_workout_id ON program_entries(workout_id);

-- Fitness profile indexes
CREATE INDEX IF NOT EXISTS idx_fitness_profiles_family_member ON fitness_profiles(family_member_id);
CREATE INDEX IF NOT EXISTS idx_fitness_profiles_level ON fitness_profiles(fitness_level);

-- Progress tracking indexes
CREATE INDEX IF NOT EXISTS idx_body_measurements_profile ON body_measurements(fitness_profile_id);
CREATE INDEX IF NOT EXISTS idx_body_measurements_date ON body_measurements(measured_at);
CREATE INDEX IF NOT EXISTS idx_workout_logs_profile ON workout_logs(fitness_profile_id);
CREATE INDEX IF NOT EXISTS idx_workout_logs_date ON workout_logs(started_at);
CREATE INDEX IF NOT EXISTS idx_workout_logs_workout ON workout_logs(workout_id);
CREATE INDEX IF NOT EXISTS idx_exercise_logs_workout_log ON exercise_logs(workout_log_id);
CREATE INDEX IF NOT EXISTS idx_exercise_logs_exercise ON exercise_logs(exercise_id);
CREATE INDEX IF NOT EXISTS idx_personal_records_profile ON personal_records(fitness_profile_id);
CREATE INDEX IF NOT EXISTS idx_personal_records_exercise ON personal_records(exercise_id);

-- ============================================================================
-- TRIGGERS FOR updated_at
-- ============================================================================

DROP TRIGGER IF EXISTS update_exercises_updated_at ON exercises;
CREATE TRIGGER update_exercises_updated_at BEFORE UPDATE ON exercises
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

DROP TRIGGER IF EXISTS update_workout_plans_updated_at ON workout_plans;
CREATE TRIGGER update_workout_plans_updated_at BEFORE UPDATE ON workout_plans
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

DROP TRIGGER IF EXISTS update_training_programs_updated_at ON training_programs;
CREATE TRIGGER update_training_programs_updated_at BEFORE UPDATE ON training_programs
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

DROP TRIGGER IF EXISTS update_fitness_profiles_updated_at ON fitness_profiles;
CREATE TRIGGER update_fitness_profiles_updated_at BEFORE UPDATE ON fitness_profiles
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

