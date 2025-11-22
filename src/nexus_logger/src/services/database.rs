use crate::error::{LoggerError, Result};
use chrono::{DateTime, Utc};
use nexus_core::services::{ClickHouseConfig, ClickHouseService};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// ClickHouse-based database for storing events, sessions, and screenshots
#[derive(Clone)]
pub struct Database {
    service: Arc<ClickHouseService>,
    initialized: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone)]
pub struct Metrics {
    pub session_id: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub keyboard_events: u64,
    pub keyboard_presses: u64,
    pub keyboard_releases: u64,
    pub mouse_events: u64,
    pub mouse_clicks: u64,
    pub mouse_releases: u64,
    pub mouse_moves: u64,
    pub key_frequency: HashMap<String, u64>,
    pub mouse_button_frequency: HashMap<String, u64>,
    pub events_per_second: f64,
}

impl Database {
    /// Create a new database instance with ClickHouse service from environment
    pub async fn new() -> Result<Self> {
        let config = ClickHouseConfig::from_env();
        let connection_url = config.connection_url();
        eprintln!("Connecting to ClickHouse at: {}", connection_url);

        let service = ClickHouseService::new(config).await.map_err(|e| {
            LoggerError::Configuration(format!("Failed to create ClickHouse service: {}", e))
        })?;

        let db = Self {
            service: Arc::new(service),
            initialized: Arc::new(Mutex::new(false)),
        };

        db.init_schema().await?;
        Ok(db)
    }

    /// Create a new database instance with custom ClickHouse configuration
    pub async fn with_config(config: ClickHouseConfig) -> Result<Self> {
        let connection_url = config.connection_url();
        eprintln!("Connecting to ClickHouse at: {}", connection_url);

        let service = ClickHouseService::new(config).await.map_err(|e| {
            LoggerError::Configuration(format!("Failed to create ClickHouse service: {}", e))
        })?;

        let db = Self {
            service: Arc::new(service),
            initialized: Arc::new(Mutex::new(false)),
        };

        db.init_schema().await?;
        Ok(db)
    }

    /// Initialize the database schema
    async fn init_schema(&self) -> Result<()> {
        let mut initialized = self.initialized.lock().await;
        if *initialized {
            return Ok(());
        }

        // Create events table with flexible schema for dynamic event types
        // Using JSON for additional metadata to support future event types
        self.service
            .execute(
                "CREATE TABLE IF NOT EXISTS events (
                id UUID DEFAULT generateUUIDv4(),
                session_id String NOT NULL,
                event_type LowCardinality(String) NOT NULL,
                event_subtype LowCardinality(String),
                key Nullable(String),
                button Nullable(String),
                x Nullable(Int32),
                y Nullable(Int32),
                pressed Nullable(UInt8),
                timestamp DateTime64(3, 'UTC') NOT NULL,
                timecode Nullable(Float64),
                metadata String DEFAULT '{}',
                screenshot_id Nullable(UUID),
                created_at DateTime DEFAULT now()
            ) ENGINE = MergeTree()
            PARTITION BY toYYYYMM(timestamp)
            ORDER BY (session_id, timestamp)
            SETTINGS index_granularity = 8192",
            )
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to create events table: {}", e)))?;

        // Create sessions table
        self.service
            .execute(
                "CREATE TABLE IF NOT EXISTS sessions (
                id String PRIMARY KEY,
                start_time DateTime64(3, 'UTC') NOT NULL,
                end_time Nullable(DateTime64(3, 'UTC')),
                keyboard_events UInt64 DEFAULT 0,
                keyboard_presses UInt64 DEFAULT 0,
                keyboard_releases UInt64 DEFAULT 0,
                mouse_events UInt64 DEFAULT 0,
                mouse_clicks UInt64 DEFAULT 0,
                mouse_releases UInt64 DEFAULT 0,
                mouse_moves UInt64 DEFAULT 0,
                created_at DateTime DEFAULT now()
            ) ENGINE = ReplacingMergeTree(created_at)
            ORDER BY id",
            )
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to create sessions table: {}", e)))?;

        // Create screenshots/frames table for storing image references
        // This allows linking clicks to screenshots for analysis over multiple frames
        self.service
            .execute(
                "CREATE TABLE IF NOT EXISTS screenshots (
                id UUID DEFAULT generateUUIDv4(),
                session_id String NOT NULL,
                timestamp DateTime64(3, 'UTC') NOT NULL,
                frame_number UInt64,
                file_path String,
                width UInt32,
                height UInt32,
                click_x Nullable(Int32),
                click_y Nullable(Int32),
                metadata String DEFAULT '{}',
                created_at DateTime DEFAULT now()
            ) ENGINE = MergeTree()
            PARTITION BY toYYYYMM(timestamp)
            ORDER BY (session_id, timestamp, frame_number)
            SETTINGS index_granularity = 8192",
            )
            .await
            .map_err(|e| {
                LoggerError::Other(format!("Failed to create screenshots table: {}", e))
            })?;

        // Create key frequency materialized view for analytics
        self.service
            .execute(
                "CREATE TABLE IF NOT EXISTS key_frequency (
                session_id String NOT NULL,
                key String NOT NULL,
                count UInt64 DEFAULT 0
            ) ENGINE = SummingMergeTree()
            ORDER BY (session_id, key)",
            )
            .await
            .map_err(|e| {
                LoggerError::Other(format!("Failed to create key_frequency table: {}", e))
            })?;

        // Create mouse button frequency materialized view
        self.service
            .execute(
                "CREATE TABLE IF NOT EXISTS mouse_button_frequency (
                session_id String NOT NULL,
                button String NOT NULL,
                count UInt64 DEFAULT 0
            ) ENGINE = SummingMergeTree()
            ORDER BY (session_id, button)",
            )
            .await
            .map_err(|e| {
                LoggerError::Other(format!(
                    "Failed to create mouse_button_frequency table: {}",
                    e
                ))
            })?;

        *initialized = true;
        Ok(())
    }

    /// Create a new session
    pub async fn create_session(&self, session_id: &str) -> Result<()> {
        let start_time = Utc::now();
        let start_time_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        self.service
            .insert(&format!(
                "INSERT INTO sessions (id, start_time) VALUES ('{}', '{}')",
                session_id, start_time_str
            ))
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to create session: {}", e)))?;

        Ok(())
    }

    /// Insert an event with flexible metadata support
    pub async fn insert_event(
        &self,
        session_id: &str,
        event_type: &str,
        event_subtype: Option<&str>,
        key: Option<&str>,
        button: Option<&str>,
        x: Option<i32>,
        y: Option<i32>,
        pressed: Option<bool>,
        timestamp: &str,
        timecode: Option<f64>,
        metadata: Option<Value>,
        screenshot_id: Option<&str>,
    ) -> Result<()> {
        // Helper to properly escape strings for ClickHouse SQL
        // Must escape backslashes first, then single quotes
        fn escape_sql_string(s: &str) -> String {
            s.replace('\\', "\\\\").replace('\'', "\\'")
        }

        let metadata_str = metadata
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());

        let event_subtype_str = event_subtype
            .map(|s| format!("'{}'", escape_sql_string(s)))
            .unwrap_or_else(|| "NULL".to_string());
        let key_str = key
            .map(|s| format!("'{}'", escape_sql_string(s)))
            .unwrap_or_else(|| "NULL".to_string());
        let button_str = button
            .map(|s| format!("'{}'", escape_sql_string(s)))
            .unwrap_or_else(|| "NULL".to_string());
        let x_str = x
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".to_string());
        let y_str = y
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".to_string());
        let pressed_str = pressed.map(|v| if v { "1" } else { "0" }).unwrap_or("NULL");
        let timecode_str = timecode
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".to_string());
        let screenshot_id_str = screenshot_id
            .map(|s| format!("'{}'", escape_sql_string(s)))
            .unwrap_or_else(|| "NULL".to_string());

        // Parse timestamp - support both RFC3339 and other formats
        let timestamp_dt = if let Ok(dt) = DateTime::parse_from_rfc3339(timestamp) {
            dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
        } else if let Ok(dt) =
            chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S%.f")
        {
            format!("{}", dt.format("%Y-%m-%d %H:%M:%S%.3f"))
        } else {
            // Try to use as-is
            timestamp.to_string()
        };

        let query = format!(
            "INSERT INTO events (
                session_id, event_type, event_subtype, key, button, x, y, pressed,
                timestamp, timecode, metadata, screenshot_id
            ) VALUES (
                '{}', '{}', {}, {}, {}, {}, {}, {}, '{}', {}, '{}', {}
            )",
            escape_sql_string(session_id),
            escape_sql_string(event_type),
            event_subtype_str,
            key_str,
            button_str,
            x_str,
            y_str,
            pressed_str,
            timestamp_dt,
            timecode_str,
            escape_sql_string(&metadata_str),
            screenshot_id_str
        );

        self.service
            .insert(&query)
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to insert event: {}", e)))?;

        Ok(())
    }

    /// Store a screenshot/frame reference for click analysis
    pub async fn insert_screenshot(
        &self,
        session_id: &str,
        timestamp: DateTime<Utc>,
        frame_number: u64,
        file_path: &str,
        width: u32,
        height: u32,
        click_x: Option<i32>,
        click_y: Option<i32>,
        metadata: Option<Value>,
    ) -> Result<String> {
        let screenshot_id = uuid::Uuid::new_v4().to_string();
        let timestamp_str = timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let metadata_str = metadata
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());

        let click_x_str = click_x
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".to_string());
        let click_y_str = click_y
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".to_string());

        let query = format!(
            "INSERT INTO screenshots (
                id, session_id, timestamp, frame_number, file_path, width, height,
                click_x, click_y, metadata
            ) VALUES (
                '{}', '{}', '{}', {}, '{}', {}, {}, {}, {}, '{}'
            )",
            screenshot_id,
            session_id.replace('\'', "''"),
            timestamp_str,
            frame_number,
            file_path.replace('\'', "''"),
            width,
            height,
            click_x_str,
            click_y_str,
            metadata_str.replace('\'', "''")
        );

        self.service
            .insert(&query)
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to insert screenshot: {}", e)))?;

        Ok(screenshot_id)
    }

    /// Update session metrics
    pub async fn update_session_metrics(
        &self,
        session_id: &str,
        keyboard_events: u64,
        keyboard_presses: u64,
        keyboard_releases: u64,
        mouse_events: u64,
        mouse_clicks: u64,
        mouse_releases: u64,
        mouse_moves: u64,
    ) -> Result<()> {
        // ClickHouse uses INSERT with ReplacingMergeTree, so we insert a new row
        // The engine will merge duplicates based on the ORDER BY key
        let query = format!(
            "INSERT INTO sessions (
                id, start_time, keyboard_events, keyboard_presses, keyboard_releases,
                mouse_events, mouse_clicks, mouse_releases, mouse_moves
            ) VALUES (
                '{}', now(), {}, {}, {}, {}, {}, {}, {}
            )",
            session_id.replace('\'', "''"),
            keyboard_events,
            keyboard_presses,
            keyboard_releases,
            mouse_events,
            mouse_clicks,
            mouse_releases,
            mouse_moves
        );

        self.service
            .insert(&query)
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to update session metrics: {}", e)))?;

        Ok(())
    }

    /// Update key frequency
    pub async fn update_key_frequency(&self, session_id: &str, key: &str) -> Result<()> {
        let query = format!(
            "INSERT INTO key_frequency (session_id, key, count) VALUES ('{}', '{}', 1)",
            session_id.replace('\'', "''"),
            key.replace('\'', "''")
        );

        self.service
            .insert(&query)
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to update key frequency: {}", e)))?;

        Ok(())
    }

    /// Update mouse button frequency
    pub async fn update_mouse_button_frequency(
        &self,
        session_id: &str,
        button: &str,
    ) -> Result<()> {
        let query = format!(
            "INSERT INTO mouse_button_frequency (session_id, button, count) VALUES ('{}', '{}', 1)",
            session_id.replace('\'', "''"),
            button.replace('\'', "''")
        );

        self.service.insert(&query).await.map_err(|e| {
            LoggerError::Other(format!("Failed to update mouse button frequency: {}", e))
        })?;

        Ok(())
    }

    /// End a session
    pub async fn end_session(&self, session_id: &str) -> Result<()> {
        let end_time = Utc::now();
        let end_time_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        // For ReplacingMergeTree, we need to insert a new row with updated end_time
        // First get the existing start_time
        let block = self
            .service
            .query(&format!(
                "SELECT start_time FROM sessions WHERE id = '{}' ORDER BY created_at DESC LIMIT 1",
                session_id.replace('\'', "''")
            ))
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to query session: {}", e)))?;

        if let Some(row) = block.rows().next() {
            // Try to get start_time, with fallback if missing
            let start_time = match row.get::<String, _>("start_time") {
                Ok(st) if !st.is_empty() => st,
                Ok(_) | Err(_) => {
                    // If start_time is missing or empty, use current time as fallback
                    // This can happen if the session was created but not properly initialized
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
                }
            };

            let update_query = format!(
                "INSERT INTO sessions (id, start_time, end_time) VALUES ('{}', '{}', '{}')",
                session_id.replace('\'', "''"),
                start_time,
                end_time_str
            );

            self.service
                .insert(&update_query)
                .await
                .map_err(|e| LoggerError::Other(format!("Failed to end session: {}", e)))?;
        } else {
            // Session not found - this can happen if the session was never properly created
            // Log a warning but don't fail, as this is a cleanup operation
            eprintln!("Warning: Session {} not found when ending session", session_id);
        }

        Ok(())
    }

    /// Get session metrics
    pub async fn get_session_metrics(&self, session_id: &str) -> Result<Metrics> {
        // Get session data
        let block = self
            .service
            .query(&format!(
                "SELECT start_time, end_time, keyboard_events, keyboard_presses, keyboard_releases,
                    mouse_events, mouse_clicks, mouse_releases, mouse_moves
             FROM sessions
             WHERE id = '{}'
             ORDER BY created_at DESC
             LIMIT 1",
                session_id.replace('\'', "''")
            ))
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to query session: {}", e)))?;

        if block.row_count() == 0 {
            return Err(LoggerError::Other(format!(
                "Session {} not found",
                session_id
            )));
        }

        // Parse session data using rows iterator
        let mut rows = block.rows();
        let row = rows
            .next()
            .ok_or_else(|| LoggerError::Other("No session data found".to_string()))?;

        let start_time_str: String = row
            .get("start_time")
            .map_err(|_| LoggerError::Other("Failed to get start_time".to_string()))?;
        let end_time_opt: Option<String> = row.get("end_time").ok();
        let keyboard_events: u64 = row
            .get("keyboard_events")
            .map_err(|_| LoggerError::Other("Failed to get keyboard_events".to_string()))?;
        let keyboard_presses: u64 = row
            .get("keyboard_presses")
            .map_err(|_| LoggerError::Other("Failed to get keyboard_presses".to_string()))?;
        let keyboard_releases: u64 = row
            .get("keyboard_releases")
            .map_err(|_| LoggerError::Other("Failed to get keyboard_releases".to_string()))?;
        let mouse_events: u64 = row
            .get("mouse_events")
            .map_err(|_| LoggerError::Other("Failed to get mouse_events".to_string()))?;
        let mouse_clicks: u64 = row
            .get("mouse_clicks")
            .map_err(|_| LoggerError::Other("Failed to get mouse_clicks".to_string()))?;
        let mouse_releases: u64 = row
            .get("mouse_releases")
            .map_err(|_| LoggerError::Other("Failed to get mouse_releases".to_string()))?;
        let mouse_moves: u64 = row
            .get("mouse_moves")
            .map_err(|_| LoggerError::Other("Failed to get mouse_moves".to_string()))?;

        let start_time = match DateTime::parse_from_rfc3339(&start_time_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => {
                chrono::NaiveDateTime::parse_from_str(&start_time_str, "%Y-%m-%d %H:%M:%S%.f")
                    .map_err(|e| LoggerError::Other(format!("Failed to parse start_time: {}", e)))?
                    .and_utc()
            }
        };

        let end_time = end_time_opt.and_then(|s| match DateTime::parse_from_rfc3339(&s) {
            Ok(dt) => Some(dt.with_timezone(&Utc)),
            Err(_) => chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
                .ok()
                .map(|ndt| ndt.and_utc()),
        });

        // Get key frequency
        let key_block = self
            .service
            .query(&format!(
                "SELECT key, sum(count) as total
             FROM key_frequency
             WHERE session_id = '{}'
             GROUP BY key",
                session_id.replace('\'', "''")
            ))
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to query key frequency: {}", e)))?;

        let mut key_frequency = HashMap::new();
        for row in key_block.rows() {
            let key: String = row
                .get("key")
                .map_err(|_| LoggerError::Other("Failed to get key".to_string()))?;
            let count: u64 = row
                .get("total")
                .map_err(|_| LoggerError::Other("Failed to get count".to_string()))?;
            key_frequency.insert(key, count);
        }

        // Get mouse button frequency
        let button_block = self
            .service
            .query(&format!(
                "SELECT button, sum(count) as total
             FROM mouse_button_frequency
             WHERE session_id = '{}'
             GROUP BY button",
                session_id.replace('\'', "''")
            ))
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to query button frequency: {}", e)))?;

        let mut mouse_button_frequency = HashMap::new();
        for row in button_block.rows() {
            let button: String = row
                .get("button")
                .map_err(|_| LoggerError::Other("Failed to get button".to_string()))?;
            let count: u64 = row
                .get("total")
                .map_err(|_| LoggerError::Other("Failed to get count".to_string()))?;
            mouse_button_frequency.insert(button, count);
        }

        // Calculate events per second
        let duration = end_time
            .unwrap_or_else(Utc::now)
            .signed_duration_since(start_time);
        let total_events = keyboard_events + mouse_events;
        let events_per_second = if duration.num_seconds() > 0 {
            total_events as f64 / duration.num_seconds() as f64
        } else {
            0.0
        };

        Ok(Metrics {
            session_id: session_id.to_string(),
            start_time,
            end_time,
            keyboard_events,
            keyboard_presses,
            keyboard_releases,
            mouse_events,
            mouse_clicks,
            mouse_releases,
            mouse_moves,
            key_frequency,
            mouse_button_frequency,
            events_per_second,
        })
    }

    /// Get screenshots for a click location (for analyzing what was clicked over multiple frames)
    pub async fn get_screenshots_for_click(
        &self,
        session_id: &str,
        click_x: i32,
        click_y: i32,
        time_window_seconds: f64,
    ) -> Result<Vec<(String, String, u64)>> {
        // Find screenshots within time window of clicks at this location
        let block = self
            .service
            .query(&format!(
                "SELECT s.id, s.file_path, s.frame_number
             FROM screenshots s
             INNER JOIN events e ON s.session_id = e.session_id
             WHERE s.session_id = '{}'
               AND e.x = {}
               AND e.y = {}
               AND e.event_type = 'mouse'
               AND e.event_subtype = 'click'
               AND abs(toUnixTimestamp(s.timestamp) - toUnixTimestamp(e.timestamp)) <= {}
             ORDER BY s.timestamp",
                session_id.replace('\'', "''"),
                click_x,
                click_y,
                time_window_seconds
            ))
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to query screenshots: {}", e)))?;

        let mut results = Vec::new();
        for row in block.rows() {
            let id: String = row
                .get("id")
                .map_err(|_| LoggerError::Other("Failed to get screenshot id".to_string()))?;
            let file_path: String = row
                .get("file_path")
                .map_err(|_| LoggerError::Other("Failed to get file_path".to_string()))?;
            let frame_number: u64 = row
                .get("frame_number")
                .map_err(|_| LoggerError::Other("Failed to get frame_number".to_string()))?;
            results.push((id, file_path, frame_number));
        }

        Ok(results)
    }

    /// Get all events for a session ordered by timestamp/timecode
    /// Uses HTTP interface to avoid LowCardinality type issues with native protocol
    pub async fn get_session_events(&self, session_id: &str) -> Result<Vec<TimelineEvent>> {
        use nexus_core::services::ClickHouseConfig;
        
        // Get config to build HTTP URL
        let config = ClickHouseConfig::from_env();
        let http_port = if config.port == 9000 { 8123 } else { config.port };
        let url = format!("http://{}:{}", config.host, http_port);
        
        // Build query - HTTP interface handles LowCardinality automatically
        let query = format!(
            "SELECT 
                toString(event_type) as event_type,
                toString(event_subtype) as event_subtype,
                key,
                button,
                x,
                y,
                pressed,
                timestamp,
                timecode,
                metadata
            FROM events
            WHERE session_id = '{}'
            ORDER BY COALESCE(timecode, toUnixTimestamp(timestamp)) ASC
            FORMAT JSONEachRow",
            session_id.replace('\'', "''")
        );

        // Use HTTP client to query
        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .query(&[("database", &config.database)])
            .basic_auth(&config.username, Some(&config.password))
            .body(query)
            .send()
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to send HTTP request: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LoggerError::Other(format!(
                "ClickHouse HTTP query failed: {}",
                error_text
            )));
        }

        let text = response.text().await.map_err(|e| {
            LoggerError::Other(format!("Failed to read HTTP response: {}", e))
        })?;

        let mut events = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let row: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                LoggerError::Other(format!("Failed to parse JSON row: {}", e))
            })?;

            let event_type = row
                .get("event_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LoggerError::Other("Missing event_type".to_string()))?
                .to_string();
            let event_subtype = row.get("event_subtype").and_then(|v| v.as_str()).map(String::from);
            let key = row.get("key").and_then(|v| v.as_str()).map(String::from);
            let button = row.get("button").and_then(|v| v.as_str()).map(String::from);
            let x = row.get("x").and_then(|v| v.as_i64()).map(|v| v as i32);
            let y = row.get("y").and_then(|v| v.as_i64()).map(|v| v as i32);
            let pressed = row.get("pressed").and_then(|v| v.as_u64()).map(|v| v != 0);
            let timestamp_str = row
                .get("timestamp")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LoggerError::Other("Missing timestamp".to_string()))?;
            let timecode = row.get("timecode").and_then(|v| v.as_f64());
            let metadata_str = row
                .get("metadata")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");

            let timestamp = match DateTime::parse_from_rfc3339(timestamp_str) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => {
                    chrono::NaiveDateTime::parse_from_str(timestamp_str, "%Y-%m-%d %H:%M:%S%.f")
                        .map_err(|e| {
                            LoggerError::Other(format!("Failed to parse timestamp: {}", e))
                        })?
                        .and_utc()
                }
            };

            let metadata: Value = serde_json::from_str(metadata_str)
                .unwrap_or_else(|_| Value::Null);

            events.push(TimelineEvent {
                event_type,
                event_subtype,
                key,
                button,
                x,
                y,
                pressed,
                timestamp,
                timecode,
                metadata,
            });
        }

        Ok(events)
    }

    /// Get session start time from database
    pub async fn get_session_start_time(&self, session_id: &str) -> Result<Option<DateTime<Utc>>> {
        use nexus_core::services::ClickHouseConfig;
        
        // Get config to build HTTP URL
        let config = ClickHouseConfig::from_env();
        let http_port = if config.port == 9000 { 8123 } else { config.port };
        let url = format!("http://{}:{}", config.host, http_port);
        
        let query = format!(
            "SELECT start_time FROM sessions WHERE id = '{}' ORDER BY created_at DESC LIMIT 1 FORMAT JSONEachRow",
            session_id.replace('\'', "''")
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .query(&[("database", &config.database)])
            .basic_auth(&config.username, Some(&config.password))
            .body(query)
            .send()
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to send HTTP request: {}", e)))?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let text = response.text().await.map_err(|e| {
            LoggerError::Other(format!("Failed to read HTTP response: {}", e))
        })?;

        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let row: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                LoggerError::Other(format!("Failed to parse JSON row: {}", e))
            })?;

            if let Some(start_time_str) = row.get("start_time").and_then(|v| v.as_str()) {
                let start_time = match DateTime::parse_from_rfc3339(start_time_str) {
                    Ok(dt) => dt.with_timezone(&Utc),
                    Err(_) => {
                        chrono::NaiveDateTime::parse_from_str(start_time_str, "%Y-%m-%d %H:%M:%S%.f")
                            .map_err(|e| {
                                LoggerError::Other(format!("Failed to parse start_time: {}", e))
                            })?
                            .and_utc()
                    }
                };
                return Ok(Some(start_time));
            }
        }

        Ok(None)
    }

    /// Get events in a time window around a video timestamp
    /// Returns events within [timestamp - window_before, timestamp + window_after]
    pub async fn get_events_in_window(
        &self,
        session_id: &str,
        video_timestamp: f64,
        window_before: f64,
        window_after: f64,
    ) -> Result<Vec<TimelineEvent>> {
        use nexus_core::services::ClickHouseConfig;
        
        let config = ClickHouseConfig::from_env();
        let http_port = if config.port == 9000 { 8123 } else { config.port };
        let url = format!("http://{}:{}", config.host, http_port);
        
        // Query events where timecode is within the window, or calculate from timestamp
        let min_timecode = video_timestamp - window_before;
        let max_timecode = video_timestamp + window_after;
        
        let query = format!(
            "SELECT 
                toString(event_type) as event_type,
                toString(event_subtype) as event_subtype,
                key,
                button,
                x,
                y,
                pressed,
                timestamp,
                timecode,
                metadata
            FROM events
            WHERE session_id = '{}'
              AND (
                (timecode IS NOT NULL AND timecode >= {} AND timecode <= {})
                OR (timecode IS NULL AND toUnixTimestamp(timestamp) >= {} AND toUnixTimestamp(timestamp) <= {})
              )
            ORDER BY COALESCE(timecode, toUnixTimestamp(timestamp)) ASC
            FORMAT JSONEachRow",
            session_id.replace('\'', "''"),
            min_timecode,
            max_timecode,
            min_timecode,
            max_timecode
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .query(&[("database", &config.database)])
            .basic_auth(&config.username, Some(&config.password))
            .body(query)
            .send()
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to send HTTP request: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LoggerError::Other(format!(
                "ClickHouse HTTP query failed: {}",
                error_text
            )));
        }

        let text = response.text().await.map_err(|e| {
            LoggerError::Other(format!("Failed to read HTTP response: {}", e))
        })?;

        let mut events = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let row: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                LoggerError::Other(format!("Failed to parse JSON row: {}", e))
            })?;

            let event_type = row
                .get("event_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LoggerError::Other("Missing event_type".to_string()))?
                .to_string();
            let event_subtype = row.get("event_subtype").and_then(|v| v.as_str()).map(String::from);
            let key = row.get("key").and_then(|v| v.as_str()).map(String::from);
            let button = row.get("button").and_then(|v| v.as_str()).map(String::from);
            let x = row.get("x").and_then(|v| v.as_i64()).map(|v| v as i32);
            let y = row.get("y").and_then(|v| v.as_i64()).map(|v| v as i32);
            let pressed = row.get("pressed").and_then(|v| v.as_u64()).map(|v| v != 0);
            let timestamp_str = row
                .get("timestamp")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LoggerError::Other("Missing timestamp".to_string()))?;
            let timecode = row.get("timecode").and_then(|v| v.as_f64());
            let metadata_str = row
                .get("metadata")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");

            let timestamp = match DateTime::parse_from_rfc3339(timestamp_str) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => {
                    chrono::NaiveDateTime::parse_from_str(timestamp_str, "%Y-%m-%d %H:%M:%S%.f")
                        .map_err(|e| {
                            LoggerError::Other(format!("Failed to parse timestamp: {}", e))
                        })?
                        .and_utc()
                }
            };

            let metadata: Value = serde_json::from_str(metadata_str)
                .unwrap_or_else(|_| Value::Null);

            events.push(TimelineEvent {
                event_type,
                event_subtype,
                key,
                button,
                x,
                y,
                pressed,
                timestamp,
                timecode,
                metadata,
            });
        }

        Ok(events)
    }
}

/// Event for timeline display
#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub event_type: String,
    pub event_subtype: Option<String>,
    pub key: Option<String>,
    pub button: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub pressed: Option<bool>,
    pub timestamp: DateTime<Utc>,
    pub timecode: Option<f64>,
    pub metadata: Value,
}
