//! MCP Server for the fitness/personal trainer module.
//!
//! This module provides Model Context Protocol tools for AI agent integration,
//! enabling agents to manage exercises, workouts, training programs, fitness profiles,
//! and track progress.

use crate::error::ToolboxError;
use crate::nutrition::Database;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters, ServerHandler},
    model::*,
    schemars, tool, tool_router, ErrorData as McpError,
};
use serde::{Deserialize, Serialize};
use sqlx::{types::BigDecimal, PgPool};
use std::sync::Arc;
use uuid::Uuid;

use super::FitnessService;

/// MCP Server for fitness module
#[derive(Clone)]
pub struct FitnessMcpServer {
    pool: Arc<PgPool>,
    pub tool_router: ToolRouter<Self>,
}

// Tool definitions
#[tool_router]
impl FitnessMcpServer {
    pub async fn new() -> Result<Self, ToolboxError> {
        let _ = dotenvy::dotenv();
        let db = Database::new().await?;
        Ok(Self {
            pool: Arc::new(db.pool().clone()),
            tool_router: Self::tool_router(),
        })
    }

    pub async fn with_database(db: Database) -> Result<Self, ToolboxError> {
        Ok(Self {
            pool: Arc::new(db.pool().clone()),
            tool_router: Self::tool_router(),
        })
    }

    /// Get all available tools
    pub fn list_all_tools(&self) -> Vec<Tool> {
        self.tool_router.list_all()
    }

    // ========================================================================
    // EXERCISE MANAGEMENT TOOLS
    // ========================================================================

    /// Manage exercises: create, update, or delete
    #[tool(
        description = "Manage exercises. Action: 'create' (name, description?, muscle_groups?, equipment?, exercise_type?, difficulty_level?, instructions?, video_url?, calories_per_minute?), 'update' (id, same optional fields), or 'delete' (id)."
    )]
    async fn manage_exercise(
        &self,
        params: Parameters<ManageExerciseParams>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.action.as_str() {
            "create" => {
                let name = params.0.name.ok_or_else(|| {
                    McpError::invalid_params("name is required for create action", None)
                })?;

                let muscle_groups = params.0.muscle_groups.map(|mg| serde_json::json!(mg));
                let equipment = params.0.equipment.map(|eq| serde_json::json!(eq));
                let calories = params
                    .0
                    .calories_per_minute
                    .map(|c| c.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid calories value: {}", e), None)
                    })?;

                let exercise = FitnessService::create_exercise(
                    &self.pool,
                    &name,
                    params.0.description.as_deref(),
                    muscle_groups,
                    equipment,
                    params.0.exercise_type.as_deref().unwrap_or("strength"),
                    params
                        .0
                        .difficulty_level
                        .as_deref()
                        .unwrap_or("intermediate"),
                    params.0.instructions.as_deref(),
                    params.0.video_url.as_deref(),
                    calories,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Created exercise: {} (ID: {})\nType: {}\nDifficulty: {}\nMuscle groups: {:?}\nEquipment: {:?}",
                    exercise.name,
                    exercise.id,
                    exercise.exercise_type,
                    exercise.difficulty_level,
                    exercise.muscle_groups,
                    exercise.equipment
                ))]))
            }
            "update" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for update action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let muscle_groups = params.0.muscle_groups.map(|mg| serde_json::json!(mg));
                let equipment = params.0.equipment.map(|eq| serde_json::json!(eq));
                let calories = params
                    .0
                    .calories_per_minute
                    .map(|c| c.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid calories value: {}", e), None)
                    })?;

                let exercise = FitnessService::update_exercise(
                    &self.pool,
                    uuid,
                    params.0.name.as_deref(),
                    params.0.description.as_deref(),
                    muscle_groups,
                    equipment,
                    params.0.exercise_type.as_deref(),
                    params.0.difficulty_level.as_deref(),
                    params.0.instructions.as_deref(),
                    params.0.video_url.as_deref(),
                    calories,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Updated exercise: {} ({})",
                    exercise.name, exercise.id
                ))]))
            }
            "delete" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for delete action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                FitnessService::delete_exercise(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted exercise: {}",
                    id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!(
                    "Unknown action: {}. Valid actions: create, update, delete",
                    params.0.action
                ),
                None,
            )),
        }
    }

    /// Query exercises: search, get by ID, or batch get
    #[tool(
        description = "Query exercises. Use 'search' with optional search_term, exercise_type, difficulty_level, muscle_group, equipment; 'get' with id; or 'batch' with ids array."
    )]
    async fn query_exercises(
        &self,
        params: Parameters<QueryExercisesParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        match params.0.query_type.as_str() {
            "search" => {
                let exercises = FitnessService::list_exercises(
                    &self.pool,
                    params.0.search_term.as_deref(),
                    params.0.exercise_type.as_deref(),
                    params.0.difficulty_level.as_deref(),
                    params.0.muscle_group.as_deref(),
                    params.0.equipment.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                let mut output = format!("Found {} exercise(s):\n\n", exercises.len());
                for ex in exercises {
                    output.push_str(&format!(
                        "- {} ({})\n  Type: {} | Difficulty: {}\n  Muscle groups: {:?}\n  Equipment: {:?}\n",
                        ex.name,
                        ex.id,
                        ex.exercise_type,
                        ex.difficulty_level,
                        ex.muscle_groups,
                        ex.equipment
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "get" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for get query", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let exercise = FitnessService::get_exercise(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                let mut output = format!(
                    "Exercise: {}\nID: {}\nType: {}\nDifficulty: {}\n",
                    exercise.name, exercise.id, exercise.exercise_type, exercise.difficulty_level
                );
                if let Some(desc) = &exercise.description {
                    output.push_str(&format!("Description: {}\n", desc));
                }
                if let Some(mg) = &exercise.muscle_groups {
                    output.push_str(&format!("Muscle groups: {}\n", mg));
                }
                if let Some(eq) = &exercise.equipment {
                    output.push_str(&format!("Equipment: {}\n", eq));
                }
                if let Some(inst) = &exercise.instructions {
                    output.push_str(&format!("Instructions: {}\n", inst));
                }
                if let Some(url) = &exercise.video_url {
                    output.push_str(&format!("Video URL: {}\n", url));
                }

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "batch" => {
                let ids = params.0.ids.ok_or_else(|| {
                    McpError::invalid_params("ids array is required for batch query", None)
                })?;

                let uuids: Result<Vec<Uuid>, McpError> = ids
                    .into_iter()
                    .map(|id| {
                        Uuid::parse_str(&id).map_err(|e| {
                            McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                        })
                    })
                    .collect();
                let uuids = uuids?;

                let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                    let pool = &self.pool;
                    async move {
                        FitnessService::get_exercise(pool, id)
                            .await
                            .map_err(convert_error)
                    }
                }))
                .await;

                let mut output = format!("Batch get exercises ({} requested):\n\n", results.len());
                for (idx, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(ex) => {
                            output.push_str(&format!(
                                "[{}] {} ({}) - {} | {}\n",
                                idx + 1,
                                ex.name,
                                ex.id,
                                ex.exercise_type,
                                ex.difficulty_level
                            ));
                        }
                        Err(e) => {
                            output.push_str(&format!("[{}] Error: {}\n", idx + 1, e));
                        }
                    }
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            _ => Err(McpError::invalid_params(
                format!(
                    "Unknown query_type: {}. Valid types: search, get, batch",
                    params.0.query_type
                ),
                None,
            )),
        }
    }

    // ========================================================================
    // WORKOUT MANAGEMENT TOOLS
    // ========================================================================

    /// Manage workout plans: create, update, or delete
    #[tool(
        description = "Manage workout plans. Action: 'create' (name, description?, workout_type?, difficulty_level?, estimated_duration_minutes?, exercises?), 'update' (id, same optional fields), or 'delete' (id)."
    )]
    async fn manage_workout(
        &self,
        params: Parameters<ManageWorkoutParams>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.action.as_str() {
            "create" => {
                let name = params.0.name.ok_or_else(|| {
                    McpError::invalid_params("name is required for create action", None)
                })?;

                let exercises: Result<Vec<_>, McpError> = params
                    .0
                    .exercises
                    .unwrap_or_default()
                    .into_iter()
                    .enumerate()
                    .map(|(idx, ex)| {
                        let uuid = Uuid::parse_str(&ex.exercise_id).map_err(|e| {
                            McpError::invalid_params(format!("Invalid exercise UUID: {}", e), None)
                        })?;
                        Ok((
                            uuid,
                            ex.order_index.unwrap_or(idx as i32),
                            ex.sets,
                            ex.reps,
                            ex.duration_seconds,
                            ex.rest_seconds,
                            ex.notes,
                        ))
                    })
                    .collect();
                let exercises = exercises?;

                let workout = FitnessService::create_workout_plan(
                    &self.pool,
                    &name,
                    params.0.description.as_deref(),
                    params.0.workout_type.as_deref().unwrap_or("strength"),
                    params
                        .0
                        .difficulty_level
                        .as_deref()
                        .unwrap_or("intermediate"),
                    params.0.estimated_duration_minutes,
                    exercises,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Created workout plan: {} (ID: {})\nType: {}\nDifficulty: {}\nDuration: {:?} minutes",
                    workout.name,
                    workout.id,
                    workout.workout_type,
                    workout.difficulty_level,
                    workout.estimated_duration_minutes
                ))]))
            }
            "update" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for update action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let workout = FitnessService::update_workout_plan(
                    &self.pool,
                    uuid,
                    params.0.name.as_deref(),
                    params.0.description.as_deref(),
                    params.0.workout_type.as_deref(),
                    params.0.difficulty_level.as_deref(),
                    params.0.estimated_duration_minutes,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Updated workout plan: {} ({})",
                    workout.name, workout.id
                ))]))
            }
            "delete" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for delete action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                FitnessService::delete_workout_plan(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted workout plan: {}",
                    id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!(
                    "Unknown action: {}. Valid actions: create, update, delete",
                    params.0.action
                ),
                None,
            )),
        }
    }

    /// Manage workout content: add or remove exercises
    #[tool(
        description = "Manage workout content. Action: 'add_exercise' (workout_id, exercise_id, order_index?, sets?, reps?, duration_seconds?, rest_seconds?, notes?) or 'remove_exercise' (workout_id, exercise_id)."
    )]
    async fn manage_workout_content(
        &self,
        params: Parameters<ManageWorkoutContentParams>,
    ) -> Result<CallToolResult, McpError> {
        let workout_id = params
            .0
            .workout_id
            .ok_or_else(|| McpError::invalid_params("workout_id is required", None))?;
        let workout_uuid = Uuid::parse_str(&workout_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid workout UUID: {}", e), None))?;

        match params.0.action.as_str() {
            "add_exercise" => {
                let exercise_id = params.0.exercise_id.ok_or_else(|| {
                    McpError::invalid_params("exercise_id is required for add_exercise", None)
                })?;
                let exercise_uuid = Uuid::parse_str(&exercise_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid exercise UUID: {}", e), None)
                })?;

                let we = FitnessService::add_workout_exercise(
                    &self.pool,
                    workout_uuid,
                    exercise_uuid,
                    params.0.order_index.unwrap_or(0),
                    params.0.sets,
                    params.0.reps,
                    params.0.duration_seconds,
                    params.0.rest_seconds,
                    params.0.notes.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Added exercise {} to workout {}\nSets: {:?}, Reps: {:?}, Duration: {:?}s, Rest: {:?}s",
                    exercise_id, workout_id, we.sets, we.reps, we.duration_seconds, we.rest_seconds
                ))]))
            }
            "remove_exercise" => {
                let exercise_id = params.0.exercise_id.ok_or_else(|| {
                    McpError::invalid_params("exercise_id is required for remove_exercise", None)
                })?;
                let exercise_uuid = Uuid::parse_str(&exercise_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid exercise UUID: {}", e), None)
                })?;

                FitnessService::remove_workout_exercise(&self.pool, workout_uuid, exercise_uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Removed exercise {} from workout {}",
                    exercise_id, workout_id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!(
                    "Unknown action: {}. Valid actions: add_exercise, remove_exercise",
                    params.0.action
                ),
                None,
            )),
        }
    }

    /// Query workout plans: search, get by ID, or batch get
    #[tool(
        description = "Query workout plans. Use 'search' with optional search_term, workout_type, difficulty_level; 'get' with id (add full=true for exercises); or 'batch' with ids array."
    )]
    async fn query_workouts(
        &self,
        params: Parameters<QueryWorkoutsParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        match params.0.query_type.as_str() {
            "search" => {
                let workouts = FitnessService::list_workout_plans(
                    &self.pool,
                    params.0.search_term.as_deref(),
                    params.0.workout_type.as_deref(),
                    params.0.difficulty_level.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                let mut output = format!("Found {} workout plan(s):\n\n", workouts.len());
                for w in workouts {
                    output.push_str(&format!(
                        "- {} ({})\n  Type: {} | Difficulty: {} | Duration: {:?} min\n",
                        w.name,
                        w.id,
                        w.workout_type,
                        w.difficulty_level,
                        w.estimated_duration_minutes
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "get" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for get query", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                if params.0.full.unwrap_or(false) {
                    let workout = FitnessService::get_workout_plan_with_details(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    let mut output = format!(
                        "Workout Plan: {}\nID: {}\nType: {}\nDifficulty: {}\nDuration: {:?} min\n\nExercises ({}):\n",
                        workout.workout.name,
                        workout.workout.id,
                        workout.workout.workout_type,
                        workout.workout.difficulty_level,
                        workout.workout.estimated_duration_minutes,
                        workout.exercises.len()
                    );

                    for we in &workout.exercises {
                        output.push_str(&format!(
                            "  {}. {} ({})\n     Sets: {:?}, Reps: {:?}, Duration: {:?}s, Rest: {:?}s\n",
                            we.workout_exercise.order_index,
                            we.exercise.name,
                            we.exercise.id,
                            we.workout_exercise.sets,
                            we.workout_exercise.reps,
                            we.workout_exercise.duration_seconds,
                            we.workout_exercise.rest_seconds
                        ));
                    }
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    let workout = FitnessService::get_workout_plan(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    Ok(CallToolResult::success(vec![Content::text(format!(
                        "Workout Plan: {}\nID: {}\nType: {}\nDifficulty: {}\nDuration: {:?} min",
                        workout.name,
                        workout.id,
                        workout.workout_type,
                        workout.difficulty_level,
                        workout.estimated_duration_minutes
                    ))]))
                }
            }
            "batch" => {
                let ids = params.0.ids.ok_or_else(|| {
                    McpError::invalid_params("ids array is required for batch query", None)
                })?;

                let uuids: Result<Vec<Uuid>, McpError> = ids
                    .into_iter()
                    .map(|id| {
                        Uuid::parse_str(&id).map_err(|e| {
                            McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                        })
                    })
                    .collect();
                let uuids = uuids?;

                let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                    let pool = &self.pool;
                    async move {
                        FitnessService::get_workout_plan(pool, id)
                            .await
                            .map_err(convert_error)
                    }
                }))
                .await;

                let mut output =
                    format!("Batch get workout plans ({} requested):\n\n", results.len());
                for (idx, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(w) => {
                            output.push_str(&format!(
                                "[{}] {} ({}) - {} | {}\n",
                                idx + 1,
                                w.name,
                                w.id,
                                w.workout_type,
                                w.difficulty_level
                            ));
                        }
                        Err(e) => {
                            output.push_str(&format!("[{}] Error: {}\n", idx + 1, e));
                        }
                    }
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            _ => Err(McpError::invalid_params(
                format!(
                    "Unknown query_type: {}. Valid types: search, get, batch",
                    params.0.query_type
                ),
                None,
            )),
        }
    }

    // ========================================================================
    // TRAINING PROGRAM TOOLS
    // ========================================================================

    /// Manage training programs: create, update, delete, or manage entries
    #[tool(
        description = "Manage training programs. Action: 'create' (name, description?, start_date?, end_date?, is_template, goal?, weeks?), 'update' (id, same optional fields), 'delete' (id), 'add_entry' (program_id, workout_id, day_of_week?, week_number?, date?, notes?), or 'remove_entry' (entry_id)."
    )]
    async fn manage_training_program(
        &self,
        params: Parameters<ManageTrainingProgramParams>,
    ) -> Result<CallToolResult, McpError> {
        use chrono::NaiveDate;

        match params.0.action.as_str() {
            "create" => {
                let name = params.0.name.ok_or_else(|| {
                    McpError::invalid_params("name is required for create action", None)
                })?;

                let start_date = params
                    .0
                    .start_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| McpError::invalid_params(format!("Invalid start_date: {}", e), None))?;
                let end_date = params
                    .0
                    .end_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| McpError::invalid_params(format!("Invalid end_date: {}", e), None))?;

                let program = FitnessService::create_training_program(
                    &self.pool,
                    &name,
                    params.0.description.as_deref(),
                    start_date,
                    end_date,
                    params.0.is_template.unwrap_or(false),
                    params.0.goal.as_deref(),
                    params.0.weeks,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Created training program: {} (ID: {})\nTemplate: {}\nGoal: {:?}\nWeeks: {:?}",
                    program.name, program.id, program.is_template, program.goal, program.weeks
                ))]))
            }
            "update" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for update action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let start_date = params
                    .0
                    .start_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| McpError::invalid_params(format!("Invalid start_date: {}", e), None))?;
                let end_date = params
                    .0
                    .end_date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| McpError::invalid_params(format!("Invalid end_date: {}", e), None))?;

                let program = FitnessService::update_training_program(
                    &self.pool,
                    uuid,
                    params.0.name.as_deref(),
                    params.0.description.as_deref(),
                    start_date,
                    end_date,
                    params.0.goal.as_deref(),
                    params.0.weeks,
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Updated training program: {} ({})",
                    program.name, program.id
                ))]))
            }
            "delete" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for delete action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                FitnessService::delete_training_program(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted training program: {}",
                    id
                ))]))
            }
            "add_entry" => {
                let program_id = params.0.program_id.ok_or_else(|| {
                    McpError::invalid_params("program_id is required for add_entry", None)
                })?;
                let program_uuid = Uuid::parse_str(&program_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid program UUID: {}", e), None))?;
                let workout_id = params.0.workout_id.ok_or_else(|| {
                    McpError::invalid_params("workout_id is required for add_entry", None)
                })?;
                let workout_uuid = Uuid::parse_str(&workout_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid workout UUID: {}", e), None))?;

                let date = params
                    .0
                    .date
                    .as_ref()
                    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()
                    .map_err(|e| McpError::invalid_params(format!("Invalid date: {}", e), None))?;

                let entry = FitnessService::add_program_entry(
                    &self.pool,
                    program_uuid,
                    workout_uuid,
                    params.0.day_of_week,
                    params.0.week_number,
                    date,
                    params.0.notes.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Added entry to training program {}\nEntry ID: {}\nWorkout: {}\nDay: {:?}, Week: {:?}, Date: {:?}",
                    program_id, entry.id, workout_id, entry.day_of_week, entry.week_number, entry.date
                ))]))
            }
            "remove_entry" => {
                let entry_id = params.0.entry_id.ok_or_else(|| {
                    McpError::invalid_params("entry_id is required for remove_entry", None)
                })?;
                let uuid = Uuid::parse_str(&entry_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                FitnessService::remove_program_entry(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Removed program entry: {}",
                    entry_id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown action: {}. Valid actions: create, update, delete, add_entry, remove_entry", params.0.action),
                None,
            )),
        }
    }

    /// Query training programs: search, get by ID, or batch get
    #[tool(
        description = "Query training programs. Use 'search' with optional search_term, is_template, goal; 'get' with id (add full=true for entries); or 'batch' with ids array."
    )]
    async fn query_training_programs(
        &self,
        params: Parameters<QueryTrainingProgramsParams>,
    ) -> Result<CallToolResult, McpError> {
        use futures::future::join_all;

        match params.0.query_type.as_str() {
            "search" => {
                let programs = FitnessService::list_training_programs(
                    &self.pool,
                    params.0.search_term.as_deref(),
                    params.0.is_template,
                    params.0.goal.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                let mut output = format!("Found {} training program(s):\n\n", programs.len());
                for p in programs {
                    output.push_str(&format!(
                        "- {} ({})\n  Template: {} | Goal: {:?} | Weeks: {:?}\n",
                        p.name, p.id, p.is_template, p.goal, p.weeks
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "get" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for get query", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                if params.0.full.unwrap_or(false) {
                    let program =
                        FitnessService::get_training_program_with_entries(&self.pool, uuid)
                            .await
                            .map_err(convert_error)?;

                    let mut output = format!(
                        "Training Program: {}\nID: {}\nTemplate: {}\nGoal: {:?}\nWeeks: {:?}\n\nEntries ({}):\n",
                        program.program.name,
                        program.program.id,
                        program.program.is_template,
                        program.program.goal,
                        program.program.weeks,
                        program.entries.len()
                    );

                    for entry in &program.entries {
                        let day_info = if let Some(date) = entry.entry.date {
                            format!("{}", date)
                        } else if let (Some(week), Some(day)) =
                            (entry.entry.week_number, entry.entry.day_of_week)
                        {
                            let day_name = match day {
                                0 => "Mon",
                                1 => "Tue",
                                2 => "Wed",
                                3 => "Thu",
                                4 => "Fri",
                                5 => "Sat",
                                6 => "Sun",
                                _ => "?",
                            };
                            format!("Week {}, {}", week, day_name)
                        } else if let Some(day) = entry.entry.day_of_week {
                            let day_name = match day {
                                0 => "Mon",
                                1 => "Tue",
                                2 => "Wed",
                                3 => "Thu",
                                4 => "Fri",
                                5 => "Sat",
                                6 => "Sun",
                                _ => "?",
                            };
                            day_name.to_string()
                        } else {
                            "Unscheduled".to_string()
                        };

                        output.push_str(&format!(
                            "  - {}: {} ({})\n",
                            day_info, entry.workout.name, entry.workout.id
                        ));
                    }
                    Ok(CallToolResult::success(vec![Content::text(output)]))
                } else {
                    let program = FitnessService::get_training_program(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                    Ok(CallToolResult::success(vec![Content::text(format!(
                        "Training Program: {}\nID: {}\nTemplate: {}\nGoal: {:?}\nWeeks: {:?}",
                        program.name, program.id, program.is_template, program.goal, program.weeks
                    ))]))
                }
            }
            "batch" => {
                let ids = params.0.ids.ok_or_else(|| {
                    McpError::invalid_params("ids array is required for batch query", None)
                })?;

                let uuids: Result<Vec<Uuid>, McpError> = ids
                    .into_iter()
                    .map(|id| {
                        Uuid::parse_str(&id).map_err(|e| {
                            McpError::invalid_params(format!("Invalid UUID '{}': {}", id, e), None)
                        })
                    })
                    .collect();
                let uuids = uuids?;

                let results: Vec<_> = join_all(uuids.iter().map(|&id| {
                    let pool = &self.pool;
                    async move {
                        FitnessService::get_training_program(pool, id)
                            .await
                            .map_err(convert_error)
                    }
                }))
                .await;

                let mut output = format!(
                    "Batch get training programs ({} requested):\n\n",
                    results.len()
                );
                for (idx, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(p) => {
                            output.push_str(&format!(
                                "[{}] {} ({}) - Template: {} | Goal: {:?}\n",
                                idx + 1,
                                p.name,
                                p.id,
                                p.is_template,
                                p.goal
                            ));
                        }
                        Err(e) => {
                            output.push_str(&format!("[{}] Error: {}\n", idx + 1, e));
                        }
                    }
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            _ => Err(McpError::invalid_params(
                format!(
                    "Unknown query_type: {}. Valid types: search, get, batch",
                    params.0.query_type
                ),
                None,
            )),
        }
    }

    // ========================================================================
    // FITNESS PROFILE TOOLS
    // ========================================================================

    /// Manage fitness profiles: create, update, or delete (linked to family_member)
    #[tool(
        description = "Manage fitness profiles (linked to nutrition family_members). Action: 'create' (family_member_id, current_weight_kg?, target_weight_kg?, height_cm?, fitness_level?, goals?, restrictions?, activity_level?), 'update' (id, same optional fields), or 'delete' (id)."
    )]
    async fn manage_fitness_profile(
        &self,
        params: Parameters<ManageFitnessProfileParams>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.action.as_str() {
            "create" => {
                let family_member_id = params.0.family_member_id.ok_or_else(|| {
                    McpError::invalid_params("family_member_id is required for create action", None)
                })?;
                let family_member_uuid = Uuid::parse_str(&family_member_id).map_err(|e| {
                    McpError::invalid_params(format!("Invalid family_member UUID: {}", e), None)
                })?;

                let current_weight = params
                    .0
                    .current_weight_kg
                    .map(|w| w.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid weight: {}", e), None)
                    })?;
                let target_weight = params
                    .0
                    .target_weight_kg
                    .map(|w| w.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid target weight: {}", e), None)
                    })?;
                let height = params
                    .0
                    .height_cm
                    .map(|h| h.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid height: {}", e), None)
                    })?;

                let goals = params.0.goals.map(|g| serde_json::json!(g));
                let restrictions = params.0.restrictions.map(|r| serde_json::json!(r));

                let profile = FitnessService::create_fitness_profile(
                    &self.pool,
                    family_member_uuid,
                    current_weight,
                    target_weight,
                    height,
                    params.0.fitness_level.as_deref().unwrap_or("beginner"),
                    goals,
                    restrictions,
                    params.0.activity_level.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Created fitness profile (ID: {})\nLinked to family member: {}\nFitness level: {}\nActivity level: {:?}",
                    profile.id, profile.family_member_id, profile.fitness_level, profile.activity_level
                ))]))
            }
            "update" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for update action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let current_weight = params
                    .0
                    .current_weight_kg
                    .map(|w| w.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid weight: {}", e), None)
                    })?;
                let target_weight = params
                    .0
                    .target_weight_kg
                    .map(|w| w.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid target weight: {}", e), None)
                    })?;
                let height = params
                    .0
                    .height_cm
                    .map(|h| h.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid height: {}", e), None)
                    })?;

                let goals = params.0.goals.map(|g| serde_json::json!(g));
                let restrictions = params.0.restrictions.map(|r| serde_json::json!(r));

                let profile = FitnessService::update_fitness_profile(
                    &self.pool,
                    uuid,
                    current_weight,
                    target_weight,
                    height,
                    params.0.fitness_level.as_deref(),
                    goals,
                    restrictions,
                    params.0.activity_level.as_deref(),
                )
                .await
                .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Updated fitness profile: {}",
                    profile.id
                ))]))
            }
            "delete" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for delete action", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                FitnessService::delete_fitness_profile(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted fitness profile: {}",
                    id
                ))]))
            }
            _ => Err(McpError::invalid_params(
                format!(
                    "Unknown action: {}. Valid actions: create, update, delete",
                    params.0.action
                ),
                None,
            )),
        }
    }

    /// Query fitness profiles: list all, get by ID, or get by family_member_id
    #[tool(
        description = "Query fitness profiles. Use 'list' for all profiles, 'get' with id, or 'by_family_member' with family_member_id."
    )]
    async fn query_fitness_profiles(
        &self,
        params: Parameters<QueryFitnessProfilesParams>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.query_type.as_str() {
            "list" => {
                let profiles = FitnessService::list_fitness_profiles(&self.pool)
                    .await
                    .map_err(convert_error)?;

                if profiles.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "No fitness profiles found.",
                    )]));
                }

                let mut output = format!("Found {} fitness profile(s):\n\n", profiles.len());
                for p in profiles {
                    output.push_str(&format!(
                        "- {} (Profile ID: {})\n  Family member: {}\n  Fitness level: {}\n  Weight: {:?} kg (target: {:?} kg)\n",
                        p.family_member_name, p.profile.id, p.profile.family_member_id,
                        p.profile.fitness_level, p.profile.current_weight_kg, p.profile.target_weight_kg
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "get" => {
                let id = params.0.id.ok_or_else(|| {
                    McpError::invalid_params("id is required for get query", None)
                })?;
                let uuid = Uuid::parse_str(&id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let profile = FitnessService::get_fitness_profile(&self.pool, uuid)
                    .await
                    .map_err(convert_error)?;

                let mut output = format!(
                    "Fitness Profile: {}\nFamily member: {}\nFitness level: {}\nActivity level: {:?}\n",
                    profile.id, profile.family_member_id, profile.fitness_level, profile.activity_level
                );
                output.push_str(&format!(
                    "Current weight: {:?} kg\n",
                    profile.current_weight_kg
                ));
                output.push_str(&format!(
                    "Target weight: {:?} kg\n",
                    profile.target_weight_kg
                ));
                output.push_str(&format!("Height: {:?} cm\n", profile.height_cm));
                output.push_str(&format!("Goals: {:?}\n", profile.goals));
                output.push_str(&format!("Restrictions: {:?}\n", profile.restrictions));

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "by_family_member" => {
                let family_member_id = params.0.family_member_id.ok_or_else(|| {
                    McpError::invalid_params(
                        "family_member_id is required for by_family_member query",
                        None,
                    )
                })?;
                let uuid = Uuid::parse_str(&family_member_id)
                    .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

                let profile =
                    FitnessService::get_fitness_profile_by_family_member(&self.pool, uuid)
                        .await
                        .map_err(convert_error)?;

                let mut output = format!(
                    "Fitness Profile: {}\nFamily member: {}\nFitness level: {}\nActivity level: {:?}\n",
                    profile.id, profile.family_member_id, profile.fitness_level, profile.activity_level
                );
                output.push_str(&format!(
                    "Current weight: {:?} kg\n",
                    profile.current_weight_kg
                ));
                output.push_str(&format!(
                    "Target weight: {:?} kg\n",
                    profile.target_weight_kg
                ));
                output.push_str(&format!("Height: {:?} cm\n", profile.height_cm));
                output.push_str(&format!("Goals: {:?}\n", profile.goals));
                output.push_str(&format!("Restrictions: {:?}\n", profile.restrictions));

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            _ => Err(McpError::invalid_params(
                format!(
                    "Unknown query_type: {}. Valid types: list, get, by_family_member",
                    params.0.query_type
                ),
                None,
            )),
        }
    }

    // ========================================================================
    // PROGRESS TRACKING TOOLS
    // ========================================================================

    /// Log a completed workout with exercise details
    #[tool(
        description = "Log a completed workout. Params: fitness_profile_id, workout_id?, workout_name?, duration_minutes?, calories_burned?, notes?, rating (1-5)?, perceived_difficulty (1-10)?, exercise_logs?[]."
    )]
    async fn log_workout(
        &self,
        params: Parameters<LogWorkoutParams>,
    ) -> Result<CallToolResult, McpError> {
        let fitness_profile_id = Uuid::parse_str(&params.0.fitness_profile_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid profile UUID: {}", e), None))?;

        let workout_id = params
            .0
            .workout_id
            .as_ref()
            .map(|id| Uuid::parse_str(id))
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid workout UUID: {}", e), None))?;

        let workout_log = FitnessService::log_workout(
            &self.pool,
            fitness_profile_id,
            workout_id,
            params.0.workout_name.as_deref(),
            chrono::Utc::now(),
            Some(chrono::Utc::now()),
            params.0.duration_minutes,
            params.0.calories_burned,
            params.0.notes.as_deref(),
            params.0.rating,
            params.0.perceived_difficulty,
        )
        .await
        .map_err(convert_error)?;

        // Log individual exercises if provided
        if let Some(exercise_logs) = params.0.exercise_logs {
            for (idx, ex) in exercise_logs.into_iter().enumerate() {
                let exercise_id = ex
                    .exercise_id
                    .as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid exercise UUID: {}", e), None)
                    })?;

                let weight = ex
                    .weight_kg
                    .map(|w| w.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid weight: {}", e), None)
                    })?;

                let distance = ex
                    .distance_meters
                    .map(|d| d.to_string().parse::<BigDecimal>())
                    .transpose()
                    .map_err(|e| {
                        McpError::invalid_params(format!("Invalid distance: {}", e), None)
                    })?;

                let reps_json = ex.reps_per_set.map(|r| serde_json::json!(r));

                FitnessService::log_exercise(
                    &self.pool,
                    workout_log.id,
                    exercise_id,
                    ex.exercise_name.as_deref(),
                    ex.order_index.unwrap_or(idx as i32),
                    ex.sets_completed,
                    reps_json,
                    weight,
                    ex.duration_seconds,
                    distance,
                    ex.notes.as_deref(),
                    ex.is_personal_record,
                )
                .await
                .map_err(convert_error)?;
            }
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Logged workout (ID: {})\nWorkout: {:?}\nDuration: {:?} min\nCalories: {:?}\nRating: {:?}/5\nRPE: {:?}/10",
            workout_log.id,
            workout_log.workout_name,
            workout_log.duration_minutes,
            workout_log.calories_burned,
            workout_log.rating,
            workout_log.perceived_difficulty
        ))]))
    }

    /// Log body measurements
    #[tool(
        description = "Log body measurements. Params: fitness_profile_id, weight_kg?, body_fat_percentage?, waist_cm?, chest_cm?, hips_cm?, left_arm_cm?, right_arm_cm?, left_thigh_cm?, right_thigh_cm?, neck_cm?, notes?."
    )]
    async fn log_measurement(
        &self,
        params: Parameters<LogMeasurementParams>,
    ) -> Result<CallToolResult, McpError> {
        let fitness_profile_id = Uuid::parse_str(&params.0.fitness_profile_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid profile UUID: {}", e), None))?;

        let weight = params
            .0
            .weight_kg
            .map(|w| w.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid weight: {}", e), None))?;
        let body_fat = params
            .0
            .body_fat_percentage
            .map(|bf| bf.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid body fat: {}", e), None))?;
        let waist = params
            .0
            .waist_cm
            .map(|w| w.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid waist: {}", e), None))?;
        let chest = params
            .0
            .chest_cm
            .map(|c| c.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid chest: {}", e), None))?;
        let hips = params
            .0
            .hips_cm
            .map(|h| h.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid hips: {}", e), None))?;
        let left_arm = params
            .0
            .left_arm_cm
            .map(|la| la.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid left arm: {}", e), None))?;
        let right_arm = params
            .0
            .right_arm_cm
            .map(|ra| ra.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid right arm: {}", e), None))?;
        let left_thigh = params
            .0
            .left_thigh_cm
            .map(|lt| lt.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid left thigh: {}", e), None))?;
        let right_thigh = params
            .0
            .right_thigh_cm
            .map(|rt| rt.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid right thigh: {}", e), None))?;
        let neck = params
            .0
            .neck_cm
            .map(|n| n.to_string().parse::<BigDecimal>())
            .transpose()
            .map_err(|e| McpError::invalid_params(format!("Invalid neck: {}", e), None))?;

        let measurement = FitnessService::log_body_measurement(
            &self.pool,
            fitness_profile_id,
            weight,
            body_fat,
            waist,
            chest,
            hips,
            left_arm,
            right_arm,
            left_thigh,
            right_thigh,
            neck,
            params.0.notes.as_deref(),
        )
        .await
        .map_err(convert_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Logged body measurement (ID: {})\nWeight: {:?} kg\nBody fat: {:?}%\nWaist: {:?} cm\nMeasured at: {}",
            measurement.id,
            measurement.weight_kg,
            measurement.body_fat_percentage,
            measurement.waist_cm,
            measurement.measured_at
        ))]))
    }

    /// Query progress: workout history, measurements, PRs, and summary
    #[tool(
        description = "Query progress data. Type: 'summary' (fitness_profile_id), 'workout_history' (fitness_profile_id, limit?), 'measurements' (fitness_profile_id, limit?), or 'personal_records' (fitness_profile_id, exercise_id?)."
    )]
    async fn query_progress(
        &self,
        params: Parameters<QueryProgressParams>,
    ) -> Result<CallToolResult, McpError> {
        let fitness_profile_id = Uuid::parse_str(&params.0.fitness_profile_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid profile UUID: {}", e), None))?;

        match params.0.query_type.as_str() {
            "summary" => {
                let summary = FitnessService::get_progress_summary(&self.pool, fitness_profile_id)
                    .await
                    .map_err(convert_error)?;

                let mut output = format!(
                    "Progress Summary for profile {}\n\n",
                    fitness_profile_id
                );
                output.push_str(&format!("Total workouts: {}\n", summary.total_workouts));
                output.push_str(&format!("Total duration: {} minutes\n", summary.total_duration_minutes));
                output.push_str(&format!("Total calories burned: {}\n", summary.total_calories_burned));
                output.push_str(&format!("Workouts this week: {}\n", summary.workouts_this_week));
                output.push_str(&format!("Workouts this month: {}\n", summary.workouts_this_month));
                output.push_str(&format!("Personal records: {}\n", summary.personal_records_count));
                if let Some(weight_change) = summary.weight_change_kg {
                    output.push_str(&format!("Weight change: {} kg\n", weight_change));
                }

                if !summary.recent_prs.is_empty() {
                    output.push_str("\nRecent PRs:\n");
                    for pr in &summary.recent_prs {
                        output.push_str(&format!(
                            "  - {}: {} ({})\n",
                            pr.exercise_name, pr.record.value, pr.record.record_type
                        ));
                    }
                }

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "workout_history" => {
                let limit = params.0.limit.map(|l| l as i64);
                let logs = FitnessService::get_workout_logs(&self.pool, fitness_profile_id, limit, None, None)
                    .await
                    .map_err(convert_error)?;

                if logs.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text("No workout history found.")]));
                }

                let mut output = format!("Workout History ({} workouts):\n\n", logs.len());
                for log in logs {
                    output.push_str(&format!(
                        "- {} ({})\n  Date: {}\n  Duration: {:?} min | Calories: {:?} | Rating: {:?}/5\n",
                        log.workout_name.as_deref().unwrap_or("Unknown workout"),
                        log.id,
                        log.started_at.format("%Y-%m-%d %H:%M"),
                        log.duration_minutes,
                        log.calories_burned,
                        log.rating
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "measurements" => {
                let limit = params.0.limit.map(|l| l as i64);
                let measurements = FitnessService::get_body_measurements(&self.pool, fitness_profile_id, limit)
                    .await
                    .map_err(convert_error)?;

                if measurements.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text("No measurements found.")]));
                }

                let mut output = format!("Body Measurements ({} entries):\n\n", measurements.len());
                for m in measurements {
                    output.push_str(&format!(
                        "- {} | Weight: {:?} kg | Body fat: {:?}% | Waist: {:?} cm\n",
                        m.measured_at.format("%Y-%m-%d"),
                        m.weight_kg,
                        m.body_fat_percentage,
                        m.waist_cm
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            "personal_records" => {
                let exercise_id = params.0.exercise_id
                    .as_ref()
                    .map(|id| Uuid::parse_str(id))
                    .transpose()
                    .map_err(|e| McpError::invalid_params(format!("Invalid exercise UUID: {}", e), None))?;

                let prs = FitnessService::get_personal_records(&self.pool, fitness_profile_id, exercise_id)
                    .await
                    .map_err(convert_error)?;

                if prs.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text("No personal records found.")]));
                }

                let mut output = format!("Personal Records ({} PRs):\n\n", prs.len());
                for pr in prs {
                    output.push_str(&format!(
                        "- {}: {} ({}) - Achieved: {}\n",
                        pr.exercise_name,
                        pr.record.value,
                        pr.record.record_type,
                        pr.record.achieved_at.format("%Y-%m-%d")
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown query_type: {}. Valid types: summary, workout_history, measurements, personal_records", params.0.query_type),
                None,
            )),
        }
    }

    // ========================================================================
    // RECOMMENDATION & SAFETY TOOLS
    // ========================================================================

    /// Calculate caloric needs and recommendations based on profile data
    #[tool(
        description = "Calculate caloric needs. Params: weight_kg, height_cm, age_years, is_male, activity_level (sedentary/light/moderate/active/very_active)."
    )]
    async fn calculate_recommendations(
        &self,
        params: Parameters<CalculateRecommendationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let needs = FitnessService::calculate_caloric_needs(
            params.0.weight_kg,
            params.0.height_cm,
            params.0.age_years,
            params.0.is_male,
            &params.0.activity_level,
        );

        let output = format!(
            "Caloric Needs Calculation\n\n\
            Input:\n\
              Weight: {} kg\n\
              Height: {} cm\n\
              Age: {} years\n\
              Sex: {}\n\
              Activity level: {}\n\n\
            Results:\n\
              BMR (Basal Metabolic Rate): {:.0} calories/day\n\
              TDEE (Total Daily Energy Expenditure): {:.0} calories/day\n\n\
            Recommendations:\n\
              Weight loss (-500 cal deficit): {:.0} calories/day\n\
              Maintenance: {:.0} calories/day\n\
              Weight gain (+500 cal surplus): {:.0} calories/day\n\n\
              Recommended protein intake: {:.0}g/day",
            params.0.weight_kg,
            params.0.height_cm,
            params.0.age_years,
            if params.0.is_male { "Male" } else { "Female" },
            needs.activity_level,
            needs.bmr,
            needs.tdee,
            needs.weight_loss,
            needs.maintenance,
            needs.weight_gain,
            needs.protein_g
        );

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Check if a workout is safe given user restrictions
    #[tool(
        description = "Check if a workout contains exercises that may conflict with user restrictions. Params: fitness_profile_id, workout_id."
    )]
    async fn check_restrictions(
        &self,
        params: Parameters<CheckRestrictionsParams>,
    ) -> Result<CallToolResult, McpError> {
        let fitness_profile_id = Uuid::parse_str(&params.0.fitness_profile_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid profile UUID: {}", e), None))?;
        let workout_id = Uuid::parse_str(&params.0.workout_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid workout UUID: {}", e), None))?;

        let warnings =
            FitnessService::check_workout_restrictions(&self.pool, fitness_profile_id, workout_id)
                .await
                .map_err(convert_error)?;

        if warnings.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                "✓ Workout is safe - no conflicts with user restrictions found.",
            )]))
        } else {
            let mut output = format!(
                "⚠️ WARNING: {} potential conflict(s) found:\n\n",
                warnings.len()
            );
            for warning in warnings {
                output.push_str(&format!("- {}\n", warning));
            }
            output.push_str(
                "\nPlease review these exercises and consider modifications or alternatives.",
            );
            Ok(CallToolResult::success(vec![Content::text(output)]))
        }
    }
}

// ========================================================================
// PARAMETER STRUCTS
// ========================================================================

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageExerciseParams {
    action: String,
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    muscle_groups: Option<Vec<String>>,
    equipment: Option<Vec<String>>,
    exercise_type: Option<String>,
    difficulty_level: Option<String>,
    instructions: Option<String>,
    video_url: Option<String>,
    calories_per_minute: Option<f64>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryExercisesParams {
    query_type: String,
    search_term: Option<String>,
    exercise_type: Option<String>,
    difficulty_level: Option<String>,
    muscle_group: Option<String>,
    equipment: Option<String>,
    id: Option<String>,
    ids: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct WorkoutExerciseInput {
    exercise_id: String,
    order_index: Option<i32>,
    sets: Option<i32>,
    reps: Option<i32>,
    duration_seconds: Option<i32>,
    rest_seconds: Option<i32>,
    notes: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageWorkoutParams {
    action: String,
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    workout_type: Option<String>,
    difficulty_level: Option<String>,
    estimated_duration_minutes: Option<i32>,
    exercises: Option<Vec<WorkoutExerciseInput>>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageWorkoutContentParams {
    action: String,
    workout_id: Option<String>,
    exercise_id: Option<String>,
    order_index: Option<i32>,
    sets: Option<i32>,
    reps: Option<i32>,
    duration_seconds: Option<i32>,
    rest_seconds: Option<i32>,
    notes: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryWorkoutsParams {
    query_type: String,
    search_term: Option<String>,
    workout_type: Option<String>,
    difficulty_level: Option<String>,
    id: Option<String>,
    ids: Option<Vec<String>>,
    full: Option<bool>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageTrainingProgramParams {
    action: String,
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    is_template: Option<bool>,
    goal: Option<String>,
    weeks: Option<i32>,
    program_id: Option<String>,
    workout_id: Option<String>,
    day_of_week: Option<i32>,
    week_number: Option<i32>,
    date: Option<String>,
    notes: Option<String>,
    entry_id: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryTrainingProgramsParams {
    query_type: String,
    search_term: Option<String>,
    is_template: Option<bool>,
    goal: Option<String>,
    id: Option<String>,
    ids: Option<Vec<String>>,
    full: Option<bool>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ManageFitnessProfileParams {
    action: String,
    id: Option<String>,
    family_member_id: Option<String>,
    current_weight_kg: Option<f64>,
    target_weight_kg: Option<f64>,
    height_cm: Option<f64>,
    fitness_level: Option<String>,
    goals: Option<Vec<String>>,
    restrictions: Option<Vec<String>>,
    activity_level: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryFitnessProfilesParams {
    query_type: String,
    id: Option<String>,
    family_member_id: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct ExerciseLogInput {
    exercise_id: Option<String>,
    exercise_name: Option<String>,
    order_index: Option<i32>,
    sets_completed: Option<i32>,
    reps_per_set: Option<Vec<i32>>,
    weight_kg: Option<f64>,
    duration_seconds: Option<i32>,
    distance_meters: Option<f64>,
    notes: Option<String>,
    is_personal_record: Option<bool>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct LogWorkoutParams {
    fitness_profile_id: String,
    workout_id: Option<String>,
    workout_name: Option<String>,
    duration_minutes: Option<i32>,
    calories_burned: Option<i32>,
    notes: Option<String>,
    rating: Option<i32>,
    perceived_difficulty: Option<i32>,
    exercise_logs: Option<Vec<ExerciseLogInput>>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct LogMeasurementParams {
    fitness_profile_id: String,
    weight_kg: Option<f64>,
    body_fat_percentage: Option<f64>,
    waist_cm: Option<f64>,
    chest_cm: Option<f64>,
    hips_cm: Option<f64>,
    left_arm_cm: Option<f64>,
    right_arm_cm: Option<f64>,
    left_thigh_cm: Option<f64>,
    right_thigh_cm: Option<f64>,
    neck_cm: Option<f64>,
    notes: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct QueryProgressParams {
    query_type: String,
    fitness_profile_id: String,
    limit: Option<i32>,
    exercise_id: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct CalculateRecommendationsParams {
    weight_kg: f64,
    height_cm: f64,
    age_years: i32,
    is_male: bool,
    activity_level: String,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
struct CheckRestrictionsParams {
    fitness_profile_id: String,
    workout_id: String,
}

// Error conversion helper
fn convert_error(err: ToolboxError) -> McpError {
    match err {
        ToolboxError::NotFound(msg) => McpError::invalid_params(msg, None),
        ToolboxError::Validation(msg) => McpError::invalid_params(msg, None),
        ToolboxError::Database(msg) => McpError::internal_error(msg, None),
        ToolboxError::Configuration(msg) => McpError::internal_error(msg, None),
        _ => McpError::internal_error(err.to_string(), None),
    }
}

// Implement ServerHandler
impl ServerHandler for FitnessMcpServer {
    fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<InitializeResult, McpError>> + Send + '_ {
        async move {
            Ok(InitializeResult {
                protocol_version: ProtocolVersion::V_2024_11_05,
                capabilities: ServerCapabilities {
                    tools: Some(ToolsCapability { list_changed: None }),
                    ..Default::default()
                },
                server_info: Implementation {
                    name: "fitness-mcp-server".into(),
                    version: "0.1.0".into(),
                    icons: None,
                    title: Some("Personal Trainer & Fitness Management".into()),
                    website_url: None,
                },
                instructions: Some("Personal trainer system. Manage exercises, workout plans, training programs, fitness profiles, and track progress. Integrates with nutrition module for holistic health management.".into()),
            })
        }
    }

    fn on_initialized(
        &self,
        _context: rmcp::service::NotificationContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        async move {}
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        let tool_router = self.tool_router.clone();
        let server = self.clone();
        async move {
            let tool_call_context =
                rmcp::handler::server::tool::ToolCallContext::new(&server, request, context);
            tool_router.call(tool_call_context).await
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = self.tool_router.list_all();
        async move {
            Ok(ListToolsResult {
                tools,
                next_cursor: None,
            })
        }
    }
}
