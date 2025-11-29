//! Domain models for the fitness module.
//!
//! This module defines all the data structures used in the fitness/personal trainer
//! system, including exercises, workout plans, training programs, fitness profiles,
//! and progress tracking.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{types::BigDecimal, FromRow};
use uuid::Uuid;

// ============================================================================
// CORE EXERCISE MODELS
// ============================================================================

/// Exercise model - the building blocks of workouts
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Exercise {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub muscle_groups: Option<serde_json::Value>, // JSONB array
    pub equipment: Option<serde_json::Value>,     // JSONB array
    pub exercise_type: String,
    pub difficulty_level: String,
    pub instructions: Option<String>,
    pub video_url: Option<String>,
    pub calories_per_minute: Option<BigDecimal>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Exercise type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExerciseType {
    Strength,
    Cardio,
    Flexibility,
    Plyometric,
    Balance,
}

impl ExerciseType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExerciseType::Strength => "strength",
            ExerciseType::Cardio => "cardio",
            ExerciseType::Flexibility => "flexibility",
            ExerciseType::Plyometric => "plyometric",
            ExerciseType::Balance => "balance",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "strength" => Some(ExerciseType::Strength),
            "cardio" => Some(ExerciseType::Cardio),
            "flexibility" => Some(ExerciseType::Flexibility),
            "plyometric" => Some(ExerciseType::Plyometric),
            "balance" => Some(ExerciseType::Balance),
            _ => None,
        }
    }
}

/// Difficulty level enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DifficultyLevel {
    Beginner,
    Intermediate,
    Advanced,
}

impl DifficultyLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            DifficultyLevel::Beginner => "beginner",
            DifficultyLevel::Intermediate => "intermediate",
            DifficultyLevel::Advanced => "advanced",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "beginner" => Some(DifficultyLevel::Beginner),
            "intermediate" => Some(DifficultyLevel::Intermediate),
            "advanced" => Some(DifficultyLevel::Advanced),
            _ => None,
        }
    }
}

// ============================================================================
// WORKOUT PLAN MODELS
// ============================================================================

/// Workout plan model - a collection of exercises
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkoutPlan {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub workout_type: String,
    pub difficulty_level: String,
    pub estimated_duration_minutes: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Workout type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkoutType {
    Strength,
    Cardio,
    Hiit,
    Flexibility,
    FullBody,
    UpperBody,
    LowerBody,
    Core,
    Custom,
}

impl WorkoutType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkoutType::Strength => "strength",
            WorkoutType::Cardio => "cardio",
            WorkoutType::Hiit => "hiit",
            WorkoutType::Flexibility => "flexibility",
            WorkoutType::FullBody => "full_body",
            WorkoutType::UpperBody => "upper_body",
            WorkoutType::LowerBody => "lower_body",
            WorkoutType::Core => "core",
            WorkoutType::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "strength" => Some(WorkoutType::Strength),
            "cardio" => Some(WorkoutType::Cardio),
            "hiit" => Some(WorkoutType::Hiit),
            "flexibility" => Some(WorkoutType::Flexibility),
            "full_body" => Some(WorkoutType::FullBody),
            "upper_body" => Some(WorkoutType::UpperBody),
            "lower_body" => Some(WorkoutType::LowerBody),
            "core" => Some(WorkoutType::Core),
            "custom" => Some(WorkoutType::Custom),
            _ => None,
        }
    }
}

/// Workout exercise junction model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkoutExercise {
    pub id: Uuid,
    pub workout_id: Uuid,
    pub exercise_id: Uuid,
    pub order_index: i32,
    pub sets: Option<i32>,
    pub reps: Option<i32>,
    pub duration_seconds: Option<i32>,
    pub rest_seconds: Option<i32>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Workout exercise with exercise details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkoutExerciseWithDetails {
    #[serde(flatten)]
    pub workout_exercise: WorkoutExercise,
    pub exercise: Exercise,
}

/// Workout plan with full details (exercises)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkoutPlanWithDetails {
    #[serde(flatten)]
    pub workout: WorkoutPlan,
    pub exercises: Vec<WorkoutExerciseWithDetails>,
}

// ============================================================================
// TRAINING PROGRAM MODELS
// ============================================================================

/// Training program model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TrainingProgram {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub is_template: bool,
    pub goal: Option<String>,
    pub weeks: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fitness goal enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitnessGoal {
    WeightLoss,
    MuscleGain,
    Endurance,
    Strength,
    Flexibility,
    GeneralFitness,
    Athletic,
    Rehabilitation,
}

impl FitnessGoal {
    pub fn as_str(&self) -> &'static str {
        match self {
            FitnessGoal::WeightLoss => "weight_loss",
            FitnessGoal::MuscleGain => "muscle_gain",
            FitnessGoal::Endurance => "endurance",
            FitnessGoal::Strength => "strength",
            FitnessGoal::Flexibility => "flexibility",
            FitnessGoal::GeneralFitness => "general_fitness",
            FitnessGoal::Athletic => "athletic",
            FitnessGoal::Rehabilitation => "rehabilitation",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "weight_loss" => Some(FitnessGoal::WeightLoss),
            "muscle_gain" => Some(FitnessGoal::MuscleGain),
            "endurance" => Some(FitnessGoal::Endurance),
            "strength" => Some(FitnessGoal::Strength),
            "flexibility" => Some(FitnessGoal::Flexibility),
            "general_fitness" => Some(FitnessGoal::GeneralFitness),
            "athletic" => Some(FitnessGoal::Athletic),
            "rehabilitation" => Some(FitnessGoal::Rehabilitation),
            _ => None,
        }
    }
}

/// Program entry model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProgramEntry {
    pub id: Uuid,
    pub program_id: Uuid,
    pub workout_id: Uuid,
    pub day_of_week: Option<i32>,
    pub week_number: Option<i32>,
    pub date: Option<NaiveDate>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Program entry with workout details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramEntryWithWorkout {
    #[serde(flatten)]
    pub entry: ProgramEntry,
    pub workout: WorkoutPlan,
}

/// Training program with all entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingProgramWithEntries {
    #[serde(flatten)]
    pub program: TrainingProgram,
    pub entries: Vec<ProgramEntryWithWorkout>,
}

// ============================================================================
// FITNESS PROFILE MODELS
// ============================================================================

/// Fitness profile model (linked to family_member)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FitnessProfile {
    pub id: Uuid,
    pub family_member_id: Uuid,
    pub current_weight_kg: Option<BigDecimal>,
    pub target_weight_kg: Option<BigDecimal>,
    pub height_cm: Option<BigDecimal>,
    pub fitness_level: String,
    pub goals: Option<serde_json::Value>,       // JSONB array
    pub restrictions: Option<serde_json::Value>, // JSONB array
    pub activity_level: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Activity level enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityLevel {
    Sedentary,
    Light,
    Moderate,
    Active,
    VeryActive,
}

impl ActivityLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActivityLevel::Sedentary => "sedentary",
            ActivityLevel::Light => "light",
            ActivityLevel::Moderate => "moderate",
            ActivityLevel::Active => "active",
            ActivityLevel::VeryActive => "very_active",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sedentary" => Some(ActivityLevel::Sedentary),
            "light" => Some(ActivityLevel::Light),
            "moderate" => Some(ActivityLevel::Moderate),
            "active" => Some(ActivityLevel::Active),
            "very_active" => Some(ActivityLevel::VeryActive),
            _ => None,
        }
    }

    /// Get the activity multiplier for TDEE calculation
    pub fn multiplier(&self) -> f64 {
        match self {
            ActivityLevel::Sedentary => 1.2,
            ActivityLevel::Light => 1.375,
            ActivityLevel::Moderate => 1.55,
            ActivityLevel::Active => 1.725,
            ActivityLevel::VeryActive => 1.9,
        }
    }
}

/// Fitness profile with family member name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitnessProfileWithMember {
    #[serde(flatten)]
    pub profile: FitnessProfile,
    pub family_member_name: String,
}

// ============================================================================
// PROGRESS TRACKING MODELS
// ============================================================================

/// Body measurements model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BodyMeasurement {
    pub id: Uuid,
    pub fitness_profile_id: Uuid,
    pub measured_at: DateTime<Utc>,
    pub weight_kg: Option<BigDecimal>,
    pub body_fat_percentage: Option<BigDecimal>,
    pub waist_cm: Option<BigDecimal>,
    pub chest_cm: Option<BigDecimal>,
    pub hips_cm: Option<BigDecimal>,
    pub left_arm_cm: Option<BigDecimal>,
    pub right_arm_cm: Option<BigDecimal>,
    pub left_thigh_cm: Option<BigDecimal>,
    pub right_thigh_cm: Option<BigDecimal>,
    pub neck_cm: Option<BigDecimal>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Workout log model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkoutLog {
    pub id: Uuid,
    pub fitness_profile_id: Uuid,
    pub workout_id: Option<Uuid>,
    pub workout_name: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_minutes: Option<i32>,
    pub calories_burned: Option<i32>,
    pub notes: Option<String>,
    pub rating: Option<i32>,
    pub perceived_difficulty: Option<i32>,
    pub created_at: DateTime<Utc>,
}

/// Exercise log model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ExerciseLog {
    pub id: Uuid,
    pub workout_log_id: Uuid,
    pub exercise_id: Option<Uuid>,
    pub exercise_name: Option<String>,
    pub order_index: i32,
    pub sets_completed: Option<i32>,
    pub reps_per_set: Option<serde_json::Value>, // JSONB array [12, 10, 8]
    pub weight_kg: Option<BigDecimal>,
    pub duration_seconds: Option<i32>,
    pub distance_meters: Option<BigDecimal>,
    pub notes: Option<String>,
    pub is_personal_record: Option<bool>,
    pub created_at: DateTime<Utc>,
}

/// Workout log with exercise logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkoutLogWithExercises {
    #[serde(flatten)]
    pub workout_log: WorkoutLog,
    pub exercise_logs: Vec<ExerciseLog>,
}

/// Personal record model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PersonalRecord {
    pub id: Uuid,
    pub fitness_profile_id: Uuid,
    pub exercise_id: Uuid,
    pub record_type: String,
    pub value: BigDecimal,
    pub achieved_at: DateTime<Utc>,
    pub workout_log_id: Option<Uuid>,
    pub exercise_log_id: Option<Uuid>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Personal record type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordType {
    MaxWeight,
    MaxReps,
    MaxDuration,
    FastestTime,
    LongestDistance,
}

impl RecordType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecordType::MaxWeight => "max_weight",
            RecordType::MaxReps => "max_reps",
            RecordType::MaxDuration => "max_duration",
            RecordType::FastestTime => "fastest_time",
            RecordType::LongestDistance => "longest_distance",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "max_weight" => Some(RecordType::MaxWeight),
            "max_reps" => Some(RecordType::MaxReps),
            "max_duration" => Some(RecordType::MaxDuration),
            "fastest_time" => Some(RecordType::FastestTime),
            "longest_distance" => Some(RecordType::LongestDistance),
            _ => None,
        }
    }
}

/// Personal record with exercise name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalRecordWithExercise {
    #[serde(flatten)]
    pub record: PersonalRecord,
    pub exercise_name: String,
}

// ============================================================================
// PROGRESS SUMMARY MODELS
// ============================================================================

/// Progress summary for a fitness profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressSummary {
    pub fitness_profile_id: Uuid,
    pub total_workouts: i64,
    pub total_duration_minutes: i64,
    pub total_calories_burned: i64,
    pub workouts_this_week: i64,
    pub workouts_this_month: i64,
    pub current_streak_days: i32,
    pub longest_streak_days: i32,
    pub personal_records_count: i64,
    pub weight_change_kg: Option<BigDecimal>,
    pub recent_measurements: Vec<BodyMeasurement>,
    pub recent_prs: Vec<PersonalRecordWithExercise>,
}

/// Caloric needs calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaloricNeeds {
    pub bmr: f64,           // Basal Metabolic Rate
    pub tdee: f64,          // Total Daily Energy Expenditure
    pub weight_loss: f64,   // Calories for weight loss (-500)
    pub maintenance: f64,   // Maintenance calories
    pub weight_gain: f64,   // Calories for weight gain (+500)
    pub protein_g: f64,     // Recommended protein intake
    pub activity_level: String,
}

// ============================================================================
// REQUEST MODELS
// ============================================================================

/// Create exercise request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateExerciseRequest {
    pub name: String,
    pub description: Option<String>,
    pub muscle_groups: Option<Vec<String>>,
    pub equipment: Option<Vec<String>>,
    pub exercise_type: Option<String>,
    pub difficulty_level: Option<String>,
    pub instructions: Option<String>,
    pub video_url: Option<String>,
    pub calories_per_minute: Option<f64>,
}

/// Update exercise request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateExerciseRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub muscle_groups: Option<Vec<String>>,
    pub equipment: Option<Vec<String>>,
    pub exercise_type: Option<String>,
    pub difficulty_level: Option<String>,
    pub instructions: Option<String>,
    pub video_url: Option<String>,
    pub calories_per_minute: Option<f64>,
}

/// Create workout plan request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkoutPlanRequest {
    pub name: String,
    pub description: Option<String>,
    pub workout_type: Option<String>,
    pub difficulty_level: Option<String>,
    pub estimated_duration_minutes: Option<i32>,
    pub exercises: Option<Vec<WorkoutExerciseInput>>,
}

/// Workout exercise input for creating/adding exercises
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkoutExerciseInput {
    pub exercise_id: String,
    pub order_index: Option<i32>,
    pub sets: Option<i32>,
    pub reps: Option<i32>,
    pub duration_seconds: Option<i32>,
    pub rest_seconds: Option<i32>,
    pub notes: Option<String>,
}

/// Update workout plan request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWorkoutPlanRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub workout_type: Option<String>,
    pub difficulty_level: Option<String>,
    pub estimated_duration_minutes: Option<i32>,
}

/// Create training program request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTrainingProgramRequest {
    pub name: String,
    pub description: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub is_template: bool,
    pub goal: Option<String>,
    pub weeks: Option<i32>,
}

/// Update training program request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTrainingProgramRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub goal: Option<String>,
    pub weeks: Option<i32>,
}

/// Create fitness profile request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFitnessProfileRequest {
    pub family_member_id: String,
    pub current_weight_kg: Option<f64>,
    pub target_weight_kg: Option<f64>,
    pub height_cm: Option<f64>,
    pub fitness_level: Option<String>,
    pub goals: Option<Vec<String>>,
    pub restrictions: Option<Vec<String>>,
    pub activity_level: Option<String>,
}

/// Update fitness profile request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFitnessProfileRequest {
    pub current_weight_kg: Option<f64>,
    pub target_weight_kg: Option<f64>,
    pub height_cm: Option<f64>,
    pub fitness_level: Option<String>,
    pub goals: Option<Vec<String>>,
    pub restrictions: Option<Vec<String>>,
    pub activity_level: Option<String>,
}

/// Log body measurement request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogBodyMeasurementRequest {
    pub fitness_profile_id: String,
    pub weight_kg: Option<f64>,
    pub body_fat_percentage: Option<f64>,
    pub waist_cm: Option<f64>,
    pub chest_cm: Option<f64>,
    pub hips_cm: Option<f64>,
    pub left_arm_cm: Option<f64>,
    pub right_arm_cm: Option<f64>,
    pub left_thigh_cm: Option<f64>,
    pub right_thigh_cm: Option<f64>,
    pub neck_cm: Option<f64>,
    pub notes: Option<String>,
}

/// Log workout request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogWorkoutRequest {
    pub fitness_profile_id: String,
    pub workout_id: Option<String>,
    pub workout_name: Option<String>,
    pub duration_minutes: Option<i32>,
    pub calories_burned: Option<i32>,
    pub notes: Option<String>,
    pub rating: Option<i32>,
    pub perceived_difficulty: Option<i32>,
    pub exercise_logs: Option<Vec<ExerciseLogInput>>,
}

/// Exercise log input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExerciseLogInput {
    pub exercise_id: Option<String>,
    pub exercise_name: Option<String>,
    pub order_index: Option<i32>,
    pub sets_completed: Option<i32>,
    pub reps_per_set: Option<Vec<i32>>,
    pub weight_kg: Option<f64>,
    pub duration_seconds: Option<i32>,
    pub distance_meters: Option<f64>,
    pub notes: Option<String>,
    pub is_personal_record: Option<bool>,
}

