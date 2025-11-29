# Nexus Agents

A modular Rust framework for building and managing AI agents with multi-agent swarm coordination, voice interfaces, screen capture, code execution sandboxes, and Model Context Protocol (MCP) integration.

## Overview

Nexus Agents is a comprehensive AI agent framework organized as a Cargo workspace with multiple specialized modules. It provides tools for building conversational AI agents that can coordinate tasks, interact with voice and screen interfaces, execute code safely, and integrate with MCP servers.

## Architecture

The project is structured as a workspace with the following modules:

### Core Modules

- **`nexus`** - Main CLI orchestrator that provides a unified interface to all modules
- **`nexus_core`** - Core agent framework with swarm coordination, chat interfaces, and tool execution
- **`nexus_audio`** - Audio recording, voice listening, and text-to-speech capabilities
- **`nexus_screen`** - Screen recording and screenshot capture functionality
- **`nexus_logger`** - Event logging, screen recording, and AI-powered click context analysis
- **`nexus_mcp`** - Model Context Protocol server implementation and client tools
- **`nexus_sandbox`** - Docker-based code execution sandbox for safe Python execution
- **`nexus_gui`** - GUI components built with Iced framework

## Installation

### Prerequisites

- Rust 1.70+ (edition 2021)
- Docker (for `nexus_sandbox` module)
- System dependencies (see module-specific sections below)

### Configuration

```bash
# Copy environment template and configure
cp env.example .env
# Edit .env with your API keys and settings
```

### Build

```bash
# Build all modules
cargo build

# Build specific module
cargo build -p nexus-core
cargo build -p nexus-audio
cargo build -p nexus-screen
cargo build -p nexus-mcp
cargo build -p nexus-sandbox
cargo build -p nexus-gui
cargo build -p nexus
```

## Module Details

### `nexus` - Main CLI

The primary entry point that orchestrates all modules through a unified CLI interface.

**Usage:**
```bash
# Audio commands
nexus audio record [OPTIONS]
nexus audio listen [OPTIONS]
nexus audio speak [OPTIONS]
nexus audio test-voice [OPTIONS]

# Screen commands
nexus screen screenshot [OPTIONS]
nexus screen record [OPTIONS]

# GUI commands
nexus gui show [OPTIONS]

# Core agent commands
nexus core chat [OPTIONS]

# MCP agent commands
nexus mcp-agent [OPTIONS]

# Testing commands
nexus test-python [OPTIONS]
nexus search-mcp-tools [OPTIONS]
nexus test-mcp-flow [OPTIONS]
```

### `nexus_core` - Core Agent Framework

Provides the foundation for building AI agents with:
- **Agent System**: Configurable agents with custom system prompts and tools
- **Swarm Coordination**: Multi-agent task decomposition and parallel execution
- **Chat Interface**: Interactive TUI for conversational agents
- **Tool Registry**: Extensible tool system (calculator, Python execution, etc.)
- **LLM Client**: Integration with OpenAI-compatible APIs

**Key Features:**
- Task decomposition and dependency management
- Agent-to-task assignment optimization
- Streaming chat responses
- PDF document upload and processing
- Tool discovery and execution

**Usage:**
```bash
# Standalone chat interface
nexus-core chat [OPTIONS]

# With streaming
nexus-core chat --stream

# With specific agent
nexus-core chat --agent calculator

# With custom model
nexus-core chat --model gpt-4o-mini
```

**Available Agents:**
- `calculator` - Mathematical calculations
- `swarm_coordinator` - Multi-agent task coordination
- `task_decomposition` - Task breakdown and planning
- `swarm_router` - Task-to-agent assignment

**Available Tools:**
- `calculator` - Mathematical operations
- `execute_python` - Safe Python code execution (via sandbox)
- Tool discovery from MCP servers

### `nexus_audio` - Audio Interface

Provides voice interaction capabilities:
- **Audio Recording**: Capture audio from microphone
- **Voice Listening**: Real-time voice activity detection and transcription
- **Text-to-Speech**: Convert text to speech (supports multiple backends)
- **TUI Interface**: Terminal-based audio visualization

**Usage:**
```bash
# Record audio
nexus-audio record --output recording.wav

# Listen for voice input
nexus-audio listen

# Speak text
nexus-audio speak "Hello, world!"

# Test voice capabilities
nexus-audio test-voice
```

**Dependencies:**
- `cpal` - Cross-platform audio I/O
- `whisper-rs` - Speech recognition
- `rodio` - Audio playback
- ONNX models for VAD (Voice Activity Detection)

### `nexus_screen` - Screen Capture

Screen recording and screenshot functionality:
- **Screen Recording**: Capture screen with configurable FPS and duration
- **Screenshot**: Capture single screen images
- **Window Info**: Query window geometry and information
- **TUI Interface**: Terminal-based recording controls

**Usage:**
```bash
# Record screen (default: 60fps, until Ctrl+C)
nexus-screen record

# Record with custom FPS
nexus-screen record --fps 30

# Record for specific duration
nexus-screen record --duration 10

# Record without audio
nexus-screen record --no-audio

# Take screenshot
nexus-screen screenshot --output screenshot.png
```

**System Dependencies (Linux):**
```bash
sudo apt-get install -y \
    libpipewire-0.3-dev \
    libgbm-dev \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libavfilter-dev \
    libavdevice-dev \
    libswscale-dev \
    libswresample-dev \
    pkg-config
```

### `nexus_logger` - Event Logging and Click Context Analysis

Comprehensive logging system with screen/input capture and AI-powered click analysis:
- **Unified Recording**: Synchronized screen recording with keyboard/mouse events
- **Click Context Analysis**: AI-powered analysis of user clicks with contextual frame capture
- **Database Integration**: Store events in ClickHouse for analytics
- **Event Capture**: Track keyboard, mouse, and custom events with precise timestamps

**Key Features:**
- Real-time event logging with video synchronization
- AI analysis of click context using vision models
- Configurable frame capture (1-6 frames per click)
- Video extraction or live screen capture modes
- Frame-by-frame descriptions and summaries
- ClickHouse integration for querying event history

**Usage:**
```bash
# Start unified recording with click analysis
nexus-logger unified --session my-session

# Capture with custom frame settings
NEXUS_LOGGER_CLICK_CONTEXT_FRAME_COUNT=5 \
NEXUS_LOGGER_CLICK_CONTEXT_FRAME_INTERVAL_MS=500 \
nexus-logger unified --session my-session

# Query recent click analyses
python scripts/query_click_analysis.py
```

**Configuration:**
See `env.example` for complete click context configuration options including:
- Frame count and interval timing
- AI model selection for analysis
- Token limits and cost control
- Monitor selection and frame saving

**Dependencies:**
- ClickHouse for event storage
- OpenAI-compatible API for vision analysis
- `nexus_screen` for screen capture
- FFmpeg for video frame extraction

### `nexus_mcp` - Model Context Protocol

MCP server implementation and tooling:
- **MCP Server**: Standards-compliant MCP server with tools and resources
- **Code Generation**: Generate API clients from MCP server definitions
- **External Server Integration**: Connect to external MCP servers
- **Multi-Server Management**: Start and manage multiple MCP servers

**Usage:**
```bash
# Run MCP server (stdio mode)
nexus-mcp-server

# Run MCP server (HTTP mode)
nexus-mcp-server --transport http --bind 127.0.0.1:8000

# Start multiple servers from config
nexus-mcp start-servers --config mcp-servers.toml

# Generate Python API client
nexus-mcp generate-code --output clients/python

# Generate external server tools
nexus-mcp generate-external --config mcp-servers.toml

# Interactive shell
nexus-mcp shell
```

**Configuration:**
See `mcp-servers.example.toml` for server configuration format.

**Available Tools:**
- `echo` - Echo tool for testing
- `increment_counter` - Counter management
- `get_counter` - Counter retrieval
- `add` - Addition operation
- `generate_code_api` - Code generation from MCP tools

### `nexus_sandbox` - Code Execution Sandbox

Docker-based sandbox for safe code execution:
- **Python Execution**: Execute Python code in isolated containers
- **Docker Integration**: Container lifecycle management
- **Shell Execution**: Run shell commands in sandboxed environment
- **Python 3.03 Support**: Legacy Python version support

**Usage:**
```bash
# Execute Python code
nexus-sandbox exec-code --code "print('Hello, World!')"

# Execute in Docker container
nexus-sandbox docker-exec --image python:3.11 --command "python -c 'print(42)'"

# Run shell command
nexus-sandbox shell --command "ls -la"

# Python 3.03 execution
nexus-sandbox py03 --code "print('Hello')"
```

**Features:**
- Automatic container cleanup
- Resource limits and isolation
- Temporary file management
- Error handling and reporting

### `nexus_gui` - GUI Components

Iced-based GUI components:
- **Microphone Icon**: Visual microphone state indicator
- **Recording State**: UI components for recording status

**Usage:**
```bash
# Show GUI interface
nexus-gui show
```

## Environment Variables

See `env.example` for a complete configuration template with all available options.

### Core Configuration

- `DEFAULT_MODEL` - Default LLM model for general text processing (default: `gpt-5-nano-2025-08-07`)
- `LLM_API_KEY` - API key for general LLM/text processing (falls back to `OPENAI_API_KEY` for backward compatibility)
- `LLM_BASE_URL` - Base URL for general LLM API (falls back to `OPENAI_BASE_URL`, default: `https://api.openai.com/v1`)
- `OPENAI_API_KEY` - OpenAI API key (deprecated, use `LLM_API_KEY`; kept for backward compatibility)
- `OPENAI_BASE_URL` - Base URL for OpenAI-compatible API (deprecated, use `LLM_BASE_URL`; kept for backward compatibility)
- `VISION_API_KEY` - API key for vision/image processing models (falls back to `LLM_API_KEY` or `OPENAI_API_KEY`)
- `VISION_BASE_URL` - Base URL for vision/image processing API (falls back to `LLM_BASE_URL` or `OPENAI_BASE_URL`, default: `https://api.openai.com/v1`)
- `VISION_MODEL` - Model for vision/image processing tasks (falls back to `NEXUS_LOGGER_CLICK_CONTEXT_MODEL`, default: `gpt-4o-mini`)
- `RUST_LOG` - Log level (`trace`, `debug`, `info`, `warn`, `error`)

### Module-Specific

#### `nexus_screen`
- `PKG_CONFIG_PATH` - Path for pkg-config (Linux)

#### `nexus_logger` - Click Context Analysis
- `NEXUS_LOGGER_CLICK_CONTEXT_ENABLED` - Enable/disable click analysis (default: `true`)
- `NEXUS_LOGGER_CLICK_CONTEXT_FRAME_COUNT` - Number of frames to capture per click, 1-6 (default: `3`)
- `NEXUS_LOGGER_CLICK_CONTEXT_FRAME_INTERVAL_MS` - Delay between frames in milliseconds, min 250 (default: `1000`)
- `NEXUS_LOGGER_CLICK_CONTEXT_MODEL` - AI model for per-frame analysis (deprecated, use `VISION_MODEL`; kept for backward compatibility, default: `gpt-4o-mini`)
- `NEXUS_LOGGER_CLICK_CONTEXT_SUMMARY_MODEL` - AI model for generating summary (deprecated, use `VISION_MODEL`; kept for backward compatibility, default: `gpt-4o-mini`)
- `NEXUS_LOGGER_CLICK_CONTEXT_FRAME_TOKENS` - Max tokens for per-frame analysis (default: `200`)
- `NEXUS_LOGGER_CLICK_CONTEXT_SUMMARY_TOKENS` - Max tokens for summary (default: `120`)
- `NEXUS_LOGGER_CLICK_CONTEXT_MONITOR` - Monitor index to capture from (optional)
- `NEXUS_LOGGER_CLICK_CONTEXT_SAVE_FRAMES` - Directory to save frame images (optional)

**Note**: Click context analysis uses vision/image processing models. Set `VISION_API_KEY`, `VISION_BASE_URL`, and `VISION_MODEL` to use different models/APIs for image processing vs. general text processing.

#### ClickHouse Configuration
- `CLICKHOUSE_HOST` - ClickHouse host (default: `localhost`)
- `CLICKHOUSE_PORT` - ClickHouse HTTP port (default: `8123`)
- `CLICKHOUSE_USER` - ClickHouse user (default: `default`)
- `CLICKHOUSE_PASSWORD` - ClickHouse password (default: `default`)
- `CLICKHOUSE_DATABASE` - ClickHouse database (default: `default`)

#### MCP Servers
- MCP server configuration via `mcp-servers.toml`

## Logging

All modules support structured logging:
- Console output (configurable via `RUST_LOG`)
- File logging to `logs/` directory with timestamps
- Log files named: `nexus_{module}_{timestamp}.log`

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run tests for specific module
cargo test -p nexus-core
cargo test -p nexus-sandbox
```

### Code Quality

```bash
# Format code
cargo fmt

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
```

### Benchmarks

```bash
# Run benchmarks
cargo bench -p nexus-screen
```

## Project Structure

```
nexus-agents/
├── Cargo.toml              # Workspace configuration
├── Makefile                # Build shortcuts
├── env.example             # Environment configuration template
├── src/
│   ├── nexus/             # Main CLI orchestrator
│   ├── nexus_core/        # Core agent framework
│   ├── nexus_audio/       # Audio interface
│   ├── nexus_screen/      # Screen capture
│   ├── nexus_logger/      # Event logging & click analysis
│   ├── nexus_mcp/         # MCP server/client
│   ├── nexus_sandbox/     # Code execution sandbox
│   └── nexus_gui/         # GUI components
├── scripts/                # Utility scripts (queries, analysis)
├── servers/                # MCP server implementations (Python)
└── logs/                   # Application logs
```

## Examples

### Basic Chat Agent

```bash
nexus core chat --stream
```

### Multi-Agent Swarm

```bash
nexus core chat --agent swarm_coordinator --stream
```

### Voice-Enabled Agent

```bash
# Record voice input
nexus audio record --output input.wav

# Process with agent
nexus core chat --stream
```

### MCP Agent with Tools

```bash
nexus mcp-agent --servers-dir servers --stream
```

### Screen Recording

```bash
nexus screen record --fps 30 --duration 60 --output demo.mp4
```

## Contributing

This project follows Rust best practices:
- Use `cargo fmt` for formatting
- Run `cargo clippy` before committing
- Write tests for new features
- Document public APIs
- Follow workspace dependency management

## License

[Add license information here]

## Acknowledgments

Built with:
- `tokio` - Async runtime
- `clap` - CLI parsing
- `ratatui` - Terminal UI
- `iced` - GUI framework
- `rmcp` - MCP protocol implementation
- `bollard` - Docker client
- `ffmpeg-next` - Screen recording
- `whisper-rs` - Speech recognition

