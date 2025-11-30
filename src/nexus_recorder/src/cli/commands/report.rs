use crate::error::Result;
use crate::services::Database;
use chrono::{DateTime, Local, Utc};
use clap::Args;
use serde_json::Value;

/// CLI arguments for the report subcommand.
#[derive(Args)]
pub struct ReportArgs {
    /// Session ID to generate report for (if not provided, shows summary of all sessions)
    #[arg(short, long)]
    pub session_id: Option<String>,

    /// Show report for the last (most recent) session
    #[arg(short, long)]
    pub last: bool,

    /// Show detailed event breakdown
    #[arg(short, long)]
    pub detailed: bool,

    /// Show all events (no limit). Only applies when --detailed is used.
    #[arg(short, long)]
    pub all: bool,
}

/// Runs the report command based on args.
pub async fn run_report(args: ReportArgs) -> Result<()> {
    let db = Database::new().await?;

    if args.last {
        // Get the last session ID
        let last_session_id = get_last_session_id(&db).await?;
        if let Some(session_id) = last_session_id {
            generate_session_report(&db, &session_id, args.detailed, args.all).await?;
        } else {
            println!("⚠️  No sessions found in database.");
        }
    } else if let Some(session_id) = args.session_id {
        generate_session_report(&db, &session_id, args.detailed, args.all).await?;
    } else {
        generate_summary_report(&db, args.detailed).await?;
    }

    Ok(())
}

/// Get the most recent session ID from the database
async fn get_last_session_id(_db: &Database) -> Result<Option<String>> {
    use nexus_core::services::ClickHouseConfig;

    let config = ClickHouseConfig::from_env();
    let http_port = if config.port == 9000 {
        8123
    } else {
        config.port
    };
    let url = format!("http://{}:{}", config.host, http_port);

    let query = "SELECT id FROM sessions ORDER BY start_time DESC LIMIT 1 FORMAT JSONEachRow";

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .query(&[("database", &config.database)])
        .basic_auth(&config.username, Some(&config.password))
        .body(query)
        .send()
        .await
        .map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to send HTTP request: {}", e))
        })?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let text = response.text().await.map_err(|e| {
        crate::error::RecorderError::Other(format!("Failed to read HTTP response: {}", e))
    })?;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(row) = serde_json::from_str::<Value>(line) {
            if let Some(id) = row.get("id").and_then(|v| v.as_str()) {
                return Ok(Some(id.to_string()));
            }
        }
    }

    Ok(None)
}

async fn generate_summary_report(_db: &Database, detailed: bool) -> Result<()> {
    use nexus_core::services::ClickHouseConfig;

    let config = ClickHouseConfig::from_env();
    let http_port = if config.port == 9000 {
        8123
    } else {
        config.port
    };
    let url = format!("http://{}:{}", config.host, http_port);

    println!("\n╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                    📊 Nexus Logger - Data Collection Report                  ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");

    // Get all sessions
    let query = "SELECT 
        id,
        start_time,
        end_time,
        keyboard_events,
        keyboard_presses,
        keyboard_releases,
        mouse_events,
        mouse_clicks,
        mouse_releases,
        mouse_moves
    FROM sessions
    ORDER BY start_time DESC
    FORMAT JSONEachRow";

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .query(&[("database", &config.database)])
        .basic_auth(&config.username, Some(&config.password))
        .body(query)
        .send()
        .await
        .map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to send HTTP request: {}", e))
        })?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(crate::error::RecorderError::Other(format!(
            "ClickHouse HTTP query failed: {}",
            error_text
        )));
    }

    let text = response.text().await.map_err(|e| {
        crate::error::RecorderError::Other(format!("Failed to read HTTP response: {}", e))
    })?;

    let mut sessions = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let row: Value = serde_json::from_str(line).map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to parse JSON row: {}", e))
        })?;

        sessions.push(row);
    }

    // "Total Sessions: " = 16 chars, so number gets: 74 - 16 = 58 chars
    println!("║ Total Sessions: {:>58} ║", sessions.len());
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");

    if sessions.is_empty() {
        println!("║ No sessions found in database.                                            ║");
        println!(
            "╚══════════════════════════════════════════════════════════════════════════════╝\n"
        );
        return Ok(());
    }

    // Calculate totals
    let mut total_keyboard_events = 0u64;
    let mut total_keyboard_presses = 0u64;
    let mut total_keyboard_releases = 0u64;
    let mut total_mouse_events = 0u64;
    let mut total_mouse_clicks = 0u64;
    let mut total_mouse_releases = 0u64;
    let mut total_mouse_moves = 0u64;

    for session in &sessions {
        total_keyboard_events += session
            .get("keyboard_events")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        total_keyboard_presses += session
            .get("keyboard_presses")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        total_keyboard_releases += session
            .get("keyboard_releases")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        total_mouse_events += session
            .get("mouse_events")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        total_mouse_clicks += session
            .get("mouse_clicks")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        total_mouse_releases += session
            .get("mouse_releases")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        total_mouse_moves += session
            .get("mouse_moves")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
    }

    println!("║ 📈 Overall Statistics:                                                       ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
    // Box is 78 chars: "║ " (2) + content (74) + " ║" (2) = 78
    // "Keyboard Events: " = 17 chars, so number gets: 74 - 17 = 57 chars
    println!("║ Keyboard Events: {:>57} ║", total_keyboard_events);
    // "  Presses: " = 11 chars, so number gets: 74 - 11 = 63 chars
    println!("║   Presses: {:>63} ║", total_keyboard_presses);
    // "  Releases: " = 12 chars, so number gets: 74 - 12 = 62 chars
    println!("║   Releases: {:>62} ║", total_keyboard_releases);
    // "Mouse Events: " = 14 chars, so number gets: 74 - 14 = 60 chars
    println!("║ Mouse Events: {:>60} ║", total_mouse_events);
    // "  Clicks: " = 10 chars, so number gets: 74 - 10 = 64 chars
    println!("║   Clicks: {:>64} ║", total_mouse_clicks);
    // "  Releases: " = 12 chars, so number gets: 74 - 12 = 62 chars
    println!("║   Releases: {:>62} ║", total_mouse_releases);
    // "  Moves: " = 9 chars, so number gets: 74 - 9 = 65 chars
    println!("║   Moves: {:>65} ║", total_mouse_moves);
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");

    // Get event type counts
    let event_query = "SELECT 
        toString(event_type) as event_type,
        count() as count
    FROM events
    GROUP BY event_type
    ORDER BY count DESC
    FORMAT JSONEachRow";

    let response = client
        .post(&url)
        .query(&[("database", &config.database)])
        .basic_auth(&config.username, Some(&config.password))
        .body(event_query)
        .send()
        .await
        .map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to send HTTP request: {}", e))
        })?;

    if response.status().is_success() {
        let text = response.text().await.map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to read HTTP response: {}", e))
        })?;

        println!("║ 📋 Event Types:                                                           ║");
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let row: Value = serde_json::from_str(line).map_err(|e| {
                crate::error::RecorderError::Other(format!("Failed to parse JSON row: {}", e))
            })?;

            let event_type = row
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let count: u64 = row.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            // Box width: 78, borders: 4, label: 2 spaces, so available: 72
            // Event type: max 30, count: right-aligned in remaining space
            println!("║   {:<30} {:>40} ║", event_type, count);
        }
        println!(
            "╠══════════════════════════════════════════════════════════════════════════════╣"
        );
    }

    // Get top keys
    let key_query = "SELECT 
        key,
        sum(count) as total
    FROM key_frequency
    GROUP BY key
    ORDER BY total DESC
    LIMIT 10
    FORMAT JSONEachRow";

    let response = client
        .post(&url)
        .query(&[("database", &config.database)])
        .basic_auth(&config.username, Some(&config.password))
        .body(key_query)
        .send()
        .await
        .map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to send HTTP request: {}", e))
        })?;

    if response.status().is_success() {
        let text = response.text().await.map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to read HTTP response: {}", e))
        })?;

        let mut has_keys = false;
        println!("║ ⌨️  Top 10 Keys:                                                          ║");
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            has_keys = true;
            let row: Value = serde_json::from_str(line).map_err(|e| {
                crate::error::RecorderError::Other(format!("Failed to parse JSON row: {}", e))
            })?;

            let key = row.get("key").and_then(|v| v.as_str()).unwrap_or("unknown");
            let count: u64 = row.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("║   {:<30} {:>40} ║", key, count);
        }
        if !has_keys {
            println!("║   (no key frequency data)                                               ║");
        }
        println!(
            "╠══════════════════════════════════════════════════════════════════════════════╣"
        );
    }

    // Get top mouse buttons
    let button_query = "SELECT 
        button,
        sum(count) as total
    FROM mouse_button_frequency
    GROUP BY button
    ORDER BY total DESC
    FORMAT JSONEachRow";

    let response = client
        .post(&url)
        .query(&[("database", &config.database)])
        .basic_auth(&config.username, Some(&config.password))
        .body(button_query)
        .send()
        .await
        .map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to send HTTP request: {}", e))
        })?;

    if response.status().is_success() {
        let text = response.text().await.map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to read HTTP response: {}", e))
        })?;

        let mut has_buttons = false;
        println!("║ 🖱️  Mouse Button Clicks:                                                   ║");
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            has_buttons = true;
            let row: Value = serde_json::from_str(line).map_err(|e| {
                crate::error::RecorderError::Other(format!("Failed to parse JSON row: {}", e))
            })?;

            let button = row
                .get("button")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let count: u64 = row.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("║   {:<30} {:>40} ║", button, count);
        }
        if !has_buttons {
            println!(
                "║   (no mouse button frequency data)                                        ║"
            );
        }
        println!(
            "╠══════════════════════════════════════════════════════════════════════════════╣"
        );
    }

    // Get screenshots count
    let screenshot_query = "SELECT count() as count FROM screenshots FORMAT JSONEachRow";
    let response = client
        .post(&url)
        .query(&[("database", &config.database)])
        .basic_auth(&config.username, Some(&config.password))
        .body(screenshot_query)
        .send()
        .await
        .map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to send HTTP request: {}", e))
        })?;

    if response.status().is_success() {
        let text = response.text().await.map_err(|e| {
            crate::error::RecorderError::Other(format!("Failed to read HTTP response: {}", e))
        })?;

        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let row: Value = serde_json::from_str(line).map_err(|e| {
                crate::error::RecorderError::Other(format!("Failed to parse JSON row: {}", e))
            })?;

            let count: u64 = row.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            // "📸 Screenshots Stored: " = 22 chars (emoji counts as 1), so number gets: 74 - 22 = 52 chars
            println!("║ 📸 Screenshots Stored: {:>52} ║", count);
        }
    }

    println!("╚══════════════════════════════════════════════════════════════════════════════╝\n");

    if detailed {
        println!("📋 Session Details:\n");
        for (idx, session) in sessions.iter().take(20).enumerate() {
            let session_id = session
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let start_time_str = session
                .get("start_time")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let end_time_str = session.get("end_time").and_then(|v| v.as_str());

            let start_time = parse_datetime(start_time_str).unwrap_or_else(Utc::now);
            let duration = if let Some(end_str) = end_time_str {
                if let Some(end_time) = parse_datetime(end_str) {
                    format_duration(end_time.signed_duration_since(start_time))
                } else {
                    "ongoing".to_string()
                }
            } else {
                "ongoing".to_string()
            };

            println!("{}. Session: {}", idx + 1, session_id);
            println!(
                "   Started: {}",
                start_time.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S")
            );
            println!("   Duration: {}", duration);
            println!(
                "   Keyboard: {} events ({} presses, {} releases)",
                session
                    .get("keyboard_events")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                session
                    .get("keyboard_presses")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                session
                    .get("keyboard_releases")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            );
            println!(
                "   Mouse: {} events ({} clicks, {} releases, {} moves)",
                session
                    .get("mouse_events")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                session
                    .get("mouse_clicks")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                session
                    .get("mouse_releases")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                session
                    .get("mouse_moves")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            );
            println!();
        }
        if sessions.len() > 20 {
            println!("... and {} more sessions", sessions.len() - 20);
        }
    }

    Ok(())
}

async fn generate_session_report(
    db: &Database,
    session_id: &str,
    detailed: bool,
    all_events: bool,
) -> Result<()> {
    println!("\n╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║              📊 Session Report: {:<40} ║", session_id);
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");

    // Get session metrics
    match db.get_session_metrics(session_id).await {
        Ok(metrics) => {
            println!(
                "║ Start Time: {:>64} ║",
                metrics
                    .start_time
                    .with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M:%S")
            );
            if let Some(end_time) = metrics.end_time {
                println!(
                    "║ End Time: {:>66} ║",
                    end_time.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S")
                );
                let duration = end_time.signed_duration_since(metrics.start_time);
                println!("║ Duration: {:>64} ║", format_duration(duration));
            } else {
                println!("║ End Time: {:>66} ║", "ongoing");
            }
            println!(
                "╠══════════════════════════════════════════════════════════════════════════════╣"
            );
            println!("║ Keyboard Events: {:>60} ║", metrics.keyboard_events);
            println!("║   Presses: {:>66} ║", metrics.keyboard_presses);
            println!("║   Releases: {:>64} ║", metrics.keyboard_releases);
            println!("║ Mouse Events: {:>62} ║", metrics.mouse_events);
            println!("║   Clicks: {:>66} ║", metrics.mouse_clicks);
            println!("║   Releases: {:>64} ║", metrics.mouse_releases);
            println!("║   Moves: {:>68} ║", metrics.mouse_moves);
            println!("║ Events/sec: {:>64.2} ║", metrics.events_per_second);

            if !metrics.key_frequency.is_empty() {
                println!("╠══════════════════════════════════════════════════════════════════════════════╣");
                println!("║ Top Keys:                                                                   ║");
                let mut sorted_keys: Vec<_> = metrics.key_frequency.iter().collect();
                sorted_keys.sort_by(|a, b| b.1.cmp(a.1));
                for (key, count) in sorted_keys.iter().take(10) {
                    println!("║   {:<30} {:>44} ║", key, count);
                }
            }

            if !metrics.mouse_button_frequency.is_empty() {
                println!("╠══════════════════════════════════════════════════════════════════════════════╣");
                println!("║ Mouse Button Clicks:                                                       ║");
                let mut sorted_buttons: Vec<_> = metrics.mouse_button_frequency.iter().collect();
                sorted_buttons.sort_by(|a, b| b.1.cmp(a.1));
                for (button, count) in sorted_buttons.iter() {
                    println!("║   {:<30} {:>44} ║", button, count);
                }
            }
        }
        Err(e) => {
            println!("║ Error: {:<68} ║", format!("{}", e));
        }
    }

    println!("╚══════════════════════════════════════════════════════════════════════════════╝\n");

    if detailed {
        // Get all events for this session
        match db.get_session_events(session_id).await {
            Ok(events) => {
                println!("📋 Event Timeline ({} events):\n", events.len());
                let limit = if all_events {
                    events.len()
                } else {
                    100.min(events.len())
                };
                for event in events.iter().take(limit) {
                    print_event(event);
                }
                if !all_events && events.len() > 100 {
                    println!(
                        "\n... and {} more events (use --all to show all events)",
                        events.len() - 100
                    );
                }
            }
            Err(e) => {
                println!("⚠️  Failed to get events: {}", e);
            }
        }
    }

    Ok(())
}

fn print_event(event: &crate::services::storage::TimelineEvent) {
    let time_str = if let Some(tc) = event.timecode {
        format!("{:.2}s", tc)
    } else {
        event
            .timestamp
            .with_timezone(&Local)
            .format("%H:%M:%S%.3f")
            .to_string()
    };
    match event.event_type.as_str() {
        "keyboard" => {
            if let Some(key) = &event.key {
                let action = if event.pressed.unwrap_or(false) {
                    "PRESS"
                } else {
                    "RELEASE"
                };
                println!("  [{}] ⌨️  {}: {}", time_str, action, key);
            }
        }
        "mouse" => {
            if let Some(subtype) = &event.event_subtype {
                match subtype.as_str() {
                    "click" => {
                        let button = event.button.as_deref().unwrap_or("unknown");
                        let coords = if let (Some(x), Some(y)) = (event.x, event.y) {
                            format!("({}, {})", x, y)
                        } else {
                            String::new()
                        };
                        println!("  [{}] 🖱️  CLICK: {} {}", time_str, button, coords);
                    }
                    "move" => {
                        let coords = if let (Some(x), Some(y)) = (event.x, event.y) {
                            format!("({}, {})", x, y)
                        } else {
                            String::new()
                        };
                        println!("  [{}] 🖱️  MOVE: {}", time_str, coords);
                    }
                    _ => {
                        println!("  [{}] 🖱️  {}: {:?}", time_str, subtype, event.button);
                    }
                }
            }
        }
        "transcription" => {
            // Get transcription text
            let text = event
                .metadata
                .get("text")
                .and_then(|v| v.as_str())
                .or_else(|| event.key.as_deref());

            if let Some(text) = text {
                // Determine source from metadata
                let source = if let Some(metadata) = event.metadata.as_object() {
                    if let Some(source_str) = metadata.get("source").and_then(|v| v.as_str()) {
                        match source_str {
                            "monitor_output" => "📺 Desktop Audio",
                            "microphone" => "🎤 Microphone",
                            _ => "🎤 Transcription",
                        }
                    } else if let Some(monitor_desktop) = metadata.get("monitor_desktop_audio") {
                        if monitor_desktop.as_bool().unwrap_or(false) {
                            "📺 Desktop Audio"
                        } else {
                            "🎤 Microphone"
                        }
                    } else {
                        "🎤 Transcription"
                    }
                } else {
                    "🎤 Transcription"
                };

                // Skip [BLANK_AUDIO] transcriptions to reduce noise
                if text != "[BLANK_AUDIO]" {
                    println!("  [{}] {}: {}", time_str, source, text);
                }
            }
        }
        "analysis" => {
            if let Some(summary) = event.metadata.get("summary").and_then(|v| v.as_str()) {
                println!("  [{}] 🧠 Analysis: {}", time_str, summary);
            }
        }
        "overlay" => {
            if let Some(text) = event.metadata.get("text").and_then(|v| v.as_str()) {
                println!("  [{}] 🏷️  Overlay: {}", time_str, text);
            }
        }
        "audio" => {
            if let Some(subtype) = &event.event_subtype {
                // Check metadata to determine if it's microphone or desktop audio
                let audio_source = if let Some(metadata) = event.metadata.as_object() {
                    if let Some(monitor_desktop) = metadata.get("monitor_desktop_audio") {
                        if monitor_desktop.as_bool().unwrap_or(false) {
                            "📺 Desktop Audio"
                        } else {
                            "🎤 Microphone"
                        }
                    } else {
                        "🎙️  Audio"
                    }
                } else {
                    "🎙️  Audio"
                };

                // Show output path if available
                let path_info = if let Some(metadata) = event.metadata.as_object() {
                    if let Some(path) = metadata.get("output_path").and_then(|v| v.as_str()) {
                        format!(" → {}", path)
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                println!(
                    "  [{}] {} {}: {}{}",
                    time_str,
                    audio_source,
                    subtype,
                    if subtype == "recording_start" {
                        "started"
                    } else {
                        "stopped"
                    },
                    path_info
                );
            }
        }
        _ => {
            println!(
                "  [{}] {}: {:?}",
                time_str, event.event_type, event.event_subtype
            );
        }
    }
}

fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
}

fn format_duration(duration: chrono::Duration) -> String {
    let secs = duration.num_seconds();
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}
