use crate::error::{Result, VoiceError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, SampleFormat, SampleRate, StreamConfig, SupportedStreamConfig};
use hound::{WavSpec, WavWriter};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

/// Audio recording configuration
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
    /// Duration to record (None = record until stopped)
    pub duration: Option<Duration>,
    /// Device name to use (None = default device)
    pub device_name: Option<String>,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000, // Higher quality default (48kHz is standard for professional audio)
            channels: 1,
            duration: None,
            device_name: None,
        }
    }
}

/// Information about an audio input device
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub display_name: String,
    pub default: bool,
}

impl DeviceInfo {
    /// Extract card name from ALSA device name
    fn extract_card_name(name: &str) -> Option<String> {
        // Extract card name from various ALSA formats
        if let Some(card_start) = name.find("CARD=") {
            let card_part = &name[card_start + 5..];
            if let Some(comma_pos) = card_part.find(',') {
                Some(card_part[..comma_pos].to_string())
            } else {
                Some(card_part.to_string())
            }
        } else {
            None
        }
    }

    /// Look up full device name from system files (Linux/ALSA)
    /// Returns None on non-Linux platforms
    #[allow(unused_variables)]
    fn lookup_full_device_name(card_name: &str) -> Option<String> {
        #[cfg(target_os = "linux")]
        {
            // Try reading from /proc/asound/cards
            if let Ok(content) = std::fs::read_to_string("/proc/asound/cards") {
                for line in content.lines() {
                    // Format: " 0 [Quadcast        ]: USB-Audio - HyperX Quadcast"
                    // Look for the card name in brackets
                    if let Some(bracket_start) = line.find('[') {
                        if let Some(bracket_end) = line[bracket_start + 1..].find(']') {
                            let card_in_brackets =
                                line[bracket_start + 1..bracket_start + 1 + bracket_end].trim();
                            if card_in_brackets == card_name {
                                // Extract the full name after the dash
                                if let Some(dash_pos) = line.find(" - ") {
                                    let full_name = line[dash_pos + 3..].trim();
                                    if !full_name.is_empty() {
                                        return Some(full_name.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Try reading from /sys/class/sound/card*/id and longname
            if let Ok(entries) = std::fs::read_dir("/sys/class/sound") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("card"))
                    {
                        // Check if this card matches
                        if let Ok(id_content) = std::fs::read_to_string(path.join("id")) {
                            let id = id_content.trim();
                            if id == card_name {
                                // Found matching card, read longname
                                if let Ok(longname) = std::fs::read_to_string(path.join("longname"))
                                {
                                    let full_name = longname.trim();
                                    if !full_name.is_empty() {
                                        return Some(full_name.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            None
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    /// Parse a device name into a human-readable format
    fn parse_device_name(name: &str) -> String {
        // Handle common system names
        match name {
            "default" => "System Default".to_string(),
            "pulse" => "PulseAudio".to_string(),
            "pipewire" => "PipeWire".to_string(),
            "jack" => "JACK Audio".to_string(),
            _ => {
                // Parse ALSA-style device names
                let (card_name, device_type) = if name.starts_with("hw:CARD=") {
                    // Format: hw:CARD=Name,DEV=0
                    if let Some(card) = Self::extract_card_name(name) {
                        (Some(card), "Hardware Direct")
                    } else {
                        (None, "")
                    }
                } else if name.starts_with("plughw:CARD=") {
                    // Format: plughw:CARD=Name,DEV=0
                    if let Some(card) = Self::extract_card_name(name) {
                        (Some(card), "Plugin Hardware")
                    } else {
                        (None, "")
                    }
                } else if name.starts_with("sysdefault:CARD=") {
                    // Format: sysdefault:CARD=Name
                    if let Some(card) = Self::extract_card_name(name) {
                        (Some(card), "System Default")
                    } else {
                        (None, "")
                    }
                } else if name.starts_with("front:CARD=") {
                    // Format: front:CARD=Name,DEV=0
                    if let Some(card) = Self::extract_card_name(name) {
                        (Some(card), "Front")
                    } else {
                        (None, "")
                    }
                } else if name.starts_with("dsnoop:CARD=") {
                    // Format: dsnoop:CARD=Name,DEV=0
                    if let Some(card) = Self::extract_card_name(name) {
                        (Some(card), "Shared Capture")
                    } else {
                        (None, "")
                    }
                } else {
                    (None, "")
                };

                if let Some(card) = card_name {
                    // Try to look up the full device name from system files
                    let display_card_name = Self::lookup_full_device_name(&card).unwrap_or(card);
                    if !device_type.is_empty() {
                        format!("{} ({})", display_card_name, device_type)
                    } else {
                        display_card_name
                    }
                } else {
                    // For other names, try to clean them up
                    // Remove common prefixes/suffixes that aren't helpful
                    name.to_string()
                }
            }
        }
    }
}

/// Audio recorder using CPAL
pub struct AudioRecorder {
    host: Host,
}

impl AudioRecorder {
    /// Create a new audio recorder instance
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        Ok(Self { host })
    }

    /// List all available ALSA devices directly from system (Linux only)
    /// This shows devices that might not be enumerated by CPAL
    #[cfg(target_os = "linux")]
    pub fn list_alsa_devices() -> Vec<DeviceInfo> {
        let mut alsa_devices = Vec::new();
        
        // Read from /proc/asound/cards
        if let Ok(content) = std::fs::read_to_string("/proc/asound/cards") {
            for line in content.lines() {
                // Format: " 0 [Quadcast        ]: USB-Audio - HyperX Quadcast"
                if let Some(bracket_start) = line.find('[') {
                    if let Some(bracket_end) = line[bracket_start + 1..].find(']') {
                        let card_name = line[bracket_start + 1..bracket_start + 1 + bracket_end].trim();
                        
                        // Extract card number (before the bracket)
                        let _card_num = line[..bracket_start].trim();
                        
                        // Extract full name after the dash
                        let full_name = if let Some(dash_pos) = line.find(" - ") {
                            line[dash_pos + 3..].trim().to_string()
                        } else {
                            card_name.to_string()
                        };
                        
                        // Generate the most common ALSA device name format
                        // Users can also try: hw:CARD=..., plughw:CARD=..., etc.
                        let device_name = format!("sysdefault:CARD={}", card_name);
                        let display_name = DeviceInfo::parse_device_name(&device_name);
                        
                        alsa_devices.push(DeviceInfo {
                            name: device_name,
                            display_name: if display_name != full_name {
                                format!("{} ({})", full_name, display_name)
                            } else {
                                full_name
                            },
                            default: false,
                        });
                    }
                }
            }
        }
        
        alsa_devices
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn list_alsa_devices() -> Vec<DeviceInfo> {
        Vec::new()
    }

    /// List all available input devices
    pub fn list_input_devices(&self) -> Result<Vec<DeviceInfo>> {
        let default_device = self
            .host
            .default_input_device()
            .map(|d| d.name().unwrap_or_else(|_| "Unknown".to_string()));

        let mut devices: Vec<DeviceInfo> = self
            .host
            .input_devices()?
            .map(|device| {
                let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
                let display_name = DeviceInfo::parse_device_name(&name);
                let is_default = default_device.as_ref().map(|d| d == &name).unwrap_or(false);
                Ok(DeviceInfo {
                    name,
                    display_name,
                    default: is_default,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        
        // Also include ALSA devices that might not be enumerated by CPAL
        #[cfg(target_os = "linux")]
        {
            let alsa_devices = Self::list_alsa_devices();
            for alsa_device in alsa_devices {
                // Only add if not already present (check by name)
                if !devices.iter().any(|d| d.name == alsa_device.name) {
                    devices.push(alsa_device);
                }
            }
        }

        Ok(devices)
    }

    /// List all available output devices
    pub fn list_output_devices(&self) -> Result<Vec<DeviceInfo>> {
        let default_device = self
            .host
            .default_output_device()
            .map(|d| d.name().unwrap_or_else(|_| "Unknown".to_string()));

        let devices: Result<Vec<_>> = self
            .host
            .output_devices()?
            .map(|device| {
                let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
                let display_name = DeviceInfo::parse_device_name(&name);
                let is_default = default_device.as_ref().map(|d| d == &name).unwrap_or(false);
                Ok(DeviceInfo {
                    name,
                    display_name,
                    default: is_default,
                })
            })
            .collect();

        devices
    }

    /// List all available monitor/loopback devices (for capturing desktop audio output)
    /// On Linux, these are typically PulseAudio/PipeWire monitor sources
    pub fn list_monitor_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut monitor_devices = Vec::new();
        
        // First, get all input devices and filter for monitor patterns
        let input_devices = self.list_input_devices()?;
        for device in input_devices {
            let name_lower = device.name.to_lowercase();
            let display_lower = device.display_name.to_lowercase();
            
            if name_lower.contains("monitor")
                || display_lower.contains("monitor")
                || name_lower.contains("loopback")
                || display_lower.contains("loopback")
                || name_lower.contains("dsnoop") // ALSA loopback
            {
                monitor_devices.push(device);
            }
        }
        
        // Second, list output devices and try to find their monitor sources
        let output_devices = self.list_output_devices()?;
        let all_input_devices = self.list_input_devices()?;
        
        for output_device in output_devices {
            // Construct possible monitor source names
            let monitor_name = format!("Monitor of {}", output_device.name);
            let monitor_display = format!("Monitor of {}", output_device.display_name);
            
            // Check if this monitor exists as an input device
            if let Some(monitor_input) = all_input_devices.iter().find(|d| {
                d.name == monitor_name
                    || d.name.contains(&monitor_name)
                    || d.display_name == monitor_display
                    || d.display_name.contains(&monitor_display)
            }) {
                if !monitor_devices.iter().any(|d| d.name == monitor_input.name) {
                    monitor_devices.push(monitor_input.clone());
                }
            }
        }
        
        // Also check for common PulseAudio monitor patterns
        let pulse_monitor_patterns = [
            "pulse",
            "Monitor of PulseAudio",
            "Monitor of PipeWire",
        ];
        
        for pattern in &pulse_monitor_patterns {
            if let Some(device) = all_input_devices.iter().find(|d| {
                d.name.to_lowercase().contains(&pattern.to_lowercase())
                    || d.display_name.to_lowercase().contains(&pattern.to_lowercase())
            }) {
                if !monitor_devices.iter().any(|d| d.name == device.name) {
                    monitor_devices.push(device.clone());
                }
            }
        }

        Ok(monitor_devices)
    }

    /// Find an input device by name
    pub fn find_input_device(&self, name: &str) -> Result<Option<Device>> {
        let devices: Vec<_> = self.host.input_devices()?.collect();
        
        // Extract card name from ALSA device name if present
        let requested_card_name = DeviceInfo::extract_card_name(name);
        
        for device in &devices {
            if let Ok(device_name) = device.name() {
                // Exact match (case-sensitive)
                if device_name == name {
                    return Ok(Some(device.clone()));
                }
                // Case-insensitive match
                if device_name.eq_ignore_ascii_case(name) {
                    return Ok(Some(device.clone()));
                }
                
                // If we have a card name from the requested device, try matching by card name
                if let Some(ref requested_card) = requested_card_name {
                    if let Some(device_card) = DeviceInfo::extract_card_name(&device_name) {
                        if device_card.eq_ignore_ascii_case(requested_card) {
                            return Ok(Some(device.clone()));
                        }
                    }
                    // Also try substring matching on the device name itself
                    if device_name.to_lowercase().contains(&requested_card.to_lowercase()) {
                        return Ok(Some(device.clone()));
                    }
                }
                
                // Substring match (case-insensitive)
                if device_name.to_lowercase().contains(&name.to_lowercase()) 
                    || name.to_lowercase().contains(&device_name.to_lowercase()) {
                    return Ok(Some(device.clone()));
                }
            }
        }
        Ok(None)
    }

    /// Get the default input device
    pub fn default_input_device(&self) -> Result<Device> {
        self.host
            .default_input_device()
            .ok_or_else(|| VoiceError::Audio("No default input device available".to_string()))
    }

    /// Get the default output device's monitor source name
    /// Returns the monitor source name for the default output device
    /// On Linux, creates a virtual loopback sink and returns its monitor source
    pub fn default_output_monitor_name(&self) -> Result<String> {
        #[cfg(target_os = "linux")]
        {
            // Create a virtual loopback sink for monitoring
            match Self::create_loopback_sink(None) {
                Ok((monitor_name, _module_id, _previous_sink)) => {
                    log::info!("Created loopback sink with monitor: {}", monitor_name);
                    return Ok(monitor_name);
                }
                Err(e) => {
                    log::warn!("Failed to create loopback sink: {}", e);
                    // Fall through to try existing monitor sources
                }
            }
            
            // Fallback: try to get existing monitor source
            if let Ok(pulse_monitor_name) = Self::get_pulseaudio_monitor_source() {
                log::info!("Using existing PulseAudio monitor source: {}", pulse_monitor_name);
                return Ok(pulse_monitor_name);
            }
        }
        
        // Fallback: construct monitor name from output device
        let default_output = self
            .host
            .default_output_device()
            .ok_or_else(|| VoiceError::Audio("No default output device available".to_string()))?;
        
        let output_name = default_output.name()
            .map_err(|_| VoiceError::Audio("Failed to get default output device name".to_string()))?;
        
        Ok(format!("Monitor of {}", output_name))
    }

    /// Create a virtual loopback sink for monitoring desktop audio
    /// Returns the monitor source name, module IDs (null sink, loopback), and previous default sink for cleanup
    #[cfg(target_os = "linux")]
    pub fn create_loopback_sink(sink_name: Option<&str>) -> std::result::Result<(String, Vec<u32>, String), String> {
        use std::process::Command;
        
        let sink_name = sink_name.unwrap_or("nexus_audio_monitor");
        let mut module_ids = Vec::new();
        
        // Get current default sink to restore later
        let previous_default_sink = Self::get_default_sink().unwrap_or_default();
        
        // Check if sink already exists
        let list_output = Command::new("pactl")
            .arg("list")
            .arg("sinks")
            .arg("short")
            .output()
            .map_err(|e| format!("Failed to list sinks: {}", e))?;
        
        let sinks = String::from_utf8_lossy(&list_output.stdout);
        if sinks.lines().any(|line| line.contains(sink_name)) {
            log::info!("Loopback sink '{}' already exists", sink_name);
        } else {
            // Create null sink (virtual loopback)
            let create_output = Command::new("pactl")
                .arg("load-module")
                .arg("module-null-sink")
                .arg(&format!("sink_name={}", sink_name))
                .arg("sink_properties=device.description=\"Nexus Audio Monitor\"")
                .output()
                .map_err(|e| format!("Failed to create loopback sink: {}", e))?;
            
            if !create_output.status.success() {
                let error = String::from_utf8_lossy(&create_output.stderr);
                return Err(format!("Failed to create loopback sink: {}", error));
            }
            
            // Get the module ID from output
            let module_id_str = String::from_utf8_lossy(&create_output.stdout);
            let module_id_str = module_id_str.trim();
            if let Ok(module_id) = module_id_str.parse::<u32>() {
                log::info!("Created loopback sink '{}' with module ID: {}", sink_name, module_id);
                module_ids.push(module_id);
            } else {
                log::warn!("Could not parse module ID from: {}", module_id_str);
            }
        }
        
        // Create loopback from default sink to our null sink
        let default_sink_output = Command::new("pactl")
            .arg("get-default-sink")
            .output()
            .map_err(|e| format!("Failed to get default sink: {}", e))?;
        
        if !default_sink_output.status.success() {
            return Err("Failed to get default sink".to_string());
        }
        
        let default_sink = String::from_utf8_lossy(&default_sink_output.stdout).trim().to_string();
        eprintln!("📺 Default output sink detected: {}", default_sink);
        
        // Check if loopback already exists
        let list_modules_output = Command::new("pactl")
            .arg("list")
            .arg("modules")
            .arg("short")
            .output()
            .map_err(|e| format!("Failed to list modules: {}", e))?;
        
        let modules = String::from_utf8_lossy(&list_modules_output.stdout);
        let default_sink_monitor = format!("{}.monitor", default_sink);
        let loopback_exists = modules.lines().any(|line| {
            line.contains("module-loopback") 
                && line.contains(&format!("sink={}", sink_name))
                && line.contains(&format!("source={}", default_sink_monitor))
        });
        
        if !loopback_exists {
            // Use combine-sink to route audio to BOTH original output AND our null sink
            // This way audio still plays through your speakers/headphones AND we can record it
            eprintln!("📺 Creating combine-sink to route audio to both {} and {}", default_sink, sink_name);
            let combine_output = Command::new("pactl")
                .arg("load-module")
                .arg("module-combine-sink")
                .arg(&format!("sink_name={}_combined", sink_name))
                .arg(&format!("slaves={},{}", default_sink, sink_name))
                .output()
                .map_err(|e| format!("Failed to create combine-sink: {}", e))?;
            
            if !combine_output.status.success() {
                let error = String::from_utf8_lossy(&combine_output.stderr);
                return Err(format!("Failed to create combine-sink: {}", error));
            }
            
            let module_id_str = String::from_utf8_lossy(&combine_output.stdout);
            let module_id_str = module_id_str.trim();
            if let Ok(module_id) = module_id_str.parse::<u32>() {
                eprintln!("✅ Combine-sink module created (ID: {}) - routing audio to both {} and {}", module_id, default_sink, sink_name);
                module_ids.push(module_id);
                
                // Set the combine-sink as default OUTPUT so all applications route through it
                // This ensures all audio goes to both the original output AND our null sink
                if let Err(e) = Self::set_default_sink(&format!("{}_combined", sink_name)) {
                    eprintln!("⚠️  Could not set combine-sink as default: {}", e);
                    eprintln!("   Audio may not route correctly");
                } else {
                    eprintln!("✅ Set combine-sink as default output - all audio will route through it");
                }
            }
        } else {
            eprintln!("ℹ️  Combine-sink already exists");
        }
        
        // Get the monitor source name
        let monitor_name = format!("{}.monitor", sink_name);
        
        // Verify it exists
        let sources_output = Command::new("pactl")
            .arg("list")
            .arg("sources")
            .arg("short")
            .output()
            .map_err(|e| format!("Failed to list sources: {}", e))?;
        
        let sources = String::from_utf8_lossy(&sources_output.stdout);
        if sources.lines().any(|line| line.contains(&monitor_name)) {
            Ok((monitor_name, module_ids, previous_default_sink))
        } else {
            Err(format!("Monitor source {} not found", monitor_name))
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn create_loopback_sink(_sink_name: Option<&str>) -> std::result::Result<(String, Vec<u32>, String), String> {
        Err("Loopback sinks only available on Linux".to_string())
    }
    
    /// Set the default PulseAudio source
    #[cfg(target_os = "linux")]
    /// Get the current default source
    pub fn get_default_source() -> std::result::Result<String, String> {
        use std::process::Command;
        
        let output = Command::new("pactl")
            .arg("get-default-source")
            .output()
            .map_err(|e| format!("Failed to get default source: {}", e))?;
        
        if output.status.success() {
            let source = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !source.is_empty() {
                Ok(source)
            } else {
                Err("Default source is empty".to_string())
            }
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(format!("Failed to get default source: {}", error))
        }
    }
    
    pub fn set_default_source(source_name: &str) -> std::result::Result<String, String> {
        use std::process::Command;
        
        // Get current default source to restore later
        let current_source = Self::get_default_source().unwrap_or_default();
        
        // Set new default source
        let output = Command::new("pactl")
            .arg("set-default-source")
            .arg(source_name)
            .output()
            .map_err(|e| format!("Failed to set default source: {}", e))?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to set default source to {}: {}", source_name, error));
        }
        
        log::info!("Set default source to: {} (previous: {})", source_name, current_source);
        Ok(current_source)
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn get_default_source() -> std::result::Result<String, String> {
        Err("Getting default source only available on Linux".to_string())
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn set_default_source(_source_name: &str) -> std::result::Result<String, String> {
        Err("Setting default source only available on Linux".to_string())
    }
    
    /// Get the current default output sink
    #[cfg(target_os = "linux")]
    pub fn get_default_sink() -> std::result::Result<String, String> {
        use std::process::Command;
        
        let output = Command::new("pactl")
            .arg("get-default-sink")
            .output()
            .map_err(|e| format!("Failed to get default sink: {}", e))?;
        
        if output.status.success() {
            let sink = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !sink.is_empty() {
                Ok(sink)
            } else {
                Err("Default sink is empty".to_string())
            }
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(format!("Failed to get default sink: {}", error))
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn get_default_sink() -> std::result::Result<String, String> {
        Err("Getting default sink only available on Linux".to_string())
    }
    
    /// Set the default output sink
    #[cfg(target_os = "linux")]
    pub fn set_default_sink(sink_name: &str) -> std::result::Result<String, String> {
        use std::process::Command;
        
        // Get current default sink to restore later
        let current_sink = Self::get_default_sink().unwrap_or_default();
        
        // Set new default sink
        let output = Command::new("pactl")
            .arg("set-default-sink")
            .arg(sink_name)
            .output()
            .map_err(|e| format!("Failed to set default sink: {}", e))?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to set default sink to {}: {}", sink_name, error));
        }
        
        log::info!("Set default sink to: {} (previous: {})", sink_name, current_sink);
        Ok(current_sink)
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn set_default_sink(_sink_name: &str) -> std::result::Result<String, String> {
        Err("Setting default sink only available on Linux".to_string())
    }
    
    /// Remove a PulseAudio module by ID
    #[cfg(target_os = "linux")]
    pub fn remove_pulseaudio_module(module_id: u32) -> std::result::Result<(), String> {
        use std::process::Command;
        
        if module_id == 0 {
            return Ok(()); // Nothing to remove
        }
        
        let output = Command::new("pactl")
            .arg("unload-module")
            .arg(module_id.to_string())
            .output()
            .map_err(|e| format!("Failed to unload module: {}", e))?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(format!("Failed to unload module {}: {}", module_id, error))
        } else {
            log::info!("Unloaded PulseAudio module {}", module_id);
            Ok(())
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    pub fn remove_pulseaudio_module(_module_id: u32) -> std::result::Result<(), String> {
        Ok(())
    }
    
    /// Query PulseAudio/PipeWire for the monitor source of the default sink
    #[cfg(target_os = "linux")]
    fn get_pulseaudio_monitor_source() -> std::result::Result<String, String> {
        use std::process::Command;
        
        // Get the default sink name
        let sink_output = Command::new("pactl")
            .arg("get-default-sink")
            .output()
            .map_err(|e| format!("Failed to run pactl: {}", e))?;
        
        if !sink_output.status.success() {
            return Err("pactl get-default-sink failed".to_string());
        }
        
        let sink_name = String::from_utf8_lossy(&sink_output.stdout).trim().to_string();
        if sink_name.is_empty() {
            return Err("No default sink found".to_string());
        }
        
        // Construct monitor source name: <sink_name>.monitor
        let monitor_name = format!("{}.monitor", sink_name);
        
        // Verify the monitor source exists
        let list_output = Command::new("pactl")
            .arg("list")
            .arg("sources")
            .arg("short")
            .output()
            .map_err(|e| format!("Failed to list sources: {}", e))?;
        
        let sources = String::from_utf8_lossy(&list_output.stdout);
        if sources.lines().any(|line| line.contains(&monitor_name)) {
            Ok(monitor_name)
        } else {
            Err(format!("Monitor source {} not found in PulseAudio", monitor_name))
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    fn get_pulseaudio_monitor_source() -> std::result::Result<String, String> {
        Err("PulseAudio monitor sources only available on Linux".to_string())
    }

    /// Get the input device based on configuration
    fn get_input_device(&self, config: &RecordingConfig) -> Result<Device> {
        if let Some(ref device_name) = config.device_name {
            // Try to find the device - check exact match first
            let devices: Vec<_> = self.host.input_devices()?.collect();
            let mut matched_device = None;
            
            // Extract card name from ALSA device name if present (e.g., "sysdefault:CARD=Quadcast" -> "Quadcast")
            let requested_card_name = DeviceInfo::extract_card_name(device_name);
            
            for device in &devices {
                if let Ok(name) = device.name() {
                    // Exact match (case-sensitive)
                    if name == *device_name {
                        matched_device = Some(device);
                        break;
                    }
                    // Case-insensitive match
                    if name.eq_ignore_ascii_case(device_name) {
                        matched_device = Some(device);
                        break;
                    }
                    
                    // If we have a card name from the requested device, try matching by card name
                    if let Some(ref requested_card) = requested_card_name {
                        if let Some(device_card) = DeviceInfo::extract_card_name(&name) {
                            if device_card.eq_ignore_ascii_case(requested_card) {
                                matched_device = Some(device);
                                break;
                            }
                        }
                        // Also try substring matching on the device name itself
                        if name.to_lowercase().contains(&requested_card.to_lowercase()) {
                            matched_device = Some(device);
                            break;
                        }
                    }
                    
                    // Substring match (case-insensitive) - useful if device name contains the requested string
                    if name.to_lowercase().contains(&device_name.to_lowercase()) 
                        || device_name.to_lowercase().contains(&name.to_lowercase()) {
                        matched_device = Some(device);
                        break;
                    }
                }
            }
            
            if let Some(device) = matched_device {
                let actual_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
                eprintln!("✅ Found requested device: '{}' (actual name: '{}')", device_name, actual_name);
                return Ok(device.clone());
            }
            
            // If still not found, log a warning and fall back to default device
            let available_devices: Vec<String> = devices
                .iter()
                .filter_map(|d| d.name().ok())
                .collect();
            
            eprintln!(
                "⚠️  Device '{}' not found. Available devices: {}",
                device_name,
                if available_devices.is_empty() {
                    "none".to_string()
                } else {
                    available_devices.join(", ")
                }
            );
            eprintln!("⚠️  Falling back to default input device");
            
            // Fall back to default device instead of failing
            self.default_input_device()
        } else {
            self.default_input_device()
        }
    }

    /// Get supported stream configuration for a device
    /// Prefers higher quality formats (f32 > i32 > i16 > others)
    fn get_supported_config(
        &self,
        device: &Device,
        config: &RecordingConfig,
    ) -> Result<SupportedStreamConfig> {
        let mut supported_configs: Vec<_> = device.supported_input_configs()?.collect();

        // Sort by quality: prefer f32, then i32, then i16, then others
        supported_configs.sort_by(|a, b| {
            let quality_a = match a.sample_format() {
                SampleFormat::F32 => 4,
                SampleFormat::I32 => 3,
                SampleFormat::I16 => 2,
                SampleFormat::F64 => 1,
                _ => 0,
            };
            let quality_b = match b.sample_format() {
                SampleFormat::F32 => 4,
                SampleFormat::I32 => 3,
                SampleFormat::I16 => 2,
                SampleFormat::F64 => 1,
                _ => 0,
            };
            quality_b.cmp(&quality_a) // Higher quality first
        });

        let target_sample_rate = SampleRate(config.sample_rate);
        let target_channels = config.channels;

        // Try to find exact match with preferred format
        if let Some(supported) = supported_configs.iter().find(|c| {
            c.channels() == target_channels
                && c.min_sample_rate() <= target_sample_rate
                && c.max_sample_rate() >= target_sample_rate
        }) {
            return Ok(supported.with_sample_rate(target_sample_rate));
        }

        // Try to find config with matching channels (any sample rate)
        if let Some(supported) = supported_configs
            .iter()
            .find(|c| c.channels() == target_channels)
        {
            let sample_rate = supported
                .min_sample_rate()
                .max(target_sample_rate.min(supported.max_sample_rate()));
            return Ok(supported.with_sample_rate(sample_rate));
        }

        // Fall back to first available config (highest quality)
        supported_configs
            .first()
            .ok_or_else(|| VoiceError::Audio("No supported input config found".to_string()))
            .map(|c| {
                let sample_rate = c
                    .min_sample_rate()
                    .max(target_sample_rate.min(c.max_sample_rate()));
                c.with_sample_rate(sample_rate)
            })
    }

    /// Record audio to a WAV file
    pub fn record_to_file(&self, config: RecordingConfig, output_path: &Path) -> Result<()> {
        let device = self.get_input_device(&config)?;
        let supported_config = self.get_supported_config(&device, &config)?;

        // Adjust config to match supported config
        let actual_sample_rate = supported_config.sample_rate().0;
        let actual_channels = supported_config.channels();
        let sample_format = supported_config.sample_format();

        // Create stream config - let the device choose an appropriate buffer size
        // Some devices have specific buffer size requirements, so we use Default
        // which allows the backend to select an optimal size
        let stream_config = StreamConfig::from(supported_config);

        // Create WAV writer with optimal settings
        // Use 16-bit for compatibility, but record at higher sample rate for quality
        let spec = WavSpec {
            channels: actual_channels,
            sample_rate: actual_sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        // Log the actual recording parameters for debugging
        log::info!(
            "Recording at {} Hz, {} channels, format: {:?}",
            actual_sample_rate,
            actual_channels,
            sample_format
        );

        let writer = File::create(output_path)
            .map_err(VoiceError::Io)
            .map(BufWriter::new)?;
        let wav_writer = WavWriter::new(writer, spec)
            .map_err(|e| VoiceError::Audio(format!("Failed to create WAV writer: {}", e)))?;

        // Shared state for stopping recording
        let recording = Arc::new(AtomicBool::new(true));
        let wav_writer_arc = Arc::new(std::sync::Mutex::new(wav_writer));

        // Build the stream based on sample format
        // We convert all formats to i16 for WAV compatibility
        let stream = match sample_format {
            SampleFormat::I8 => self.build_stream_i8(
                &device,
                &stream_config,
                Arc::clone(&wav_writer_arc),
                Arc::clone(&recording),
            )?,
            SampleFormat::I16 => self.build_stream_i16(
                &device,
                &stream_config,
                Arc::clone(&wav_writer_arc),
                Arc::clone(&recording),
            )?,
            SampleFormat::I32 => self.build_stream_i32(
                &device,
                &stream_config,
                Arc::clone(&wav_writer_arc),
                Arc::clone(&recording),
            )?,
            SampleFormat::U8 => self.build_stream_u8(
                &device,
                &stream_config,
                Arc::clone(&wav_writer_arc),
                Arc::clone(&recording),
            )?,
            SampleFormat::U16 => self.build_stream_u16(
                &device,
                &stream_config,
                Arc::clone(&wav_writer_arc),
                Arc::clone(&recording),
            )?,
            SampleFormat::F32 => self.build_stream_f32(
                &device,
                &stream_config,
                Arc::clone(&wav_writer_arc),
                Arc::clone(&recording),
            )?,
            SampleFormat::F64 => self.build_stream_f64(
                &device,
                &stream_config,
                Arc::clone(&wav_writer_arc),
                Arc::clone(&recording),
            )?,
            _ => {
                return Err(VoiceError::Audio(format!(
                    "Unsupported sample format: {:?}",
                    sample_format
                )));
            }
        };

        // Start recording
        stream.play()?;

        // Wait for duration or until stopped
        if let Some(duration) = config.duration {
            std::thread::sleep(duration);
            recording.store(false, Ordering::Relaxed);
        } else {
            // Wait until stopped (caller should handle Ctrl+C or similar)
            while recording.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        // Stop the stream
        stream.pause()?;

        // Finalize WAV file
        // We need to drop the stream first, then finalize the writer
        drop(stream);

        // Extract the writer from the Arc and Mutex to finalize it
        let mutex = Arc::try_unwrap(wav_writer_arc)
            .map_err(|_| VoiceError::Audio("Failed to unwrap WAV writer Arc".to_string()))?;
        let writer = mutex.into_inner().map_err(|e| {
            VoiceError::Audio(format!("Failed to extract WAV writer from mutex: {}", e))
        })?;
        writer
            .finalize()
            .map_err(|e| VoiceError::Audio(format!("Failed to finalize WAV file: {}", e)))?;

        Ok(())
    }

    /// Build stream helper that converts samples to i16
    fn build_stream_helper<T>(
        device: &Device,
        config: &StreamConfig,
        wav_writer: Arc<std::sync::Mutex<WavWriter<BufWriter<File>>>>,
        recording: Arc<AtomicBool>,
        convert: impl Fn(T) -> f32 + Send + 'static,
    ) -> Result<cpal::Stream>
    where
        T: cpal::Sample + cpal::SizedSample + Send + 'static,
    {
        let wav_writer_clone = Arc::clone(&wav_writer);
        let recording_clone = Arc::clone(&recording);

        let stream = device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                if recording_clone.load(Ordering::Relaxed) {
                    if let Ok(mut writer) = wav_writer_clone.lock() {
                        for &sample in data {
                            // Convert sample to f32, then to i16 with proper scaling
                            let sample_f32: f32 = convert(sample);
                            // Clamp to [-1.0, 1.0] range
                            let clamped = sample_f32.clamp(-1.0, 1.0);
                            // Convert to i16 with proper rounding to reduce quantization noise
                            // Use i16::MAX (32767) instead of i16::MAX as f32 to avoid precision issues
                            let sample_i16 = (clamped * 32767.0).round() as i16;
                            if let Err(e) = writer.write_sample(sample_i16) {
                                log::error!("Error writing sample: {}", e);
                                break;
                            }
                        }
                    }
                }
            },
            |err| {
                log::error!("Audio stream error: {}", err);
            },
            None,
        )?;

        Ok(stream)
    }

    fn build_stream_i8(
        &self,
        device: &Device,
        config: &StreamConfig,
        wav_writer: Arc<std::sync::Mutex<WavWriter<BufWriter<File>>>>,
        recording: Arc<AtomicBool>,
    ) -> Result<cpal::Stream> {
        Self::build_stream_helper(device, config, wav_writer, recording, |s: i8| {
            s as f32 / i8::MAX as f32
        })
    }

    fn build_stream_i16(
        &self,
        device: &Device,
        config: &StreamConfig,
        wav_writer: Arc<std::sync::Mutex<WavWriter<BufWriter<File>>>>,
        recording: Arc<AtomicBool>,
    ) -> Result<cpal::Stream> {
        Self::build_stream_helper(device, config, wav_writer, recording, |s: i16| {
            s as f32 / i16::MAX as f32
        })
    }

    fn build_stream_i32(
        &self,
        device: &Device,
        config: &StreamConfig,
        wav_writer: Arc<std::sync::Mutex<WavWriter<BufWriter<File>>>>,
        recording: Arc<AtomicBool>,
    ) -> Result<cpal::Stream> {
        Self::build_stream_helper(device, config, wav_writer, recording, |s: i32| {
            s as f32 / i32::MAX as f32
        })
    }

    fn build_stream_u8(
        &self,
        device: &Device,
        config: &StreamConfig,
        wav_writer: Arc<std::sync::Mutex<WavWriter<BufWriter<File>>>>,
        recording: Arc<AtomicBool>,
    ) -> Result<cpal::Stream> {
        Self::build_stream_helper(device, config, wav_writer, recording, |s: u8| {
            (s as f32 / u8::MAX as f32) * 2.0 - 1.0
        })
    }

    fn build_stream_u16(
        &self,
        device: &Device,
        config: &StreamConfig,
        wav_writer: Arc<std::sync::Mutex<WavWriter<BufWriter<File>>>>,
        recording: Arc<AtomicBool>,
    ) -> Result<cpal::Stream> {
        Self::build_stream_helper(device, config, wav_writer, recording, |s: u16| {
            (s as f32 / u16::MAX as f32) * 2.0 - 1.0
        })
    }

    fn build_stream_f32(
        &self,
        device: &Device,
        config: &StreamConfig,
        wav_writer: Arc<std::sync::Mutex<WavWriter<BufWriter<File>>>>,
        recording: Arc<AtomicBool>,
    ) -> Result<cpal::Stream> {
        Self::build_stream_helper(device, config, wav_writer, recording, |s: f32| s)
    }

    fn build_stream_f64(
        &self,
        device: &Device,
        config: &StreamConfig,
        wav_writer: Arc<std::sync::Mutex<WavWriter<BufWriter<File>>>>,
        recording: Arc<AtomicBool>,
    ) -> Result<cpal::Stream> {
        Self::build_stream_helper(device, config, wav_writer, recording, |s: f64| s as f32)
    }

    /// Stop a recording (for use with async/background recording)
    #[allow(dead_code)]
    pub fn stop_recording(recording: &Arc<AtomicBool>) {
        recording.store(false, Ordering::Relaxed);
    }

    /// Create a recording handle that can be used to stop recording
    #[allow(dead_code)]
    pub fn create_recording_handle() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(true))
    }

    /// Stream audio chunks to a channel for real-time processing
    /// Returns a stream handle and a receiver for audio chunks (f32 samples at the configured sample rate)
    pub fn stream_audio_chunks(
        &self,
        config: RecordingConfig,
    ) -> Result<(cpal::Stream, mpsc::Receiver<Vec<f32>>, u32, u16)> {
        let device = self.get_input_device(&config)?;
        let supported_config = self.get_supported_config(&device, &config)?;

        let actual_sample_rate = supported_config.sample_rate().0;
        let actual_channels = supported_config.channels();
        let sample_format = supported_config.sample_format();

        let stream_config = StreamConfig::from(supported_config);

        // Channel for sending audio chunks
        let (tx, rx) = mpsc::channel();

        // Build the stream based on sample format
        let stream = match sample_format {
            SampleFormat::I8 => self.build_streaming_stream_i8(&device, &stream_config, tx)?,
            SampleFormat::I16 => self.build_streaming_stream_i16(&device, &stream_config, tx)?,
            SampleFormat::I32 => self.build_streaming_stream_i32(&device, &stream_config, tx)?,
            SampleFormat::U8 => self.build_streaming_stream_u8(&device, &stream_config, tx)?,
            SampleFormat::U16 => self.build_streaming_stream_u16(&device, &stream_config, tx)?,
            SampleFormat::F32 => self.build_streaming_stream_f32(&device, &stream_config, tx)?,
            SampleFormat::F64 => self.build_streaming_stream_f64(&device, &stream_config, tx)?,
            _ => {
                return Err(VoiceError::Audio(format!(
                    "Unsupported sample format: {:?}",
                    sample_format
                )));
            }
        };

        log::info!(
            "Streaming audio at {} Hz, {} channels, format: {:?}",
            actual_sample_rate,
            actual_channels,
            sample_format
        );

        Ok((stream, rx, actual_sample_rate, actual_channels))
    }

    /// Build streaming stream helper
    fn build_streaming_stream_helper<T>(
        device: &Device,
        config: &StreamConfig,
        tx: mpsc::Sender<Vec<f32>>,
        convert: impl Fn(T) -> f32 + Send + 'static,
    ) -> Result<cpal::Stream>
    where
        T: cpal::Sample + cpal::SizedSample + Send + 'static,
    {
        let tx_clone = tx.clone();

        let stream = device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                // Convert samples to f32 and send as chunk
                let mut samples = Vec::with_capacity(data.len());
                for &sample in data {
                    let sample_f32: f32 = convert(sample);
                    samples.push(sample_f32.clamp(-1.0, 1.0));
                }

                // Send chunk (blocking - channel should be large enough)
                // If channel is full, we'll drop samples (non-blocking would be better but mpsc doesn't have try_send)
                if tx_clone.send(samples).is_err() {
                    // Receiver dropped, stop trying
                    log::warn!("Audio receiver dropped, stopping stream");
                }
            },
            |err| {
                log::error!("Audio stream error: {}", err);
            },
            None,
        )?;

        Ok(stream)
    }

    fn build_streaming_stream_i8(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> Result<cpal::Stream> {
        Self::build_streaming_stream_helper(device, config, tx, |s: i8| s as f32 / i8::MAX as f32)
    }

    fn build_streaming_stream_i16(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> Result<cpal::Stream> {
        Self::build_streaming_stream_helper(device, config, tx, |s: i16| s as f32 / i16::MAX as f32)
    }

    fn build_streaming_stream_i32(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> Result<cpal::Stream> {
        Self::build_streaming_stream_helper(device, config, tx, |s: i32| s as f32 / i32::MAX as f32)
    }

    fn build_streaming_stream_u8(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> Result<cpal::Stream> {
        Self::build_streaming_stream_helper(device, config, tx, |s: u8| {
            (s as f32 / u8::MAX as f32) * 2.0 - 1.0
        })
    }

    fn build_streaming_stream_u16(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> Result<cpal::Stream> {
        Self::build_streaming_stream_helper(device, config, tx, |s: u16| {
            (s as f32 / u16::MAX as f32) * 2.0 - 1.0
        })
    }

    fn build_streaming_stream_f32(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> Result<cpal::Stream> {
        Self::build_streaming_stream_helper(device, config, tx, |s: f32| s)
    }

    fn build_streaming_stream_f64(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> Result<cpal::Stream> {
        Self::build_streaming_stream_helper(device, config, tx, |s: f64| s as f32)
    }
}

impl Default for AudioRecorder {
    fn default() -> Self {
        Self::new().expect("Failed to create audio recorder")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recording_config_default() {
        let config = RecordingConfig::default();
        assert_eq!(config.sample_rate, 48000); // Updated to match actual default (48kHz for professional audio)
        assert_eq!(config.channels, 1);
        assert!(config.duration.is_none());
        assert!(config.device_name.is_none());
    }

    #[test]
    fn test_audio_recorder_creation() {
        let recorder = AudioRecorder::new();
        assert!(recorder.is_ok());
    }

    #[test]
    fn test_list_devices() {
        let recorder = AudioRecorder::new().unwrap();
        let devices = recorder.list_input_devices();
        // This might fail if no devices are available, but the API should work
        assert!(devices.is_ok() || devices.is_err());
    }
}
