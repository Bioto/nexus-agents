# Nexus Agents Architecture

## Naming Conventions

This project follows standard Rust naming conventions with a specific pattern for workspace crates:

### Directory Names
- **Format**: `snake_case`
- **Examples**: `nexus_audio`, `nexus_core`, `nexus_mcp`
- **Location**: `src/nexus_*`

### Package Names (Cargo.toml)
- **Format**: `kebab-case`
- **Examples**: `nexus-audio`, `nexus-core`, `nexus-mcp`
- **Rationale**: Package names in Cargo.toml use kebab-case, which is the standard Rust convention for crate names

### Binary Names
- **Format**: `kebab-case`
- **Examples**: `nexus-audio`, `nexus-core`, `nexus-mcp-server`
- **Rationale**: Binary names match package names for consistency

### Library Names (lib.rs)
- **Format**: `snake_case`
- **Examples**: `nexus_audio`, `nexus_core`, `nexus_mcp`
- **Rationale**: Library names use snake_case, which matches the directory structure

### Summary
- **Directories**: `nexus_audio/` (snake_case)
- **Package**: `nexus-audio` (kebab-case in Cargo.toml)
- **Binary**: `nexus-audio` (kebab-case)
- **Library**: `nexus_audio` (snake_case in code)

This pattern ensures:
1. Directories are readable and match Rust module conventions
2. Package names follow Cargo's kebab-case standard
3. Binary names are user-friendly (kebab-case is more readable in CLI)
4. Library names match Rust's snake_case convention for modules

## Module Organization

### Core Modules
- **`nexus`** - Main CLI orchestrator (high-level commands only)
- **`nexus_core`** - Core agent framework, models, services, tools
- **`nexus_mcp`** - MCP protocol implementation and utilities
- **`nexus_sandbox`** - Code execution sandbox

### Interface Modules
- **`nexus_audio`** - Audio recording and voice interfaces
- **`nexus_screen`** - Screen capture and recording
- **`nexus_gui`** - GUI components
- **`nexus_logger`** - Event logging and analysis

### Tool Modules
- **`x_toolbox`** - Domain-specific tools (nutrition, etc.)

## Command Organization Principles

1. **High-level orchestration** → `nexus` CLI
   - Commands that coordinate multiple modules
   - User-facing entry points

2. **Core functionality** → `nexus_core` CLI
   - Agent framework commands
   - Core tool testing

3. **Module-specific utilities** → Respective module CLIs
   - MCP utilities → `nexus_mcp`
   - Audio utilities → `nexus_audio`
   - Screen utilities → `nexus_screen`

## Dependency Hierarchy

```
nexus (orchestrator)
├── nexus-core
│   ├── nexus_sandbox
│   └── nexus_mcp
│       └── x_toolbox
├── nexus-audio
├── nexus-screen
└── nexus-gui

nexus_logger
├── nexus-core
├── nexus-screen
└── nexus-audio
```

No circular dependencies are allowed.

