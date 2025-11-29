mod commands;

use clap::{Parser, Subcommand};

/// X Toolbox - A collection of utility tools
#[derive(Parser)]
#[command(name = "x-toolbox")]
#[command(about = "A collection of utility tools", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Nutrition management commands
    Nutrition(commands::nutrition::NutritionArgs),
}

// Re-export command handlers for convenience
pub use commands::nutrition::run_nutrition;
