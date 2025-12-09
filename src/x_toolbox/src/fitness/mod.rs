//! Fitness/Personal Trainer module.
//!
//! This module provides a comprehensive fitness and personal training system
//! with support for:
//! - Exercise database with muscle groups, equipment, and difficulty levels
//! - Workout plans with customizable exercises, sets, reps, and rest times
//! - Training programs for multi-week schedules
//! - Fitness profiles linked to nutrition family members
//! - Progress tracking with body measurements, workout logs, and personal records
//!
//! The module integrates with the nutrition module through linked profiles,
//! allowing AI agents to provide holistic health recommendations.

pub mod config;
pub mod mcp_server;
pub mod models;
pub mod service;

pub use mcp_server::FitnessMcpServer;
pub use models::*;
pub use service::FitnessService;
