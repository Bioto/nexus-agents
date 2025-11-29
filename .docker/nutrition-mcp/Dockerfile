# Dockerfile for building nutrition-mcp-server
# Uses stable Rust and SQLx offline mode
#
# Build:
#   DOCKER_BUILDKIT=1 docker build -f .docker/nutrition-mcp/Dockerfile -t bioto/mcp:latest .
#
# Prerequisites:
#   Run `cargo sqlx prepare` in src/x_toolbox/ to update the .sqlx cache
#   whenever database queries change.

FROM rust:1.91-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev

WORKDIR /build

# Copy Cargo.toml and Cargo.lock
COPY Cargo.lock ./
COPY src/x_toolbox/Cargo.toml ./src/x_toolbox/

# Transform Cargo.toml: replace workspace refs with explicit versions
RUN sed -i \
    -e 's/clap = { workspace = true }/clap = { version = "4", features = ["derive", "env"] }/' \
    -e 's/tokio = { workspace = true, features = \["full"\] }/tokio = { version = "1", features = ["full"] }/' \
    -e 's/uuid = { workspace = true }/uuid = { version = "1", features = ["v4", "serde"] }/' \
    -e 's/reqwest = { workspace = true }/reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "multipart", "rustls-tls"] }/' \
    ./src/x_toolbox/Cargo.toml

# Copy source code and SQLx offline cache
COPY src/x_toolbox/src ./src/x_toolbox/src
COPY src/x_toolbox/migrations ./src/x_toolbox/migrations
COPY src/x_toolbox/.sqlx ./src/x_toolbox/.sqlx

# Build the binary with SQLx offline mode
ENV CARGO_TARGET_DIR=/build/target
ENV SQLX_OFFLINE=true
RUN cd src/x_toolbox && \
    cargo build --release --bin nutrition-mcp-server && \
    cp /build/target/release/nutrition-mcp-server /nutrition-mcp-server

# Runtime stage
FROM alpine:3.20 AS runtime

RUN apk add --no-cache ca-certificates

# Copy binary
COPY --from=builder /nutrition-mcp-server /usr/local/bin/nutrition-mcp-server

# Copy migrations (sqlx::migrate! runs at startup)
WORKDIR /app/src/x_toolbox
COPY --from=builder /build/src/x_toolbox/migrations ./migrations

# Default environment (override in docker-compose)
ENV POSTGRES_HOST=postgres
ENV POSTGRES_PORT=5432
ENV POSTGRES_DATABASE=nutrition
ENV POSTGRES_USER=postgres
ENV POSTGRES_PASSWORD=postgres
ENV DATABASE_URL=postgresql://postgres:postgres@postgres:5432/nutrition

EXPOSE 8002

ENTRYPOINT ["nutrition-mcp-server"]
CMD ["--transport", "http", "--bind", "0.0.0.0:8002"]

