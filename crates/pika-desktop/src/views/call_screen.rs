use iced::widget::{button, center, column, container, row, text, Space};
use iced::{Alignment, Element, Fill, Theme};
use pika_core::{CallState, CallStatus};

use crate::theme;
use crate::Message;

/// Full-screen call overlay (matches the iOS CallScreenView layout).
pub fn call_screen_view<'a>(
    call: &'a CallState,
    peer_name: &'a str,
) -> Element<'a, Message, Theme> {
    let status_text = match &call.status {
        CallStatus::Offering => "Calling\u{2026}",
        CallStatus::Ringing => "Incoming call\u{2026}",
        CallStatus::Connecting => "Connecting\u{2026}",
        CallStatus::Active => "Active",
        CallStatus::Ended { .. } => "Call ended",
    };

    // Peer avatar (initial in a circle)
    let initial = peer_name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    let avatar = container(text(initial).size(42).color(iced::Color::WHITE).center())
        .width(112)
        .height(112)
        .center_x(112)
        .center_y(112)
        .style(|_: &Theme| container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgba(
                1.0, 1.0, 1.0, 0.18,
            ))),
            border: iced::border::rounded(56),
            ..Default::default()
        });

    let mut content = column![
        // Dismiss button row
        row![
            button(text("\u{2304}").size(20).color(iced::Color::WHITE).center())
                .on_press(Message::DismissCallScreen)
                .padding([4, 12])
                .style(theme::call_control_button_style),
            Space::new().width(Fill),
        ],
        Space::new().height(40),
        // Peer info
        center(avatar).width(Fill),
        text(peer_name)
            .size(24)
            .color(iced::Color::WHITE)
            .center()
            .width(Fill),
        text(status_text)
            .size(16)
            .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.7))
            .center()
            .width(Fill),
    ]
    .spacing(8)
    .width(Fill);

    // Duration for active calls
    if matches!(call.status, CallStatus::Active) {
        if let Some(started_at) = call.started_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let elapsed = (now - started_at).max(0);
            let mins = elapsed / 60;
            let secs = elapsed % 60;
            content = content.push(
                text(format!("{mins:02}:{secs:02}"))
                    .size(20)
                    .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.9))
                    .center()
                    .width(Fill),
            );
        }
    }

    // Debug stats
    if let Some(debug) = &call.debug {
        let stats = format!(
            "TX:{} RX:{} drop:{} jitter:{}ms{}",
            debug.tx_frames,
            debug.rx_frames,
            debug.rx_dropped,
            debug.jitter_buffer_ms,
            debug
                .last_rtt_ms
                .map(|rtt| format!(" rtt:{rtt}ms"))
                .unwrap_or_default()
        );
        content = content.push(
            container(
                text(stats)
                    .size(11)
                    .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.6)),
            )
            .center_x(Fill)
            .padding([4, 12]),
        );
    }

    content = content.push(Space::new().height(Fill));

    // Control row
    let controls: Element<'a, Message, Theme> = match &call.status {
        CallStatus::Ringing => row![
            button(text("Decline").size(14).color(iced::Color::WHITE).center())
                .on_press(Message::RejectCall)
                .padding([12, 24])
                .style(theme::danger_button_style),
            Space::new().width(48),
            button(text("Accept").size(14).color(iced::Color::WHITE).center())
                .on_press(Message::AcceptCall)
                .padding([12, 24])
                .style(theme::call_accept_button_style),
        ]
        .align_y(Alignment::Center)
        .into(),
        CallStatus::Offering | CallStatus::Connecting | CallStatus::Active => {
            let mute_label = if call.is_muted { "Unmute" } else { "Mute" };
            let mute_style: fn(&Theme, button::Status) -> button::Style = if call.is_muted {
                theme::call_muted_button_style
            } else {
                theme::call_control_button_style
            };
            row![
                button(text(mute_label).size(14).color(iced::Color::WHITE).center())
                    .on_press(Message::ToggleMute)
                    .padding([12, 24])
                    .style(mute_style),
                Space::new().width(48),
                button(text("End").size(14).color(iced::Color::WHITE).center())
                    .on_press(Message::EndCall)
                    .padding([12, 24])
                    .style(theme::danger_button_style),
            ]
            .align_y(Alignment::Center)
            .into()
        }
        CallStatus::Ended { reason } => {
            let reason_text = if reason.is_empty() {
                "Call ended".to_string()
            } else {
                reason.clone()
            };
            column![
                text(reason_text)
                    .size(14)
                    .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.7))
                    .center()
                    .width(Fill),
                row![
                    button(text("Done").size(14).color(iced::Color::WHITE).center())
                        .on_press(Message::DismissCallScreen)
                        .padding([10, 20])
                        .style(theme::secondary_button_style),
                    Space::new().width(24),
                    button(
                        text("Start Again")
                            .size(14)
                            .color(iced::Color::WHITE)
                            .center()
                    )
                    .on_press(Message::StartCall)
                    .padding([10, 20])
                    .style(theme::call_accept_button_style),
                ]
                .align_y(Alignment::Center),
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .width(Fill)
            .into()
        }
    };

    content = content.push(center(controls).width(Fill));
    content = content.push(Space::new().height(20));

    container(content)
        .width(Fill)
        .height(Fill)
        .padding([20, 24])
        .style(theme::call_screen_bg_style)
        .into()
}
