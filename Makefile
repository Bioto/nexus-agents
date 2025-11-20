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