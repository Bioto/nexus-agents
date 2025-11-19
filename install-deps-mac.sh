#!/bin/bash
# Install script for macOS dependencies for nexus-agents

set -e

echo "Installing dependencies for nexus-agents on macOS..."

# Check if Homebrew is installed
if ! command -v brew &> /dev/null; then
    echo "Homebrew is not installed. Installing Homebrew..."
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    
    # Add Homebrew to PATH for Apple Silicon Macs
    if [[ $(uname -m) == "arm64" ]]; then
        echo 'eval "$(/opt/homebrew/bin/brew shellenv)"' >> ~/.zprofile
        eval "$(/opt/homebrew/bin/brew shellenv)"
    fi
else
    echo "Homebrew already installed: $(brew --version | head -n 1)"
fi

# Update Homebrew
brew update

# Install Xcode Command Line Tools if not already installed
if ! xcode-select -p &> /dev/null; then
    echo "Installing Xcode Command Line Tools..."
    xcode-select --install
    echo "Please complete the Xcode Command Line Tools installation, then run this script again."
    exit 1
else
    echo "Xcode Command Line Tools already installed"
fi

# Install Rust toolchain if not already installed
if ! command -v rustc &> /dev/null; then
    echo "Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "Rust toolchain already installed: $(rustc --version)"
fi

# Install build dependencies via Homebrew
echo "Installing build dependencies..."
# FFmpeg libraries for nexus-screen, PipeWire for screen recording (if available),
# audio libraries for nexus-audio, llvm (provides libclang for bindgen), and cmake
brew install \
    pkg-config \
    openssl@3 \
    ffmpeg \
    pipewire \
    alsa-lib \
    portaudio \
    llvm \
    cmake

# Note: On macOS, some libraries may have different names or be provided by the system
# GBM is typically not needed on macOS (it's Linux-specific)
# PipeWire may not be available via Homebrew; macOS uses CoreAudio/CoreVideo instead

# Optional: Install Docker for nexus_py Python execution
if ! command -v docker &> /dev/null; then
    echo ""
    echo "Docker is not installed. nexus_py requires Docker for Python execution."
    echo "To install Docker Desktop for Mac, visit:"
    echo "  https://www.docker.com/products/docker-desktop/"
    echo ""
    read -p "Would you like to install Docker via Homebrew? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        brew install --cask docker
        echo "Docker Desktop installed. Please open Docker Desktop from Applications to complete setup."
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

# Set up PKG_CONFIG_PATH for Homebrew libraries
if [[ $(uname -m) == "arm64" ]]; then
    HOMEBREW_PREFIX="/opt/homebrew"
else
    HOMEBREW_PREFIX="/usr/local"
fi

if [ -z "$PKG_CONFIG_PATH" ] || [[ ! "$PKG_CONFIG_PATH" == *"$HOMEBREW_PREFIX"* ]]; then
    echo ""
    echo "Setting PKG_CONFIG_PATH for Homebrew libraries..."
    export PKG_CONFIG_PATH="$HOMEBREW_PREFIX/lib/pkgconfig:$PKG_CONFIG_PATH"
    
    # Add to shell config
    if [ -f "$HOME/.zshrc" ]; then
        if ! grep -q "PKG_CONFIG_PATH.*$HOMEBREW_PREFIX" "$HOME/.zshrc"; then
            echo "export PKG_CONFIG_PATH=\"$HOMEBREW_PREFIX/lib/pkgconfig:\$PKG_CONFIG_PATH\"" >> "$HOME/.zshrc"
            echo "Added PKG_CONFIG_PATH to ~/.zshrc"
        fi
    elif [ -f "$HOME/.bash_profile" ]; then
        if ! grep -q "PKG_CONFIG_PATH.*$HOMEBREW_PREFIX" "$HOME/.bash_profile"; then
            echo "export PKG_CONFIG_PATH=\"$HOMEBREW_PREFIX/lib/pkgconfig:\$PKG_CONFIG_PATH\"" >> "$HOME/.bash_profile"
            echo "Added PKG_CONFIG_PATH to ~/.bash_profile"
        fi
    fi
fi

# Set up OpenSSL for Rust (Homebrew version)
if [ -z "$OPENSSL_DIR" ]; then
    echo ""
    echo "Setting OPENSSL_DIR for Rust..."
    export OPENSSL_DIR="$HOMEBREW_PREFIX/opt/openssl@3"
    
    # Add to shell config
    if [ -f "$HOME/.zshrc" ]; then
        if ! grep -q "OPENSSL_DIR.*$HOMEBREW_PREFIX" "$HOME/.zshrc"; then
            echo "export OPENSSL_DIR=\"$HOMEBREW_PREFIX/opt/openssl@3\"" >> "$HOME/.zshrc"
            echo "Added OPENSSL_DIR to ~/.zshrc"
        fi
    elif [ -f "$HOME/.bash_profile" ]; then
        if ! grep -q "OPENSSL_DIR.*$HOMEBREW_PREFIX" "$HOME/.bash_profile"; then
            echo "export OPENSSL_DIR=\"$HOMEBREW_PREFIX/opt/openssl@3\"" >> "$HOME/.bash_profile"
            echo "Added OPENSSL_DIR to ~/.bash_profile"
        fi
    fi
fi

# Set up LIBCLANG_PATH for bindgen (used by ffmpeg-next)
if [ -z "$LIBCLANG_PATH" ]; then
    echo ""
    echo "Setting LIBCLANG_PATH for bindgen..."
    export LIBCLANG_PATH="$HOMEBREW_PREFIX/opt/llvm/lib"
    
    # Add to shell config
    if [ -f "$HOME/.zshrc" ]; then
        if ! grep -q "LIBCLANG_PATH.*$HOMEBREW_PREFIX" "$HOME/.zshrc"; then
            echo "export LIBCLANG_PATH=\"$HOMEBREW_PREFIX/opt/llvm/lib\"" >> "$HOME/.zshrc"
            echo "Added LIBCLANG_PATH to ~/.zshrc"
        fi
    elif [ -f "$HOME/.bash_profile" ]; then
        if ! grep -q "LIBCLANG_PATH.*$HOMEBREW_PREFIX" "$HOME/.bash_profile"; then
            echo "export LIBCLANG_PATH=\"$HOMEBREW_PREFIX/opt/llvm/lib\"" >> "$HOME/.bash_profile"
            echo "Added LIBCLANG_PATH to ~/.bash_profile"
        fi
    fi
fi

echo ""
echo "✓ All dependencies installed successfully!"
echo ""
echo "Note: On macOS, some features may work differently:"
echo "  - Screen recording uses CoreGraphics/CoreVideo instead of PipeWire"
echo "  - Audio uses CoreAudio instead of ALSA/PulseAudio"
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
echo "If you encounter build errors, make sure to:"
echo "  1. Source your shell config: source ~/.zshrc (or ~/.bash_profile)"
echo "  2. Set OPENSSL_DIR: export OPENSSL_DIR=\"$HOMEBREW_PREFIX/opt/openssl@3\""
echo "  3. Set LIBCLANG_PATH: export LIBCLANG_PATH=\"$HOMEBREW_PREFIX/opt/llvm/lib\""
echo ""

