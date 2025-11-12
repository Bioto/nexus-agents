use crate::agent_service::{AgentService, AgentStreamEvent};
use crate::client::Client;
use crate::models::{Agent, ChatHistory, MessageRole, Result};
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
use std::io;
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

pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut ChatState,
    client: &Client,
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

    #[derive(Debug)]
    enum StreamUpdate {
        Chunk(String),
        ToolExecuting(String),
        ToolResult(String),
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
            if let Event::Key(key) = event::read()
                .map_err(|e| crate::models::Error::Other(format!("Failed to read event: {}", e)))?
            {
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
                        if !state.input.trim().is_empty() && !state.is_loading {
                            let user_input = state.input.trim().to_string();
                            state.input.clear();
                            state.status = String::from("Sending...");
                            state.is_loading = true;
                            state.streaming_content.clear();
                            state.add_user(user_input.clone());

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
                                    let service = AgentService::new(&client, agent);
                                    let stream_tx_clone = stream_tx.clone();

                                    tokio::spawn(async move {
                                        match service.chat_stream(request).await {
                                            Ok(mut event_stream) => {
                                                while let Some(event_result) =
                                                    event_stream.next().await
                                                {
                                                    match event_result {
                                                        Ok(event) => match event {
                                                            AgentStreamEvent::ContentDelta(
                                                                content,
                                                            ) => {
                                                                let _ = stream_tx_clone.send(
                                                                    StreamUpdate::Chunk(content),
                                                                );
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
                                    let service = AgentService::new(&client, agent);
                                    let stream_tx_clone = stream_tx.clone();

                                    tokio::spawn(async move {
                                        match service.chat(request).await {
                                            Ok(message) => {
                                                if let Some(content) = message.content {
                                                    let _ = stream_tx_clone
                                                        .send(StreamUpdate::Chunk(content));
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
                                    match client_clone.chat_completion_text_stream(request).await {
                                        Ok(mut text_stream) => {
                                            while let Some(content_result) =
                                                text_stream.next().await
                                            {
                                                match content_result {
                                                    Ok(content) => {
                                                        let _ = stream_tx_clone
                                                            .send(StreamUpdate::Chunk(content));
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
                                    match client_clone.chat_completion(request).await {
                                        Ok(resp) => {
                                            if let Some(choice) = resp.choices.first() {
                                                if let Some(content) = &choice.message.content {
                                                    let _ = stream_tx_clone
                                                        .send(StreamUpdate::Chunk(content.clone()));
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
                Constraint::Min(1),    // Messages + Sidebar
                Constraint::Length(3), // Input
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

    // Render help popup if visible
    if state.show_help {
        render_help_popup(f, f.area());
    }
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

        let content_text = message.content.as_deref().unwrap_or("");
        let content = textwrap::wrap(content_text, (area.width as usize).saturating_sub(2));
        for line in content {
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

        let content = textwrap::wrap(
            &state.streaming_content,
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
    let max_scroll = if max_lines > visible_height {
        max_lines - visible_height
    } else {
        0
    };

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

/// Run the TUI with SwarmCoordinatorService for swarm mode
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
                                    let (status_tx_inner, mut status_rx) = mpsc::unbounded_channel::<String>();
                                    let stream_tx_for_status = stream_tx_clone.clone();
                                    tokio::spawn(async move {
                                        while let Some(status) = status_rx.recv().await {
                                            let _ = stream_tx_for_status.send(StreamUpdate::StatusUpdate(status));
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
                                    let (status_tx_inner, mut status_rx) = mpsc::unbounded_channel::<String>();
                                    let stream_tx_for_status = stream_tx_clone.clone();
                                    tokio::spawn(async move {
                                        while let Some(status) = status_rx.recv().await {
                                            let _ = stream_tx_for_status.send(StreamUpdate::StatusUpdate(status));
                                        }
                                    });

                                    tokio::spawn(async move {
                                        match swarm_coordinator.chat(request, Some(status_tx_inner)).await {
                                            Ok(message) => {
                                                let _ = stream_tx_clone.send(StreamUpdate::Chunk(
                                                    message.content.unwrap_or_default(),
                                                ));
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
