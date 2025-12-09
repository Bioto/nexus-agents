//! Webcam service module for V4L2 camera control and recording.
//!
//! This module provides:
//! - Device discovery and information
//! - Video recording from webcams
//! - Pan/Tilt/Zoom (PTZ) control
//! - Format conversion utilities
//! - Camera feed splitting (one camera → multiple virtual cameras)
//!
//! The services are designed to work independently - you can record video
//! while another module controls PTZ, or vice versa.
//!
//! # Example: Recording and PTZ Control
//!
//! ```no_run
//! use nexus_recorder::{WebcamRecorder, WebcamRecordingConfig, WebcamController, PtzControl};
//! use std::sync::Arc;
//! use std::sync::Mutex;
//! use nexus_recorder::services::webcam::device::WebcamDevice;
//!
//! // Open device once and share between services
//! let device = Arc::new(Mutex::new(WebcamDevice::open("/dev/video0")?));
//!
//! // Start recording in a separate thread
//! let mut recorder = WebcamRecorder::new(WebcamRecordingConfig {
//!     device_path: "/dev/video0".to_string(),
//!     output_path: "output/recording.mp4".into(),
//!     enable_preview: true,
//!     ..Default::default()
//! })?;
//! let handle = recorder.record_async()?;
//!
//! // Control PTZ while recording
//! let controller = WebcamController::new(Arc::clone(&device))?;
//! controller.control(PtzControl::PanRelative(1))?; // Pan right
//! controller.control(PtzControl::TiltRelative(1))?; // Tilt up
//!
//! // Stop recording when done
//! handle.stop();
//! ```
//!
//! # Example: Camera Splitting
//!
//! ```no_run
//! use nexus_recorder::services::webcam::splitter::{WebcamSplitter, SplitterConfig};
//!
//! // Split one camera to two virtual devices
//! let config = SplitterConfig {
//!     input_device: "/dev/video0".to_string(),
//!     output_devices: vec![
//!         "/dev/video10".to_string(),
//!         "/dev/video11".to_string(),
//!     ],
//!     ..Default::default()
//! };
//!
//! let mut splitter = WebcamSplitter::new(config)?;
//! let handle = splitter.start()?;
//!
//! // App 1 can now use /dev/video10
//! // App 2 can now use /dev/video11
//!
//! handle.stop();
//! ```

pub mod controller;
pub mod device;
pub mod format;
pub mod recorder;
pub mod splitter;

pub use controller::{PtzControl, PtzState, WebcamController};
pub use device::{list_v4l2_devices, show_device_info, WebcamDevice, WebcamDeviceInfo};
pub use format::{mjpeg_to_rgb, yuv_to_rgb, yuyv_to_rgb};
pub use recorder::{WebcamRecorder, WebcamRecordingConfig};
pub use splitter::{SplitterConfig, SplitterHandle, WebcamSplitter};
