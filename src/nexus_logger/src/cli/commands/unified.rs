use crate::error::Result;
use crate::services::capture::InputEvent;
use crate::services::unified_recording::{
    DefaultEventCallback, EventCallback, InputCaptureConfig, OverlayLabel, ScreenRecordingConfig,
    UnifiedRecordingConfig, UnifiedRecordingService,
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
    #[arg(short = 'o', long, default_value = "recording.mp4")]
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

    /// Disable audio recording
    #[arg(long)]
    pub no_audio: bool,

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
            return Err(crate::error::LoggerError::Configuration(
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
        crate::error::LoggerError::Other(format!("Failed to set signal handler: {}", e))
    })?;

    // Create configuration
    let config = UnifiedRecordingConfig {
        screen_config: ScreenRecordingConfig {
            output_path: args.output.clone(),
            framerate: args.framerate,
            duration_secs: if args.duration > 0 {
                Some(args.duration)
            } else {
                None
            },
            monitor_index: args.monitor,
            include_audio: !args.no_audio,
        },
        input_config: InputCaptureConfig {
            output_file: args.events.clone(),
            format: args.events_format.clone(),
        },
        database_path: args.database.clone(),
        capture_keyboard: !args.no_keyboard,
        capture_mouse: !args.no_mouse,
        capture_mouse_moves: args.mouse_moves,
        show_timestamp: !args.no_timestamp,
        show_labels: !args.no_labels,
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
    println!("   Audio: {}", if !args.no_audio { "✓" } else { "✗" });
    println!(
        "   Timestamp overlay: {}",
        if !args.no_timestamp { "✓" } else { "✗" }
    );
    println!(
        "   Event labels: {}",
        if !args.no_labels { "✓" } else { "✗" }
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
                crate::error::LoggerError::Other(format!("Failed to create tray icon: {}", e))
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
        .map_err(|e| crate::error::LoggerError::Other(format!("Failed to create icon: {}", e)))
}
