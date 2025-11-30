//! Webcam PTZ (Pan/Tilt/Zoom) control service.
//!
//! This service handles pan, tilt, and zoom controls for V4L2 devices.
//! It can operate independently from video recording, allowing concurrent
//! control and recording operations.

use crate::error::{RecorderError, Result};
use crate::services::webcam::device::WebcamDevice;
use log::{debug, info, warn};
use std::sync::{Arc, Mutex};
use v4l::control::{Control, Value};

// V4L2 Camera Class Control IDs (from v4l2-controls.h)
const V4L2_CTRL_CLASS_CAMERA: u32 = 0x009a0000;
const V4L2_CID_CAMERA_CLASS_BASE: u32 = V4L2_CTRL_CLASS_CAMERA | 0x900;
const V4L2_CID_PAN_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 8;
const V4L2_CID_TILT_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 9;
const V4L2_CID_ZOOM_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 13;

// Default ranges for PTZ (in arc-seconds)
const DEFAULT_PAN_MIN: i64 = -648000;  // -180 degrees
const DEFAULT_PAN_MAX: i64 = 648000;   // +180 degrees
const DEFAULT_TILT_MIN: i64 = -324000; // -90 degrees
const DEFAULT_TILT_MAX: i64 = 324000;  // +90 degrees
const DEFAULT_ZOOM_MIN: i64 = 100;
const DEFAULT_ZOOM_MAX: i64 = 500;

/// PTZ control state.
#[derive(Debug, Clone, Copy)]
pub struct PtzState {
    pub pan: Option<i64>,
    pub tilt: Option<i64>,
    pub zoom: Option<i64>,
}

impl Default for PtzState {
    fn default() -> Self {
        Self {
            pan: None,
            tilt: None,
            zoom: None,
        }
    }
}

/// PTZ control command.
#[derive(Debug, Clone, Copy)]
pub enum PtzControl {
    /// Pan relative adjustment (positive = right, negative = left)
    PanRelative(i64),
    /// Pan absolute position
    PanAbsolute(i64),
    /// Tilt relative adjustment (positive = up, negative = down)
    TiltRelative(i64),
    /// Tilt absolute position
    TiltAbsolute(i64),
    /// Zoom relative adjustment (positive = zoom in, negative = zoom out)
    ZoomRelative(i64),
    /// Zoom absolute position
    ZoomAbsolute(i64),
    /// Reset all PTZ to center/default
    Reset,
    /// Get current PTZ state
    GetState,
}

/// Webcam PTZ controller service.
///
/// This service provides pan, tilt, and zoom control for V4L2 devices.
/// It can operate independently from video recording.
pub struct WebcamController {
    device: Arc<Mutex<WebcamDevice>>,
    pan_ctrl: Option<(u32, i64, i64)>,
    tilt_ctrl: Option<(u32, i64, i64)>,
    zoom_ctrl: Option<(u32, i64, i64)>,
}

impl WebcamController {
    /// Create a new webcam controller.
    ///
    /// The device can be shared with other services (like WebcamRecorder)
    /// by wrapping it in an Arc<Mutex<>>.
    pub fn new(device: Arc<Mutex<WebcamDevice>>) -> Result<Self> {
        let device_guard = device.lock()
            .map_err(|e| RecorderError::Other(format!("Failed to lock device: {}", e)))?;
        
        let controls: Vec<_> = device_guard.device().query_controls().unwrap_or_default();
        drop(device_guard);

        // Probe for PTZ controls
        let mut pan_ctrl = None;
        let mut tilt_ctrl = None;
        let mut zoom_ctrl = None;

        let device_guard = device.lock()
            .map_err(|e| RecorderError::Other(format!("Failed to lock device: {}", e)))?;

        // Try reading pan control
        match device_guard.device().control(V4L2_CID_PAN_ABSOLUTE) {
            Ok(ctrl) => {
                if let Value::Integer(val) = ctrl.value {
                    let (min, max) = controls.iter()
                        .find(|c| c.id == V4L2_CID_PAN_ABSOLUTE)
                        .map(|c| (c.minimum, c.maximum))
                        .unwrap_or((DEFAULT_PAN_MIN, DEFAULT_PAN_MAX));
                    pan_ctrl = Some((V4L2_CID_PAN_ABSOLUTE, min, max));
                    info!("Pan control available: {} [{} to {}]", val, min, max);
                }
            }
            Err(e) => {
                debug!("Pan control not available: {}", e);
            }
        }

        // Try reading tilt control
        match device_guard.device().control(V4L2_CID_TILT_ABSOLUTE) {
            Ok(ctrl) => {
                if let Value::Integer(val) = ctrl.value {
                    let (min, max) = controls.iter()
                        .find(|c| c.id == V4L2_CID_TILT_ABSOLUTE)
                        .map(|c| (c.minimum, c.maximum))
                        .unwrap_or((DEFAULT_TILT_MIN, DEFAULT_TILT_MAX));
                    tilt_ctrl = Some((V4L2_CID_TILT_ABSOLUTE, min, max));
                    info!("Tilt control available: {} [{} to {}]", val, min, max);
                }
            }
            Err(e) => {
                debug!("Tilt control not available: {}", e);
            }
        }

        // Try reading zoom control
        match device_guard.device().control(V4L2_CID_ZOOM_ABSOLUTE) {
            Ok(ctrl) => {
                if let Value::Integer(val) = ctrl.value {
                    let (min, max) = controls.iter()
                        .find(|c| c.id == V4L2_CID_ZOOM_ABSOLUTE)
                        .map(|c| (c.minimum, c.maximum))
                        .unwrap_or((DEFAULT_ZOOM_MIN, DEFAULT_ZOOM_MAX));
                    zoom_ctrl = Some((V4L2_CID_ZOOM_ABSOLUTE, min, max));
                    info!("Zoom control available: {} [{} to {}]", val, min, max);
                }
            }
            Err(e) => {
                debug!("Zoom control not available: {}", e);
            }
        }

        drop(device_guard);

        if pan_ctrl.is_none() && tilt_ctrl.is_none() && zoom_ctrl.is_none() {
            warn!("No PTZ controls detected for this camera");
        }

        Ok(Self {
            device,
            pan_ctrl,
            tilt_ctrl,
            zoom_ctrl,
        })
    }

    /// Create a new controller by opening a device path.
    pub fn open(device_path: impl AsRef<str>) -> Result<Self> {
        let device = WebcamDevice::open(device_path)?;
        Self::new(Arc::new(Mutex::new(device)))
    }

    /// Execute a PTZ control command.
    pub fn control(&self, cmd: PtzControl) -> Result<PtzState> {
        let mut device_guard = self.device.lock()
            .map_err(|e| RecorderError::Other(format!("Failed to lock device: {}", e)))?;

        match cmd {
            PtzControl::PanRelative(delta) => {
                self.adjust_ptz(&mut *device_guard, self.pan_ctrl, delta)?;
            }
            PtzControl::PanAbsolute(value) => {
                self.set_ptz_absolute(&mut *device_guard, self.pan_ctrl, value)?;
            }
            PtzControl::TiltRelative(delta) => {
                self.adjust_ptz(&mut *device_guard, self.tilt_ctrl, delta)?;
            }
            PtzControl::TiltAbsolute(value) => {
                self.set_ptz_absolute(&mut *device_guard, self.tilt_ctrl, value)?;
            }
            PtzControl::ZoomRelative(delta) => {
                self.adjust_ptz(&mut *device_guard, self.zoom_ctrl, delta)?;
            }
            PtzControl::ZoomAbsolute(value) => {
                self.set_ptz_absolute(&mut *device_guard, self.zoom_ctrl, value)?;
            }
            PtzControl::Reset => {
                self.reset_ptz(&mut *device_guard)?;
            }
            PtzControl::GetState => {
                // Will return state below
            }
        }

        // Get current state
        let state = self.get_state(&device_guard)?;
        drop(device_guard);

        Ok(state)
    }

    /// Adjust a PTZ control by a relative amount.
    fn adjust_ptz(
        &self,
        device: &mut WebcamDevice,
        ctrl: Option<(u32, i64, i64)>,
        delta: i64,
    ) -> Result<()> {
        if let Some((id, min, max)) = ctrl {
            let current = device.device().control(id)
                .map_err(|e| RecorderError::Other(format!("Failed to read control {}: {}", id, e)))?;

            if let Value::Integer(val) = current.value {
                // Small step: ~2 degrees for pan/tilt, small increment for zoom
                let step = ((max - min) / 100).max(7200);
                let new_val = (val + delta * step).clamp(min, max);
                
                device.device_mut().set_control(Control {
                    id,
                    value: Value::Integer(new_val),
                }).map_err(|e| RecorderError::Other(format!("Failed to set control {}: {}", id, e)))?;

                debug!("Control {} adjusted to {}", id, new_val);
            }
        }
        Ok(())
    }

    /// Set a PTZ control to an absolute value.
    fn set_ptz_absolute(
        &self,
        device: &mut WebcamDevice,
        ctrl: Option<(u32, i64, i64)>,
        value: i64,
    ) -> Result<()> {
        if let Some((id, min, max)) = ctrl {
            let clamped_value = value.clamp(min, max);
            
            device.device_mut().set_control(Control {
                id,
                value: Value::Integer(clamped_value),
            }).map_err(|e| RecorderError::Other(format!("Failed to set control {}: {}", id, e)))?;

            debug!("Control {} set to {}", id, clamped_value);
        }
        Ok(())
    }

    /// Reset all PTZ controls to center/default.
    fn reset_ptz(&self, device: &mut WebcamDevice) -> Result<()> {
        if let Some((id, min, max)) = self.pan_ctrl {
            let center = (min + max) / 2;
            device.device_mut().set_control(Control {
                id,
                value: Value::Integer(center),
            }).map_err(|e| RecorderError::Other(format!("Failed to reset pan: {}", e)))?;
        }

        if let Some((id, min, max)) = self.tilt_ctrl {
            let center = (min + max) / 2;
            device.device_mut().set_control(Control {
                id,
                value: Value::Integer(center),
            }).map_err(|e| RecorderError::Other(format!("Failed to reset tilt: {}", e)))?;
        }

        if let Some((id, min, max)) = self.zoom_ctrl {
            let default = min; // Usually zoom out is minimum
            device.device_mut().set_control(Control {
                id,
                value: Value::Integer(default),
            }).map_err(|e| RecorderError::Other(format!("Failed to reset zoom: {}", e)))?;
        }

        Ok(())
    }

    /// Get current PTZ state.
    fn get_state(&self, device: &WebcamDevice) -> Result<PtzState> {
        let pan = self.pan_ctrl.and_then(|(id, _, _)| {
            device.device().control(id).ok().and_then(|ctrl| {
                if let Value::Integer(val) = ctrl.value {
                    Some(val)
                } else {
                    None
                }
            })
        });

        let tilt = self.tilt_ctrl.and_then(|(id, _, _)| {
            device.device().control(id).ok().and_then(|ctrl| {
                if let Value::Integer(val) = ctrl.value {
                    Some(val)
                } else {
                    None
                }
            })
        });

        let zoom = self.zoom_ctrl.and_then(|(id, _, _)| {
            device.device().control(id).ok().and_then(|ctrl| {
                if let Value::Integer(val) = ctrl.value {
                    Some(val)
                } else {
                    None
                }
            })
        });

        Ok(PtzState { pan, tilt, zoom })
    }

    /// Get current PTZ state (convenience method).
    pub fn state(&self) -> Result<PtzState> {
        let device_guard = self.device.lock()
            .map_err(|e| RecorderError::Other(format!("Failed to lock device: {}", e)))?;
        let state = self.get_state(&device_guard)?;
        drop(device_guard);
        Ok(state)
    }

    /// Check if pan control is available.
    pub fn has_pan(&self) -> bool {
        self.pan_ctrl.is_some()
    }

    /// Check if tilt control is available.
    pub fn has_tilt(&self) -> bool {
        self.tilt_ctrl.is_some()
    }

    /// Check if zoom control is available.
    pub fn has_zoom(&self) -> bool {
        self.zoom_ctrl.is_some()
    }
}

