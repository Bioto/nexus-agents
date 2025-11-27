#!/bin/bash
# Install script for Linux (Ubuntu/Debian) dependencies for nexus-agents

set -e

echo "Installing dependencies for nexus-agents on Linux..."

# Update package list
sudo apt-get update

# Install Rust toolchain if not already installed
if ! command -v rustc &> /dev/null; then
    echo "Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "Rust toolchain already installed: $(rustc --version)"
fi

# Install build dependencies
echo "Installing build dependencies..."
# FFmpeg libraries for nexus-screen, PipeWire for screen recording,
# GBM for graphics, audio libraries for nexus-audio, libclang for bindgen, and cmake
sudo apt-get install -y \
    build-essential \
    pkg-config \
    cmake \
    libssl-dev \
    ca-certificates \
    libclang-dev \
    linux-libc-dev \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libavfilter-dev \
    libavdevice-dev \
    libswscale-dev \
    libswresample-dev \
    libpipewire-0.3-dev \
    libgbm-dev \
    libasound2-dev \
    libpulse-dev \
    pulseaudio-utils \
    libgtk-3-dev \
    libxdo-dev

# Install runtime dependencies (if you want to run the binaries)
echo "Installing runtime dependencies..."
echo "Auto-detecting FFmpeg runtime package versions..."

# Auto-detect available FFmpeg runtime packages
FFMPEG_RUNTIME_PKGS=""
for pkg in libavcodec libavformat libavutil libavfilter libavdevice libswscale libswresample; do
    # Find the available runtime package (without -dev suffix)
    runtime_pkg=$(apt-cache search "^${pkg}[0-9]" 2>/dev/null | grep -E "^${pkg}[0-9]+ " | head -1 | awk '{print $1}')
    if [ -n "$runtime_pkg" ]; then
        FFMPEG_RUNTIME_PKGS="$FFMPEG_RUNTIME_PKGS $runtime_pkg"
        echo "  Found: $runtime_pkg"
    else
        echo "  Warning: Could not find runtime package for $pkg"
    fi
done

# Function to find the correct package name (handles both old and new t64 naming)
find_package() {
    local pkg=$1
    # Try the package name as-is first
    if apt-cache show "$pkg" &>/dev/null; then
        echo "$pkg"
        return 0
    fi
    # Try with t64 suffix (newer Ubuntu/Debian versions)
    if apt-cache show "${pkg}t64" &>/dev/null; then
        echo "${pkg}t64"
        return 0
    fi
    # Return original name if neither found (let apt-get handle the error)
    echo "$pkg"
    return 1
}

# Install runtime libraries
# Note: libssl3 might not exist on older Ubuntu versions, so we'll try libssl1.1 or libssl1.0.0 as fallback
SSL_RUNTIME=""
if apt-cache show libssl3 &>/dev/null || apt-cache show libssl3t64 &>/dev/null; then
    SSL_RUNTIME=$(find_package "libssl3")
elif apt-cache show libssl1.1 &>/dev/null || apt-cache show libssl1.1t64 &>/dev/null; then
    SSL_RUNTIME=$(find_package "libssl1.1")
else
    SSL_RUNTIME=$(find_package "libssl1.0.0")
fi

# Find runtime package names (handles t64 suffix)
PIPEWIRE_RUNTIME=$(find_package "libpipewire-0.3-0")
GBM_RUNTIME=$(find_package "libgbm1")
ALSA_RUNTIME=$(find_package "libasound2")
PULSE_RUNTIME=$(find_package "libpulse0")

echo "Installing runtime packages: $SSL_RUNTIME, $PIPEWIRE_RUNTIME, $GBM_RUNTIME, $ALSA_RUNTIME, $PULSE_RUNTIME, FFmpeg libraries..."

# Build the install command - only include FFMPEG_RUNTIME_PKGS if it's not empty
INSTALL_CMD="sudo apt-get install -y $SSL_RUNTIME $PIPEWIRE_RUNTIME $GBM_RUNTIME $ALSA_RUNTIME $PULSE_RUNTIME"
if [ -n "$FFMPEG_RUNTIME_PKGS" ]; then
    INSTALL_CMD="$INSTALL_CMD $FFMPEG_RUNTIME_PKGS"
fi

eval $INSTALL_CMD

# Optional: Install Docker for nexus_sandbox Python execution
if ! command -v docker &> /dev/null; then
    echo ""
    echo "Docker is not installed. nexus_sandbox requires Docker for Python execution."
    echo "To install Docker, run:"
    echo "  curl -fsSL https://get.docker.com -o get-docker.sh"
    echo "  sudo sh get-docker.sh"
    echo ""
    read -p "Would you like to install Docker now? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        curl -fsSL https://get.docker.com -o get-docker.sh
        sudo sh get-docker.sh
        sudo usermod -aG docker $USER
        echo "Docker installed. You may need to log out and back in for group changes to take effect."
    fi
else
    echo "Docker already installed: $(docker --version)"
fi

# Optional: Install uv for Python execution
if ! command -v uv &> /dev/null; then
    echo ""
    echo "uv (Python package manager) is not installed. It's used for Python execution."
    echo "To install uv, run:"
    echo "  curl -LsSf https://astral.sh/uv/install.sh | sh"
    echo ""
    read -p "Would you like to install uv now? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        curl -LsSf https://astral.sh/uv/install.sh | sh
        echo "uv installed. Add ~/.cargo/bin to your PATH if not already there."
    fi
else
    echo "uv already installed: $(uv --version)"
fi

# Set PKG_CONFIG_PATH if needed (for custom pkg-config setups)
if [ -z "$PKG_CONFIG_PATH" ] || [[ ! "$PKG_CONFIG_PATH" == *"/usr/lib/x86_64-linux-gnu/pkgconfig"* ]]; then
    echo ""
    echo "Setting PKG_CONFIG_PATH..."
    export PKG_CONFIG_PATH="/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:$PKG_CONFIG_PATH"
    
    # Add to shell config if not already present
    if [ -f "$HOME/.zshrc" ]; then
        if ! grep -q "PKG_CONFIG_PATH.*x86_64-linux-gnu" "$HOME/.zshrc"; then
            echo 'export PKG_CONFIG_PATH="/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:$PKG_CONFIG_PATH"' >> "$HOME/.zshrc"
            echo "Added PKG_CONFIG_PATH to ~/.zshrc"
        fi
    elif [ -f "$HOME/.bashrc" ]; then
        if ! grep -q "PKG_CONFIG_PATH.*x86_64-linux-gnu" "$HOME/.bashrc"; then
            echo 'export PKG_CONFIG_PATH="/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:$PKG_CONFIG_PATH"' >> "$HOME/.bashrc"
            echo "Added PKG_CONFIG_PATH to ~/.bashrc"
        fi
    fi
fi

echo ""
echo "✓ All dependencies installed successfully!"
echo ""
echo "To build the project, run:"
echo "  cargo build --release"
echo ""
echo "Or to build a specific package:"
echo "  cargo build --release -p nexus"
echo "  cargo build --release -p nexus-core"
echo "  cargo build --release -p nexus-screen"
echo "  cargo build --release -p nexus-audio"
echo ""

