# Nexus Screen

Screen recording service for AI agents.

## System Dependencies

This project requires the following system libraries on Linux:

### Required Packages

```bash
sudo apt-get update
sudo apt-get install -y \
    libpipewire-0.3-dev \
    libgbm-dev \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libavfilter-dev \
    libavdevice-dev \
    libswscale-dev \
    libswresample-dev \
    pkg-config
```

### PKG_CONFIG_PATH Setup

If you're using Linuxbrew or have a custom pkg-config setup, you may need to set `PKG_CONFIG_PATH` to include system library paths:

```bash
export PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:$PKG_CONFIG_PATH
```

You can add this to your `~/.zshrc` or `~/.bashrc` to make it permanent:

```bash
echo 'export PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:$PKG_CONFIG_PATH' >> ~/.zshrc
```

## Building

After installing dependencies and setting PKG_CONFIG_PATH:

```bash
cargo build -p nexus-screen
```

## Usage

```bash
# Record screen (default: 60 fps, until Ctrl+C)
cargo run -p nexus-screen -- record

# Record with custom frame rate
cargo run -p nexus-screen -- record --fps 30

# Record for specific duration
cargo run -p nexus-screen -- record --duration 10

# Record without audio
cargo run -p nexus-screen -- record --no-audio

# Specify output file
cargo run -p nexus-screen -- record --output my_recording.mp4
```

