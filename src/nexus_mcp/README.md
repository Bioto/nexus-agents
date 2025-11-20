# Nexus MCP

## Overview
Nexus MCP is a Model Context Protocol (MCP) server implementation in Rust, providing a set of tools, resources, and prompts for AI agents.

## Code Quality Improvements (Latest Update)

### Error Handling
- Introduced `NexusError` (using `thiserror`) for unified and robust error handling across the codebase.
- Replaced ad-hoc `Box<dyn Error>` and custom error enums with `NexusError`.
- Improved error context and messages.

### Configuration
- Switched to `regex` for reliable environment variable expansion in configuration files (`${VAR}` and `$VAR` syntax).
- Improved validation logic in `config.rs`.

### Client Stability
- Enhanced `mcp_client.rs` with a more robust Server-Sent Events (SSE) parser that handles multi-line data and standard SSE formats.
- Improved error reporting during client initialization and tool fetching.

### Concurrency
- Optimized `NexusMcpServer` to use `AtomicI32` for counters instead of `Mutex<i32>`, reducing overhead.

### Fixes
- Fixed transport selection bug in `nexus-mcp-server` binary (now respects `--transport` flag).
- Fixed `stdio` transport instantiation.

## Usage

### Running the Server
```bash
# Run in stdio mode (default)
cargo run --bin nexus-mcp-server

# Run in HTTP mode
cargo run --bin nexus-mcp-server -- --transport http --bind 127.0.0.1:8000
```

### CLI Tools
```bash
# Start multiple servers from config
cargo run --bin nexus-mcp -- start-servers --config mcp-servers.toml

# Generate Python API client
cargo run --bin nexus-mcp -- generate-code --output clients/python
```

