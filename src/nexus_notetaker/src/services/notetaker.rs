use crate::Result;
use chrono::{DateTime, Utc};
use log::{debug, info};
use nexus_core::{ChatCompletionRequest, Message, NexusApiService};
use nexus_storage::clickhouse::{Database, Metrics, TimelineEvent};
use serde_json::Value;
use std::fmt::Write as _;

/// Loaded session data used for summarization.
#[derive(Debug, Clone)]
pub struct SessionData {
    pub session_id: String,
    pub start_time: DateTime<Utc>,
    pub metrics: Metrics,
    pub events: Vec<TimelineEvent>,
}

/// Service that pulls session data from ClickHouse and asks the LLM for notes.
pub struct NotetakerService {
    db: Database,
    api: NexusApiService,
    model: String,
}

impl NotetakerService {
    /// Create a new notetaker service using environment-backed clients.
    pub async fn new(model: Option<String>) -> Result<Self> {
        let db = Database::new().await?;
        let api = NexusApiService::from_env()?;
        let model = resolve_model(model);
        info!("Notetaker using model {}", model);
        Ok(Self { db, api, model })
    }

    /// Load session metrics and contextual events for the given session.
    pub async fn load_session(&self, session_id: &str) -> Result<SessionData> {
        let metrics = self.db.get_session_metrics(session_id).await?;
        let start_time = self
            .db
            .get_session_start_time(session_id)
            .await?
            .unwrap_or(metrics.start_time);
        let events = self.db.get_session_events(session_id).await?;

        Ok(SessionData {
            session_id: session_id.to_string(),
            start_time,
            metrics,
            events,
        })
    }

    /// Summarize the session into structured notes using the LLM.
    pub async fn summarize(&self, data: SessionData, max_events: usize) -> Result<String> {
        let prompt = build_prompt(&data, max_events);
        let system_prompt = "\
You are an expert AI notetaker. Using the provided session context, generate clear and structured notes. Focus on what was discussed, key observations, and notable events from webcam, desktop, and audio analysis.

Organize your response with the following sections:
- **Overview:** Brief summary of the session purpose and participants, if available.
- **Timeline Highlights:** Concise, timestamped bullet points of important moments or discussions.
- **Decisions:** Any decisions reached during the session.
- **Action Items:** Tasks with assignees and due dates, if specified.
- **Risks / Concerns:** Any issues or open risks.
- **Open Questions:** Outstanding questions or areas needing follow-up.
- **Sentiment & Engagement:** Observed sentiment and engagement from participants, including notable changes.

Guidelines:
- Use bullet points and keep the writing concise.
- Include timestamps for events wherever possible.
- Do **not** speculate or invent facts; only report what’s supported by the context.
- Exclude any content that cannot be directly substantiated by the session data.
";

        let request = ChatCompletionRequest::new(
            self.model.clone(),
            vec![Message::system(system_prompt), Message::user(prompt)],
        );

        debug!("Dispatching summarization request to model {}", self.model);
        let response = self.api.chat(request).await?;
        let notes = response
            .content
            .map(|c| c.extract_text())
            .unwrap_or_else(|| "No content returned from model.".to_string());
        Ok(notes)
    }
}

fn resolve_model(model: Option<String>) -> String {
    model
        .or_else(|| std::env::var("DEFAULT_MODEL").ok())
        .unwrap_or_else(|| "gpt-5-nano-2025-08-07".to_string())
}

fn build_prompt(data: &SessionData, max_events: usize) -> String {
    let mut prompt = String::new();

    writeln!(
        &mut prompt,
        "Session ID: {}\nStart: {}\nMetrics:",
        data.session_id, data.start_time
    )
    .ok();
    writeln!(
        &mut prompt,
        "- keyboard events: {} (presses {}, releases {})",
        data.metrics.keyboard_events, data.metrics.keyboard_presses, data.metrics.keyboard_releases
    )
    .ok();
    writeln!(
        &mut prompt,
        "- mouse events: {} (clicks {}, releases {}, moves {})",
        data.metrics.mouse_events,
        data.metrics.mouse_clicks,
        data.metrics.mouse_releases,
        data.metrics.mouse_moves
    )
    .ok();

    let contextual_events: Vec<String> = data
        .events
        .iter()
        .filter(|e| is_contextual_event(e))
        .take(max_events)
        .filter_map(|e| format_event(e, data.start_time))
        .collect();

    if contextual_events.is_empty() {
        writeln!(
            &mut prompt,
            "\nNo contextual analysis events were found; summarize based on metadata only."
        )
        .ok();
    } else {
        writeln!(
            &mut prompt,
            "\nContextual events (limited to {}):",
            contextual_events.len()
        )
        .ok();
        for event in contextual_events {
            writeln!(&mut prompt, "- {}", event).ok();
        }
    }

    prompt
}

fn is_contextual_event(event: &TimelineEvent) -> bool {
    if event.event_type == "analysis" {
        return true;
    }

    if let Some(sub) = event.event_subtype.as_deref() {
        let sub_l = sub.to_lowercase();
        return sub_l.contains("analysis")
            || sub_l.contains("sentiment")
            || sub_l.contains("transcript")
            || sub_l.contains("transcription")
            || sub_l.contains("context")
            || sub_l.contains("webcam")
            || sub_l.contains("desktop")
            || sub_l.contains("audio");
    }

    false
}

fn format_event(event: &TimelineEvent, baseline: DateTime<Utc>) -> Option<String> {
    let timecode = event
        .timecode
        .unwrap_or_else(|| {
            let delta = event.timestamp.signed_duration_since(baseline);
            (delta.num_milliseconds() as f64 / 1000.0).max(0.0)
        })
        .max(0.0);

    let kind = event
        .event_subtype
        .as_deref()
        .unwrap_or_else(|| event.event_type.as_str());

    let details = metadata_summary(&event.metadata)?;
    Some(format!("[{:>7.2}s] {}: {}", timecode, kind, details))
}

fn metadata_summary(metadata: &Value) -> Option<String> {
    if metadata.is_null() {
        return None;
    }

    if let Some(obj) = metadata.as_object() {
        for key in [
            "analysis_text",
            "text",
            "transcript",
            "summary",
            "content",
            "description",
            "notes",
        ] {
            if let Some(val) = obj.get(key).and_then(|v| v.as_str()) {
                let cleaned = strip_code_fences(val);
                return Some(truncate(&cleaned, 600));
            }
        }

        // Build a compact sentiment block if available
        let mut parts = Vec::new();
        for key in ["sentiment", "attention", "energy"] {
            if let Some(val) = obj.get(key).and_then(|v| v.as_str()) {
                parts.push(format!("{}: {}", key, val));
            }
        }
        if !parts.is_empty() {
            return Some(parts.join(", "));
        }
    }

    let raw = metadata.to_string();
    if raw == "{}" {
        None
    } else {
        Some(truncate(&raw, 300))
    }
}

fn strip_code_fences(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(stripped) = trimmed.strip_prefix("```json") {
        return stripped
            .trim_matches('`')
            .trim()
            .trim_matches('\n')
            .to_string();
    }
    if let Some(stripped) = trimmed.strip_prefix("```") {
        return stripped
            .trim_matches('`')
            .trim()
            .trim_matches('\n')
            .to_string();
    }
    trimmed.to_string()
}

fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}…", &text[..max_len])
    }
}
