chat:
	cargo run -- chat --stream

# ClickHouse commands
clickhouse-up:
	docker compose -f .docker/docker-compose.clickhouse.yaml up -d

clickhouse-down:
	docker compose -f .docker/docker-compose.clickhouse.yaml down

clickhouse-logs:
	docker compose -f .docker/docker-compose.clickhouse.yaml logs -f

clickhouse-status:
	docker compose -f .docker/docker-compose.clickhouse.yaml ps

clickhouse-restart:
	docker compose -f .docker/docker-compose.clickhouse.yaml restart

# X Toolbox Nutrition Database commands
nutrition-db-start:
	cd src/x_toolbox && docker compose up -d

nutrition-db-stop:
	cd src/x_toolbox && docker compose down

nutrition-db-reset:
	cd src/x_toolbox && docker compose down -v && docker compose up -d && sleep 5 && docker exec -i x_toolbox_postgres psql -U postgres -d nutrition < migrations/001_initial_schema.sql

nutrition-db-migrate:
	cd src/x_toolbox && docker exec -i x_toolbox_postgres psql -U postgres -d nutrition < migrations/001_initial_schema.sql

nutrition-db-status:
	cd src/x_toolbox && docker compose ps

nutrition-db-logs:
	cd src/x_toolbox && docker compose logs -f

nutrition-prepare-sqlx:
	export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/nutrition" && cd src/x_toolbox && cargo sqlx prepare

# Nutrition MCP Server commands
nutrition-mcp-start:
	export POSTGRES_HOST=localhost && export POSTGRES_PORT=5432 && export POSTGRES_DATABASE=nutrition && export POSTGRES_USER=postgres && export POSTGRES_PASSWORD=postgres && cargo run --bin nutrition-mcp-server -- --transport http --bind 127.0.0.1:8002

nutrition-mcp-stdio:
	export POSTGRES_HOST=localhost && export POSTGRES_PORT=5432 && export POSTGRES_DATABASE=nutrition && export POSTGRES_USER=postgres && export POSTGRES_PASSWORD=postgres && cargo run --bin nutrition-mcp-server -- --transport stdio

# Nutrition CLI commands
# Usage: make nutrition-import FILE=__test_files__/recipes.csv
#        make nutrition-import FILE=__test_files__/recipes.csv SKIP_ERRORS=1
nutrition-import:
	@if [ -z "$(FILE)" ]; then \
		echo "Error: FILE is required. Usage: make nutrition-import FILE=path/to/file.csv"; \
		exit 1; \
	fi
	export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/nutrition" && export POSTGRES_HOST=localhost && export POSTGRES_PORT=5432 && export POSTGRES_DATABASE=nutrition && export POSTGRES_USER=postgres && export POSTGRES_PASSWORD=postgres && cargo run --bin x-toolbox -- nutrition import $(FILE) $(if $(SKIP_ERRORS),--skip-errors,)

# Usage: make nutrition-import-ingredients DIR=__test_files__/FoodData_Central_foundation_food_csv_2025-04-24
#        make nutrition-import-ingredients DIR=__test_files__/FoodData_Central_foundation_food_csv_2025-04-24 SKIP_ERRORS=1
nutrition-import-ingredients:
	@if [ -z "$(DIR)" ]; then \
		echo "Error: DIR is required. Usage: make nutrition-import-ingredients DIR=path/to/usda/csv/directory"; \
		exit 1; \
	fi
	export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/nutrition" && export POSTGRES_HOST=localhost && export POSTGRES_PORT=5432 && export POSTGRES_DATABASE=nutrition && export POSTGRES_USER=postgres && export POSTGRES_PASSWORD=postgres && cargo run --bin x-toolbox -- nutrition import-ingredients $(DIR) $(if $(SKIP_ERRORS),--skip-errors,)

# Usage: make nutrition-import-branded DIR=__test_files__/FoodData_Central_branded_food_csv_2025-04-24
#        make nutrition-import-branded DIR=__test_files__/FoodData_Central_branded_food_csv_2025-04-24 SKIP_ERRORS=1
#        make nutrition-import-branded DIR=__test_files__/FoodData_Central_branded_food_csv_2025-04-24 LIMIT=1000
nutrition-import-branded:
	@if [ -z "$(DIR)" ]; then \
		echo "Error: DIR is required. Usage: make nutrition-import-branded DIR=path/to/usda/csv/directory [LIMIT=N]"; \
		exit 1; \
	fi
	export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/nutrition" && export POSTGRES_HOST=localhost && export POSTGRES_PORT=5432 && export POSTGRES_DATABASE=nutrition && export POSTGRES_USER=postgres && export POSTGRES_PASSWORD=postgres && cargo run --bin x-toolbox -- nutrition import-branded-ingredients $(DIR) $(if $(SKIP_ERRORS),--skip-errors,) $(if $(LIMIT),--limit $(LIMIT),)