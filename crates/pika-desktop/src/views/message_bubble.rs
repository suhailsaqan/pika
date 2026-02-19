use iced::widget::{column, container, row, text, Space};
use iced::{Element, Fill, Theme};
use pika_core::{ChatMessage, MessageDeliveryState};

use crate::theme;
use crate::Message;

/// Renders a single message as a styled bubble.
pub fn message_bubble<'a>(msg: &'a ChatMessage, is_group: bool) -> Element<'a, Message, Theme> {
    let timestamp = theme::relative_time(msg.timestamp);

    let delivery_indicator = match &msg.delivery {
        MessageDeliveryState::Pending => " \u{231B}",
        MessageDeliveryState::Sent => "",
        MessageDeliveryState::Failed { .. } => " \u{26A0}",
    };

    let time_text = format!("{timestamp}{delivery_indicator}");

    if msg.is_mine {
        // ── Sent: right-aligned blue bubble ─────────────────────────
        let bubble = container(
            column![
                text(&msg.display_content)
                    .size(14)
                    .color(iced::Color::WHITE),
                text(time_text)
                    .size(10)
                    .color(iced::Color::WHITE.scale_alpha(0.6)),
            ]
            .spacing(2),
        )
        .padding([8, 12])
        .max_width(500)
        .style(theme::bubble_sent_style);

        row![Space::new().width(Fill), bubble].width(Fill).into()
    } else {
        // ── Received: left-aligned dark bubble ──────────────────────
        let sender_name = if is_group {
            msg.sender_name.as_deref().unwrap_or("Unknown")
        } else {
            ""
        };

        let mut bubble_content = column![].spacing(2);

        if !sender_name.is_empty() {
            bubble_content =
                bubble_content.push(text(sender_name).size(12).color(theme::ACCENT_BLUE));
        }

        bubble_content = bubble_content.push(
            text(&msg.display_content)
                .size(14)
                .color(theme::TEXT_PRIMARY),
        );

        bubble_content = bubble_content.push(text(time_text).size(10).color(theme::TEXT_FADED));

        let bubble = container(bubble_content)
            .padding([8, 12])
            .max_width(500)
            .style(theme::bubble_received_style);

        row![bubble, Space::new().width(Fill)].width(Fill).into()
    }
}
