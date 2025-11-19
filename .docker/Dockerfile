# Multi-stage Dockerfile for building nexus-agents on Ubuntu
FROM ubuntu:24.04 AS builder

# Prevent interactive prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive

# Install Rust toolchain and build dependencies
RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    cmake \
    git \
    libssl-dev \
    ca-certificates \
    # libclang is required by bindgen (used by ffmpeg-next)
    libclang-dev \
    # FFmpeg libraries for nexus-screen
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libavfilter-dev \
    libavdevice-dev \
    libswscale-dev \
    libswresample-dev \
    # PipeWire for screen recording
    libpipewire-0.3-dev \
    # GBM for graphics
    libgbm-dev \
    # Audio libraries for nexus-audio
    libasound2-dev \
    libpulse-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Rust using rustup
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Set working directory
WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock* ./
COPY src ./src

# Build the project in release mode
# Using BuildKit cache mounts for faster rebuilds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release

# Runtime stage - minimal Ubuntu image
FROM ubuntu:24.04 AS runtime

# Install runtime dependencies only
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    # FFmpeg runtime libraries
    libavcodec60 \
    libavformat60 \
    libavutil58 \
    libavfilter9 \
    libavdevice60 \
    libswscale7 \
    libswresample4 \
    # PipeWire runtime
    libpipewire-0.3-0 \
    # GBM runtime
    libgbm1 \
    # Audio runtime libraries (libasound2t64 is the actual package on Ubuntu 24.04)
    libasound2t64 \
    libpulse0 \
    && rm -rf /var/lib/apt/lists/*

# Copy binaries from builder
COPY --from=builder /build/target/release/nexus /usr/local/bin/nexus
COPY --from=builder /build/target/release/nexus-core /usr/local/bin/nexus-core
COPY --from=builder /build/target/release/nexus-audio /usr/local/bin/nexus-audio
COPY --from=builder /build/target/release/nexus-screen /usr/local/bin/nexus-screen
COPY --from=builder /build/target/release/nexus-gui /usr/local/bin/nexus-gui
COPY --from=builder /build/target/release/nexus_py /usr/local/bin/nexus_py
COPY --from=builder /build/target/release/nexus-mcp-server /usr/local/bin/nexus-mcp-server

# Set working directory
WORKDIR /workspace

# Default entrypoint
ENTRYPOINT ["nexus"]

# Build instructions:
# From workspace root:
#   docker build -f .docker/Dockerfile -t nexus-agents .
#
# Run:
#   docker run --rm nexus-agents --help

