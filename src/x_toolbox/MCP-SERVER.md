# Nutrition MCP Server

The Nutrition MCP Server exposes the nutrition module functionality through the Model Context Protocol, allowing AI agents to interact with the recipe and nutrition database.

## Available MCP Tools

### Ingredient Management
1. **create_ingredient** - Create a new ingredient with optional description
2. **get_ingredient** - Get ingredient details by UUID (includes nutritional info if available)
3. **list_ingredients** - List all ingredients with optional search term
4. **update_ingredient** - Update ingredient name and/or description
5. **delete_ingredient** - Delete an ingredient by UUID
6. **search_ingredients** - Search ingredients by name or description

### Recipe Management
7. **create_recipe** - Create a recipe with ingredients and cooking steps
8. **get_recipe** - Get full recipe details including ingredients and steps
9. **list_recipes** - List recipes with optional search/filter by ingredient
10. **update_recipe** - Update recipe metadata (name, description, servings, times)
11. **delete_recipe** - Delete a recipe by UUID
12. **search_recipes** - Search recipes with optional ingredient filter

### Nutritional Information
13. **add_nutritional_info** - Add or update nutritional data for an ingredient (per 100g)
14. **calculate_recipe_nutrition** - Calculate total and per-serving nutrition for a recipe

## Running the Server

### Standalone Mode

**HTTP Transport:**
```bash
# Using Make
make nutrition-mcp-start

# Or directly
cargo run --bin nutrition-mcp-server -- --transport http --bind 127.0.0.1:8002

# Or with environment variables
export POSTGRES_HOST=localhost
export POSTGRES_PORT=5432
export POSTGRES_DATABASE=nutrition
export POSTGRES_USER=postgres
export POSTGRES_PASSWORD=postgres
cargo run --bin nutrition-mcp-server -- --transport http --bind 127.0.0.1:8002
```

**Stdio Transport:**
```bash
make nutrition-mcp-stdio

# Or directly
cargo run --bin nutrition-mcp-server -- --transport stdio
```

### Multi-Server Mode (via nexus_mcp)

The nutrition server is configured in `src/nexus_mcp/mcp-servers.toml`:

```toml
[[servers]]
name = "nutrition"
server_type = "nutrition"
transport = "http"
bind = "127.0.0.1:8002"
path = "/mcp"
```

Start with other servers:
```bash
# Set environment variables
export POSTGRES_HOST=localhost
export POSTGRES_PORT=5432
export POSTGRES_DATABASE=nutrition
export POSTGRES_USER=postgres
export POSTGRES_PASSWORD=postgres

# Start all configured servers
cargo run --bin nexus_mcp -- start-servers --config src/nexus_mcp/mcp-servers.toml
```

## Integration with Cursor AI

Add to your `~/.cursor/mcp.json` or project `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "nutrition": {
      "command": "cargo",
      "args": [
        "run",
        "--bin",
        "nutrition-mcp-server",
        "--",
        "--transport",
        "stdio"
      ],
      "cwd": "/path/to/nexus-agents",
      "env": {
        "POSTGRES_HOST": "localhost",
        "POSTGRES_PORT": "5432",
        "POSTGRES_DATABASE": "nutrition",
        "POSTGRES_USER": "postgres",
        "POSTGRES_PASSWORD": "postgres"
      }
    }
  }
}
```

Or for HTTP transport:

```json
{
  "mcpServers": {
    "nutrition": {
      "url": "http://127.0.0.1:8002/mcp"
    }
  }
}
```

## Prerequisites

1. **Database must be running:**
   ```bash
   make nutrition-db-start
   # Or
   cd src/x_toolbox && docker compose up -d
   ```

2. **Environment variables must be set** (see above examples)

## Server Information

- **Name:** `nutrition-mcp-server`
- **Title:** "Nutrition & Recipe Management"
- **Version:** `0.1.0`
- **Protocol:** MCP 2024-11-05
- **Capabilities:** Tools (14 nutrition/recipe management tools)
- **Default HTTP Endpoint:** `http://127.0.0.1:8002/mcp`

## Tool Examples

### Create an Ingredient
```json
{
  "name": "create_ingredient",
  "arguments": {
    "name": "Chicken Breast",
    "description": "Boneless, skinless chicken breast"
  }
}
```

### Add Nutritional Info
```json
{
  "name": "add_nutritional_info",
  "arguments": {
    "ingredient_id": "90efa6b6-ee71-48bb-8253-feaa0e9eab52",
    "calories_per_100g": 165,
    "protein_g": 31,
    "carbs_g": 0,
    "fat_g": 3.6
  }
}
```

### Create a Recipe
```json
{
  "name": "create_recipe",
  "arguments": {
    "name": "Grilled Chicken",
    "description": "Simple grilled chicken breast",
    "servings": 2,
    "prep_time_minutes": 10,
    "cook_time_minutes": 15,
    "ingredients": [
      {
        "ingredient_id": "90efa6b6-ee71-48bb-8253-feaa0e9eab52",
        "quantity": 300,
        "unit": "g"
      }
    ],
    "steps": [
      "Season the chicken breast with salt and pepper",
      "Grill on medium-high heat for 6-7 minutes per side",
      "Let rest for 5 minutes before serving"
    ]
  }
}
```

### Calculate Recipe Nutrition
```json
{
  "name": "calculate_recipe_nutrition",
  "arguments": {
    "id": "03ac73bd-f90c-41d1-9915-53d5435be006"
  }
}
```

## Troubleshooting

### "Address already in use" error
```bash
# Kill existing server
pkill -f "nutrition-mcp-server"
# Or find and kill by port
lsof -i :8002  # Find PID
kill <PID>
```

### Database connection errors
Make sure the database is running and environment variables are set correctly:
```bash
make nutrition-db-status  # Should show "Up" status
```

### Compilation errors with sqlx
If you see sqlx query validation errors, the query cache may need regeneration:
```bash
make nutrition-prepare-sqlx
```

