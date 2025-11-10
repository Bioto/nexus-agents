use crate::components::{MicrophoneIcon, RecordingState};
use clap::Args;
use iced::{
    executor,
    widget::button,
    window::{self, Level},
    Application, Command, Element, Length, Settings, Size, Theme,
};

#[derive(Args, Debug)]
#[command(about = "Show a GUI component")]
pub struct ShowArgs {
    /// Component to show (e.g., "microphone")
    #[arg(short, long, default_value = "microphone")]
    pub component: String,

    /// Recording state for microphone component (true = recording, false = not recording)
    #[arg(short, long, default_value = "false")]
    pub recording: bool,

    /// Window size in pixels
    #[arg(short, long, default_value = "100")]
    pub size: f32,

    /// X position for window (default: top right)
    #[arg(long)]
    pub x: Option<f32>,

    /// Y position for window (default: top right)
    #[arg(long)]
    pub y: Option<f32>,
}

pub fn run_show(args: ShowArgs) -> iced::Result {
    // Calculate position for top right corner if not specified
    let window_size = args.size;
    let padding = 20.0;
    let screen_width = 1920.0; // Default, window manager will clamp

    let x_position = args
        .x
        .unwrap_or_else(|| screen_width - window_size - padding);
    let y_position = args.y.unwrap_or(padding);

    let mut settings = Settings::with_flags(args);
    settings.antialiasing = true;
    settings.window = window::Settings {
        size: Size::new(window_size, window_size),
        position: window::Position::Specific(iced::Point::new(x_position, y_position)),
        resizable: false,
        decorations: true,
        level: Level::AlwaysOnTop,
        ..Default::default()
    };
    App::run(settings)
}

struct App {
    component: String,
    recording: bool,
}

#[derive(Debug, Clone)]
enum Message {
    ToggleRecording,
}

impl Application for App {
    type Message = Message;
    type Theme = Theme;
    type Executor = executor::Default;
    type Flags = ShowArgs;

    fn new(flags: Self::Flags) -> (Self, Command<Message>) {
        (
            App {
                component: flags.component.clone(),
                recording: flags.recording,
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        format!("Nexus - {}", self.component)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::ToggleRecording if self.component == "microphone" => {
                self.recording = !self.recording;
            }
            Message::ToggleRecording => {}
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Message> {
        match self.component.as_str() {
            "microphone" => {
                let state = if self.recording {
                    RecordingState::Recording
                } else {
                    RecordingState::NotRecording
                };
                button(MicrophoneIcon::with_state(state))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .on_press(Message::ToggleRecording)
                    .into()
            }
            _ => {
                // Default to microphone if component not found
                MicrophoneIcon::new()
            }
        }
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}
