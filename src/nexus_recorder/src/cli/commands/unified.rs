use crate::error::Result;
use crate::services::InputEvent;
use crate::services::unified_recording::{
    AudioRecordingConfig, DefaultEventCallback, EventCallback, InputCaptureConfig, OverlayLabel,
    ScreenRecordingConfig, UnifiedRecordingConfig, UnifiedRecordingService,
};
use chrono::DateTime;
use clap::Args;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(not(target_os = "linux"))]
use tray_icon::TrayIconEvent;
use tray_icon::{
    menu::{Menu, MenuItem},
    Icon, TrayIconBuilder,
};

#[cfg(target_os = "linux")]
use gtk::glib;

/// CLI arguments for the unified recording subcommand.
#[derive(Args)]
pub struct UnifiedArgs {
    /// Output video file path
    #[arg(short = 'o', long, default_value = "output/recording.mp4")]
    pub output: PathBuf,

    /// Frame rate for video recording
    #[arg(short = 'f', long, default_value = "30")]
    pub framerate: u32,

    /// Duration in seconds (0 = until stopped)
    #[arg(short = 'd', long, default_value = "0")]
    pub duration: u64,

    /// Monitor index (None = primary)
    #[arg(short = 'm', long)]
    pub monitor: Option<usize>,

    /// Use webcam instead of screen recording
    #[arg(long)]
    pub webcam: bool,

    /// Webcam device path (e.g., /dev/video0)
    #[arg(long, default_value = "/dev/video0")]
    pub webcam_device: String,

    /// Enable webcam preview window
    #[arg(long)]
    pub webcam_preview: bool,

    /// Disable screen recording (use with --webcam)
    #[arg(long)]
    pub no_screen: bool,

    /// Reset PTZ to center on webcam start (disables AI tracking mode)
    #[arg(long)]
    pub ptz_reset: bool,

    /// Reset PTZ and reconnect to reinitialize camera AI tracking
    #[arg(long)]
    pub ai_reinit: bool,

    /// Enable webcam sentiment analysis (analyzes user emotions/attention)
    #[arg(long)]
    pub webcam_analysis: bool,

    /// Webcam analysis interval in seconds (default: 5)
    #[arg(long, default_value = "5")]
    pub webcam_analysis_interval: u64,

    /// Disable system audio in screen recording
    #[arg(long)]
    pub no_audio: bool,

    /// Disable microphone audio recording
    #[arg(long)]
    pub no_mic_audio: bool,

    /// Microphone audio output file path
    #[arg(long, default_value = "output/recording.wav")]
    pub mic_audio_output: PathBuf,

    /// Microphone audio sample rate (Hz)
    #[arg(long, default_value = "48000")]
    pub mic_sample_rate: u32,

    /// Microphone device name (None = system default)
    /// Supports ALSA device names like: sysdefault:CARD=Quadcast, hw:CARD=Quadcast,DEV=0
    #[arg(long)]
    pub mic_device: Option<String>,

    /// Alias for --mic-device (shorter form, None = system default)
    #[arg(long)]
    pub device: Option<String>,

    /// Disable monitoring desktop audio output
    /// Creates a virtual loopback sink to capture system audio (Linux only)
    #[arg(long)]
    pub no_monitor_desktop_audio: bool,

    /// Path to Whisper model for transcription (defaults to .models/ggml-small-fp16.bin)
    #[arg(long)]
    pub whisper_model: Option<PathBuf>,

    /// Disable audio transcription (skips Whisper processing)
    #[arg(long)]
    pub no_transcription: bool,

    /// Database path for storing events
    #[arg(short = 'D', long, default_value = "events.db")]
    pub database: PathBuf,

    /// Events output file (None = stdout)
    #[arg(short = 'e', long)]
    pub events: Option<PathBuf>,

    /// Events output format: json, text, or both
    #[arg(long, default_value = "text")]
    pub events_format: String,

    /// Disable keyboard capture
    #[arg(long)]
    pub no_keyboard: bool,

    /// Disable mouse capture
    #[arg(long)]
    pub no_mouse: bool,

    /// Enable mouse move capture (can be verbose)
    #[arg(long)]
    pub mouse_moves: bool,

    /// Frames per second for full-video context analysis (0 = disable)
    #[arg(long, default_value = "0")]
    pub analysis_fps: f64,

    /// Enable verbose event callbacks (prints events with video timestamps)
    #[arg(long)]
    pub verbose: bool,

    /// Disable timestamp overlay on video
    #[arg(long)]
    pub no_timestamp: bool,

    /// Disable event label overlays on video
    #[arg(long)]
    pub no_labels: bool,
}

/// Runs the unified recording command based on args.
pub async fn run_unified(args: UnifiedArgs) -> Result<()> {
    // Validate events format
    match args.events_format.as_str() {
        "json" | "text" | "both" => {}
        _ => {
            return Err(crate::error::RecorderError::Configuration(
                "Events format must be 'json', 'text', or 'both'".to_string(),
            ));
        }
    }

    // Setup signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(false));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\n🛑 Stopping unified recording...");
        r.store(true, Ordering::SeqCst);
    })
    .map_err(|e| {
        crate::error::RecorderError::Other(format!("Failed to set signal handler: {}", e))
    })?;

    // Determine if we're using webcam or screen recording
    let use_webcam = args.webcam || args.no_screen;
    
    // Create configuration
    let config = UnifiedRecordingConfig {
        screen_config: if use_webcam {
            None
        } else {
            Some(ScreenRecordingConfig {
                output_path: args.output.clone(),
                framerate: args.framerate,
                duration_secs: if args.duration > 0 {
                    Some(args.duration)
                } else {
                    None
                },
                monitor_index: args.monitor,
                include_audio: !args.no_audio,
                segment_duration_secs: None, // TODO: Add CLI arg for video segmentation
            })
        },
        webcam_config: if use_webcam {
            Some(crate::services::webcam::WebcamRecordingConfig {
                device_path: args.webcam_device.clone(),
                output_path: args.output.clone(),
                framerate: args.framerate,
                max_duration_secs: if args.duration > 0 {
                    Some(args.duration)
                } else {
                    None
                },
                enable_preview: args.webcam_preview,
                preview_title: Some("Webcam Recording".to_string()),
                skip_ptz_reset: !args.ptz_reset, // Default skips PTZ reset to preserve AI tracking
                ai_reinit: args.ai_reinit, // Reset PTZ and reconnect to reinitialize AI
            })
        } else {
            None
        },
        input_config: InputCaptureConfig {
            output_file: args.events.clone(),
            format: args.events_format.clone(),
        },
        audio_configs: {
            let mut configs = Vec::new();

            // Determine model path: use provided path or default to .models/ggml-small-fp16.bin
            let model_path = args
                .whisper_model
                .clone()
                .unwrap_or_else(|| PathBuf::from(".models/ggml-small-fp16.bin"));

            // Transcription is enabled if:
            // 1. --no-transcription flag is NOT set, AND
            // 2. Model file exists
            let transcribe_enabled = !args.no_transcription && model_path.exists();

            // Check if model exists on startup and print message if not
            if !args.no_transcription && !model_path.exists() {
                eprintln!("⚠️  Whisper model not found at: {}", model_path.display());
                eprintln!("   You need to install models using install-models.sh");
                eprintln!("   Transcription will be disabled.");
            } else if args.no_transcription {
                eprintln!("ℹ️  Transcription disabled (--no-transcription flag)");
            }

            // Create microphone audio config if enabled (add FIRST so it locks onto real mic before loopback sink is created)
            // Enabled by default unless --no-mic-audio is specified
            if !args.no_mic_audio {
                // Use --device if provided, otherwise fall back to --mic-device
                // If both are None, pre-select a microphone device NOW (before loopback sink is created)
                let device_name = args.device.clone().or(args.mic_device.clone()).or_else(|| {
                    // Pre-select microphone device before desktop audio creates loopback sink
                    use crate::services::AudioRecorder;
                    if let Ok(recorder) = AudioRecorder::new() {
                        if let Ok(devices) = recorder.list_input_devices() {
                            eprintln!("🔍 Available input devices:");
                            for (idx, device) in devices.iter().enumerate() {
                                let default_marker = if device.default { " (default)" } else { "" };
                                eprintln!(
                                    "  {}. {}{} → {}",
                                    idx + 1,
                                    device.display_name,
                                    default_marker,
                                    device.name
                                );
                            }

                            // First, try to find common microphone names (case-insensitive)
                            let common_mic_names = ["quadcast", "microphone", "mic", "usb", "jack"];
                            for mic_name in &common_mic_names {
                                if let Some(mic_device) = devices.iter().find(|d| {
                                    let name_lower = d.name.to_lowercase();
                                    let display_lower = d.display_name.to_lowercase();
                                    name_lower.contains(mic_name)
                                        || display_lower.contains(mic_name)
                                }) {
                                    eprintln!(
                                        "🎤 Pre-selected microphone device: {} ({})",
                                        mic_device.display_name, mic_device.name
                                    );
                                    return Some(mic_device.name.clone());
                                }
                            }

                            // Otherwise, find first device that's not a monitor/loopback/nexus/pulse/default
                            if let Some(mic_device) = devices.iter().find(|d| {
                                let name_lower = d.name.to_lowercase();
                                let display_lower = d.display_name.to_lowercase();
                                !name_lower.contains("monitor") 
                                    && !name_lower.contains("loopback")
                                    && !name_lower.contains("dsnoop")
                                    && !name_lower.contains("nexus") // Exclude our loopback sink
                                    && !display_lower.contains("monitor")
                                    && !display_lower.contains("loopback")
                                    && d.name != "pulse" // pulse might route to monitor
                                    && d.name != "default" // default will route to monitor
                                    && d.name != "pipewire" // might also route to default
                            }) {
                                eprintln!(
                                    "🎤 Pre-selected microphone device: {} ({})",
                                    mic_device.display_name, mic_device.name
                                );
                                Some(mic_device.name.clone())
                            } else {
                                // If no suitable device found, use None to fall back to default device
                                // The audio recorder will use the system default, which is better than hardcoding "jack"
                                eprintln!("⚠️  Could not auto-detect suitable microphone device");
                                eprintln!("   Will use system default input device");
                                None // Use None to let the system choose the default
                            }
                        } else {
                            eprintln!("⚠️  Could not list devices, will use system default");
                            None // Use None to let the system choose the default
                        }
                    } else {
                        eprintln!("⚠️  Could not create audio recorder, will use system default");
                        None // Use None to let the system choose the default
                    }
                });
                configs.push(AudioRecordingConfig {
                    enabled: true,
                    output_path: args.mic_audio_output.clone(),
                    sample_rate: args.mic_sample_rate,
                    channels: 1, // Mono for mic
                    device_name,
                    monitor_desktop_audio: false,
                    transcribe: transcribe_enabled,
                    transcription_model_path: if transcribe_enabled {
                        Some(model_path.clone())
                    } else {
                        None
                    },
                });
            }

            // Create monitor desktop audio config if enabled (add AFTER microphone to avoid interference)
            // Enabled by default unless --no-monitor-desktop-audio is specified
            if !args.no_monitor_desktop_audio {
                let monitor_output_path = args
                    .mic_audio_output
                    .parent()
                    .map(|p| p.join("desktop_audio.wav"))
                    .unwrap_or_else(|| PathBuf::from("output/desktop_audio.wav"));

                configs.push(AudioRecordingConfig {
                    enabled: true,
                    output_path: monitor_output_path.clone(),
                    sample_rate: args.mic_sample_rate,
                    channels: 2,       // Stereo for desktop
                    device_name: None, // Will use default (monitor source)
                    monitor_desktop_audio: true,
                    transcribe: transcribe_enabled,
                    transcription_model_path: if transcribe_enabled {
                        Some(model_path.clone())
                    } else {
                        None
                    },
                });
            }

            configs
        },
        database_path: args.database.clone(),
        capture_keyboard: !args.no_keyboard,
        capture_mouse: !args.no_mouse,
        capture_mouse_moves: args.mouse_moves,
        show_timestamp: !args.no_timestamp,
        show_labels: !args.no_labels,
        context_fps: if args.analysis_fps > 0.0 {
            Some(args.analysis_fps)
        } else {
            None
        },
        // Use optimized writers by default for better performance
        event_writer_config: None, // Use legacy file writing (rotating writer not enabled by default)
        batch_inserter_config: None, // Use legacy direct inserts (batch inserter not enabled by default)
        // Webcam sentiment analysis configuration
        webcam_analysis_config: if use_webcam && args.webcam_analysis {
            Some(crate::services::unified_recording::WebcamAnalysisConfig::with_device(
                args.webcam_analysis_interval,
                args.webcam_device.clone(),
            ))
        } else {
            None
        },
    };

    println!("🎬 Starting unified recording...");
    println!("   Video: {}", args.output.display());
    if let Some(ref events) = args.events {
        println!("   Events: {}", events.display());
    } else {
        println!("   Events: stdout");
    }
    println!("   Database: {}", args.database.display());
    println!("   Frame rate: {} fps", args.framerate);
    if args.duration > 0 {
        println!("   Duration: {} seconds", args.duration);
    } else {
        println!("   Duration: until stopped");
    }
    println!("   Keyboard: {}", if !args.no_keyboard { "✓" } else { "✗" });
    println!("   Mouse: {}", if !args.no_mouse { "✓" } else { "✗" });
    println!(
        "   Mouse moves: {}",
        if args.mouse_moves { "✓" } else { "✗" }
    );
    // Determine model path for display (same logic as in config)
    let model_path = args
        .whisper_model
        .clone()
        .unwrap_or_else(|| PathBuf::from(".models/ggml-small-fp16.bin"));

    println!(
        "   System audio: {}",
        if !args.no_audio { "✓" } else { "✗" }
    );
    println!(
        "   Desktop audio monitoring: {}",
        if !args.no_monitor_desktop_audio {
            "✓"
        } else {
            "✗"
        }
    );

    // Display microphone config with actual device selection
    if !args.no_mic_audio {
        println!("   Microphone: ✓");
        println!("     Output: {}", args.mic_audio_output.display());
        println!("     Sample rate: {} Hz", args.mic_sample_rate);

        // Find the mic config to show actual device that was selected
        if let Some(mic_config) = config
            .audio_configs
            .iter()
            .find(|c| !c.monitor_desktop_audio)
        {
            if let Some(ref device) = mic_config.device_name {
                println!("     Device: {}", device);
            } else {
                println!("     Device: system default (not specified)");
            }
        }

        if args.no_transcription {
            println!("     Transcription: ✗ (disabled)");
        } else if model_path.exists() {
            println!("     Transcription: ✓ (model: {})", model_path.display());
        } else {
            println!("     Transcription: ✗ (model not found)");
        }
    } else {
        println!("   Microphone: ✗");
    }

    // Display desktop audio monitoring config with actual device selection
    if !args.no_monitor_desktop_audio {
        let monitor_output_path = args
            .mic_audio_output
            .parent()
            .map(|p| p.join("desktop_audio.wav"))
            .unwrap_or_else(|| PathBuf::from("output/desktop_audio.wav"));
        println!("   Desktop audio monitoring: ✓");
        println!("     Output: {}", monitor_output_path.display());
        println!("     Sample rate: {} Hz", args.mic_sample_rate);

        // Find the desktop audio config to show actual device that will be used
        if let Some(desktop_config) = config
            .audio_configs
            .iter()
            .find(|c| c.monitor_desktop_audio)
        {
            if let Some(ref device) = desktop_config.device_name {
                println!("     Device: {} (will be created)", device);
            } else {
                println!("     Device: PulseAudio loopback sink (will be auto-created)");
            }
        }

        if args.no_transcription {
            println!("     Transcription: ✗ (disabled)");
        } else if model_path.exists() {
            println!("     Transcription: ✓ (model: {})", model_path.display());
        } else {
            println!("     Transcription: ✗ (model not found)");
        }
    }
    println!(
        "   Timestamp overlay: {}",
        if !args.no_timestamp { "✓" } else { "✗" }
    );
    println!(
        "   Event labels: {}",
        if !args.no_labels { "✓" } else { "✗" }
    );
    println!(
        "   Context analysis: {}",
        if let Some(fps) = config.context_fps {
            format!("{:.2} fps", fps)
        } else {
            "disabled".to_string()
        }
    );
    if args.verbose {
        println!("   Verbose callbacks: ✓");
    }
    println!("\nPress Ctrl+C to stop\n");

    // Create service with appropriate callback
    let service = if args.verbose {
        UnifiedRecordingService::with_callback(config, VerboseEventCallback)
    } else {
        UnifiedRecordingService::with_callback(config, DefaultEventCallback)
    };

    // Start recording
    let session = service.start_recording(running.clone()).await?;
    let session_id = session.session_id().to_string();
    let recording_start = session.recording_start();

    // Create tray icon
    // Use std::sync::Mutex for blocking context (tray event handler)
    let session_for_tray = Arc::new(std::sync::Mutex::new(Some(session)));

    // Create icon (simple red circle for recording indicator)
    let icon = create_recording_icon()?;

    // On Linux, we need to initialize GTK and run the event loop
    #[cfg(target_os = "linux")]
    {
        // Initialize GTK in a separate thread
        let running_gtk = running.clone();
        let session_clone_gtk = session_for_tray.clone();
        std::thread::spawn(move || {
            // Initialize GTK
            if gtk::init().is_err() {
                eprintln!("⚠️  Failed to initialize GTK - tray icon may not appear");
                return;
            }

            // On Linux, click events don't work - we MUST use a menu
            // Create a menu for the tray icon
            let menu = Menu::new();
            let stop_item = MenuItem::new("Stop Recording", true, None);
            let stop_id = stop_item.id().clone(); // Clone ID before appending
            menu.append(&stop_item).unwrap();

            // Keep stop_item alive (menu borrows it)
            let _stop_item = stop_item;

            // Create tray icon with menu
            let tray_icon = match TrayIconBuilder::new()
                .with_icon(icon)
                .with_tooltip("Nexus Logger - Recording in progress\nRight-click for menu")
                .with_menu(Box::new(menu))
                .build()
            {
                Ok(icon) => {
                    eprintln!("✅ Tray icon created successfully with menu");
                    eprintln!("ℹ️  Right-click the tray icon and select 'Stop Recording' to stop");
                    icon
                }
                Err(e) => {
                    eprintln!("⚠️  Failed to create tray icon: {}", e);
                    return;
                }
            };

            // Spawn thread to poll for menu events (this is how clicks work on Linux)
            let session_clone = session_clone_gtk.clone();
            let running_clone = running_gtk.clone();
            std::thread::spawn(move || {
                use tray_icon::menu::MenuEvent;
                loop {
                    match MenuEvent::receiver().try_recv() {
                        Ok(event) => {
                            eprintln!("🔔 Menu event received: {:?}", event);
                            if event.id == stop_id {
                                eprintln!("🛑 Stop Recording menu item clicked");
                                running_clone.store(true, Ordering::SeqCst);
                                if let Ok(session_guard) = session_clone.try_lock() {
                                    if let Some(s) = session_guard.as_ref() {
                                        s.stop();
                                        eprintln!("✅ Session stop() called");
                                    }
                                }
                            }
                        }
                        Err(crossbeam_channel::TryRecvError::Empty) => {
                            // No event, continue
                        }
                        Err(crossbeam_channel::TryRecvError::Disconnected) => {
                            eprintln!("⚠️  Menu event receiver disconnected");
                            break;
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    if running_clone.load(Ordering::SeqCst) {
                        break;
                    }
                }
            });

            // Keep the tray icon alive by keeping GTK running
            // Run GTK event loop until stopped
            let running_loop = running_gtk.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                if running_loop.load(Ordering::SeqCst) {
                    gtk::main_quit();
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });

            // Keep reference to tray icon
            let _tray_icon = tray_icon;

            // Run GTK main loop
            gtk::main();
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
        // For non-Linux platforms, create tray icon directly
        let _tray_icon = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("Nexus Logger - Recording in progress")
            .build()
            .map_err(|e| {
                crate::error::RecorderError::Other(format!("Failed to create tray icon: {}", e))
            })?;

        // Set up event handler
        let session_clone = session_for_tray.clone();
        let running_clone = running.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            // Handle click events
            match event {
                TrayIconEvent::Click { button, .. } => {
                    use tray_icon::MouseButton;
                    if matches!(button, MouseButton::Left) {
                        println!("\n🛑 Stopping unified recording from tray icon...");
                        running_clone.store(true, Ordering::SeqCst);
                        if let Ok(session_guard) = session_clone.try_lock() {
                            if let Some(s) = session_guard.as_ref() {
                                s.stop();
                            }
                        }
                    }
                }
                TrayIconEvent::DoubleClick { .. } => {
                    println!("\n🛑 Stopping unified recording from tray icon...");
                    running_clone.store(true, Ordering::SeqCst);
                    if let Ok(session_guard) = session_clone.try_lock() {
                        if let Some(s) = session_guard.as_ref() {
                            s.stop();
                        }
                    }
                }
                _ => {}
            }
        }));
    }

    // Handle duration if specified
    if args.duration > 0 {
        // Wait for the specified duration, then stop
        tokio::time::sleep(tokio::time::Duration::from_secs(args.duration)).await;
        if let Ok(session_guard) = session_for_tray.lock() {
            if let Some(s) = session_guard.as_ref() {
                s.stop();
            }
        }
    } else {
        // Wait until stopped (either by tray click or Ctrl+C)
        while !running.load(Ordering::SeqCst) {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        // Ensure session is stopped
        if let Ok(session_guard) = session_for_tray.lock() {
            if let Some(s) = session_guard.as_ref() {
                s.stop();
            }
        }
    }

    // Wait for recording to complete
    // We need to move the session out, so we'll use a blocking call
    let session = tokio::task::spawn_blocking(move || {
        session_for_tray.lock().ok().and_then(|mut g| g.take())
    })
    .await
    .ok()
    .flatten();

    if let Some(s) = session {
        s.wait().await?;
    }

    println!("\n✅ Unified recording complete!");
    println!("   Session ID: {}", session_id);
    println!("   Started at: {}", recording_start);
    println!("   Video: {}", args.output.display());
    println!("   Database: {}", args.database.display());

    // Generate timeline
    println!("\n📋 Generating timeline...");
    let db = crate::services::Database::new().await?;
    let events = db.get_session_events(&session_id).await?;

    if events.is_empty() {
        println!("   No events found for session {}", session_id);
    } else {
        // Get session start time from database, fallback to recording_start
        let session_start = db
            .get_session_start_time(&session_id)
            .await?
            .unwrap_or(recording_start);
        crate::services::unified_recording::print_timeline(&session_id, &events, session_start)?;
    }

    Ok(())
}

/// Verbose callback that prints events with video timestamps.
struct VerboseEventCallback;

impl EventCallback for VerboseEventCallback {
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: DateTime<chrono::Utc>,
    ) -> (bool, Option<OverlayLabel>) {
        if let InputEvent::Keyboard { key, pressed, .. } = event {
            let action = if *pressed { "PRESS" } else { "RELEASE" };
            println!("🎬 [{:8.3}s] KEYBOARD {}: {}", video_timestamp, action, key);

            // Add label for important keys
            if *pressed && (key == "Enter" || key == "Escape" || key == "Space") {
                return (
                    true,
                    Some(OverlayLabel {
                        text: format!("Key: {}", key),
                        timestamp: video_timestamp,
                        duration: Some(2.0),
                        x: None,
                        y: None,
                    }),
                );
            }
        }
        (true, None)
    }

    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: DateTime<chrono::Utc>,
    ) -> (bool, Option<OverlayLabel>) {
        match event {
            InputEvent::Mouse {
                event_type,
                button,
                x,
                y,
                ..
            } => {
                match event_type.as_str() {
                    "click" => {
                        let btn_name = button.as_deref().unwrap_or("unknown");
                        println!(
                            "🎬 [{:8.3}s] MOUSE CLICK: {} at ({}, {})",
                            video_timestamp,
                            btn_name,
                            x.unwrap_or(0),
                            y.unwrap_or(0)
                        );

                        // Add label for mouse clicks
                        return (
                            true,
                            Some(OverlayLabel {
                                text: format!("Click: {}", btn_name),
                                timestamp: video_timestamp,
                                duration: Some(1.5),
                                x: x.map(|x| x as u32),
                                y: y.map(|y| y as u32),
                            }),
                        );
                    }
                    "release" => {
                        println!(
                            "🎬 [{:8.3}s] MOUSE RELEASE: {}",
                            video_timestamp,
                            button.as_ref().unwrap_or(&"unknown".to_string())
                        );
                    }
                    "move" => {
                        println!(
                            "🎬 [{:8.3}s] MOUSE MOVE: ({}, {})",
                            video_timestamp,
                            x.unwrap_or(0),
                            y.unwrap_or(0)
                        );
                    }
                    _ => {
                        println!(
                            "🎬 [{:8.3}s] MOUSE {}: {:?}",
                            video_timestamp, event_type, button
                        );
                    }
                }
            }
            _ => {}
        }
        (true, None)
    }
}

/// Creates a simple recording icon (red circle).
fn create_recording_icon() -> Result<Icon> {
    use image::{ImageBuffer, Rgba};

    // Create a simple red circle icon (16x16 pixels)
    let size = 16;
    let mut img = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(size, size);

    let center = (size / 2) as f32;
    let radius = (size / 2 - 2) as f32;

    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32) - center;
            let dy = (y as f32) - center;
            let distance = (dx * dx + dy * dy).sqrt();

            if distance <= radius {
                // Red color for recording indicator
                img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            } else {
                // Transparent background
                img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }

    Icon::from_rgba(img.into_raw(), size, size)
        .map_err(|e| crate::error::RecorderError::Other(format!("Failed to create icon: {}", e)))
}
