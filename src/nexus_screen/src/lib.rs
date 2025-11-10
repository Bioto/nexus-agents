pub mod cli;
pub mod error;
pub mod services;
pub mod tui;

pub use cli::{run_record, run_screenshot, Cli, Commands};
pub use error::{Result, ScreenError};
pub use services::{
    screen_recorder::{RecordingConfig, ScreenRecorder},
    window_info::{WindowGeometry, WindowInfo, WindowInfoService},
};
