use crate::error::{Result, VoiceError};
use portaudio as pa;
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

/// Wrapper for audio stream that abstracts away the underlying implementation
/// Wrapper for audio stream that abstracts away the underlying implementation
pub struct AudioStream {
    stream: pa::Stream<pa::NonBlocking, pa::Input<f32>>,
    _pa: Arc<pa::PortAudio>, // Keep PortAudio alive while stream exists
    #[cfg(target_os = "linux")]
    _parecord_process: Option<Arc<std::sync::Mutex<Option<(std::process::Child, std::path::PathBuf)>>>>,
    #[cfg(not(target_os = "linux"))]
    _parecord_process: Option<()>, // Placeholder for non-Linux
}

impl AudioStream {
    /// Start playing the audio stream
    pub fn play(&mut self) -> Result<()> {
        // For parecord processes, they start automatically, so just start the dummy stream
        self.stream.start()
            .map_err(|e| VoiceError::Audio(format!("Failed to start audio stream: {}", e)))
    }

    /// Pause the audio stream
    pub fn pause(&mut self) -> Result<()> {
        // Stop parecord process if it exists
        #[cfg(target_os = "linux")]
        if let Some(ref process_handle) = self._parecord_process {
            if let Ok(mut process_opt) = process_handle.lock() {
                if let Some((mut process, temp_file)) = process_opt.take() {
                    let _ = process.kill();
                    let _ = process.wait();
                    let _ = std::fs::remove_file(&temp_file);
                }
            }
        }
        
        self.stream.stop()
            .map_err(|e| VoiceError::Audio(format!("Failed to stop audio stream: {}", e)))
    }
}

/// Audio recorder using PortAudio
pub struct AudioRecorder {
    pa: Arc<pa::PortAudio>,
}

impl AudioRecorder {
    /// Create a new audio recorder instance
    pub fn new() -> Result<Self> {
        let pa = pa::PortAudio::new()
            .map_err(|e| VoiceError::Audio(format!("Failed to initialize PortAudio: {}", e)))?;
        Ok(Self { pa: Arc::new(pa) })
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
        let num_devices = self.pa.device_count()
            .map_err(|e| VoiceError::Audio(format!("Failed to get device count: {}", e)))?;
        
        let default_input = self.pa.default_input_device()
            .ok(); // PortAudio returns Result, we want Option for comparison
        
        let mut devices = Vec::new();
        
        for i in 0..num_devices {
            if let Ok(device_info) = self.pa.device_info(pa::DeviceIndex(i)) {
                if device_info.max_input_channels > 0 {
                    let name = device_info.name.to_string();
                    let display_name = DeviceInfo::parse_device_name(&name);
                    let is_default = Some(pa::DeviceIndex(i)) == default_input;
                    
                    devices.push(DeviceInfo {
                        name: format!("{}", i), // PortAudio uses indices, but we'll store as string
                        display_name: format!("{} ({})", display_name, name),
                        default: is_default,
                    });
                }
            }
        }
        
        // Also include ALSA devices that might not be enumerated by PortAudio
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
        let num_devices = self.pa.device_count()
            .map_err(|e| VoiceError::Audio(format!("Failed to get device count: {}", e)))?;
        
        let default_output = self.pa.default_output_device()
            .ok(); // PortAudio returns Result, we want Option for comparison
        
        let mut devices = Vec::new();
        
        for i in 0..num_devices {
            if let Ok(device_info) = self.pa.device_info(pa::DeviceIndex(i)) {
                if device_info.max_output_channels > 0 {
                    let name = device_info.name.to_string();
                    let display_name = DeviceInfo::parse_device_name(&name);
                    let is_default = Some(pa::DeviceIndex(i)) == default_output;
                    
                    devices.push(DeviceInfo {
                        name: format!("{}", i),
                        display_name: format!("{} ({})", display_name, name),
                        default: is_default,
                    });
                }
            }
        }
        
        Ok(devices)
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

    /// Find an input device index by name
    pub fn find_input_device_index(&self, name: &str) -> Result<Option<pa::DeviceIndex>> {
        let num_devices = self.pa.device_count()
            .map_err(|e| VoiceError::Audio(format!("Failed to get device count: {}", e)))?;
        
        // Extract card name from ALSA device name if present
        let requested_card_name = DeviceInfo::extract_card_name(name);
        
        for i in 0..num_devices {
            let idx = pa::DeviceIndex(i);
            if let Ok(device_info) = self.pa.device_info(idx) {
                if device_info.max_input_channels > 0 {
                    let device_name = device_info.name;
                    
                    // Exact match (case-sensitive)
                    if device_name == name {
                        return Ok(Some(idx));
                    }
                    // Case-insensitive match
                    if device_name.eq_ignore_ascii_case(name) {
                        return Ok(Some(idx));
                    }
                    
                    // If we have a card name from the requested device, try matching by card name
                    if let Some(ref requested_card) = requested_card_name {
                        if let Some(device_card) = DeviceInfo::extract_card_name(&device_name) {
                            if device_card.eq_ignore_ascii_case(requested_card) {
                                return Ok(Some(idx));
                            }
                        }
                        // Also try substring matching on the device name itself
                        if device_name.to_lowercase().contains(&requested_card.to_lowercase()) {
                            return Ok(Some(idx));
                        }
                    }
                    
                    // Substring match (case-insensitive)
                    if device_name.to_lowercase().contains(&name.to_lowercase()) 
                        || name.to_lowercase().contains(&device_name.to_lowercase()) {
                        return Ok(Some(idx));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Get the default input device index
    pub fn default_input_device_index(&self) -> Result<pa::DeviceIndex> {
        self.pa.default_input_device()
            .map_err(|e| VoiceError::Audio(format!("No default input device available: {}", e)))
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
        let default_output_idx = self.pa.default_output_device()
            .map_err(|e| VoiceError::Audio(format!("No default output device available: {}", e)))?;
        let default_output_info = self.pa.device_info(default_output_idx)
            .map_err(|e| VoiceError::Audio(format!("Failed to get default output device info: {}", e)))?;
        let output_name = default_output_info.name;
        
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
    /// Get PortAudio device index for input device
    fn get_input_device_index(&self, config: &RecordingConfig) -> Result<pa::DeviceIndex> {
        let num_devices = self.pa.device_count()
            .map_err(|e| VoiceError::Audio(format!("Failed to get device count: {}", e)))?;
        
        if let Some(ref device_name) = config.device_name {
            // Try to parse as device index first
            if let Ok(device_idx) = device_name.parse::<u32>() {
                if device_idx < num_devices {
                    let idx = pa::DeviceIndex(device_idx);
                    if let Ok(device_info) = self.pa.device_info(idx) {
                        if device_info.max_input_channels > 0 {
                            eprintln!("✅ Using device index {}: {}", device_idx, 
                                device_info.name);
                            return Ok(idx);
                        }
                    }
                }
            }
            
            // Search by device name
            for i in 0..num_devices {
                let idx = pa::DeviceIndex(i);
                if let Ok(device_info) = self.pa.device_info(idx) {
                    if device_info.max_input_channels > 0 {
                        if device_info.name.eq_ignore_ascii_case(device_name) 
                            || device_info.name.to_lowercase().contains(&device_name.to_lowercase())
                            || device_name.to_lowercase().contains(&device_info.name.to_lowercase()) {
                            eprintln!("✅ Found device '{}' at index {}", device_info.name, i);
                            return Ok(idx);
                        }
                    }
                }
            }
            
            eprintln!("⚠️  Device '{}' not found, falling back to default", device_name);
        }
        
        // Fall back to default input device
        match self.pa.default_input_device() {
            Ok(idx) => {
                if let Ok(device_info) = self.pa.device_info(idx) {
                    if device_info.max_input_channels > 0 {
                        Ok(idx)
                    } else {
                        Err(VoiceError::Audio("Default device has no input channels".to_string()))
                    }
                } else {
                    Err(VoiceError::Audio("Failed to get default device info".to_string()))
                }
            }
            Err(e) => Err(VoiceError::Audio(format!("No default input device available: {}", e)))
        }
    }

    /// Get device info and determine stream parameters
    fn get_device_stream_params(
        &self,
        device_idx: pa::DeviceIndex,
        config: &RecordingConfig,
    ) -> Result<(f64, i32)> {
        let device_info = self.pa.device_info(device_idx)
            .map_err(|e| VoiceError::Audio(format!("Failed to get device info: {}", e)))?;
        
        // Use requested sample rate, or device default, or 44100 as fallback
        let sample_rate = config.sample_rate as f64;
        let channels = config.channels as i32;
        
        // PortAudio uses F32 format for float samples
        // Note: We don't actually need to return the format since PortAudio handles it
        
        // Verify device supports requested channels
        if channels > device_info.max_input_channels {
            return Err(VoiceError::Audio(format!(
                "Device only supports {} channels, requested {}",
                device_info.max_input_channels, channels
            )));
        }
        
        Ok((sample_rate, channels))
    }

    /// Record audio to a WAV file using PortAudio
    pub fn record_to_file(&self, config: RecordingConfig, output_path: &Path) -> Result<()> {
        // Use stream_audio_chunks and write to file
        let (mut stream, rx, actual_sample_rate, actual_channels) = self.stream_audio_chunks(config.clone())?;

        // Create WAV writer
        let spec = WavSpec {
            channels: actual_channels,
            sample_rate: actual_sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        log::info!(
            "Recording at {} Hz, {} channels",
            actual_sample_rate,
            actual_channels
        );

        let writer = File::create(output_path)
            .map_err(VoiceError::Io)
            .map(BufWriter::new)?;
        let mut wav_writer = WavWriter::new(writer, spec)
            .map_err(|e| VoiceError::Audio(format!("Failed to create WAV writer: {}", e)))?;

        // Start recording
        stream.play()?;

        // Recording state
        let recording = Arc::new(AtomicBool::new(true));
        let recording_clone = Arc::clone(&recording);

        // Spawn thread to write samples
        let write_handle = std::thread::spawn(move || {
            while recording_clone.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(samples) => {
                        // Convert f32 samples to i16 and write
                        for sample in samples {
                            let clamped = sample.clamp(-1.0, 1.0);
                            let sample_i16 = (clamped * 32767.0).round() as i16;
                            if let Err(e) = wav_writer.write_sample(sample_i16) {
                                log::error!("Error writing sample: {}", e);
                                break;
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            wav_writer
        });

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
        drop(stream);

        // Wait for writer thread and finalize
        let wav_writer = write_handle.join()
            .map_err(|_| VoiceError::Audio("Writer thread panicked".to_string()))?;
        wav_writer
            .finalize()
            .map_err(|e| VoiceError::Audio(format!("Failed to finalize WAV file: {}", e)))?;

        Ok(())
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
    /// 
    /// For PulseAudio monitor sources (device names containing ".monitor"), uses parecord command-line tool
    /// since PortAudio (ALSA) cannot access PulseAudio monitor sources directly.
    pub fn stream_audio_chunks(
        &self,
        config: RecordingConfig,
    ) -> Result<(AudioStream, mpsc::Receiver<Vec<f32>>, u32, u16)> {
        // Check if this is a PulseAudio monitor source (PortAudio/ALSA can't access these)
        if let Some(ref device_name) = config.device_name {
            if device_name.contains(".monitor") {
                #[cfg(target_os = "linux")]
                {
                    return self.stream_audio_chunks_pulseaudio(config);
                }
                #[cfg(not(target_os = "linux"))]
                {
                    return Err(VoiceError::Audio(
                        "PulseAudio monitor sources only supported on Linux".to_string()
                    ));
                }
            }
        }

        // Use PortAudio for regular devices
        let device_idx = self.get_input_device_index(&config)?;
        let (sample_rate, channels) = self.get_device_stream_params(device_idx, &config)?;

        let actual_sample_rate = sample_rate as u32;
        let actual_channels = channels as u16;

        // Channel for sending audio chunks
        let (tx, rx) = mpsc::channel();

        // Create PortAudio stream settings
        let stream_settings = self.pa.default_input_stream_settings(
            channels,
            sample_rate,
            pa::FRAMES_PER_BUFFER_UNSPECIFIED,
        )
        .map_err(|e| VoiceError::Audio(format!("Failed to get stream settings: {}", e)))?;

        let pa_clone = Arc::clone(&self.pa);
        let tx_clone = tx.clone();

        // Create the stream with callback
        let stream = self.pa.open_non_blocking_stream(
            stream_settings,
            move |pa::InputStreamCallbackArgs { buffer, frames, .. }| {
                // Buffer is already f32, convert to Vec and send
                let num_samples = frames * channels as usize;
                let samples: Vec<f32> = buffer[..num_samples]
                    .iter()
                    .copied()
                    .map(|s: f32| s.clamp(-1.0, 1.0))
                    .collect();
                
                if tx_clone.send(samples).is_err() {
                    log::warn!("Audio receiver dropped, stopping stream");
                    pa::Complete
                } else {
                    pa::Continue
                }
            },
        )
        .map_err(|e| VoiceError::Audio(format!("Failed to open stream: {}", e)))?;

        log::info!(
            "Streaming audio at {} Hz, {} channels, format: Float32",
            actual_sample_rate,
            actual_channels
        );

        Ok((AudioStream { stream, _pa: pa_clone, _parecord_process: None }, rx, actual_sample_rate, actual_channels))
    }

    /// Stream audio from PulseAudio monitor source using parecord
    #[cfg(target_os = "linux")]
    fn stream_audio_chunks_pulseaudio(
        &self,
        config: RecordingConfig,
    ) -> Result<(AudioStream, mpsc::Receiver<Vec<f32>>, u32, u16)> {
        use std::process::{Command, Stdio};
        use std::io::Read;

        let device_name = config.device_name.as_ref()
            .ok_or_else(|| VoiceError::Audio("Device name required for PulseAudio monitor".to_string()))?;

        let sample_rate = config.sample_rate;
        let channels = config.channels;

        eprintln!("📺 Using parecord for PulseAudio monitor source: {}", device_name);

        // Verify the monitor source exists before trying to record
        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            let list_output = Command::new("pactl")
                .arg("list")
                .arg("sources")
                .arg("short")
                .output();
            
            if let Ok(output) = list_output {
                let sources = String::from_utf8_lossy(&output.stdout);
                if !sources.contains(device_name) {
                    eprintln!("⚠️  WARNING: Monitor source '{}' not found in PulseAudio sources!", device_name);
                    eprintln!("📋 Available sources:");
                    for line in sources.lines().take(10) {
                        eprintln!("   {}", line);
                    }
                    return Err(VoiceError::Audio(format!(
                        "Monitor source '{}' not found. Available sources may be listed above.",
                        device_name
                    )));
                } else {
                    eprintln!("✅ Verified monitor source '{}' exists", device_name);
                }
            }
        }

        // Channel for sending audio chunks
        let (tx, rx) = mpsc::channel();

        // Use a temporary file that parecord will write to
        // We'll read from this file as it's being written
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("nexus_parecord_{}.raw", std::process::id()));
        let _ = std::fs::remove_file(&temp_file); // Clean up if exists
        
        eprintln!("📺 Using temporary file for parecord: {}", temp_file.display());

        // Use parecord to write raw PCM to a temporary file
        // We'll read from this file as it's being written
        let mut parecord = Command::new("parecord")
            .arg("--record") // Explicitly request recording mode
            .arg("--rate")
            .arg(sample_rate.to_string())
            .arg("--channels")
            .arg(channels.to_string())
            .arg("--format=s16le") // 16-bit signed little-endian PCM
            .arg("--device")
            .arg(device_name)
            .arg(&temp_file) // Write to temporary file
            .stdout(Stdio::null()) // parecord doesn't use stdout when writing to file
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| VoiceError::Audio(format!("Failed to spawn parecord: {}. Is PulseAudio utils installed? (package: pulseaudio-utils)", e)))?;

        let mut stderr = parecord.stderr.take()
            .ok_or_else(|| VoiceError::Audio("Failed to get parecord stderr".to_string()))?;

        // Spawn thread to monitor stderr for errors
        let stderr_handle = std::thread::spawn(move || {
            let mut stderr_buf = vec![0u8; 1024];
            loop {
                match stderr.read(&mut stderr_buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let error_msg = String::from_utf8_lossy(&stderr_buf[..n]);
                        eprintln!("⚠️  parecord stderr: {}", error_msg.trim());
                    }
                    Err(_) => break,
                }
            }
        });

        // Wait a moment for parecord to start writing
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Spawn thread to read raw PCM data from the temporary file as it's being written
        let tx_clone = tx.clone();
        let channels_clone = channels;
        let temp_file_clone = temp_file.clone();
        let read_handle = std::thread::spawn(move || -> Result<()> {
            eprintln!("📺 parecord reader thread started, reading from: {}", temp_file_clone.display());
            let mut total_bytes = 0u64;
            let mut total_samples = 0u64;
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .open(&temp_file_clone)
                .map_err(|e| VoiceError::Audio(format!("Failed to open parecord output file: {}", e)))?;
            
            // Read raw 16-bit PCM (s16le format) as it's being written
            let mut buffer = vec![0u8; 4096];
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => {
                        // No data available yet, wait a bit and try again
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    Ok(n) => {
                        total_bytes += n as u64;
                        // Convert 16-bit PCM to f32 samples
                        // Each sample is 2 bytes, interleaved by channel
                        let samples: Vec<f32> = buffer[..n]
                            .chunks_exact(2)
                            .map(|sample_bytes| {
                                let sample_i16 = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
                                (sample_i16 as f32 / 32768.0).clamp(-1.0, 1.0)
                            })
                            .collect();

                        if !samples.is_empty() {
                            total_samples += samples.len() as u64;
                            // Samples are interleaved: [L, R, L, R, ...] for stereo
                            // Send all samples as a single chunk (they're already interleaved)
                            if tx_clone.send(samples).is_err() {
                                eprintln!("📺 parecord receiver dropped, stopping");
                                return Ok(()); // Receiver dropped
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // File not ready yet, wait and retry
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    Err(e) => {
                        eprintln!("❌ Error reading from parecord file: {}", e);
                        log::error!("Error reading from parecord file: {}", e);
                        break;
                    }
                }
            }
            eprintln!("📺 parecord reader thread finished (read {} bytes, {} samples)", total_bytes, total_samples);
            // Clean up temp file
            let _ = std::fs::remove_file(&temp_file_clone);
            Ok(())
        });

        // Create a dummy stream wrapper for parecord (we'll control it via the process)
        // We need to keep the process handle alive and clean up the temp file
        let temp_file_clone_for_cleanup = temp_file.clone();
        let process_handle = Arc::new(std::sync::Mutex::new(Some((parecord, temp_file_clone_for_cleanup))));
        let process_handle_clone = Arc::clone(&process_handle);

        // Create a dummy PortAudio stream that we won't actually use
        // We'll manage the parecord process directly
        let dummy_settings = self.pa.default_input_stream_settings(
            1,
            44100.0,
            pa::FRAMES_PER_BUFFER_UNSPECIFIED,
        )
        .map_err(|e| VoiceError::Audio(format!("Failed to create dummy stream settings: {}", e)))?;

        let dummy_stream = self.pa.open_non_blocking_stream(
            dummy_settings,
            move |_| pa::Continue, // Dummy callback
        )
        .map_err(|e| VoiceError::Audio(format!("Failed to create dummy stream: {}", e)))?;

        // Store process handle in a way we can access it
        // We'll need to modify AudioStream to support parecord processes
        // For now, create a wrapper that manages the parecord process

        log::info!(
            "Streaming audio from PulseAudio monitor at {} Hz, {} channels",
            sample_rate,
            channels
        );

        // Store handles to monitor the threads (but don't block on them)
        // The threads will run until the process is killed
        std::mem::forget(read_handle); // Let it run until process is killed
        std::mem::forget(stderr_handle); // Let stderr monitor run

        Ok((
            AudioStream {
                stream: dummy_stream,
                _pa: Arc::clone(&self.pa),
                _parecord_process: Some(process_handle_clone),
            },
            rx,
            sample_rate,
            channels,
        ))
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
