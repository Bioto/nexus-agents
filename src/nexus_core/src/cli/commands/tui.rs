use crate::agent_service::{AgentService, AgentStreamEvent};
use crate::client::{LLMClient, ResponsesClient};
use crate::models::{Agent, ChatHistory, ContentPart, MessageContent, MessageRole, Result};
use crate::services::SwarmCoordinatorService;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame, Terminal,
};
use ratatui_explorer::{FileExplorer, Theme};
use std::io;
use std::path::PathBuf;
use textwrap;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

pub struct ChatState {
    messages: ChatHistory,
    input: String,
    scroll_offset: usize,
    status: String,
    is_loading: bool,
    streaming_content: String,
    auto_scroll: bool,
    tool_messages: Vec<String>,
    agent_name: Option<String>,
    tools: Vec<(String, String)>, // (name, description)
    sidebar_visible: bool,
    show_help: bool,
    file_explorer: Option<FileExplorer>,
    pending_file: Option<PathBuf>, // File selected but not yet processed
    pending_file_content: Option<MessageContent>, // Processed file content ready to send
}

impl Default for ChatState {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatState {
    pub fn new() -> Self {
        Self {
            messages: ChatHistory::new(),
            input: String::new(),
            scroll_offset: 0,
            status: String::from("Ready"),
            is_loading: false,
            streaming_content: String::new(),
            auto_scroll: true,
            tool_messages: Vec::new(),
            agent_name: None,
            tools: Vec::new(),
            sidebar_visible: true,
            show_help: false,
            file_explorer: None,
            pending_file: None,
            pending_file_content: None,
        }
    }

    pub fn set_agent_name(&mut self, name: impl Into<String>) {
        self.agent_name = Some(name.into());
    }

    pub fn set_tools(&mut self, tools: Vec<(String, String)>) {
        self.tools = tools;
    }

    pub fn add_system(&mut self, content: impl Into<String>) {
        self.messages.add_system(content);
    }

    pub fn add_user(&mut self, content: impl Into<String>) {
        self.messages.add_user(content);
        self.scroll_offset = usize::MAX;
    }

    pub fn add_assistant(&mut self, content: impl Into<String>) {
        self.messages.add_assistant(content);
        self.scroll_offset = usize::MAX;
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(1);
        }
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut ChatState,
    client: &ResponsesClient,
    agent: Option<&Agent>,
    stream: bool,
    model: &str,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    top_p: Option<f32>,
    frequency_penalty: Option<f32>,
    presence_penalty: Option<f32>,
) -> Result<()> {
    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<StreamUpdate>();
    let (file_tx, mut file_rx) = mpsc::unbounded_channel::<MessageContent>();

    #[derive(Debug)]
    enum StreamUpdate {
        Chunk(String),
        ToolExecuting(String),
        ToolResult(String),
        Done,
        Error(String),
    }

    loop {
        // Process all pending file content first
        while let Ok(content) = file_rx.try_recv() {
            // Check if it's an error message
            if let MessageContent::String(ref text) = content {
                if text.starts_with("Error:") {
                    state.status = text.clone();
                    state.pending_file_content = None;
                    // Clear pending_file to prevent retry
                    state.pending_file = None;
                    continue;
                }
            }
            state.pending_file_content = Some(content);
            state.status = String::from("File ready - type your message and press Enter");
        }
        
        // If we're processing a file but haven't received a response, show a more informative status
        if state.pending_file.is_some() && state.pending_file_content.is_none() {
            // Keep showing processing status - it will update when file_rx receives the result
        }

        // Process all pending stream updates first
        while let Ok(update) = stream_rx.try_recv() {
            match update {
                StreamUpdate::Chunk(content) => {
                    state.streaming_content.push_str(&content);
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::ToolExecuting(tool_name) => {
                    state.tool_messages.push(format!("🔧 {}", tool_name));
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::ToolResult(result) => {
                    if let Some(last) = state.tool_messages.last_mut() {
                        last.push_str(&format!(" → {}", result));
                    }
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::Done => {
                    state.is_loading = false;
                    state.status = String::from("Ready");

                    // Combine tool messages with content
                    let mut full_content = String::new();
                    if !state.tool_messages.is_empty() {
                        for tool_msg in &state.tool_messages {
                            full_content.push_str(tool_msg);
                            full_content.push('\n');
                        }
                        full_content.push('\n');
                    }
                    full_content.push_str(&state.streaming_content);

                    state.streaming_content.clear();
                    state.tool_messages.clear();
                    state.add_assistant(full_content);
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::Error(err) => {
                    state.is_loading = false;
                    state.status = format!("Error: {}", err);
                    state.messages.messages.pop();
                }
            }
        }

        // Draw the UI after processing updates
        terminal
            .draw(|f| ui(f, &mut *state))
            .map_err(|e| crate::models::Error::Other(format!("Failed to draw: {}", e)))?;

        // Dynamic poll: short timeout during loading for responsive streaming, longer when idle
        let poll_duration = if state.is_loading {
            std::time::Duration::from_millis(10)
        } else {
            std::time::Duration::from_millis(100)
        };

        if crossterm::event::poll(poll_duration)
            .map_err(|e| crate::models::Error::Other(format!("Failed to poll event: {}", e)))?
        {
            let evt = event::read()
                .map_err(|e| crate::models::Error::Other(format!("Failed to read event: {}", e)))?;

            // Handle file explorer events first if it's open
            if let Some(ref mut explorer) = state.file_explorer {
                // Check for Esc to close before handling the event
                if let Event::Key(key) = evt {
                    if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc {
                        state.file_explorer = None;
                        state.status = String::from("Ready");
                        continue;
                    }
                    
                    // Check for Enter key to select a file
                    if key.kind == KeyEventKind::Press && key.code == KeyCode::Enter {
                        // Get the currently selected file/directory
                        let current = explorer.current();
                        // Check if it's a file (not a directory)
                        if current.is_file() {
                            // Build the full path by combining current directory with file name
                            let cwd = explorer.cwd();
                            let selected_path = cwd.join(current.name());
                            if selected_path.exists() && selected_path.is_file() {
                                let file_name = current.name().to_string();
                                state.pending_file = Some(selected_path);
                                state.file_explorer = None;
                                state.status = format!("Processing {}...", file_name);
                                continue;
                            }
                        }
                        // If it's a directory, let the explorer handle navigation by continuing
                    }
                }

                // Pass event to file explorer (this will handle directory navigation)
                if let Err(e) = explorer.handle(&evt) {
                    state.status = format!("File explorer error: {}", e);
                    state.file_explorer = None;
                    continue;
                }

                // Continue to render file explorer
                continue;
            }

            // Process pending file if any (do this before key handling)
            if let Some(file_path) = state.pending_file.take() {
                let file_name = file_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
                    .to_string();
                let file_name_for_status = file_name.clone();
                let client_clone = client.clone();
                let file_path_clone = file_path.clone();
                let input_text = state.input.clone();
                let file_tx_clone = file_tx.clone();

                tokio::spawn(async move {
                    let is_image = is_image_file(&file_path_clone);
                    let is_pdf = is_pdf_file(&file_path_clone);
                    
                    if is_image {
                        // Base64 encode image
                        match std::fs::read(&file_path_clone) {
                            Ok(file_data) => {
                                use base64::Engine;
                                let base64_data = base64::engine::general_purpose::STANDARD
                                    .encode(&file_data);
                                let content = if input_text.trim().is_empty() {
                                    MessageContent::with_image("", base64_data)
                                } else {
                                    MessageContent::with_image(input_text, base64_data)
                                };
                                let _ = file_tx_clone.send(content);
                            }
                            Err(e) => {
                                // Send error through channel as a text message
                                let error_content = MessageContent::String(format!(
                                    "Error: Failed to read image file: {}",
                                    e
                                ));
                                let _ = file_tx_clone.send(error_content);
                            }
                        }
                    } else if is_pdf {
                        // Upload PDF file
                        match client_clone.upload_pdf(&file_path_clone).await {
                            Ok(uploaded_file) => {
                                let content = if input_text.trim().is_empty() {
                                    MessageContent::with_file("", uploaded_file.file_id)
                                } else {
                                    MessageContent::with_file(input_text, uploaded_file.file_id)
                                };
                                let _ = file_tx_clone.send(content);
                            }
                            Err(e) => {
                                // Send error through channel as a text message
                                let error_content = MessageContent::String(format!(
                                    "Error: Failed to upload file: {}",
                                    e
                                ));
                                let _ = file_tx_clone.send(error_content);
                            }
                        }
                    } else {
                        // Read file contents as text
                        match std::fs::read_to_string(&file_path_clone) {
                            Ok(file_contents) => {
                                let wrapped_contents = format!("<attached_file filename=\"{}\">{}</attached_file>", file_name, file_contents);
                                let content = if input_text.trim().is_empty() {
                                    MessageContent::String(wrapped_contents)
                                } else {
                                    MessageContent::String(format!("{}\n\n{}", input_text, wrapped_contents))
                                };
                                let _ = file_tx_clone.send(content);
                            }
                            Err(e) => {
                                // Send error through channel as a text message
                                let error_content = MessageContent::String(format!(
                                    "Error: Failed to read file: {}",
                                    e
                                ));
                                let _ = file_tx_clone.send(error_content);
                            }
                        }
                    }
                });
                // Update status to show which file is being processed
                state.status = format!("Processing {}...", &file_name_for_status);
            }

            if let Event::Key(key) = evt {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Esc => {
                        return Ok(());
                    }
                    KeyCode::Enter => {
                        // Don't allow sending if a file is still being processed
                        if state.pending_file.is_some() {
                            state.status = String::from("Please wait for file upload to complete...");
                            continue;
                        }
                        
                        if (!state.input.trim().is_empty() || state.pending_file_content.is_some())
                            && !state.is_loading
                        {
                            let user_input = state.input.trim().to_string();
                            state.input.clear();

                            // Create message with file content if available
                            let message =
                                if let Some(file_content) = state.pending_file_content.take() {
                                    if user_input.is_empty() {
                                        crate::models::Message::user_with_content(file_content)
                                    } else {
                                        // Combine text with file content
                                        let combined_content = match file_content {
                                            MessageContent::String(text) => MessageContent::String(
                                                format!("{} {}", user_input, text),
                                            ),
                                            MessageContent::Array(mut parts) => {
                                                // Prepend text to the first text part or add as new part
                                                if let Some(ContentPart::Text { text: ref mut t }) =
                                                    parts.first_mut()
                                                {
                                                    *t = format!("{} {}", user_input, t);
                                                } else {
                                                    parts.insert(
                                                        0,
                                                        ContentPart::Text { text: user_input },
                                                    );
                                                }
                                                MessageContent::Array(parts)
                                            }
                                        };
                                        crate::models::Message::user_with_content(combined_content)
                                    }
                                } else {
                                    crate::models::Message::user(user_input.clone())
                                };

                            state.messages.add_message(message);
                            state.scroll_offset = usize::MAX;

                            state.status = String::from("Sending...");
                            state.is_loading = true;
                            state.streaming_content.clear();

                            let mut request = state.messages.to_chat_request(model.to_string());

                            if let Some(temp) = temperature {
                                request = request.with_temperature(temp);
                            }
                            if let Some(max) = max_tokens {
                                request = request.with_max_tokens(max);
                            }
                            if let Some(top_p_val) = top_p {
                                request = request.with_top_p(top_p_val);
                            }
                            if let Some(freq) = frequency_penalty {
                                request = request.with_frequency_penalty(freq);
                            }
                            if let Some(pres) = presence_penalty {
                                request = request.with_presence_penalty(pres);
                            }
                            // Use agent service if we have an agent
                            if let Some(agent) = agent {
                                if stream {
                                    let service = AgentService::new(client, agent);
                                    let stream_tx_clone = stream_tx.clone();

                                    tokio::spawn(async move {
                                        match service.chat_stream(request).await {
                                            Ok(mut event_stream) => {
                                                while let Some(event_result) =
                                                    event_stream.next().await
                                                {
                                                    match event_result {
                                                        Ok(event) => {
                                                            match event {
                                                            AgentStreamEvent::ContentDelta(
                                                                content,
                                                            ) => {
                                                                if !content.is_empty() {
                                                                    let _ = stream_tx_clone.send(
                                                                        StreamUpdate::Chunk(content),
                                                                    );
                                                                }
                                                            }
                                                            AgentStreamEvent::ToolCallsStarted(
                                                                _,
                                                            ) => {
                                                                // Silent - don't show in output
                                                            }
                                                            AgentStreamEvent::ToolExecuting(
                                                                tool_name,
                                                            ) => {
                                                                let _ = stream_tx_clone.send(
                                                                    StreamUpdate::ToolExecuting(
                                                                        tool_name,
                                                                    ),
                                                                );
                                                            }
                                                            AgentStreamEvent::ToolResult {
                                                                tool_name: _,
                                                                result,
                                                            } => {
                                                                let _ = stream_tx_clone.send(
                                                                    StreamUpdate::ToolResult(
                                                                        result,
                                                                    ),
                                                                );
                                                            }
                                                            AgentStreamEvent::Done => {
                                                                let _ = stream_tx_clone
                                                                    .send(StreamUpdate::Done);
                                                            }
                                                        }
                                                        },
                                                        Err(e) => {
                                                            let _ = stream_tx_clone.send(
                                                                StreamUpdate::Error(e.to_string()),
                                                            );
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                let _ = stream_tx_clone
                                                    .send(StreamUpdate::Error(e.to_string()));
                                            }
                                        }
                                    });

                                    state.status = String::from("Streaming (agent mode)...");
                                } else {
                                    let service = AgentService::new(client, agent);
                                    let stream_tx_clone = stream_tx.clone();

                                    tokio::spawn(async move {
                                        match service.chat(request).await {
                                            Ok(message) => {
                                                if let Some(content) = &message.content {
                                                    let text = content.extract_text();
                                                    if !text.is_empty() {
                                                    let _ = stream_tx_clone
                                                            .send(StreamUpdate::Chunk(text));
                                                    }
                                                }
                                                let _ = stream_tx_clone.send(StreamUpdate::Done);
                                            }
                                            Err(e) => {
                                                let _ = stream_tx_clone
                                                    .send(StreamUpdate::Error(e.to_string()));
                                            }
                                        }
                                    });

                                    state.status = String::from("Sending (agent mode)...");
                                }
                            } else if stream {
                                let client_clone = client.clone();
                                let stream_tx_clone = stream_tx.clone();

                                tokio::spawn(async move {
                                    match client_clone.chat_stream(request).await {
                                        Ok(mut chunk_stream) => {
                                            while let Some(chunk_result) =
                                                tokio_stream::StreamExt::next(&mut chunk_stream).await
                                            {
                                                match chunk_result {
                                                    Ok(chunk) => {
                                                        if let Some(choice) = chunk.choices.first() {
                                                            if let Some(content) = &choice.delta.content {
                                                                let _ = stream_tx_clone
                                                                    .send(StreamUpdate::Chunk(content.clone()));
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        let _ = stream_tx_clone.send(
                                                            StreamUpdate::Error(e.to_string()),
                                                        );
                                                        break;
                                                    }
                                                }
                                            }
                                            let _ = stream_tx_clone.send(StreamUpdate::Done);
                                        }
                                        Err(e) => {
                                            let _ = stream_tx_clone
                                                .send(StreamUpdate::Error(e.to_string()));
                                        }
                                    }
                                });

                                state.status = String::from("Streaming...");
                            } else {
                                let client_clone = client.clone();
                                let stream_tx_clone = stream_tx.clone();

                                tokio::spawn(async move {
                                    match client_clone.chat(request).await {
                                        Ok(resp) => {
                                            if let Some(choice) = resp.choices.first() {
                                                if let Some(content) = &choice.message.content {
                                                    let text = content.extract_text();
                                                    if !text.is_empty() {
                                                        let _ = stream_tx_clone
                                                            .send(StreamUpdate::Chunk(text));
                                                    }
                                                }
                                                let _ = stream_tx_clone.send(StreamUpdate::Done);
                                            } else {
                                                let _ = stream_tx_clone.send(StreamUpdate::Error(
                                                    "No response from assistant".to_string(),
                                                ));
                                            }
                                        }
                                        Err(e) => {
                                            let _ = stream_tx_clone
                                                .send(StreamUpdate::Error(e.to_string()));
                                        }
                                    }
                                });

                                state.status = String::from("Sending...");
                            }

                            // After spawning, continue to process/draw immediately
                            continue;
                        }
                    }
                    KeyCode::Backspace => {
                        state.input.pop();
                    }
                    KeyCode::Up => {
                        state.auto_scroll = false;
                        state.scroll_up();
                    }
                    KeyCode::Down => {
                        state.auto_scroll = false;
                        state.scroll_down();
                    }
                    KeyCode::Char('t') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                        // Toggle sidebar visibility
                        state.sidebar_visible = !state.sidebar_visible;
                    }
                    KeyCode::Char('h') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                        // Toggle help popup
                        state.show_help = !state.show_help;
                    }
                    KeyCode::Char('o') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                        // Open file picker
                        if state.file_explorer.is_none() {
                            match FileExplorer::with_theme(Theme::default().add_default_title()) {
                                Ok(explorer) => {
                                    state.file_explorer = Some(explorer);
                                    state.status = String::from(
                                        "File picker opened (Enter to select, Esc to cancel)",
                                    );
                                }
                                Err(e) => {
                                    state.status = format!("Failed to open file picker: {}", e);
                                }
                            }
                        } else {
                            // Close file picker
                            state.file_explorer = None;
                            state.status = String::from("Ready");
                        }
                    }
                    KeyCode::Char(c) => {
                        state.input.push(c);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, state: &mut ChatState) {
    // First, split for help bar at top
    let main_chunks = Layout::default()
        .constraints([
            Constraint::Length(1), // Help bar
            Constraint::Min(1),    // Rest of UI
        ])
        .split(f.area());

    render_help_bar(f, main_chunks[0]);

    // Calculate status height based on content
    // Account for borders (2 lines) and wrap text to available width
    let status_width = main_chunks[1].width.saturating_sub(2); // Account for borders
    let status_lines = if status_width > 0 {
        textwrap::wrap(&state.status, status_width as usize).len()
    } else {
        1
    };
    // Minimum 3 lines (1 for borders + 1 for content), maximum 10 lines to prevent taking too much space
    let status_height = (status_lines + 2).clamp(3, 10);

    if state.sidebar_visible {
        // With sidebar - status at top spanning full width, then messages/sidebar side by side
        let vertical_chunks = Layout::default()
            .constraints([
                Constraint::Length(status_height as u16), // Status (dynamic)
                Constraint::Min(1),                       // Messages + Sidebar
                Constraint::Length(3),                    // Input
            ])
            .split(main_chunks[1]);

        let middle_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
            .split(vertical_chunks[1]);

        render_status(f, vertical_chunks[0], state);
        render_messages(f, middle_chunks[0], state);
        render_sidebar(f, middle_chunks[1], state);
        render_input(f, vertical_chunks[2], state);
    } else {
        // No sidebar - use original 3-panel layout
        let chunks = Layout::default()
            .constraints([
                Constraint::Length(status_height as u16), // Status (dynamic)
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(main_chunks[1]);

        render_status(f, chunks[0], state);
        render_messages(f, chunks[1], state);
        render_input(f, chunks[2], state);
    }

    // Render file explorer if visible
    if let Some(ref explorer) = state.file_explorer {
        render_file_explorer(f, f.area(), explorer);
    }

    // Render help popup if visible
    if state.show_help {
        render_help_popup(f, f.area());
    }
}

fn render_file_explorer(f: &mut Frame, area: Rect, explorer: &FileExplorer) {
    // Use full screen for the file explorer
    // Clear the entire area first
    f.render_widget(Clear, area);

    // Render the file explorer widget full screen
    // In ratatui 0.29, WidgetRef types need to be rendered differently
    let widget = explorer.widget();
    use ratatui::widgets::WidgetRef;
    widget.render_ref(area, f.buffer_mut());
}

/// Check if a file is an image based on its extension
fn is_image_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext_lower = ext.to_lowercase();
            matches!(
                ext_lower.as_str(),
                "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg"
            )
        })
        .unwrap_or(false)
}

/// Check if a file is a PDF based on its extension
fn is_pdf_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase() == "pdf")
        .unwrap_or(false)
}

/// Replace <attached_file filename="...">...</attached_file> with just the filename
fn replace_attached_file_with_name(text: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;
    
    while let Some(start_idx) = remaining.find("<attached_file filename=\"") {
        // Add text before the tag
        result.push_str(&remaining[..start_idx]);
        
        // Find the end of the filename attribute
        let filename_start = start_idx + "<attached_file filename=\"".len();
        if let Some(filename_end) = remaining[filename_start..].find('"') {
            let filename = &remaining[filename_start..filename_start + filename_end];
            
            // Find the closing tag
            if let Some(close_idx) = remaining[filename_start + filename_end..].find("</attached_file>") {
                let tag_end = filename_start + filename_end + close_idx + "</attached_file>".len();
                // Replace the entire tag with just the filename
                result.push_str(&format!("[Attached file: {}]", filename));
                remaining = &remaining[tag_end..];
            } else {
                // Malformed tag, keep as is
                result.push_str(&remaining[start_idx..]);
                break;
            }
        } else {
            // Malformed tag, keep as is
            result.push_str(&remaining[start_idx..]);
            break;
        }
    }
    
    // Add remaining text
    result.push_str(remaining);
    result
}

fn render_messages(f: &mut Frame, area: Rect, state: &mut ChatState) {
    let mut lines = Vec::new();

    for message in &state.messages.messages {
        let (prefix, color) = match message.role {
            MessageRole::System => ("[System]", Color::Yellow),
            MessageRole::User => ("[You]", Color::Cyan),
            MessageRole::Assistant => ("[Assistant]", Color::Green),
            MessageRole::Tool => ("[Tool]", Color::Magenta),
        };

        lines.push(Line::from(vec![Span::styled(
            prefix,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));

        let content_text = message
            .content
            .as_ref()
            .map(|c| c.extract_text())
            .unwrap_or_default();
        let display_text = replace_attached_file_with_name(&content_text);
        let wrapped_lines = textwrap::wrap(&display_text, (area.width as usize).saturating_sub(2));
        for line in wrapped_lines {
            lines.push(Line::from(line.to_string()));
        }
        lines.push(Line::from(""));
    }

    if !state.streaming_content.is_empty() {
        let (prefix, color) = ("[Assistant]", Color::Green);
        lines.push(Line::from(vec![Span::styled(
            prefix,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));

        let display_streaming = replace_attached_file_with_name(&state.streaming_content);
        let content = textwrap::wrap(
            &display_streaming,
            (area.width as usize).saturating_sub(2),
        );
        for line in content {
            lines.push(Line::from(line.to_string()));
        }
    } else if state.is_loading {
        lines.push(Line::from(vec![Span::styled(
            "Thinking...",
            Style::default().fg(Color::Yellow),
        )]));
    }

    // Account for borders (2 lines) when calculating visible height
    let visible_height = (area.height.saturating_sub(2)) as usize;
    let max_lines = lines.len();
    let max_scroll = max_lines.saturating_sub(visible_height);

    if state.scroll_offset == usize::MAX {
        state.scroll_offset = max_scroll;
    } else if state.auto_scroll {
        state.scroll_offset = max_scroll;
    } else {
        // Clamp scroll offset to valid range first
        state.scroll_offset = state.scroll_offset.min(max_scroll);

        // Re-enable auto-scroll only if user scrolled exactly to bottom
        if state.scroll_offset == max_scroll {
            state.auto_scroll = true;
        }
    }

    let title = if let Some(agent_name) = &state.agent_name {
        format!("{} (↑/↓ to scroll, Ctrl+Q or Esc to quit)", agent_name)
    } else {
        String::from("Messages (↑/↓ to scroll, Ctrl+Q or Esc to quit)")
    };

    let block = Block::default().borders(Borders::ALL).title(title);

    let scrollbar = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let scrollbar_state = ScrollbarState::new(max_scroll).position(state.scroll_offset);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((state.scroll_offset as u16, 0));

    f.render_widget(paragraph, area);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state.clone());
}

fn render_input(f: &mut Frame, area: Rect, state: &ChatState) {
    let input = Paragraph::new(state.input.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if state.is_loading {
                    "Input (Loading...)"
                } else {
                    "Input (Enter to send)"
                }),
        )
        .style(Style::default().fg(Color::White));

    f.render_widget(input, area);

    f.set_cursor_position(Position::new(
        area.x + state.input.len() as u16 + 1,
        area.y + 1,
    ));
}

fn render_help_bar(f: &mut Frame, area: Rect) {
    let help_text = "Press Ctrl+H to view keybindings";
    let help_line = Line::from(vec![Span::styled(
        help_text,
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )]);
    let help_paragraph = Paragraph::new(help_line);
    f.render_widget(help_paragraph, area);
}

fn render_status(f: &mut Frame, area: Rect, state: &ChatState) {
    // Wrap the status text to fit within the available width (accounting for borders)
    let available_width = area.width.saturating_sub(2);
    let wrapped_lines: Vec<Line> = if available_width > 0 {
        textwrap::wrap(&state.status, available_width as usize)
            .iter()
            .map(|line| Line::from(line.to_string()))
            .collect()
    } else {
        vec![Line::from(state.status.as_str())]
    };

    let status = Paragraph::new(wrapped_lines)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(Color::Blue));

    f.render_widget(status, area);
}

fn render_help_popup(f: &mut Frame, area: Rect) {
    // Create a centered popup
    let popup_width = 50;
    let popup_height = 16;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the popup area first
    f.render_widget(Clear, popup_area);

    // Fill the entire popup area with black background using empty lines
    // Account for borders (2 lines height, 2 columns width)
    let inner_height = popup_area.height.saturating_sub(2);
    let inner_width = popup_area.width.saturating_sub(2);
    let mut background_lines = Vec::new();
    let empty_spaces = " ".repeat(inner_width as usize);
    for _ in 0..inner_height {
        background_lines.push(Line::from(vec![Span::styled(
            empty_spaces.clone(),
            Style::default().bg(Color::Black),
        )]));
    }
    // Render background without borders
    let background_paragraph =
        Paragraph::new(background_lines).style(Style::default().bg(Color::Black));
    f.render_widget(background_paragraph, popup_area);

    // Build content lines
    #[allow(clippy::vec_init_then_push)]
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        "Keybindings",
        Style::default()
            .fg(Color::Yellow)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "",
        Style::default().bg(Color::Black),
    )]));
    lines.push(Line::from(vec![
        Span::styled(
            "Ctrl+Q / Esc",
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " - Quit",
            Style::default().bg(Color::Black).fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "Ctrl+T",
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " - Toggle sidebar",
            Style::default().bg(Color::Black).fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "Ctrl+O",
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " - Open file picker",
            Style::default().bg(Color::Black).fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "Ctrl+H",
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " - Show/hide this help",
            Style::default().bg(Color::Black).fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " - Send message",
            Style::default().bg(Color::Black).fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "↑/↓",
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " - Scroll messages",
            Style::default().bg(Color::Black).fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "End",
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " - Jump to bottom (swarm mode)",
            Style::default().bg(Color::Black).fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![Span::styled(
        "",
        Style::default().bg(Color::Black),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "Press Ctrl+H to close",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC)
            .bg(Color::Black),
    )]));

    // Fill remaining space with empty black lines
    let content_height = lines.len();
    let remaining_height = inner_height.saturating_sub(content_height as u16);
    for _ in 0..remaining_height {
        lines.push(Line::from(vec![Span::styled(
            empty_spaces.clone(),
            Style::default().bg(Color::Black),
        )]));
    }

    let help_paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .style(Style::default().bg(Color::Black)),
        )
        .style(Style::default().bg(Color::Black).fg(Color::White));

    f.render_widget(help_paragraph, popup_area);
}

fn render_sidebar(f: &mut Frame, area: Rect, state: &ChatState) {
    let mut lines = Vec::new();

    // Agent name section
    if let Some(agent_name) = &state.agent_name {
        lines.push(Line::from(vec![Span::styled(
            "Agent:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        let name_wrapped = textwrap::wrap(agent_name, (area.width as usize).saturating_sub(4));
        for line in name_wrapped {
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::White),
            )]));
        }
        lines.push(Line::from(""));
    }

    // Tools section
    if !state.tools.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "Tools:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));

        for (name, description) in &state.tools {
            // Tool name in bold
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", name),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));

            // Description wrapped
            let wrapped = textwrap::wrap(description, (area.width as usize).saturating_sub(6));
            for line in wrapped {
                lines.push(Line::from(vec![Span::styled(
                    format!("    {}", line),
                    Style::default().fg(Color::DarkGray),
                )]));
            }

            lines.push(Line::from(""));
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            "No tools available",
            Style::default().fg(Color::DarkGray),
        )]));
    }

    let title = "Agent Details";
    let sidebar_widget =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));

    f.render_widget(sidebar_widget, area);
}

#[allow(clippy::too_many_arguments)]
pub async fn run_swarm(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut ChatState,
    swarm_coordinator: &SwarmCoordinatorService,
    stream: bool,
    model: &str,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    top_p: Option<f32>,
    frequency_penalty: Option<f32>,
    presence_penalty: Option<f32>,
) -> Result<()> {

    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<StreamUpdate>();

    #[derive(Debug)]
    enum StreamUpdate {
        Chunk(String),
        ToolExecuting(String),
        ToolResult(String),
        StatusUpdate(String),
        Done,
        Error(String),
    }

    loop {
        // Process all pending stream updates first
        while let Ok(update) = stream_rx.try_recv() {
            match update {
                StreamUpdate::Chunk(content) => {
                    state.streaming_content.push_str(&content);
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::ToolExecuting(tool_name) => {
                    state.tool_messages.push(format!("🔧 {}", tool_name));
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::ToolResult(result) => {
                    if let Some(last) = state.tool_messages.last_mut() {
                        last.push_str(&format!(" → {}", result));
                    }
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::StatusUpdate(status) => {
                    state.status = status;
                }
                StreamUpdate::Done => {
                    state.is_loading = false;
                    state.status = String::from("Ready");

                    // Combine tool messages with content
                    let mut full_content = String::new();
                    if !state.tool_messages.is_empty() {
                        for tool_msg in &state.tool_messages {
                            full_content.push_str(tool_msg);
                            full_content.push('\n');
                        }
                        full_content.push('\n');
                    }
                    full_content.push_str(&state.streaming_content);

                    state.streaming_content.clear();
                    state.tool_messages.clear();
                    state.add_assistant(full_content);
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::Error(err) => {
                    state.is_loading = false;
                    state.status = format!("Error: {}", err);
                    state.streaming_content.clear();
                    state.tool_messages.clear();
                    state.messages.messages.pop();
                }
            }
        }

        // Draw UI
        terminal
            .draw(|f| ui(f, state))
            .map_err(|e| crate::models::Error::Other(format!("Terminal draw error: {}", e)))?;

        // Handle input
        if event::poll(std::time::Duration::from_millis(100))
            .map_err(|e| crate::models::Error::Other(format!("Event poll error: {}", e)))?
        {
            if let Event::Key(key_event) = event::read()
                .map_err(|e| crate::models::Error::Other(format!("Event read error: {}", e)))?
            {
                if key_event.kind == KeyEventKind::Press {
                    match key_event.code {
                        KeyCode::Char('c')
                            if key_event.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            return Ok(());
                        }
                        KeyCode::Enter => {
                            if !state.input.is_empty() && !state.is_loading {
                                let user_input = state.input.clone();
                                state.input.clear();
                                state.add_user(&user_input);
                                state.is_loading = true;
                                state.status = String::from("Thinking...");

                                // Create chat request
                                let mut request = state.messages.to_chat_request(model.to_string());

                                if let Some(temp) = temperature {
                                    request = request.with_temperature(temp);
                                }
                                if let Some(max_tok) = max_tokens {
                                    request = request.with_max_tokens(max_tok);
                                }
                                if let Some(tp) = top_p {
                                    request = request.with_top_p(tp);
                                }
                                if let Some(fp) = frequency_penalty {
                                    request = request.with_frequency_penalty(fp);
                                }
                                if let Some(pp) = presence_penalty {
                                    request = request.with_presence_penalty(pp);
                                }

                                // Use swarm coordinator service
                                if stream {
                                    let stream_tx_clone = stream_tx.clone();
                                    let swarm_coordinator = swarm_coordinator.clone();
                                    // Create a status sender that wraps messages in StreamUpdate::StatusUpdate
                                    let (status_tx_inner, mut status_rx) =
                                        mpsc::unbounded_channel::<String>();
                                    let stream_tx_for_status = stream_tx_clone.clone();
                                    tokio::spawn(async move {
                                        while let Some(status) = status_rx.recv().await {
                                            let _ = stream_tx_for_status
                                                .send(StreamUpdate::StatusUpdate(status));
                                        }
                                    });

                                    tokio::spawn(async move {
                                        match swarm_coordinator
                                            .chat_stream(request, Some(status_tx_inner))
                                            .await
                                        {
                                            Ok(mut event_stream) => {
                                                while let Some(event) = event_stream.next().await {
                                                    match event {
                                                        Ok(AgentStreamEvent::ContentDelta(
                                                            content,
                                                        )) => {
                                                            let _ = stream_tx_clone
                                                                .send(StreamUpdate::Chunk(content));
                                                        }
                                                        Ok(AgentStreamEvent::ToolExecuting(
                                                            tool_name,
                                                        )) => {
                                                            let _ = stream_tx_clone.send(
                                                                StreamUpdate::ToolExecuting(
                                                                    tool_name,
                                                                ),
                                                            );
                                                        }
                                                        Ok(AgentStreamEvent::ToolResult {
                                                            result,
                                                            ..
                                                        }) => {
                                                            let _ = stream_tx_clone.send(
                                                                StreamUpdate::ToolResult(result),
                                                            );
                                                        }
                                                        Ok(AgentStreamEvent::Done) => {
                                                            let _ = stream_tx_clone
                                                                .send(StreamUpdate::Done);
                                                            break;
                                                        }
                                                        Err(e) => {
                                                            let _ = stream_tx_clone.send(
                                                                StreamUpdate::Error(e.to_string()),
                                                            );
                                                            break;
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                let _ = stream_tx_clone
                                                    .send(StreamUpdate::Error(e.to_string()));
                                            }
                                        }
                                    });
                                } else {
                                    let stream_tx_clone = stream_tx.clone();
                                    let swarm_coordinator = swarm_coordinator.clone();
                                    // Create a status sender that wraps messages in StreamUpdate::StatusUpdate
                                    let (status_tx_inner, mut status_rx) =
                                        mpsc::unbounded_channel::<String>();
                                    let stream_tx_for_status = stream_tx_clone.clone();
                                    tokio::spawn(async move {
                                        while let Some(status) = status_rx.recv().await {
                                            let _ = stream_tx_for_status
                                                .send(StreamUpdate::StatusUpdate(status));
                                        }
                                    });

                                    tokio::spawn(async move {
                                        match swarm_coordinator
                                            .chat(request, Some(status_tx_inner))
                                            .await
                                        {
                                            Ok(message) => {
                                                if let Some(content) = &message.content {
                                                    let text = content.extract_text();
                                                    if !text.is_empty() {
                                                        let _ = stream_tx_clone
                                                            .send(StreamUpdate::Chunk(text));
                                                    }
                                                }
                                                let _ = stream_tx_clone.send(StreamUpdate::Done);
                                            }
                                            Err(e) => {
                                                let _ = stream_tx_clone
                                                    .send(StreamUpdate::Error(e.to_string()));
                                            }
                                        }
                                    });
                                }
                            }
                        }
                        KeyCode::Char('t')
                            if key_event.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            // Toggle sidebar visibility
                            state.sidebar_visible = !state.sidebar_visible;
                        }
                        KeyCode::Char('h')
                            if key_event.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            // Toggle help popup
                            state.show_help = !state.show_help;
                        }
                        KeyCode::Char(c) => {
                            state.input.push(c);
                        }
                        KeyCode::Backspace => {
                            state.input.pop();
                        }
                        KeyCode::Up => {
                            state.auto_scroll = false;
                            state.scroll_up();
                        }
                        KeyCode::Down => {
                            state.auto_scroll = false;
                            state.scroll_down();
                        }
                        KeyCode::End => {
                            state.auto_scroll = true;
                            state.scroll_offset = usize::MAX;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
