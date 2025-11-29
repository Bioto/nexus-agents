# Docker Configuration

This directory contains Docker configurations for various services in the nexus-agents project, organized by stack.

## Directory Structure

```
.docker/
├── README.md                 # This file
├── nexus/                    # Main nexus-agents build
│   └── Dockerfile
├── nutrition-mcp/            # Nutrition MCP server stack
│   ├── docker-compose.yaml
│   ├── Dockerfile
│   ├── nginx/
│   │   ├── Dockerfile
│   │   ├── default.conf
│   │   ├── https.conf
│   │   └── entrypoint.sh
│   ├── ssl/
│   │   ├── README.md
│   │   ├── cert.pem          # (gitignored)
│   │   └── key.pem           # (gitignored)
│   └── test-server/
│       ├── Dockerfile
│       └── server.py
└── clickhouse/               # ClickHouse analytics stack
    ├── docker-compose.yaml
    └── config/
        └── cors.xml
```

## Stacks

### 1. Nexus (Main Build)

Full workspace build for the entire nexus-agents project.

```bash
# Build from workspace root
docker build -f .docker/nexus/Dockerfile -t nexus-agents .

# Run
docker run --rm nexus-agents --help
```

### 2. Nutrition MCP Server

Standalone MCP server for nutrition/recipe management. Includes:
- PostgreSQL database (personal + optional public instance)
- nginx reverse proxy with SSL support
- Test server for debugging

```bash
# Build the MCP server image
make nutrition-mcp-docker-build

# Start personal instance
make nutrition-mcp-docker-up

# Start with public instance
docker compose -f .docker/nutrition-mcp/docker-compose.yaml --profile public up -d

# Apply migrations
make nutrition-mcp-docker-migrate

# View logs
make nutrition-mcp-docker-logs

# Stop
make nutrition-mcp-docker-down
```

**Endpoints:**
- Direct (local dev): `http://localhost:8002/mcp`
- Via nginx (production): `https://your-domain/mcp`
- Public instance: `https://your-domain/mcp-public`

**SSL Setup:** See `nutrition-mcp/ssl/README.md`

### 3. ClickHouse

Analytics database with web UI.

```bash
# Start
make clickhouse-up

# View logs
make clickhouse-logs

# Stop
make clickhouse-down
```

**Endpoints:**
- HTTP interface: `http://localhost:8123`
- Native protocol: `localhost:9000`
- Web UI (ch-ui): `http://localhost:5521`

## Makefile Targets

| Target | Description |
|--------|-------------|
| `clickhouse-up` | Start ClickHouse stack |
| `clickhouse-down` | Stop ClickHouse stack |
| `clickhouse-logs` | View ClickHouse logs |
| `clickhouse-status` | Show ClickHouse container status |
| `clickhouse-restart` | Restart ClickHouse stack |
| `nutrition-mcp-docker-build` | Build nutrition MCP server image |
| `nutrition-mcp-docker-up` | Start nutrition MCP stack |
| `nutrition-mcp-docker-down` | Stop nutrition MCP stack |
| `nutrition-mcp-docker-logs` | View nutrition MCP logs |
| `nutrition-mcp-docker-migrate` | Apply database migrations |
| `nutrition-mcp-docker-reset` | Reset stack (destroys data) |

