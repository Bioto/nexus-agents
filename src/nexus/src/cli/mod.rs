mod commands;

use clap::{Parser, Subcommand};
use nexus_audio::Commands as AudioCommands;
use nexus_core::Commands as CoreCommands;
use nexus_gui::Commands as GuiCommands;
use nexus_screen::Commands as ScreenCommands;

/// Nexus - AI Agent Framework
#[derive(Parser)]
#[command(name = "nexus")]
#[command(about = "A framework for building and managing AI agents", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Voice interface commands from nexus-audio
    Audio {
        #[command(subcommand)]
        command: AudioCommands,
    },
    /// Screen interface commands from nexus-screen
    Screen {
        #[command(subcommand)]
        command: ScreenCommands,
    },
    /// GUI interface commands from nexus-gui
    Gui {
        #[command(subcommand)]
        command: GuiCommands,
    },
    /// Core agent framework commands from nexus-core
    Core {
        #[command(subcommand)]
        command: CoreCommands,
    },
}
