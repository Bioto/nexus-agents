mod cli;

use clap::Parser;
use cli::{Cli, Commands};
use nexus_audio::{self, Commands as AudioCommands};
use nexus_core::{self, Commands as CoreCommands};
use nexus_gui::{self, Commands as GuiCommands};
use nexus_screen::{self, Commands as ScreenCommands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Audio { command } => match command {
            AudioCommands::Record(args) => nexus_audio::run_record(args)?,
            AudioCommands::Listen(args) => nexus_audio::run_listen(args)?,
            AudioCommands::Speak(args) => nexus_audio::run_speak(args).await?,
            AudioCommands::TestVoice(args) => nexus_audio::run_test_voice(args).await?,
        },
        Commands::Screen { command } => match command {
            ScreenCommands::Screenshot(args) => nexus_screen::run_screenshot(args)?,
            ScreenCommands::Record(args) => nexus_screen::run_record(args)?,
        },
        Commands::Gui { command } => match command {
            GuiCommands::Show(args) => nexus_gui::run_show(args)?,
        },
        Commands::Core { command } => match command {
            CoreCommands::Chat(args) => nexus_core::run_chat(args)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        },
    }

    Ok(())
}
