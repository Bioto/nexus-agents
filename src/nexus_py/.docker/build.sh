#!/bin/bash
# Build script for nexus_py Docker image
# This script builds the Docker image from the workspace root

set -e

# Default values (matching DockerConfig defaults)
IMAGE_NAME="${IMAGE_NAME:-nexus_py}"
IMAGE_TAG="${IMAGE_TAG:-latest}"
DOCKERFILE_PATH="${DOCKERFILE_PATH:-src/nexus_py/.docker/Dockerfile}"
BUILD_CONTEXT="${BUILD_CONTEXT:-.}"

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
echo ""

# Build the image
docker build \
    -f "${DOCKERFILE_PATH}" \
    -t "${FULL_IMAGE_NAME}" \
    "${BUILD_CONTEXT}"

echo ""
echo "Docker image built successfully: ${FULL_IMAGE_NAME}"

