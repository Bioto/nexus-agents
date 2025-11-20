use crate::error::{LoggerError, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Database {
    conn: Arc<Mutex<Connection>>,
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
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| {
            LoggerError::Other(format!("Failed to open database: {}", e))
        })?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            LoggerError::Other(format!("Failed to lock database: {}", e))
        })?;

        // Events table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                event_subtype TEXT,
                key TEXT,
                button TEXT,
                x INTEGER,
                y INTEGER,
                pressed BOOLEAN,
                timestamp TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to create events table: {}", e)))?;

        // Sessions table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                start_time TEXT NOT NULL,
                end_time TEXT,
                keyboard_events INTEGER DEFAULT 0,
                keyboard_presses INTEGER DEFAULT 0,
                keyboard_releases INTEGER DEFAULT 0,
                mouse_events INTEGER DEFAULT 0,
                mouse_clicks INTEGER DEFAULT 0,
                mouse_releases INTEGER DEFAULT 0,
                mouse_moves INTEGER DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to create sessions table: {}", e)))?;

        // Key frequency table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS key_frequency (
                session_id TEXT NOT NULL,
                key TEXT NOT NULL,
                count INTEGER DEFAULT 0,
                PRIMARY KEY (session_id, key),
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            )",
            [],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to create key_frequency table: {}", e)))?;

        // Mouse button frequency table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mouse_button_frequency (
                session_id TEXT NOT NULL,
                button TEXT NOT NULL,
                count INTEGER DEFAULT 0,
                PRIMARY KEY (session_id, button),
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            )",
            [],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to create mouse_button_frequency table: {}", e)))?;

        // Create indexes
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id)",
            [],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to create index: {}", e)))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp)",
            [],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to create index: {}", e)))?;

        Ok(())
    }

    pub fn create_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            LoggerError::Other(format!("Failed to lock database: {}", e))
        })?;

        let start_time = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, start_time) VALUES (?1, ?2)",
            params![session_id, start_time],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to create session: {}", e)))?;

        Ok(())
    }

    pub fn insert_event(
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
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            LoggerError::Other(format!("Failed to lock database: {}", e))
        })?;

        conn.execute(
            "INSERT INTO events (
                session_id, event_type, event_subtype, key, button, x, y, pressed, timestamp
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session_id,
                event_type,
                event_subtype,
                key,
                button,
                x,
                y,
                pressed,
                timestamp
            ],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to insert event: {}", e)))?;

        Ok(())
    }

    pub fn update_session_metrics(
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
        let conn = self.conn.lock().map_err(|e| {
            LoggerError::Other(format!("Failed to lock database: {}", e))
        })?;

        conn.execute(
            "UPDATE sessions SET
                keyboard_events = ?2,
                keyboard_presses = ?3,
                keyboard_releases = ?4,
                mouse_events = ?5,
                mouse_clicks = ?6,
                mouse_releases = ?7,
                mouse_moves = ?8
            WHERE id = ?1",
            params![
                session_id,
                keyboard_events,
                keyboard_presses,
                keyboard_releases,
                mouse_events,
                mouse_clicks,
                mouse_releases,
                mouse_moves
            ],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to update session metrics: {}", e)))?;

        Ok(())
    }

    pub fn update_key_frequency(&self, session_id: &str, key: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            LoggerError::Other(format!("Failed to lock database: {}", e))
        })?;

        conn.execute(
            "INSERT INTO key_frequency (session_id, key, count)
             VALUES (?1, ?2, 1)
             ON CONFLICT(session_id, key) DO UPDATE SET count = count + 1",
            params![session_id, key],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to update key frequency: {}", e)))?;

        Ok(())
    }

    pub fn update_mouse_button_frequency(&self, session_id: &str, button: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            LoggerError::Other(format!("Failed to lock database: {}", e))
        })?;

        conn.execute(
            "INSERT INTO mouse_button_frequency (session_id, button, count)
             VALUES (?1, ?2, 1)
             ON CONFLICT(session_id, button) DO UPDATE SET count = count + 1",
            params![session_id, button],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to update mouse button frequency: {}", e)))?;

        Ok(())
    }

    pub fn end_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            LoggerError::Other(format!("Failed to lock database: {}", e))
        })?;

        let end_time = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE sessions SET end_time = ?2 WHERE id = ?1",
            params![session_id, end_time],
        )
        .map_err(|e| LoggerError::Other(format!("Failed to end session: {}", e)))?;

        Ok(())
    }

    pub fn get_session_metrics(&self, session_id: &str) -> Result<Metrics> {
        let conn = self.conn.lock().map_err(|e| {
            LoggerError::Other(format!("Failed to lock database: {}", e))
        })?;

        let mut stmt = conn
            .prepare(
                "SELECT start_time, end_time, keyboard_events, keyboard_presses, keyboard_releases,
                        mouse_events, mouse_clicks, mouse_releases, mouse_moves
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| LoggerError::Other(format!("Failed to prepare statement: {}", e)))?;

        let row = stmt
            .query_row(params![session_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, u64>(6)?,
                    row.get::<_, u64>(7)?,
                    row.get::<_, u64>(8)?,
                ))
            })
            .map_err(|e| LoggerError::Other(format!("Failed to query session: {}", e)))?;

        let start_time = DateTime::parse_from_rfc3339(&row.0)
            .map_err(|e| LoggerError::Other(format!("Failed to parse start_time: {}", e)))?
            .with_timezone(&Utc);

        let end_time = row.1.and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        // Get key frequency
        let mut key_frequency = HashMap::new();
        let mut key_stmt = conn
            .prepare("SELECT key, count FROM key_frequency WHERE session_id = ?1")
            .map_err(|e| LoggerError::Other(format!("Failed to prepare key statement: {}", e)))?;

        let key_rows = key_stmt
            .query_map(params![session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .map_err(|e| LoggerError::Other(format!("Failed to query key frequency: {}", e)))?;

        for row in key_rows {
            let (key, count) = row.map_err(|e| {
                LoggerError::Other(format!("Failed to read key frequency row: {}", e))
            })?;
            key_frequency.insert(key, count);
        }

        // Get mouse button frequency
        let mut mouse_button_frequency = HashMap::new();
        let mut button_stmt = conn
            .prepare("SELECT button, count FROM mouse_button_frequency WHERE session_id = ?1")
            .map_err(|e| LoggerError::Other(format!("Failed to prepare button statement: {}", e)))?;

        let button_rows = button_stmt
            .query_map(params![session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .map_err(|e| LoggerError::Other(format!("Failed to query button frequency: {}", e)))?;

        for row in button_rows {
            let (button, count) = row.map_err(|e| {
                LoggerError::Other(format!("Failed to read button frequency row: {}", e))
            })?;
            mouse_button_frequency.insert(button, count);
        }

        // Calculate events per second
        let duration = end_time
            .unwrap_or_else(Utc::now)
            .signed_duration_since(start_time);
        let total_events = row.2 + row.5; // keyboard_events + mouse_events
        let events_per_second = if duration.num_seconds() > 0 {
            total_events as f64 / duration.num_seconds() as f64
        } else {
            0.0
        };

        Ok(Metrics {
            session_id: session_id.to_string(),
            start_time,
            end_time,
            keyboard_events: row.2,
            keyboard_presses: row.3,
            keyboard_releases: row.4,
            mouse_events: row.5,
            mouse_clicks: row.6,
            mouse_releases: row.7,
            mouse_moves: row.8,
            key_frequency,
            mouse_button_frequency,
            events_per_second,
        })
    }
}

