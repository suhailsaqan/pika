use iced::widget::{button, container, row, text};
use iced::{Element, Fill, Theme};

use crate::theme;
use crate::Message;

/// Full-width toast notification bar.
pub fn toast_bar(message: &str) -> Element<'_, Message, Theme> {
    container(
        row![
            text(message).color(iced::Color::WHITE).width(Fill),
            button(text("\u{2715}").color(iced::Color::WHITE).size(14))
                .on_press(Message::ClearToast)
                .padding([4, 8])
                .style(|_theme: &Theme, _status: button::Status| button::Style {
                    background: None,
                    text_color: iced::Color::WHITE,
                    ..Default::default()
                }),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding([8, 16])
    .width(Fill)
    .style(theme::toast_container_style)
    .into()
}
