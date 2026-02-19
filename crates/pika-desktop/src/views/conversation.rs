use iced::widget::{button, column, container, row, rule, scrollable, text, text_input};
use iced::{Alignment, Element, Fill, Theme};
use pika_core::ChatViewState;

use crate::theme;
use crate::views::avatar::avatar_circle;
use crate::views::message_bubble::message_bubble;
use crate::Message;

/// Center pane: conversation header + message list + input bar.
pub fn conversation_view<'a>(
    chat: &'a ChatViewState,
    message_input: &str,
    avatar_cache: &mut super::avatar::AvatarCache,
) -> Element<'a, Message, Theme> {
    // ── Header bar ──────────────────────────────────────────────────
    let title = chat_title(chat);
    let subtitle = if chat.is_group {
        format!("{} members", chat.members.len())
    } else {
        String::new()
    };

    let mut header_info = column![text(title.clone()).size(16).color(theme::TEXT_PRIMARY),];
    if !subtitle.is_empty() {
        header_info = header_info.push(text(subtitle).size(12).color(theme::TEXT_SECONDARY));
    }

    let picture_url = chat.members.first().and_then(|m| m.picture_url.as_deref());

    let header_content = row![
        avatar_circle(Some(&*title), picture_url, 36.0, avatar_cache),
        header_info,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .padding([8, 16]);

    // Make group headers clickable to show group info
    let header: Element<'a, Message, Theme> = if chat.is_group {
        container(
            button(header_content)
                .on_press(Message::ShowGroupInfo)
                .width(Fill)
                .style(|_: &Theme, status: button::Status| {
                    let bg = match status {
                        button::Status::Hovered => theme::HOVER_BG,
                        _ => theme::RAIL_BG,
                    };
                    button::Style {
                        background: Some(iced::Background::Color(bg)),
                        text_color: theme::TEXT_PRIMARY,
                        border: iced::border::rounded(0),
                        ..Default::default()
                    }
                }),
        )
        .width(Fill)
        .into()
    } else {
        container(header_content)
            .width(Fill)
            .style(theme::header_bar_style)
            .into()
    };

    // ── Messages ────────────────────────────────────────────────────
    let is_group = chat.is_group;
    let messages = chat
        .messages
        .iter()
        .fold(column![].spacing(6).padding([8, 16]), |col, msg| {
            col.push(message_bubble(msg, is_group))
        });

    let message_scroll = scrollable(messages)
        .anchor_bottom()
        .height(Fill)
        .width(Fill);

    // ── Input bar ───────────────────────────────────────────────────
    let send_enabled = !message_input.trim().is_empty();

    let send_button = if send_enabled {
        button(text("Send").size(14).center())
            .on_press(Message::SendMessage)
            .padding([8, 16])
            .style(theme::primary_button_style)
    } else {
        button(text("Send").size(14).color(theme::TEXT_FADED).center())
            .padding([8, 16])
            .style(theme::secondary_button_style)
    };

    let input_bar = container(
        row![
            text_input("Message\u{2026}", message_input)
                .on_input(Message::MessageChanged)
                .on_submit(Message::SendMessage)
                .padding(10)
                .width(Fill)
                .style(theme::dark_input_style),
            send_button,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([8, 16]),
    )
    .width(Fill)
    .style(theme::input_bar_style);

    // ── Typing indicator ─────────────────────────────────────────────
    let typing_indicator: Option<Element<'a, Message, Theme>> = if !chat.typing_members.is_empty() {
        let label = match chat.typing_members.len() {
            1 => {
                let name = chat.typing_members[0].name.as_deref().unwrap_or("Someone");
                format!("{name} is typing\u{2026}")
            }
            2 => {
                let a = chat.typing_members[0].name.as_deref().unwrap_or("Someone");
                let b = chat.typing_members[1].name.as_deref().unwrap_or("Someone");
                format!("{a} and {b} are typing\u{2026}")
            }
            n => {
                let first = chat.typing_members[0].name.as_deref().unwrap_or("Someone");
                format!("{first} and {} others are typing\u{2026}", n - 1)
            }
        };
        Some(
            container(text(label).size(12).color(theme::TEXT_SECONDARY))
                .padding([4, 16])
                .into(),
        )
    } else {
        None
    };

    // ── Compose ─────────────────────────────────────────────────────
    let mut layout = column![header, rule::horizontal(1), message_scroll,]
        .width(Fill)
        .height(Fill);

    if let Some(indicator) = typing_indicator {
        layout = layout.push(indicator);
    }

    layout.push(input_bar).into()
}

fn chat_title(chat: &ChatViewState) -> String {
    if let Some(name) = &chat.group_name {
        if !name.trim().is_empty() {
            return name.clone();
        }
    }
    chat.members
        .first()
        .and_then(|m| m.name.clone())
        .unwrap_or_else(|| "Conversation".to_string())
}
