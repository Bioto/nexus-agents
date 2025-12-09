//! Service layer for fitness database operations.
//!
//! This module provides all database operations for the fitness/personal trainer
//! system, including CRUD operations for exercises, workouts, training programs,
//! fitness profiles, and progress tracking.

use crate::error::{Result, ToolboxError};
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{types::BigDecimal, PgPool};
use uuid::Uuid;

use super::models::*;

/// Service layer for fitness database operations
pub struct FitnessService;

impl FitnessService {
    // ========================================================================
    // EXERCISE OPERATIONS
    // ========================================================================

    /// Create a new exercise
    pub async fn create_exercise(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
        muscle_groups: Option<serde_json::Value>,
        equipment: Option<serde_json::Value>,
        exercise_type: &str,
        difficulty_level: &str,
        instructions: Option<&str>,
        video_url: Option<&str>,
        calories_per_minute: Option<BigDecimal>,
    ) -> Result<Exercise> {
        let exercise = sqlx::query_as::<_, Exercise>(
            r#"
            INSERT INTO exercises (name, description, muscle_groups, equipment, exercise_type, difficulty_level, instructions, video_url, calories_per_minute)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id, name, description, muscle_groups, equipment, exercise_type, difficulty_level, instructions, video_url, calories_per_minute, created_at, updated_at
            "#,
        )
        .bind(name)
        .bind(description)
        .bind(&muscle_groups)
        .bind(&equipment)
        .bind(exercise_type)
        .bind(difficulty_level)
        .bind(instructions)
        .bind(video_url)
        .bind(&calories_per_minute)
        .fetch_one(pool)
        .await?;

        Ok(exercise)
    }

    /// Get exercise by ID
    pub async fn get_exercise(pool: &PgPool, id: Uuid) -> Result<Exercise> {
        let exercise = sqlx::query_as::<_, Exercise>(
            r#"
            SELECT id, name, description, muscle_groups, equipment, exercise_type, difficulty_level, instructions, video_url, calories_per_minute, created_at, updated_at
            FROM exercises
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Exercise with id {} not found", id)))?;

        Ok(exercise)
    }

    /// Find or create exercise by name
    pub async fn find_or_create_exercise(pool: &PgPool, name: &str) -> Result<Exercise> {
        let exercise = sqlx::query_as::<_, Exercise>(
            r#"
            SELECT id, name, description, muscle_groups, equipment, exercise_type, difficulty_level, instructions, video_url, calories_per_minute, created_at, updated_at
            FROM exercises
            WHERE LOWER(name) = LOWER($1)
            LIMIT 1
            "#,
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        if let Some(exercise) = exercise {
            Ok(exercise)
        } else {
            Self::create_exercise(
                pool,
                name,
                None,
                None,
                None,
                "strength",
                "intermediate",
                None,
                None,
                None,
            )
            .await
        }
    }

    /// List exercises with optional filters
    pub async fn list_exercises(
        pool: &PgPool,
        search: Option<&str>,
        exercise_type: Option<&str>,
        difficulty: Option<&str>,
        muscle_group: Option<&str>,
        equipment: Option<&str>,
    ) -> Result<Vec<Exercise>> {
        // Build dynamic query based on filters
        let mut query = String::from(
            r#"
            SELECT id, name, description, muscle_groups, equipment, exercise_type, difficulty_level, instructions, video_url, calories_per_minute, created_at, updated_at
            FROM exercises
            WHERE 1=1
            "#,
        );

        let mut conditions = Vec::new();
        let mut bind_idx = 1;

        if search.is_some() {
            conditions.push(format!(
                "(name ILIKE ${} OR description ILIKE ${})",
                bind_idx, bind_idx
            ));
            bind_idx += 1;
        }
        if exercise_type.is_some() {
            conditions.push(format!("exercise_type = ${}", bind_idx));
            bind_idx += 1;
        }
        if difficulty.is_some() {
            conditions.push(format!("difficulty_level = ${}", bind_idx));
            bind_idx += 1;
        }
        if muscle_group.is_some() {
            conditions.push(format!("muscle_groups @> ${}::jsonb", bind_idx));
            bind_idx += 1;
        }
        if equipment.is_some() {
            conditions.push(format!("equipment @> ${}::jsonb", bind_idx));
        }

        if !conditions.is_empty() {
            query.push_str(" AND ");
            query.push_str(&conditions.join(" AND "));
        }
        query.push_str(" ORDER BY name");

        // Use dynamic query building
        let mut q = sqlx::query_as::<_, Exercise>(&query);

        if let Some(s) = search {
            q = q.bind(format!("%{}%", s));
        }
        if let Some(t) = exercise_type {
            q = q.bind(t);
        }
        if let Some(d) = difficulty {
            q = q.bind(d);
        }
        if let Some(mg) = muscle_group {
            q = q.bind(serde_json::json!([mg]));
        }
        if let Some(eq) = equipment {
            q = q.bind(serde_json::json!([eq]));
        }

        let exercises = q.fetch_all(pool).await?;
        Ok(exercises)
    }

    /// Update exercise
    pub async fn update_exercise(
        pool: &PgPool,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        muscle_groups: Option<serde_json::Value>,
        equipment: Option<serde_json::Value>,
        exercise_type: Option<&str>,
        difficulty_level: Option<&str>,
        instructions: Option<&str>,
        video_url: Option<&str>,
        calories_per_minute: Option<BigDecimal>,
    ) -> Result<Exercise> {
        let exercise = sqlx::query_as::<_, Exercise>(
            r#"
            UPDATE exercises
            SET
                name = COALESCE($1, name),
                description = COALESCE($2, description),
                muscle_groups = COALESCE($3, muscle_groups),
                equipment = COALESCE($4, equipment),
                exercise_type = COALESCE($5, exercise_type),
                difficulty_level = COALESCE($6, difficulty_level),
                instructions = COALESCE($7, instructions),
                video_url = COALESCE($8, video_url),
                calories_per_minute = COALESCE($9, calories_per_minute)
            WHERE id = $10
            RETURNING id, name, description, muscle_groups, equipment, exercise_type, difficulty_level, instructions, video_url, calories_per_minute, created_at, updated_at
            "#,
        )
        .bind(name)
        .bind(description)
        .bind(&muscle_groups)
        .bind(&equipment)
        .bind(exercise_type)
        .bind(difficulty_level)
        .bind(instructions)
        .bind(video_url)
        .bind(&calories_per_minute)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Exercise with id {} not found", id)))?;

        Ok(exercise)
    }

    /// Delete exercise
    pub async fn delete_exercise(pool: &PgPool, id: Uuid) -> Result<()> {
        let result = sqlx::query("DELETE FROM exercises WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Exercise with id {} not found",
                id
            )));
        }

        Ok(())
    }

    // ========================================================================
    // WORKOUT PLAN OPERATIONS
    // ========================================================================

    /// Create a new workout plan with exercises
    pub async fn create_workout_plan(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
        workout_type: &str,
        difficulty_level: &str,
        estimated_duration_minutes: Option<i32>,
        exercises: Vec<(
            Uuid,
            i32,
            Option<i32>,
            Option<i32>,
            Option<i32>,
            Option<i32>,
            Option<String>,
        )>,
    ) -> Result<WorkoutPlan> {
        let mut tx = pool.begin().await?;

        // Create workout plan
        let workout = sqlx::query_as::<_, WorkoutPlan>(
            r#"
            INSERT INTO workout_plans (name, description, workout_type, difficulty_level, estimated_duration_minutes)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
            "#,
        )
        .bind(name)
        .bind(description)
        .bind(workout_type)
        .bind(difficulty_level)
        .bind(estimated_duration_minutes)
        .fetch_one(&mut *tx)
        .await?;

        // Add exercises
        for (exercise_id, order_index, sets, reps, duration_seconds, rest_seconds, notes) in
            exercises
        {
            sqlx::query(
                r#"
                INSERT INTO workout_exercises (workout_id, exercise_id, order_index, sets, reps, duration_seconds, rest_seconds, notes)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                "#,
            )
            .bind(workout.id)
            .bind(exercise_id)
            .bind(order_index)
            .bind(sets)
            .bind(reps)
            .bind(duration_seconds)
            .bind(rest_seconds)
            .bind(&notes)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(workout)
    }

    /// Get workout plan by ID
    pub async fn get_workout_plan(pool: &PgPool, id: Uuid) -> Result<WorkoutPlan> {
        let workout = sqlx::query_as::<_, WorkoutPlan>(
            r#"
            SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
            FROM workout_plans
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Workout plan with id {} not found", id)))?;

        Ok(workout)
    }

    /// Get workout plan with full details (exercises)
    pub async fn get_workout_plan_with_details(
        pool: &PgPool,
        id: Uuid,
    ) -> Result<WorkoutPlanWithDetails> {
        let workout = Self::get_workout_plan(pool, id).await?;

        // Get workout exercises
        let workout_exercises = sqlx::query_as::<_, WorkoutExercise>(
            r#"
            SELECT id, workout_id, exercise_id, order_index, sets, reps, duration_seconds, rest_seconds, notes, created_at
            FROM workout_exercises
            WHERE workout_id = $1
            ORDER BY order_index
            "#,
        )
        .bind(id)
        .fetch_all(pool)
        .await?;

        let mut exercises_with_details = Vec::new();
        for we in workout_exercises {
            let exercise = Self::get_exercise(pool, we.exercise_id).await?;
            exercises_with_details.push(WorkoutExerciseWithDetails {
                workout_exercise: we,
                exercise,
            });
        }

        Ok(WorkoutPlanWithDetails {
            workout,
            exercises: exercises_with_details,
        })
    }

    /// List workout plans with optional filters
    pub async fn list_workout_plans(
        pool: &PgPool,
        search: Option<&str>,
        workout_type: Option<&str>,
        difficulty: Option<&str>,
    ) -> Result<Vec<WorkoutPlan>> {
        let workouts = if let Some(search_term) = search {
            if let Some(wt) = workout_type {
                if let Some(d) = difficulty {
                    sqlx::query_as::<_, WorkoutPlan>(
                        r#"
                        SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
                        FROM workout_plans
                        WHERE (name ILIKE $1 OR description ILIKE $1)
                        AND workout_type = $2 AND difficulty_level = $3
                        ORDER BY name
                        "#,
                    )
                    .bind(format!("%{}%", search_term))
                    .bind(wt)
                    .bind(d)
                    .fetch_all(pool)
                    .await?
                } else {
                    sqlx::query_as::<_, WorkoutPlan>(
                        r#"
                        SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
                        FROM workout_plans
                        WHERE (name ILIKE $1 OR description ILIKE $1) AND workout_type = $2
                        ORDER BY name
                        "#,
                    )
                    .bind(format!("%{}%", search_term))
                    .bind(wt)
                    .fetch_all(pool)
                    .await?
                }
            } else if let Some(d) = difficulty {
                sqlx::query_as::<_, WorkoutPlan>(
                    r#"
                    SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
                    FROM workout_plans
                    WHERE (name ILIKE $1 OR description ILIKE $1) AND difficulty_level = $2
                    ORDER BY name
                    "#,
                )
                .bind(format!("%{}%", search_term))
                .bind(d)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as::<_, WorkoutPlan>(
                    r#"
                    SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
                    FROM workout_plans
                    WHERE name ILIKE $1 OR description ILIKE $1
                    ORDER BY name
                    "#,
                )
                .bind(format!("%{}%", search_term))
                .fetch_all(pool)
                .await?
            }
        } else if let Some(wt) = workout_type {
            if let Some(d) = difficulty {
                sqlx::query_as::<_, WorkoutPlan>(
                    r#"
                    SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
                    FROM workout_plans
                    WHERE workout_type = $1 AND difficulty_level = $2
                    ORDER BY name
                    "#,
                )
                .bind(wt)
                .bind(d)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as::<_, WorkoutPlan>(
                    r#"
                    SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
                    FROM workout_plans
                    WHERE workout_type = $1
                    ORDER BY name
                    "#,
                )
                .bind(wt)
                .fetch_all(pool)
                .await?
            }
        } else if let Some(d) = difficulty {
            sqlx::query_as::<_, WorkoutPlan>(
                r#"
                SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
                FROM workout_plans
                WHERE difficulty_level = $1
                ORDER BY name
                "#,
            )
            .bind(d)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, WorkoutPlan>(
                r#"
                SELECT id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
                FROM workout_plans
                ORDER BY name
                "#,
            )
            .fetch_all(pool)
            .await?
        };

        Ok(workouts)
    }

    /// Update workout plan
    pub async fn update_workout_plan(
        pool: &PgPool,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        workout_type: Option<&str>,
        difficulty_level: Option<&str>,
        estimated_duration_minutes: Option<i32>,
    ) -> Result<WorkoutPlan> {
        let workout = sqlx::query_as::<_, WorkoutPlan>(
            r#"
            UPDATE workout_plans
            SET
                name = COALESCE($1, name),
                description = COALESCE($2, description),
                workout_type = COALESCE($3, workout_type),
                difficulty_level = COALESCE($4, difficulty_level),
                estimated_duration_minutes = COALESCE($5, estimated_duration_minutes)
            WHERE id = $6
            RETURNING id, name, description, workout_type, difficulty_level, estimated_duration_minutes, created_at, updated_at
            "#,
        )
        .bind(name)
        .bind(description)
        .bind(workout_type)
        .bind(difficulty_level)
        .bind(estimated_duration_minutes)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Workout plan with id {} not found", id)))?;

        Ok(workout)
    }

    /// Delete workout plan
    pub async fn delete_workout_plan(pool: &PgPool, id: Uuid) -> Result<()> {
        let result = sqlx::query("DELETE FROM workout_plans WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Workout plan with id {} not found",
                id
            )));
        }

        Ok(())
    }

    /// Add exercise to workout plan
    pub async fn add_workout_exercise(
        pool: &PgPool,
        workout_id: Uuid,
        exercise_id: Uuid,
        order_index: i32,
        sets: Option<i32>,
        reps: Option<i32>,
        duration_seconds: Option<i32>,
        rest_seconds: Option<i32>,
        notes: Option<&str>,
    ) -> Result<WorkoutExercise> {
        // Verify workout and exercise exist
        Self::get_workout_plan(pool, workout_id).await?;
        Self::get_exercise(pool, exercise_id).await?;

        let workout_exercise = sqlx::query_as::<_, WorkoutExercise>(
            r#"
            INSERT INTO workout_exercises (workout_id, exercise_id, order_index, sets, reps, duration_seconds, rest_seconds, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, workout_id, exercise_id, order_index, sets, reps, duration_seconds, rest_seconds, notes, created_at
            "#,
        )
        .bind(workout_id)
        .bind(exercise_id)
        .bind(order_index)
        .bind(sets)
        .bind(reps)
        .bind(duration_seconds)
        .bind(rest_seconds)
        .bind(notes)
        .fetch_one(pool)
        .await?;

        Ok(workout_exercise)
    }

    /// Remove exercise from workout plan
    pub async fn remove_workout_exercise(
        pool: &PgPool,
        workout_id: Uuid,
        exercise_id: Uuid,
    ) -> Result<()> {
        let result =
            sqlx::query("DELETE FROM workout_exercises WHERE workout_id = $1 AND exercise_id = $2")
                .bind(workout_id)
                .bind(exercise_id)
                .execute(pool)
                .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Exercise {} not found in workout {}",
                exercise_id, workout_id
            )));
        }

        Ok(())
    }

    // ========================================================================
    // TRAINING PROGRAM OPERATIONS
    // ========================================================================

    /// Create a new training program
    pub async fn create_training_program(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        is_template: bool,
        goal: Option<&str>,
        weeks: Option<i32>,
    ) -> Result<TrainingProgram> {
        // Validate dates
        if let (Some(start), Some(end)) = (start_date, end_date) {
            if start > end {
                return Err(ToolboxError::Validation(
                    "start_date must be before end_date".to_string(),
                ));
            }
        }

        let program = sqlx::query_as::<_, TrainingProgram>(
            r#"
            INSERT INTO training_programs (name, description, start_date, end_date, is_template, goal, weeks)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
            "#,
        )
        .bind(name)
        .bind(description)
        .bind(start_date)
        .bind(end_date)
        .bind(is_template)
        .bind(goal)
        .bind(weeks)
        .fetch_one(pool)
        .await?;

        Ok(program)
    }

    /// Get training program by ID
    pub async fn get_training_program(pool: &PgPool, id: Uuid) -> Result<TrainingProgram> {
        let program = sqlx::query_as::<_, TrainingProgram>(
            r#"
            SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
            FROM training_programs
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Training program with id {} not found", id)))?;

        Ok(program)
    }

    /// Get training program with entries
    pub async fn get_training_program_with_entries(
        pool: &PgPool,
        id: Uuid,
    ) -> Result<TrainingProgramWithEntries> {
        let program = Self::get_training_program(pool, id).await?;

        let entries = sqlx::query_as::<_, ProgramEntry>(
            r#"
            SELECT id, program_id, workout_id, day_of_week, week_number, date, notes, created_at
            FROM program_entries
            WHERE program_id = $1
            ORDER BY week_number NULLS FIRST, day_of_week NULLS FIRST, date NULLS FIRST
            "#,
        )
        .bind(id)
        .fetch_all(pool)
        .await?;

        let mut entries_with_workouts = Vec::new();
        for entry in entries {
            let workout = Self::get_workout_plan(pool, entry.workout_id).await?;
            entries_with_workouts.push(ProgramEntryWithWorkout { entry, workout });
        }

        Ok(TrainingProgramWithEntries {
            program,
            entries: entries_with_workouts,
        })
    }

    /// List training programs with optional filters
    pub async fn list_training_programs(
        pool: &PgPool,
        search: Option<&str>,
        is_template: Option<bool>,
        goal: Option<&str>,
    ) -> Result<Vec<TrainingProgram>> {
        let programs = if let Some(search_term) = search {
            if let Some(template) = is_template {
                if let Some(g) = goal {
                    sqlx::query_as::<_, TrainingProgram>(
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
                        FROM training_programs
                        WHERE (name ILIKE $1 OR description ILIKE $1)
                        AND is_template = $2 AND goal = $3
                        ORDER BY name
                        "#,
                    )
                    .bind(format!("%{}%", search_term))
                    .bind(template)
                    .bind(g)
                    .fetch_all(pool)
                    .await?
                } else {
                    sqlx::query_as::<_, TrainingProgram>(
                        r#"
                        SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
                        FROM training_programs
                        WHERE (name ILIKE $1 OR description ILIKE $1) AND is_template = $2
                        ORDER BY name
                        "#,
                    )
                    .bind(format!("%{}%", search_term))
                    .bind(template)
                    .fetch_all(pool)
                    .await?
                }
            } else if let Some(g) = goal {
                sqlx::query_as::<_, TrainingProgram>(
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
                    FROM training_programs
                    WHERE (name ILIKE $1 OR description ILIKE $1) AND goal = $2
                    ORDER BY name
                    "#,
                )
                .bind(format!("%{}%", search_term))
                .bind(g)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as::<_, TrainingProgram>(
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
                    FROM training_programs
                    WHERE name ILIKE $1 OR description ILIKE $1
                    ORDER BY name
                    "#,
                )
                .bind(format!("%{}%", search_term))
                .fetch_all(pool)
                .await?
            }
        } else if let Some(template) = is_template {
            if let Some(g) = goal {
                sqlx::query_as::<_, TrainingProgram>(
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
                    FROM training_programs
                    WHERE is_template = $1 AND goal = $2
                    ORDER BY name
                    "#,
                )
                .bind(template)
                .bind(g)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as::<_, TrainingProgram>(
                    r#"
                    SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
                    FROM training_programs
                    WHERE is_template = $1
                    ORDER BY name
                    "#,
                )
                .bind(template)
                .fetch_all(pool)
                .await?
            }
        } else if let Some(g) = goal {
            sqlx::query_as::<_, TrainingProgram>(
                r#"
                SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
                FROM training_programs
                WHERE goal = $1
                ORDER BY name
                "#,
            )
            .bind(g)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, TrainingProgram>(
                r#"
                SELECT id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
                FROM training_programs
                ORDER BY name
                "#,
            )
            .fetch_all(pool)
            .await?
        };

        Ok(programs)
    }

    /// Update training program
    pub async fn update_training_program(
        pool: &PgPool,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        goal: Option<&str>,
        weeks: Option<i32>,
    ) -> Result<TrainingProgram> {
        let program = sqlx::query_as::<_, TrainingProgram>(
            r#"
            UPDATE training_programs
            SET
                name = COALESCE($1, name),
                description = COALESCE($2, description),
                start_date = COALESCE($3, start_date),
                end_date = COALESCE($4, end_date),
                goal = COALESCE($5, goal),
                weeks = COALESCE($6, weeks)
            WHERE id = $7
            RETURNING id, name, description, start_date, end_date, is_template, goal, weeks, created_at, updated_at
            "#,
        )
        .bind(name)
        .bind(description)
        .bind(start_date)
        .bind(end_date)
        .bind(goal)
        .bind(weeks)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Training program with id {} not found", id)))?;

        Ok(program)
    }

    /// Delete training program
    pub async fn delete_training_program(pool: &PgPool, id: Uuid) -> Result<()> {
        let result = sqlx::query("DELETE FROM training_programs WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Training program with id {} not found",
                id
            )));
        }

        Ok(())
    }

    /// Add entry to training program
    pub async fn add_program_entry(
        pool: &PgPool,
        program_id: Uuid,
        workout_id: Uuid,
        day_of_week: Option<i32>,
        week_number: Option<i32>,
        date: Option<NaiveDate>,
        notes: Option<&str>,
    ) -> Result<ProgramEntry> {
        // Validate: must have either day_of_week+week_number (for templates) or date
        if day_of_week.is_none() && date.is_none() {
            return Err(ToolboxError::Validation(
                "Must specify either day_of_week or date".to_string(),
            ));
        }

        if let Some(dow) = day_of_week {
            if dow < 0 || dow > 6 {
                return Err(ToolboxError::Validation(
                    "day_of_week must be between 0 (Monday) and 6 (Sunday)".to_string(),
                ));
            }
        }

        // Verify program and workout exist
        Self::get_training_program(pool, program_id).await?;
        Self::get_workout_plan(pool, workout_id).await?;

        let entry = sqlx::query_as::<_, ProgramEntry>(
            r#"
            INSERT INTO program_entries (program_id, workout_id, day_of_week, week_number, date, notes)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, program_id, workout_id, day_of_week, week_number, date, notes, created_at
            "#,
        )
        .bind(program_id)
        .bind(workout_id)
        .bind(day_of_week)
        .bind(week_number)
        .bind(date)
        .bind(notes)
        .fetch_one(pool)
        .await?;

        Ok(entry)
    }

    /// Remove entry from training program
    pub async fn remove_program_entry(pool: &PgPool, entry_id: Uuid) -> Result<()> {
        let result = sqlx::query("DELETE FROM program_entries WHERE id = $1")
            .bind(entry_id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Program entry with id {} not found",
                entry_id
            )));
        }

        Ok(())
    }

    // ========================================================================
    // FITNESS PROFILE OPERATIONS
    // ========================================================================

    /// Create a new fitness profile
    pub async fn create_fitness_profile(
        pool: &PgPool,
        family_member_id: Uuid,
        current_weight_kg: Option<BigDecimal>,
        target_weight_kg: Option<BigDecimal>,
        height_cm: Option<BigDecimal>,
        fitness_level: &str,
        goals: Option<serde_json::Value>,
        restrictions: Option<serde_json::Value>,
        activity_level: Option<&str>,
    ) -> Result<FitnessProfile> {
        let profile = sqlx::query_as::<_, FitnessProfile>(
            r#"
            INSERT INTO fitness_profiles (family_member_id, current_weight_kg, target_weight_kg, height_cm, fitness_level, goals, restrictions, activity_level)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, family_member_id, current_weight_kg, target_weight_kg, height_cm, fitness_level, goals, restrictions, activity_level, created_at, updated_at
            "#,
        )
        .bind(family_member_id)
        .bind(&current_weight_kg)
        .bind(&target_weight_kg)
        .bind(&height_cm)
        .bind(fitness_level)
        .bind(&goals)
        .bind(&restrictions)
        .bind(activity_level)
        .fetch_one(pool)
        .await?;

        Ok(profile)
    }

    /// Get fitness profile by ID
    pub async fn get_fitness_profile(pool: &PgPool, id: Uuid) -> Result<FitnessProfile> {
        let profile = sqlx::query_as::<_, FitnessProfile>(
            r#"
            SELECT id, family_member_id, current_weight_kg, target_weight_kg, height_cm, fitness_level, goals, restrictions, activity_level, created_at, updated_at
            FROM fitness_profiles
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Fitness profile with id {} not found", id)))?;

        Ok(profile)
    }

    /// Get fitness profile by family member ID
    pub async fn get_fitness_profile_by_family_member(
        pool: &PgPool,
        family_member_id: Uuid,
    ) -> Result<FitnessProfile> {
        let profile = sqlx::query_as::<_, FitnessProfile>(
            r#"
            SELECT id, family_member_id, current_weight_kg, target_weight_kg, height_cm, fitness_level, goals, restrictions, activity_level, created_at, updated_at
            FROM fitness_profiles
            WHERE family_member_id = $1
            "#,
        )
        .bind(family_member_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            ToolboxError::NotFound(format!(
                "Fitness profile for family member {} not found",
                family_member_id
            ))
        })?;

        Ok(profile)
    }

    /// List all fitness profiles
    pub async fn list_fitness_profiles(pool: &PgPool) -> Result<Vec<FitnessProfileWithMember>> {
        let rows = sqlx::query(
            r#"
            SELECT fp.id, fp.family_member_id, fp.current_weight_kg, fp.target_weight_kg, fp.height_cm, fp.fitness_level, fp.goals, fp.restrictions, fp.activity_level, fp.created_at, fp.updated_at, fm.name as family_member_name
            FROM fitness_profiles fp
            JOIN family_members fm ON fp.family_member_id = fm.id
            ORDER BY fm.name
            "#,
        )
        .fetch_all(pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            use sqlx::Row;
            let profile = FitnessProfile {
                id: row.get("id"),
                family_member_id: row.get("family_member_id"),
                current_weight_kg: row.get("current_weight_kg"),
                target_weight_kg: row.get("target_weight_kg"),
                height_cm: row.get("height_cm"),
                fitness_level: row.get("fitness_level"),
                goals: row.get("goals"),
                restrictions: row.get("restrictions"),
                activity_level: row.get("activity_level"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };
            let family_member_name: String = row.get("family_member_name");
            result.push(FitnessProfileWithMember {
                profile,
                family_member_name,
            });
        }

        Ok(result)
    }

    /// Update fitness profile
    pub async fn update_fitness_profile(
        pool: &PgPool,
        id: Uuid,
        current_weight_kg: Option<BigDecimal>,
        target_weight_kg: Option<BigDecimal>,
        height_cm: Option<BigDecimal>,
        fitness_level: Option<&str>,
        goals: Option<serde_json::Value>,
        restrictions: Option<serde_json::Value>,
        activity_level: Option<&str>,
    ) -> Result<FitnessProfile> {
        let profile = sqlx::query_as::<_, FitnessProfile>(
            r#"
            UPDATE fitness_profiles
            SET
                current_weight_kg = COALESCE($1, current_weight_kg),
                target_weight_kg = COALESCE($2, target_weight_kg),
                height_cm = COALESCE($3, height_cm),
                fitness_level = COALESCE($4, fitness_level),
                goals = COALESCE($5, goals),
                restrictions = COALESCE($6, restrictions),
                activity_level = COALESCE($7, activity_level)
            WHERE id = $8
            RETURNING id, family_member_id, current_weight_kg, target_weight_kg, height_cm, fitness_level, goals, restrictions, activity_level, created_at, updated_at
            "#,
        )
        .bind(&current_weight_kg)
        .bind(&target_weight_kg)
        .bind(&height_cm)
        .bind(fitness_level)
        .bind(&goals)
        .bind(&restrictions)
        .bind(activity_level)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Fitness profile with id {} not found", id)))?;

        Ok(profile)
    }

    /// Delete fitness profile
    pub async fn delete_fitness_profile(pool: &PgPool, id: Uuid) -> Result<()> {
        let result = sqlx::query("DELETE FROM fitness_profiles WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ToolboxError::NotFound(format!(
                "Fitness profile with id {} not found",
                id
            )));
        }

        Ok(())
    }

    // ========================================================================
    // PROGRESS TRACKING OPERATIONS
    // ========================================================================

    /// Log body measurement
    pub async fn log_body_measurement(
        pool: &PgPool,
        fitness_profile_id: Uuid,
        weight_kg: Option<BigDecimal>,
        body_fat_percentage: Option<BigDecimal>,
        waist_cm: Option<BigDecimal>,
        chest_cm: Option<BigDecimal>,
        hips_cm: Option<BigDecimal>,
        left_arm_cm: Option<BigDecimal>,
        right_arm_cm: Option<BigDecimal>,
        left_thigh_cm: Option<BigDecimal>,
        right_thigh_cm: Option<BigDecimal>,
        neck_cm: Option<BigDecimal>,
        notes: Option<&str>,
    ) -> Result<BodyMeasurement> {
        // Verify profile exists
        Self::get_fitness_profile(pool, fitness_profile_id).await?;

        let measurement = sqlx::query_as::<_, BodyMeasurement>(
            r#"
            INSERT INTO body_measurements (fitness_profile_id, weight_kg, body_fat_percentage, waist_cm, chest_cm, hips_cm, left_arm_cm, right_arm_cm, left_thigh_cm, right_thigh_cm, neck_cm, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id, fitness_profile_id, measured_at, weight_kg, body_fat_percentage, waist_cm, chest_cm, hips_cm, left_arm_cm, right_arm_cm, left_thigh_cm, right_thigh_cm, neck_cm, notes, created_at
            "#,
        )
        .bind(fitness_profile_id)
        .bind(&weight_kg)
        .bind(&body_fat_percentage)
        .bind(&waist_cm)
        .bind(&chest_cm)
        .bind(&hips_cm)
        .bind(&left_arm_cm)
        .bind(&right_arm_cm)
        .bind(&left_thigh_cm)
        .bind(&right_thigh_cm)
        .bind(&neck_cm)
        .bind(notes)
        .fetch_one(pool)
        .await?;

        // Update profile's current weight if weight was provided
        if let Some(ref weight) = weight_kg {
            sqlx::query("UPDATE fitness_profiles SET current_weight_kg = $1 WHERE id = $2")
                .bind(weight)
                .bind(fitness_profile_id)
                .execute(pool)
                .await?;
        }

        Ok(measurement)
    }

    /// Get body measurements for a profile
    pub async fn get_body_measurements(
        pool: &PgPool,
        fitness_profile_id: Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<BodyMeasurement>> {
        let limit = limit.unwrap_or(100);
        let measurements = sqlx::query_as::<_, BodyMeasurement>(
            r#"
            SELECT id, fitness_profile_id, measured_at, weight_kg, body_fat_percentage, waist_cm, chest_cm, hips_cm, left_arm_cm, right_arm_cm, left_thigh_cm, right_thigh_cm, neck_cm, notes, created_at
            FROM body_measurements
            WHERE fitness_profile_id = $1
            ORDER BY measured_at DESC
            LIMIT $2
            "#,
        )
        .bind(fitness_profile_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(measurements)
    }

    /// Log a completed workout
    pub async fn log_workout(
        pool: &PgPool,
        fitness_profile_id: Uuid,
        workout_id: Option<Uuid>,
        workout_name: Option<&str>,
        started_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
        duration_minutes: Option<i32>,
        calories_burned: Option<i32>,
        notes: Option<&str>,
        rating: Option<i32>,
        perceived_difficulty: Option<i32>,
    ) -> Result<WorkoutLog> {
        // Verify profile exists
        Self::get_fitness_profile(pool, fitness_profile_id).await?;

        // Get workout name if workout_id provided
        let final_workout_name = if let Some(wid) = workout_id {
            let workout = Self::get_workout_plan(pool, wid).await?;
            Some(workout.name)
        } else {
            workout_name.map(String::from)
        };

        let log = sqlx::query_as::<_, WorkoutLog>(
            r#"
            INSERT INTO workout_logs (fitness_profile_id, workout_id, workout_name, started_at, completed_at, duration_minutes, calories_burned, notes, rating, perceived_difficulty)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, fitness_profile_id, workout_id, workout_name, started_at, completed_at, duration_minutes, calories_burned, notes, rating, perceived_difficulty, created_at
            "#,
        )
        .bind(fitness_profile_id)
        .bind(workout_id)
        .bind(&final_workout_name)
        .bind(started_at)
        .bind(completed_at)
        .bind(duration_minutes)
        .bind(calories_burned)
        .bind(notes)
        .bind(rating)
        .bind(perceived_difficulty)
        .fetch_one(pool)
        .await?;

        Ok(log)
    }

    /// Log exercise performance within a workout
    pub async fn log_exercise(
        pool: &PgPool,
        workout_log_id: Uuid,
        exercise_id: Option<Uuid>,
        exercise_name: Option<&str>,
        order_index: i32,
        sets_completed: Option<i32>,
        reps_per_set: Option<serde_json::Value>,
        weight_kg: Option<BigDecimal>,
        duration_seconds: Option<i32>,
        distance_meters: Option<BigDecimal>,
        notes: Option<&str>,
        is_personal_record: Option<bool>,
    ) -> Result<ExerciseLog> {
        // Get exercise name if exercise_id provided
        let final_exercise_name = if let Some(eid) = exercise_id {
            let exercise = Self::get_exercise(pool, eid).await?;
            Some(exercise.name)
        } else {
            exercise_name.map(String::from)
        };

        let log = sqlx::query_as::<_, ExerciseLog>(
            r#"
            INSERT INTO exercise_logs (workout_log_id, exercise_id, exercise_name, order_index, sets_completed, reps_per_set, weight_kg, duration_seconds, distance_meters, notes, is_personal_record)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id, workout_log_id, exercise_id, exercise_name, order_index, sets_completed, reps_per_set, weight_kg, duration_seconds, distance_meters, notes, is_personal_record, created_at
            "#,
        )
        .bind(workout_log_id)
        .bind(exercise_id)
        .bind(&final_exercise_name)
        .bind(order_index)
        .bind(sets_completed)
        .bind(&reps_per_set)
        .bind(&weight_kg)
        .bind(duration_seconds)
        .bind(&distance_meters)
        .bind(notes)
        .bind(is_personal_record)
        .fetch_one(pool)
        .await?;

        Ok(log)
    }

    /// Get workout logs for a profile
    pub async fn get_workout_logs(
        pool: &PgPool,
        fitness_profile_id: Uuid,
        limit: Option<i64>,
        from_date: Option<DateTime<Utc>>,
        to_date: Option<DateTime<Utc>>,
    ) -> Result<Vec<WorkoutLog>> {
        let limit = limit.unwrap_or(50);

        let logs = if let (Some(from), Some(to)) = (from_date, to_date) {
            sqlx::query_as::<_, WorkoutLog>(
                r#"
                SELECT id, fitness_profile_id, workout_id, workout_name, started_at, completed_at, duration_minutes, calories_burned, notes, rating, perceived_difficulty, created_at
                FROM workout_logs
                WHERE fitness_profile_id = $1 AND started_at >= $2 AND started_at <= $3
                ORDER BY started_at DESC
                LIMIT $4
                "#,
            )
            .bind(fitness_profile_id)
            .bind(from)
            .bind(to)
            .bind(limit)
            .fetch_all(pool)
            .await?
        } else if let Some(from) = from_date {
            sqlx::query_as::<_, WorkoutLog>(
                r#"
                SELECT id, fitness_profile_id, workout_id, workout_name, started_at, completed_at, duration_minutes, calories_burned, notes, rating, perceived_difficulty, created_at
                FROM workout_logs
                WHERE fitness_profile_id = $1 AND started_at >= $2
                ORDER BY started_at DESC
                LIMIT $3
                "#,
            )
            .bind(fitness_profile_id)
            .bind(from)
            .bind(limit)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, WorkoutLog>(
                r#"
                SELECT id, fitness_profile_id, workout_id, workout_name, started_at, completed_at, duration_minutes, calories_burned, notes, rating, perceived_difficulty, created_at
                FROM workout_logs
                WHERE fitness_profile_id = $1
                ORDER BY started_at DESC
                LIMIT $2
                "#,
            )
            .bind(fitness_profile_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        };

        Ok(logs)
    }

    /// Get workout log with exercise logs
    pub async fn get_workout_log_with_exercises(
        pool: &PgPool,
        workout_log_id: Uuid,
    ) -> Result<WorkoutLogWithExercises> {
        let workout_log = sqlx::query_as::<_, WorkoutLog>(
            r#"
            SELECT id, fitness_profile_id, workout_id, workout_name, started_at, completed_at, duration_minutes, calories_burned, notes, rating, perceived_difficulty, created_at
            FROM workout_logs
            WHERE id = $1
            "#,
        )
        .bind(workout_log_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ToolboxError::NotFound(format!("Workout log with id {} not found", workout_log_id)))?;

        let exercise_logs = sqlx::query_as::<_, ExerciseLog>(
            r#"
            SELECT id, workout_log_id, exercise_id, exercise_name, order_index, sets_completed, reps_per_set, weight_kg, duration_seconds, distance_meters, notes, is_personal_record, created_at
            FROM exercise_logs
            WHERE workout_log_id = $1
            ORDER BY order_index
            "#,
        )
        .bind(workout_log_id)
        .fetch_all(pool)
        .await?;

        Ok(WorkoutLogWithExercises {
            workout_log,
            exercise_logs,
        })
    }

    // ========================================================================
    // PERSONAL RECORDS OPERATIONS
    // ========================================================================

    /// Record or update a personal record
    pub async fn record_personal_record(
        pool: &PgPool,
        fitness_profile_id: Uuid,
        exercise_id: Uuid,
        record_type: &str,
        value: BigDecimal,
        workout_log_id: Option<Uuid>,
        exercise_log_id: Option<Uuid>,
        notes: Option<&str>,
    ) -> Result<PersonalRecord> {
        // Upsert the personal record
        let record = sqlx::query_as::<_, PersonalRecord>(
            r#"
            INSERT INTO personal_records (fitness_profile_id, exercise_id, record_type, value, workout_log_id, exercise_log_id, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (fitness_profile_id, exercise_id, record_type)
            DO UPDATE SET
                value = EXCLUDED.value,
                achieved_at = NOW(),
                workout_log_id = EXCLUDED.workout_log_id,
                exercise_log_id = EXCLUDED.exercise_log_id,
                notes = EXCLUDED.notes
            WHERE personal_records.value < EXCLUDED.value
            RETURNING id, fitness_profile_id, exercise_id, record_type, value, achieved_at, workout_log_id, exercise_log_id, notes, created_at
            "#,
        )
        .bind(fitness_profile_id)
        .bind(exercise_id)
        .bind(record_type)
        .bind(&value)
        .bind(workout_log_id)
        .bind(exercise_log_id)
        .bind(notes)
        .fetch_optional(pool)
        .await?;

        // If no row returned, the existing record was higher - fetch current record
        if let Some(r) = record {
            Ok(r)
        } else {
            sqlx::query_as::<_, PersonalRecord>(
                r#"
                SELECT id, fitness_profile_id, exercise_id, record_type, value, achieved_at, workout_log_id, exercise_log_id, notes, created_at
                FROM personal_records
                WHERE fitness_profile_id = $1 AND exercise_id = $2 AND record_type = $3
                "#,
            )
            .bind(fitness_profile_id)
            .bind(exercise_id)
            .bind(record_type)
            .fetch_one(pool)
            .await
            .map_err(|e| ToolboxError::Database(e.to_string()))
        }
    }

    /// Get personal records for a profile
    pub async fn get_personal_records(
        pool: &PgPool,
        fitness_profile_id: Uuid,
        exercise_id: Option<Uuid>,
    ) -> Result<Vec<PersonalRecordWithExercise>> {
        let rows = if let Some(eid) = exercise_id {
            sqlx::query(
                r#"
                SELECT pr.id, pr.fitness_profile_id, pr.exercise_id, pr.record_type, pr.value, pr.achieved_at, pr.workout_log_id, pr.exercise_log_id, pr.notes, pr.created_at, e.name as exercise_name
                FROM personal_records pr
                JOIN exercises e ON pr.exercise_id = e.id
                WHERE pr.fitness_profile_id = $1 AND pr.exercise_id = $2
                ORDER BY pr.achieved_at DESC
                "#,
            )
            .bind(fitness_profile_id)
            .bind(eid)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT pr.id, pr.fitness_profile_id, pr.exercise_id, pr.record_type, pr.value, pr.achieved_at, pr.workout_log_id, pr.exercise_log_id, pr.notes, pr.created_at, e.name as exercise_name
                FROM personal_records pr
                JOIN exercises e ON pr.exercise_id = e.id
                WHERE pr.fitness_profile_id = $1
                ORDER BY pr.achieved_at DESC
                "#,
            )
            .bind(fitness_profile_id)
            .fetch_all(pool)
            .await?
        };

        let mut result = Vec::new();
        for row in rows {
            use sqlx::Row;
            let record = PersonalRecord {
                id: row.get("id"),
                fitness_profile_id: row.get("fitness_profile_id"),
                exercise_id: row.get("exercise_id"),
                record_type: row.get("record_type"),
                value: row.get("value"),
                achieved_at: row.get("achieved_at"),
                workout_log_id: row.get("workout_log_id"),
                exercise_log_id: row.get("exercise_log_id"),
                notes: row.get("notes"),
                created_at: row.get("created_at"),
            };
            let exercise_name: String = row.get("exercise_name");
            result.push(PersonalRecordWithExercise {
                record,
                exercise_name,
            });
        }

        Ok(result)
    }

    // ========================================================================
    // ANALYTICS & RECOMMENDATIONS
    // ========================================================================

    /// Calculate caloric needs based on profile data
    pub fn calculate_caloric_needs(
        weight_kg: f64,
        height_cm: f64,
        age_years: i32,
        is_male: bool,
        activity_level: &str,
    ) -> CaloricNeeds {
        // Mifflin-St Jeor Equation for BMR
        let bmr = if is_male {
            10.0 * weight_kg + 6.25 * height_cm - 5.0 * (age_years as f64) + 5.0
        } else {
            10.0 * weight_kg + 6.25 * height_cm - 5.0 * (age_years as f64) - 161.0
        };

        let multiplier = ActivityLevel::from_str(activity_level)
            .map(|a| a.multiplier())
            .unwrap_or(1.55); // Default to moderate

        let tdee = bmr * multiplier;

        // Protein recommendation: 1.6-2.2g per kg for active individuals
        let protein_g = weight_kg * 1.8;

        CaloricNeeds {
            bmr,
            tdee,
            weight_loss: tdee - 500.0,
            maintenance: tdee,
            weight_gain: tdee + 500.0,
            protein_g,
            activity_level: activity_level.to_string(),
        }
    }

    /// Check if a workout is safe given user restrictions
    pub async fn check_workout_restrictions(
        pool: &PgPool,
        fitness_profile_id: Uuid,
        workout_id: Uuid,
    ) -> Result<Vec<String>> {
        let profile = Self::get_fitness_profile(pool, fitness_profile_id).await?;
        let workout = Self::get_workout_plan_with_details(pool, workout_id).await?;

        let restrictions: Vec<String> = profile
            .restrictions
            .as_ref()
            .and_then(|r| serde_json::from_value(r.clone()).ok())
            .unwrap_or_default();

        let mut warnings = Vec::new();

        // Check each exercise against restrictions
        for we in &workout.exercises {
            let exercise = &we.exercise;
            let muscle_groups: Vec<String> = exercise
                .muscle_groups
                .as_ref()
                .and_then(|mg| serde_json::from_value(mg.clone()).ok())
                .unwrap_or_default();

            for restriction in &restrictions {
                let restriction_lower = restriction.to_lowercase();

                // Check for specific restriction matches
                if restriction_lower.contains("knee") || restriction_lower.contains("leg") {
                    if muscle_groups.iter().any(|mg| {
                        let mg_lower = mg.to_lowercase();
                        mg_lower.contains("quad")
                            || mg_lower.contains("hamstring")
                            || mg_lower.contains("leg")
                    }) {
                        warnings.push(format!(
                            "Exercise '{}' targets legs - may conflict with restriction: {}",
                            exercise.name, restriction
                        ));
                    }
                }

                if restriction_lower.contains("back") {
                    if muscle_groups
                        .iter()
                        .any(|mg| mg.to_lowercase().contains("back"))
                    {
                        warnings.push(format!(
                            "Exercise '{}' targets back - may conflict with restriction: {}",
                            exercise.name, restriction
                        ));
                    }
                }

                if restriction_lower.contains("shoulder") {
                    if muscle_groups
                        .iter()
                        .any(|mg| mg.to_lowercase().contains("shoulder"))
                    {
                        warnings.push(format!(
                            "Exercise '{}' targets shoulders - may conflict with restriction: {}",
                            exercise.name, restriction
                        ));
                    }
                }

                if restriction_lower.contains("no running")
                    || restriction_lower.contains("no cardio")
                {
                    if exercise.exercise_type == "cardio" {
                        warnings.push(format!(
                            "Exercise '{}' is cardio - may conflict with restriction: {}",
                            exercise.name, restriction
                        ));
                    }
                }
            }
        }

        Ok(warnings)
    }

    /// Get progress summary for a fitness profile
    pub async fn get_progress_summary(
        pool: &PgPool,
        fitness_profile_id: Uuid,
    ) -> Result<ProgressSummary> {
        // Get total workout stats
        let stats = sqlx::query(
            r#"
            SELECT
                COUNT(*) as total_workouts,
                COALESCE(SUM(duration_minutes), 0) as total_duration,
                COALESCE(SUM(calories_burned), 0) as total_calories
            FROM workout_logs
            WHERE fitness_profile_id = $1
            "#,
        )
        .bind(fitness_profile_id)
        .fetch_one(pool)
        .await?;

        use sqlx::Row;
        let total_workouts: i64 = stats.get("total_workouts");
        let total_duration_minutes: i64 = stats.get::<i64, _>("total_duration");
        let total_calories_burned: i64 = stats.get::<i64, _>("total_calories");

        // Get this week's workouts
        let week_stats = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM workout_logs
            WHERE fitness_profile_id = $1 AND started_at >= NOW() - INTERVAL '7 days'
            "#,
        )
        .bind(fitness_profile_id)
        .fetch_one(pool)
        .await?;
        let workouts_this_week: i64 = week_stats.get("count");

        // Get this month's workouts
        let month_stats = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM workout_logs
            WHERE fitness_profile_id = $1 AND started_at >= NOW() - INTERVAL '30 days'
            "#,
        )
        .bind(fitness_profile_id)
        .fetch_one(pool)
        .await?;
        let workouts_this_month: i64 = month_stats.get("count");

        // Get PR count
        let pr_stats = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM personal_records
            WHERE fitness_profile_id = $1
            "#,
        )
        .bind(fitness_profile_id)
        .fetch_one(pool)
        .await?;
        let personal_records_count: i64 = pr_stats.get("count");

        // Get recent measurements
        let recent_measurements =
            Self::get_body_measurements(pool, fitness_profile_id, Some(5)).await?;

        // Calculate weight change
        let weight_change_kg = if recent_measurements.len() >= 2 {
            let latest = recent_measurements
                .first()
                .and_then(|m| m.weight_kg.clone());
            let oldest = recent_measurements.last().and_then(|m| m.weight_kg.clone());
            match (latest, oldest) {
                (Some(l), Some(o)) => Some(l - o),
                _ => None,
            }
        } else {
            None
        };

        // Get recent PRs
        let recent_prs = Self::get_personal_records(pool, fitness_profile_id, None).await?;
        let recent_prs: Vec<_> = recent_prs.into_iter().take(5).collect();

        Ok(ProgressSummary {
            fitness_profile_id,
            total_workouts,
            total_duration_minutes,
            total_calories_burned,
            workouts_this_week,
            workouts_this_month,
            current_streak_days: 0, // TODO: Implement streak calculation
            longest_streak_days: 0, // TODO: Implement streak calculation
            personal_records_count,
            weight_change_kg,
            recent_measurements,
            recent_prs,
        })
    }
}
