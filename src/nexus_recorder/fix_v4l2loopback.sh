#!/bin/bash
# Fix v4l2loopback configuration for webcam splitter
# This script removes exclusive_caps=1 which causes format negotiation issues

set -e

echo "🔧 Fixing v4l2loopback configuration..."
echo ""

# Check if v4l2loopback is loaded
if lsmod | grep -q v4l2loopback; then
    echo "✓ v4l2loopback is currently loaded"
    
    # Check exclusive_caps
    EXCLUSIVE_CAPS=$(cat /sys/module/v4l2loopback/parameters/exclusive_caps 2>/dev/null || echo "")
    echo "  Current exclusive_caps: $EXCLUSIVE_CAPS"
    
    if [[ "$EXCLUSIVE_CAPS" == Y* ]]; then
        echo ""
        echo "⚠️  exclusive_caps=1 detected! This causes FFmpeg errors."
        echo "   Reloading module without exclusive_caps..."
        echo ""
        
        sudo modprobe -r v4l2loopback
        echo "✓ Unloaded v4l2loopback"
    fi
else
    echo "ℹ️  v4l2loopback is not currently loaded"
fi

# Load v4l2loopback correctly
echo ""
echo "Loading v4l2loopback with correct parameters..."
sudo modprobe v4l2loopback devices=2 video_nr=10,11 card_label="Virtual1,Virtual2"

echo "✓ Loaded v4l2loopback"
echo ""

# Verify devices exist
if [ -c /dev/video10 ] && [ -c /dev/video11 ]; then
    echo "✓ Virtual cameras created:"
    ls -la /dev/video10 /dev/video11
    echo ""
    echo "✅ Setup complete! You can now run:"
    echo "   cargo run --bin nexus-recorder splitter"
else
    echo "❌ ERROR: Virtual cameras were not created"
    echo "   Check dmesg for kernel errors:"
    echo "   sudo dmesg | tail -20"
    exit 1
fi


