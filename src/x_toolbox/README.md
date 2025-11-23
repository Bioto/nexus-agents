# X Toolbox - Nutrition Module

A collection of utility tools, starting with a nutrition/recipe management system.

## Features

### Nutrition Module
- Full CRUD operations for ingredients, recipes, and nutritional information
- PostgreSQL database with migrations
- **REST API server** - HTTP JSON API for web applications
- **MCP Server** - Model Context Protocol for AI agent integration
- **CLI interface** - Command-line tools
- Searchable recipes and ingredients
- Nutritional calculation per recipe

### Three Access Modes

1. **CLI** - Direct command-line interaction
2. **REST API** - HTTP/JSON endpoints for web apps
3. **MCP Server** - AI agent integration via Model Context Protocol

## Quick Start

### 1. Start the Database

Using Make:
```bash
make nutrition-db-start
```

Or using the CLI:
```bash
x-toolbox nutrition db start
```

Or using Docker Compose directly:
```bash
cd src/x_toolbox
docker compose up -d
```

### 2. Set Environment Variables

```bash
export POSTGRES_HOST=localhost
export POSTGRES_PORT=5432
export POSTGRES_DATABASE=nutrition
export POSTGRES_USER=postgres
export POSTGRES_PASSWORD=postgres
```

### 3. Use the CLI

```bash
# Add an ingredient
x-toolbox nutrition add ingredient "Chicken Breast" --description "Boneless, skinless"

# Add nutritional info
x-toolbox nutrition add nutritional-info \
  --ingredient-id <UUID> \
  --calories 165 \
  --protein 31 \
  --carbs 0 \
  --fat 3.6

# List ingredients
x-toolbox nutrition list ingredients

# Search ingredients
x-toolbox nutrition search ingredients "Chicken"

# Create a recipe
x-toolbox nutrition add recipe "Grilled Chicken" \
  --description "Simple grilled chicken" \
  --servings 2 \
  --prep-time 10 \
  --cook-time 15

# List recipes
x-toolbox nutrition list recipes
```

### 4. Start the REST API Server

```bash
x-toolbox nutrition server --host 127.0.0.1 --port 8080
```

Then access the API at:
- `GET /api/nutrition/ingredients` - List all ingredients
- `GET /api/nutrition/ingredients/:id` - Get ingredient by ID
- `POST /api/nutrition/ingredients` - Create ingredient
- `PUT /api/nutrition/ingredients/:id` - Update ingredient
- `DELETE /api/nutrition/ingredients/:id` - Delete ingredient
- `GET /api/nutrition/recipes` - List all recipes
- `GET /api/nutrition/recipes/:id` - Get recipe with full details
- `POST /api/nutrition/recipes` - Create recipe
- `PUT /api/nutrition/recipes/:id` - Update recipe
- `DELETE /api/nutrition/recipes/:id` - Delete recipe
- `GET /api/nutrition/recipes/:id/nutrition` - Calculate nutritional info

### 5. Start the MCP Server (for AI agents)

```bash
# Standalone
make nutrition-mcp-start

# Or via multi-server launcher
cargo run --bin nexus_mcp -- start-servers --config src/nexus_mcp/mcp-servers.toml
```

See [MCP-SERVER.md](MCP-SERVER.md) for detailed MCP server documentation and AI agent integration.

## Makefile Commands

```bash
# Start database
make nutrition-db-start

# Stop database
make nutrition-db-stop

# Reset database (removes all data)
make nutrition-db-reset

# Run migrations
make nutrition-db-migrate

# Check database status
make nutrition-db-status

# View database logs
make nutrition-db-logs

# Prepare SQLx query cache (needed after changing SQL queries)
make nutrition-prepare-sqlx

# Start nutrition MCP server (for AI agents)
make nutrition-mcp-start

# Start nutrition MCP server in stdio mode
make nutrition-mcp-stdio
```

## Database Schema

- **ingredients** - Base ingredient information
- **nutritional_info** - Nutritional data per ingredient (per 100g)
- **recipes** - Recipe information (name, servings, prep/cook time)
- **recipe_ingredients** - Many-to-many junction table with quantities
- **recipe_steps** - Ordered cooking instructions

## Development

### SQLx Query Cache

This project uses sqlx's compile-time query checking. The `.sqlx/` cache is already generated and checked into version control. If you modify SQL queries:

```bash
make nutrition-prepare-sqlx
```

Or manually:
```bash
export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/nutrition"
cd src/x_toolbox
cargo sqlx prepare
```

## Testing

All interfaces tested and working:
- ✅ **Database:** start/stop/status
- ✅ **CLI:** Add ingredients, nutritional info, and recipes
- ✅ **CLI:** List and search operations
- ✅ **REST API:** All CRUD endpoints working
- ✅ **MCP Server:** 14 tools for AI agent integration
- ✅ **Makefile:** All commands functional
- ✅ **Integration:** Works with nexus_mcp multi-server launcher

## Next Steps

Future enhancements could include:
- Recipe ingredient management (add/remove ingredients from existing recipes)
- Recipe step management (add/update/delete steps)
- Advanced search and filtering
- Unit conversion system
- Import/export recipes (JSON)
- Meal planning features

