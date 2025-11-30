pub mod commands;

use clap::{Parser, Subcommand};
use nexus_core::Commands as CoreCommands;
use nexus_exporter::Commands as ExporterCommands;
use nexus_gui::Commands as GuiCommands;
use nexus_mcp::Commands as McpCommands;
use nexus_recorder::Commands as RecorderCommands;

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
    /// Recording commands (audio, screen, input capture) from nexus-recorder
    Recorder {
        #[command(subcommand)]
        command: RecorderCommands,
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
    /// MCP protocol commands from nexus-mcp
    Mcp {
        #[command(subcommand)]
        command: McpCommands,
    },
    /// Document export commands (PDF, etc.) from nexus-exporter
    Exporter {
        #[command(subcommand)]
        command: ExporterCommands,
    },
    /// Start an MCP agent with access to MCP server tools
    McpAgent(commands::mcp_agent::McpAgentArgs),
    /// Search generated MCP tool files and metadata
    SearchMcpTools(commands::search_mcp_tools::SearchMcpToolsArgs),
    /// Search for an MCP tool and immediately execute it
    TestMcpFlow(commands::test_mcp_flow::TestMcpFlowArgs),
}
