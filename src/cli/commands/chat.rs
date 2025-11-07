use crate::client::Client;
use crate::models::{ChatCompletionRequest, Message, MessageRole, Result};
use clap::Args;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame, Terminal,
};
use std::io;

#[derive(Args)]
pub struct ChatArgs {
    /// System prompt to use
    #[arg(short, long)]
    pub system: Option<String>,

    /// Model to use (default: gpt-3.5-turbo)
    #[arg(long, default_value = "gpt-3.5-turbo")]
    pub model: String,

    /// Temperature parameter (0.0 to 2.0)
    #[arg(short, long)]
    pub temperature: Option<f32>,

    /// Maximum tokens in the response
    #[arg(short = 'M', long)]
    pub max_tokens: Option<u32>,

    /// Top-p parameter
    #[arg(long)]
    pub top_p: Option<f32>,

    /// Frequency penalty
    #[arg(long)]
    pub frequency_penalty: Option<f32>,

    /// Presence penalty
    #[arg(long)]
    pub presence_penalty: Option<f32>,

    /// Base URL for the API (defaults to OpenAI or OPENAI_BASE_URL env var)
    #[arg(long)]
    pub base_url: Option<String>,

    /// API key (defaults to OPENAI_API_KEY env var)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Enable streaming responses (default: false)
    #[arg(long, default_value = "false")]
    pub stream: bool,
}

struct ChatState {
    messages: Vec<Message>,
    input: String,
    scroll_offset: usize,
    status: String,
    is_loading: bool,
    streaming_content: String, // Current streaming assistant message being built
    auto_scroll: bool, // Whether to auto-scroll to bottom
}

impl ChatState {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            scroll_offset: 0,
            status: String::from("Ready"),
            is_loading: false,
            streaming_content: String::new(),
            auto_scroll: true, // Start with auto-scroll enabled
        }
    }

    fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        // Auto-scroll to bottom - will be recalculated based on actual line count
        self.scroll_offset = usize::MAX; // Signal to scroll to bottom
    }

    fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(1);
        }
    }
}

pub async fn run_chat(args: ChatArgs) -> Result<()> {
    // Get API key
    let api_key = args
        .api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            crate::models::Error::Configuration(
                "API key not provided. Set OPENAI_API_KEY environment variable or use --api-key".to_string(),
            )
        })?;

    // Get base URL
    let base_url = args
        .base_url
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    // Create client
    let client = Client::new(api_key, base_url);

    // Initialize message history
    let mut state = ChatState::new();

    // Add system prompt if provided
    if let Some(system) = args.system {
        state.messages.push(Message::system(system));
    }

    // Setup terminal
    crossterm::terminal::enable_raw_mode().map_err(|e| {
        crate::models::Error::Other(format!("Failed to enable raw mode: {}", e))
    })?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen).map_err(|e| {
        crate::models::Error::Other(format!("Failed to enter alternate screen: {}", e))
    })?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| {
        crate::models::Error::Other(format!("Failed to create terminal: {}", e))
    })?;

    // Clone args to avoid borrowing issues
    let model = args.model.clone();
    let temperature = args.temperature;
    let max_tokens = args.max_tokens;
    let top_p = args.top_p;
    let frequency_penalty = args.frequency_penalty;
    let presence_penalty = args.presence_penalty;
    let stream = args.stream;

    let result = run_tui(
        &mut terminal,
        &mut state,
        &client,
        stream,
        &model,
        temperature,
        max_tokens,
        top_p,
        frequency_penalty,
        presence_penalty,
    )
    .await;

    // Restore terminal
    crossterm::terminal::disable_raw_mode().ok();
    crossterm::execute!(
        io::stdout(),
        crossterm::terminal::LeaveAlternateScreen
    )
    .ok();

    result
}

async fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut ChatState,
    client: &Client,
    stream: bool,
    model: &str,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    top_p: Option<f32>,
    frequency_penalty: Option<f32>,
    presence_penalty: Option<f32>,
) -> Result<()> {
    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<StreamUpdate>();
    
    enum StreamUpdate {
        Chunk(String),
        Done,
        Error(String),
    }
    
    loop {
        terminal.draw(|f| ui(f, &mut *state))
            .map_err(|e| crate::models::Error::Other(format!("Failed to draw: {}", e)))?;
        
        // Process streaming updates
        while let Ok(update) = stream_rx.try_recv() {
            match update {
                StreamUpdate::Chunk(content) => {
                    state.streaming_content.push_str(&content);
                    // Auto-scroll as content streams in
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::Done => {
                    state.is_loading = false;
                    state.status = String::from("Ready");
                    let content = state.streaming_content.clone();
                    state.streaming_content.clear();
                    state.add_message(Message::assistant(content));
                    // Keep auto-scroll enabled after message completes
                    if state.auto_scroll {
                        state.scroll_offset = usize::MAX;
                    }
                }
                StreamUpdate::Error(err) => {
                    state.is_loading = false;
                    state.status = format!("Error: {}", err);
                    state.messages.pop(); // Remove user message
                }
            }
        }

        if crossterm::event::poll(std::time::Duration::from_millis(100))
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
                            state.add_message(Message::user(user_input.clone()));

                            // Build request
                            let mut request =
                                ChatCompletionRequest::new(model.to_string(), state.messages.clone());

                            if let Some(temp) = temperature {
                                request = request.with_temperature(temp);
                            }
                            if let Some(max) = max_tokens {
                                request = request.with_max_tokens(max);
                            }
                            if let Some(top_p) = top_p {
                                request = request.with_top_p(top_p);
                            }
                            if let Some(freq) = frequency_penalty {
                                request = request.with_frequency_penalty(freq);
                            }
                            if let Some(pres) = presence_penalty {
                                request = request.with_presence_penalty(pres);
                            }

                            if stream {
                                // Start streaming request
                                let client_clone = client.clone();
                                let stream_tx_clone = stream_tx.clone();
                                
                                tokio::spawn(async move {
                                    match client_clone.chat_completion_stream(request).await {
                                        Ok(mut stream) => {
                                            while let Some(chunk_result) = stream.next().await {
                                                match chunk_result {
                                                    Ok(chunk) => {
                                                        if let Some(choice) = chunk.choices.first() {
                                                            if let Some(content) = &choice.delta.content {
                                                                let _ = stream_tx_clone.send(StreamUpdate::Chunk(content.clone()));
                                                            }
                                                            if choice.finish_reason.is_some() {
                                                                let _ = stream_tx_clone.send(StreamUpdate::Done);
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        let _ = stream_tx_clone.send(StreamUpdate::Error(e.to_string()));
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let _ = stream_tx_clone.send(StreamUpdate::Error(e.to_string()));
                                        }
                                    }
                                });

                                state.status = String::from("Streaming...");
                            } else {
                                // Non-streaming request
                                let client_clone = client.clone();
                                let stream_tx_clone = stream_tx.clone();
                                
                                tokio::spawn(async move {
                                    match client_clone.chat_completion(request).await {
                                        Ok(resp) => {
                                            if let Some(choice) = resp.choices.first() {
                                                let _ = stream_tx_clone.send(StreamUpdate::Chunk(choice.message.content.clone()));
                                                let _ = stream_tx_clone.send(StreamUpdate::Done);
                                            } else {
                                                let _ = stream_tx_clone.send(StreamUpdate::Error("No response from assistant".to_string()));
                                            }
                                        }
                                        Err(e) => {
                                            let _ = stream_tx_clone.send(StreamUpdate::Error(e.to_string()));
                                        }
                                    }
                                });

                                state.status = String::from("Sending...");
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        state.input.pop();
                    }
                    KeyCode::Up => {
                        state.auto_scroll = false; // Disable auto-scroll when user manually scrolls
                        state.scroll_up();
                    }
                    KeyCode::Down => {
                        // Scroll down - actual max will be calculated in render
                        state.scroll_offset = state.scroll_offset.saturating_add(1);
                        // If we reach the bottom, re-enable auto-scroll
                        // This will be checked in render_messages
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
    let chunks = Layout::default()
        .constraints([
            Constraint::Min(1), // Messages area
            Constraint::Length(3), // Input area (1 line + 2 borders)
            Constraint::Length(3), // Status bar (1 line + 2 borders)
        ])
        .split(f.area());

    render_messages(f, chunks[0], state);
    render_input(f, chunks[1], state);
    render_status(f, chunks[2], state);
}

fn render_messages(f: &mut Frame, area: Rect, state: &mut ChatState) {
    let mut lines = Vec::new();

    for message in &state.messages {
        let (prefix, color) = match message.role {
            MessageRole::System => ("[System]", Color::Yellow),
            MessageRole::User => ("[You]", Color::Cyan),
            MessageRole::Assistant => ("[Assistant]", Color::Green),
        };

        // Add prefix line
        lines.push(Line::from(vec![Span::styled(
            prefix,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));

        // Add message content with word wrapping
        let content = textwrap::wrap(&message.content, (area.width as usize).saturating_sub(2));
        for line in content {
            lines.push(Line::from(line.to_string()));
        }
        lines.push(Line::from("")); // Empty line between messages
    }

    // Show streaming content if available
    if !state.streaming_content.is_empty() {
        let (prefix, color) = ("[Assistant]", Color::Green);
        lines.push(Line::from(vec![Span::styled(
            prefix,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));
        
        let content = textwrap::wrap(&state.streaming_content, (area.width as usize).saturating_sub(2));
        for line in content {
            lines.push(Line::from(line.to_string()));
        }
    } else if state.is_loading {
        lines.push(Line::from(vec![Span::styled(
            "Thinking...",
            Style::default().fg(Color::Yellow),
        )]));
    }

    // Calculate actual max scroll based on lines
    let visible_height = area.height as usize;
    let max_lines = lines.len();
    let max_scroll = max_lines.saturating_sub(visible_height);
    
    // Auto-scroll to bottom if requested or if auto_scroll is enabled and we're near bottom
    if state.scroll_offset == usize::MAX {
        state.scroll_offset = max_scroll;
    } else if state.auto_scroll {
        // If auto-scroll is enabled, keep at bottom
        state.scroll_offset = max_scroll;
    } else {
        // Check if user scrolled to bottom - if so, re-enable auto-scroll
        if state.scroll_offset >= max_scroll.saturating_sub(2) {
            state.auto_scroll = true;
            state.scroll_offset = max_scroll;
        }
    }
    
    // Clamp scroll offset to valid range
    state.scroll_offset = state.scroll_offset.min(max_scroll);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Messages (↑/↓ to scroll, Ctrl+Q or Esc to quit)");

    let scrollbar = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let scrollbar_state = ScrollbarState::new(max_scroll)
        .position(state.scroll_offset);

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

    // Set cursor position
    f.set_cursor_position(ratatui::layout::Position::new(
        area.x + state.input.len() as u16 + 1,
        area.y + 1,
    ));
}

fn render_status(f: &mut Frame, area: Rect, state: &ChatState) {
    let status = Paragraph::new(state.status.as_str())
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(Color::Blue));

    f.render_widget(status, area);
}
