#!/bin/bash
# Build script for nexus_py Docker image
# This script builds the Docker image from the workspace root
#
# Usage:
#   ./build.sh                          # Build with default Python 3.14
#   PYTHON_VERSION=3.12 ./build.sh      # Build with Python 3.12
#   PYTHON_VERSION=3.11 IMAGE_TAG=v1.0 ./build.sh  # Custom Python and tag
#
# Environment variables:
#   PYTHON_VERSION  - Python version to pre-install (default: 3.14)
#   IMAGE_NAME      - Docker image name (default: nexus_py)
#   IMAGE_TAG       - Docker image tag (default: latest)
#   DOCKERFILE_PATH - Path to Dockerfile (default: src/nexus_py/.docker/Dockerfile)
#   BUILD_CONTEXT   - Build context directory (default: .)

set -e

# Default values (matching DockerConfig defaults)
IMAGE_NAME="${IMAGE_NAME:-nexus_py}"
IMAGE_TAG="${IMAGE_TAG:-latest}"
DOCKERFILE_PATH="${DOCKERFILE_PATH:-src/nexus_py/.docker/Dockerfile}"
BUILD_CONTEXT="${BUILD_CONTEXT:-.}"
PYTHON_VERSION="${PYTHON_VERSION:-3.14}"

# Full image name
FULL_IMAGE_NAME="${IMAGE_NAME}:${IMAGE_TAG}"

# Get the script directory and workspace root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Script is in src/nexus_py/.docker, so workspace root is ../../.. from script
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

# Change to workspace root for build context
cd "${WORKSPACE_ROOT}"

# Verify Dockerfile exists
if [ ! -f "${DOCKERFILE_PATH}" ]; then
    echo "Error: Dockerfile not found at ${DOCKERFILE_PATH}" >&2
    exit 1
fi

# Verify build context exists
if [ ! -d "${BUILD_CONTEXT}" ]; then
    echo "Error: Build context directory does not exist: ${BUILD_CONTEXT}" >&2
    exit 1
fi

echo "Building Docker image: ${FULL_IMAGE_NAME}"
echo "Dockerfile: ${DOCKERFILE_PATH}"
echo "Build context: ${BUILD_CONTEXT}"
echo "Python version: ${PYTHON_VERSION}"
echo ""

# Build the image
docker build \
    --build-arg PYTHON_VERSION="${PYTHON_VERSION}" \
    -f "${DOCKERFILE_PATH}" \
    -t "${FULL_IMAGE_NAME}" \
    "${BUILD_CONTEXT}"

echo ""
echo "Docker image built successfully: ${FULL_IMAGE_NAME}"

