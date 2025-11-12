pub mod cli;
pub mod client;
pub mod factories;
pub mod models;
pub mod services;
pub mod tools;

pub use cli::{run_chat, Cli, Commands};
pub use client::{Client, UploadedFile};
pub use models::*;
pub use services::*;

use std::sync::Once;

static ENV_LOADED: Once = Once::new();

/// Load environment variables from .env file (if it exists).
/// This function is safe to call multiple times; it will only load the .env file once.
pub fn load_env() {
    ENV_LOADED.call_once(|| {
        let _ = dotenvy::dotenv();
    });
}

/// Initialize the library by loading environment variables.
/// This is called automatically when using library functions, but can be called explicitly.
pub fn init() {
    load_env();
}
