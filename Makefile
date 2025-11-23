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