#!/bin/bash
# Get the script directory
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# Go up 2 levels to reach workspace root (from src/nexus_mcp/ to nexus-agents/)
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$WORKSPACE_ROOT" || exit 1
exec cargo run --package nexus_mcp --bin nexus-mcp-server

