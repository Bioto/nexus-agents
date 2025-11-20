use crate::error::{LoggerError, Result};
use chrono::Local;
use device_query::{DeviceQuery, DeviceState, Keycode};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub enum InputEvent {
    #[serde(rename = "keyboard")]
    Keyboard {
        key: String,
        pressed: bool,
        timestamp: String,
    },
    #[serde(rename = "mouse")]
    Mouse {
        event_type: String,
        button: Option<String>,
        x: Option<i32>,
        y: Option<i32>,
        timestamp: String,
    },
}

impl InputEvent {
    fn to_text(&self) -> String {
        match self {
            InputEvent::Keyboard { key, pressed, .. } => {
                let action = if *pressed { "PRESS" } else { "RELEASE" };
                format!("[KEYBOARD] {}: {}", action, key)
            }
            InputEvent::Mouse {
                event_type,
                button,
                x,
                y,
                timestamp: _,
            } => {
                match event_type.as_str() {
                    "click" => {
                        format!(
                            "[MOUSE] CLICK: {} at ({}, {})",
                            button.as_ref().unwrap_or(&"unknown".to_string()),
                            x.unwrap_or(0),
                            y.unwrap_or(0)
                        )
                    }
                    "move" => {
                        format!("[MOUSE] MOVE: ({}, {})", x.unwrap_or(0), y.unwrap_or(0))
                    }
                    _ => format!("[MOUSE] {}: {:?}", event_type, button),
                }
            }
        }
    }
}

pub fn run_capture_service(
    capture_keyboard: bool,
    capture_mouse: bool,
    capture_mouse_moves: bool,
    format: String,
    output_file: Option<PathBuf>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let device_state = DeviceState::new();
    let mut last_keys: Vec<Keycode> = vec![];
    let mut last_mouse_buttons: Vec<bool> = vec![];

    // Track last mouse position for move detection
    let mut last_mouse_pos: Option<(i32, i32)> = None;

    // Open output file if specified
    let mut file_handle: Option<std::fs::File> = if let Some(ref path) = output_file {
        Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| {
                    LoggerError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to open output file: {}", e),
                    ))
                })?,
        )
    } else {
        None
    };

    let mut write_output = |event: &InputEvent| -> Result<()> {
        let output = match format.as_str() {
            "json" => {
                serde_json::to_string(event).map_err(|e| {
                    LoggerError::Other(format!("Failed to serialize event: {}", e))
                })?
            }
            "text" => event.to_text(),
            "both" => {
                format!(
                    "{} | {}",
                    event.to_text(),
                    serde_json::to_string(event).map_err(|e| {
                        LoggerError::Other(format!("Failed to serialize event: {}", e))
                    })?
                )
            }
            _ => return Err(LoggerError::Configuration("Invalid format".to_string())),
        };

        if let Some(ref mut file) = file_handle {
            writeln!(file, "{}", output).map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to write to file: {}", e),
                ))
            })?;
        } else {
            println!("{}", output);
        }
        Ok(())
    };

    // Main capture loop
    while running.load(Ordering::SeqCst) {
        let timestamp = Local::now().to_rfc3339();

        // Capture keyboard events
        if capture_keyboard {
            let keys = device_state.get_keys();
            let keys_set: std::collections::HashSet<Keycode> = keys.iter().cloned().collect();
            let last_keys_set: std::collections::HashSet<Keycode> =
                last_keys.iter().cloned().collect();

            // Detect key presses (new keys not in last_keys)
            for key in &keys {
                if !last_keys_set.contains(key) {
                    let event = InputEvent::Keyboard {
                        key: format!("{:?}", key),
                        pressed: true,
                        timestamp: timestamp.clone(),
                    };
                    write_output(&event)?;
                }
            }

            // Detect key releases (keys in last_keys but not in current keys)
            for key in &last_keys {
                if !keys_set.contains(key) {
                    let event = InputEvent::Keyboard {
                        key: format!("{:?}", key),
                        pressed: false,
                        timestamp: timestamp.clone(),
                    };
                    write_output(&event)?;
                }
            }

            last_keys = keys;
        }

        // Capture mouse events
        if capture_mouse {
            let mouse = device_state.get_mouse();
            let current_pos = (mouse.coords.0, mouse.coords.1);
            let pos_changed = last_mouse_pos.map(|last| last != current_pos).unwrap_or(true);
            last_mouse_pos = Some(current_pos);

            // Button names for indices: 0=Left, 1=Right, 2=Middle, etc.
            let button_names = ["Left", "Right", "Middle", "X1", "X2"];

            // Detect button presses and releases
            for (idx, &pressed) in mouse.button_pressed.iter().enumerate() {
                let was_pressed = last_mouse_buttons.get(idx).copied().unwrap_or(false);
                let button_name = button_names
                    .get(idx)
                    .copied()
                    .map(String::from)
                    .unwrap_or_else(|| format!("Button{}", idx));

                if pressed && !was_pressed {
                    // Button press
                    let event = InputEvent::Mouse {
                        event_type: "click".to_string(),
                        button: Some(button_name.clone()),
                        x: Some(mouse.coords.0),
                        y: Some(mouse.coords.1),
                        timestamp: timestamp.clone(),
                    };
                    write_output(&event)?;
                } else if !pressed && was_pressed {
                    // Button release
                    let event = InputEvent::Mouse {
                        event_type: "release".to_string(),
                        button: Some(button_name.clone()),
                        x: Some(mouse.coords.0),
                        y: Some(mouse.coords.1),
                        timestamp: timestamp.clone(),
                    };
                    write_output(&event)?;
                }
            }

            // Capture mouse moves (if enabled)
            if capture_mouse_moves && pos_changed {
                let event = InputEvent::Mouse {
                    event_type: "move".to_string(),
                    button: None,
                    x: Some(mouse.coords.0),
                    y: Some(mouse.coords.1),
                    timestamp: timestamp.clone(),
                };
                write_output(&event)?;
            }

            // Update last mouse buttons
            last_mouse_buttons = mouse.button_pressed.clone();
        }

        // Small delay to avoid excessive CPU usage
        std::thread::sleep(Duration::from_millis(10));
    }

    Ok(())
}

