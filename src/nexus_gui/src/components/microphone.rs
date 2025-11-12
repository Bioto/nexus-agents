use iced::widget::{container, svg};
use iced::{Element, Length};

/// Recording state for the microphone icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingState {
    /// Microphone is not recording
    NotRecording,
    /// Microphone is currently recording
    Recording,
}

/// A reusable microphone icon component.
///
/// This component displays a microphone SVG icon that can be used throughout the application.
/// It supports two states: recording and not recording, with different visual styles.
pub struct MicrophoneIcon;

impl MicrophoneIcon {
    /// Creates a new microphone icon element in the not recording state.
    ///
    /// The icon will fill the available space and is centered within its container.
    pub fn new<Message: 'static>() -> Element<'static, Message> {
        Self::with_state(RecordingState::NotRecording)
    }

    /// Creates a new microphone icon element in the not recording state.
    ///
    /// The icon will fill the available space and is centered within its container.
    pub fn create<Message: 'static>() -> Element<'static, Message> {
        Self::with_state(RecordingState::NotRecording)
    }

    /// Creates a microphone icon with a specific recording state.
    ///
    /// # Arguments
    /// * `state` - The recording state (Recording or NotRecording)
    pub fn with_state<Message: 'static>(state: RecordingState) -> Element<'static, Message> {
        let svg_data = match state {
            RecordingState::Recording => Self::recording_svg_data(),
            RecordingState::NotRecording => Self::svg_data(),
        };

        let icon = svg::Svg::new(svg::Handle::from_memory(svg_data))
            .width(Length::Fill)
            .height(Length::Fill);

        let background_color = match state {
            RecordingState::Recording => iced::Color::from_rgba8(220, 38, 38, 0.9), // Red
            RecordingState::NotRecording => iced::Color::from_rgba8(32, 32, 36, 0.9), // Dark gray
        };

        container(icon)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(12)
            .center_x()
            .center_y()
            .style(iced::theme::Container::Custom(Box::new(
                MicContainerStyle { background_color },
            )))
            .into()
    }

    /// Creates a microphone icon with custom size.
    ///
    /// # Arguments
    /// * `size` - The size of the icon in pixels
    #[allow(dead_code)]
    pub fn with_size<Message: 'static>(size: f32) -> Element<'static, Message> {
        Self::with_size_and_state(size, RecordingState::NotRecording)
    }

    /// Creates a microphone icon with custom size and recording state.
    ///
    /// # Arguments
    /// * `size` - The size of the icon in pixels
    /// * `state` - The recording state (Recording or NotRecording)
    #[allow(dead_code)]
    pub fn with_size_and_state<Message: 'static>(
        size: f32,
        state: RecordingState,
    ) -> Element<'static, Message> {
        let svg_data = match state {
            RecordingState::Recording => Self::recording_svg_data(),
            RecordingState::NotRecording => Self::svg_data(),
        };

        let icon = svg::Svg::new(svg::Handle::from_memory(svg_data))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size));

        let background_color = match state {
            RecordingState::Recording => iced::Color::from_rgba8(220, 38, 38, 0.9), // Red
            RecordingState::NotRecording => iced::Color::from_rgba8(32, 32, 36, 0.9), // Dark gray
        };

        container(icon)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(12)
            .center_x()
            .center_y()
            .style(iced::theme::Container::Custom(Box::new(
                MicContainerStyle { background_color },
            )))
            .into()
    }

    /// Returns the SVG data for the microphone icon (not recording state).
    pub fn svg_data() -> &'static [u8] {
        MICROPHONE_SVG
    }

    /// Returns the SVG data for the microphone icon (recording state).
    pub fn recording_svg_data() -> &'static [u8] {
        MICROPHONE_RECORDING_SVG
    }
}

struct MicContainerStyle {
    background_color: iced::Color,
}

impl iced::widget::container::StyleSheet for MicContainerStyle {
    type Style = iced::Theme;

    fn appearance(&self, _style: &Self::Style) -> iced::widget::container::Appearance {
        iced::widget::container::Appearance {
            background: Some(self.background_color.into()),
            border: iced::Border::with_radius(12.0),
            ..Default::default()
        }
    }
}

const MICROPHONE_SVG: &[u8] = br#"<?xml version='1.0' encoding='UTF-8'?>
<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'>
  <path fill='#FFFFFF' d='M12 15c1.66 0 3-1.34 3-3V6c0-1.66-1.34-3-3-3S9 4.34 9 6v6c0 1.66 1.34 3 3 3zm4.3-3c0 2.3-1.7 4.2-3.9 4.5V19h2.1c.55 0 1 .45 1 1s-.45 1-1 1H9.5c-.55 0-1-.45-1-1s.45-1 1-1H11v-2.5c-2.2-.3-3.9-2.2-3.9-4.5 0-.55.45-1 1-1s1 .45 1 1c0 1.66 1.34 3 3 3s3-1.34 3-3c0-.55.45-1 1-1s1 .45 1 1z'/>
</svg>
"#;

const MICROPHONE_RECORDING_SVG: &[u8] = br#"<?xml version='1.0' encoding='UTF-8'?>
<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'>
  <path fill='#FFFFFF' d='M12 15c1.66 0 3-1.34 3-3V6c0-1.66-1.34-3-3-3S9 4.34 9 6v6c0 1.66 1.34 3 3 3zm4.3-3c0 2.3-1.7 4.2-3.9 4.5V19h2.1c.55 0 1 .45 1 1s-.45 1-1 1H9.5c-.55 0-1-.45-1-1s.45-1 1-1H11v-2.5c-2.2-.3-3.9-2.2-3.9-4.5 0-.55.45-1 1-1s1 .45 1 1c0 1.66 1.34 3 3 3s3-1.34 3-3c0-.55.45-1 1-1s1 .45 1 1z'/>
  <circle cx='12' cy='12' r='10' fill='none' stroke='#FFFFFF' stroke-width='2' opacity='0.5'/>
</svg>
"#;
