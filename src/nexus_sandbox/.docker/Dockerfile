# Build stage
FROM rustlang/rust:nightly AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    musl-tools \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /build

# Copy workspace root Cargo.toml (for workspace configuration)
# This Dockerfile must be built with workspace root as build context:
# 1. From workspace root: docker build -f src/nexus_sandbox/.docker/Dockerfile -t nexus_sandbox .
# 2. From nexus_sandbox directory: docker build -f .docker/Dockerfile -t nexus_sandbox ../..
#    (Note: ../.. sets the build context to workspace root)
COPY Cargo.toml Cargo.lock* ./

# Copy all workspace members
# Cargo needs all workspace members to be present to resolve the workspace
# We copy the entire src directory, but .dockerignore should exclude unnecessary files
COPY src ./src

# Build the binary from workspace root with musl target for Alpine compatibility
# Using BuildKit cache mounts for faster rebuilds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    rustup target add x86_64-unknown-linux-musl && \
    cargo build --release --target x86_64-unknown-linux-musl -p nexus_sandbox && \
    cp /build/target/x86_64-unknown-linux-musl/release/nexus_sandbox /build/nexus_sandbox && \
    test -f /build/nexus_sandbox || (echo "Binary not found" && find /build/target/x86_64-unknown-linux-musl/release -name "*nexus*" -type f && exit 1)

# Runtime stage
FROM alpine:latest

# Build argument for Python version (default: 3.14)
ARG PYTHON_VERSION=3.14

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    curl \
    bash

# Install uv (Python package manager)
# uv installs to ~/.local/bin by default
RUN curl -LsSf https://astral.sh/uv/install.sh | sh

# Add uv to PATH (it installs to /root/.local/bin)
ENV PATH="/root/.local/bin:$PATH"

# Verify uv installation
RUN uv --version

# Pre-install Python version to avoid download messages on first run
RUN uv python install ${PYTHON_VERSION} && \
    uv python list

# Copy the binary from builder (copied to non-cached location)
COPY --from=builder /build/nexus_sandbox /usr/local/bin/nexus_sandbox

# Set working directory for Python scripts
WORKDIR /workspace

# Default entrypoint
ENTRYPOINT ["nexus_sandbox"]

# Example usage:
# IMPORTANT: The build context must be the workspace root directory!
# 
# Option 1 - From workspace root:
#   docker build -f src/nexus_sandbox/.docker/Dockerfile -t nexus_sandbox .
#
# Option 2 - From nexus_sandbox directory (../.. sets context to workspace root):
#   docker build -f .docker/Dockerfile -t nexus_sandbox ../..
# 
# Run: docker run --rm nexus_sandbox exec-code --code 'print("Hello from container!")'
# Or: echo 'print("Hello!")' | docker run --rm -i nexus_sandbox exec-code
