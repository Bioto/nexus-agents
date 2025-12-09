//! V4L2 device discovery and information utilities.

use crate::error::{RecorderError, Result};
use log::{error, info};
use std::path::Path;
use v4l::video::Capture;
use v4l::Device;

/// Information about a V4L2 webcam device.
#[derive(Debug, Clone)]
pub struct WebcamDeviceInfo {
    pub path: String,
    pub driver: Option<String>,
    pub card: Option<String>,
    pub bus: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fourcc: Option<String>,
    pub has_ptz: bool,
}

/// A handle to an open V4L2 device.
pub struct WebcamDevice {
    device: Device,
    path: String,
}

impl WebcamDevice {
    /// Open a V4L2 device by path.
    pub fn open(path: impl AsRef<str>) -> Result<Self> {
        let path_str = path.as_ref();
        let device = Device::with_path(path_str).map_err(|e| {
            RecorderError::Other(format!("Failed to open device {}: {}", path_str, e))
        })?;

        Ok(Self {
            device,
            path: path_str.to_string(),
        })
    }

    /// Get the device path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get a reference to the underlying V4L2 device.
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Get a mutable reference to the underlying V4L2 device.
    pub fn device_mut(&mut self) -> &mut Device {
        &mut self.device
    }

    /// Query device information.
    pub fn info(&self) -> Result<WebcamDeviceInfo> {
        let caps = self
            .device
            .query_caps()
            .map_err(|e| RecorderError::Other(format!("Failed to query capabilities: {}", e)))?;

        let format = self
            .device
            .format()
            .map_err(|e| RecorderError::Other(format!("Failed to get format: {}", e)))?;

        // Check for PTZ controls
        let has_ptz = self.has_ptz_controls();

        Ok(WebcamDeviceInfo {
            path: self.path.clone(),
            driver: Some(caps.driver),
            card: Some(caps.card),
            bus: Some(caps.bus),
            width: Some(format.width),
            height: Some(format.height),
            fourcc: Some(format.fourcc.to_string()),
            has_ptz,
        })
    }

    /// Check if the device has PTZ controls.
    fn has_ptz_controls(&self) -> bool {
        // V4L2 Camera Class Control IDs
        const V4L2_CTRL_CLASS_CAMERA: u32 = 0x009a0000;
        const V4L2_CID_CAMERA_CLASS_BASE: u32 = V4L2_CTRL_CLASS_CAMERA | 0x900;
        const V4L2_CID_PAN_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 8;
        const V4L2_CID_TILT_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 9;

        // Try reading pan or tilt control
        self.device.control(V4L2_CID_PAN_ABSOLUTE).is_ok()
            || self.device.control(V4L2_CID_TILT_ABSOLUTE).is_ok()
    }
}

/// List all available V4L2 devices.
pub fn list_v4l2_devices() -> Vec<String> {
    let mut devices = Vec::new();
    for i in 0..10 {
        let device_path = format!("/dev/video{}", i);
        if Path::new(&device_path).exists() {
            devices.push(device_path);
        }
    }
    devices
}

/// Show detailed information about a V4L2 device.
pub fn show_device_info(device_path: &str) -> Result<()> {
    let dev = Device::with_path(device_path).map_err(|e| {
        RecorderError::Other(format!("Failed to open device {}: {}", device_path, e))
    })?;

    // Query capabilities
    match dev.query_caps() {
        Ok(caps) => {
            info!("Device: {}", device_path);
            info!("  Driver: {}", caps.driver);
            info!("  Card: {}", caps.card);
            info!("  Bus: {}", caps.bus);
            info!("  Capabilities: {:?}", caps.capabilities);
        }
        Err(e) => {
            error!("Failed to query capabilities: {}", e);
        }
    }

    // Get format
    match dev.format() {
        Ok(fmt) => {
            info!("Current Format:");
            info!("  Resolution: {}x{}", fmt.width, fmt.height);
            info!("  Pixel Format: {:?}", fmt.fourcc);
        }
        Err(e) => {
            error!("Failed to get format: {}", e);
        }
    }

    // List controls
    info!("Controls:");
    for ctrl in dev.query_controls().unwrap_or_default() {
        let value = dev
            .control(ctrl.id)
            .map(|v| format!("{:?}", v))
            .unwrap_or_else(|_| "N/A".to_string());
        info!(
            "  {}: [{} - {}] = {}",
            ctrl.name, ctrl.minimum, ctrl.maximum, value
        );
    }

    Ok(())
}
