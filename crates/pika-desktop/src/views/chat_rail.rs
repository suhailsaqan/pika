use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Fill, Length, Theme};
use pika_core::ChatSummary;

use crate::theme;
use crate::views::avatar::avatar_circle;
use crate::Message;

/// Left sidebar containing the chat list, new-chat form, and action buttons.
pub fn chat_rail_view<'a>(
    chat_list: &[ChatSummary],
    selected_id: Option<&str>,
    show_new_chat_form: bool,
    new_chat_input: &str,
    creating_chat: bool,
) -> Element<'a, Message, Theme> {
    // ── Header ──────────────────────────────────────────────────────
    let header = row![
        text("Chats").size(20).color(theme::TEXT_PRIMARY),
        Space::with_width(Fill),
        button(text("+").size(18).color(theme::TEXT_PRIMARY).center())
            .on_press(Message::ToggleNewChatForm)
            .padding([4, 10])
            .style(theme::secondary_button_style),
    ]
    .align_y(Alignment::Center)
    .padding([0, 4]);

    // ── New-chat form (conditional) ─────────────────────────────────
    let new_chat_form: Option<Element<'a, Message, Theme>> = if show_new_chat_form {
        let mut form = column![
            text_input("npub1\u{2026}", new_chat_input)
                .on_input(Message::NewChatChanged)
                .on_submit(Message::StartChat)
                .padding(8)
                .style(theme::dark_input_style),
        ]
        .spacing(6);

        if creating_chat {
            form = form.push(
                text("Creating chat\u{2026}")
                    .size(13)
                    .color(theme::TEXT_FADED)
                    .center()
                    .width(Fill),
            );
        } else {
            form = form.push(
                row![
                    button(text("Start Chat").size(13).center())
                        .on_press(Message::StartChat)
                        .padding([6, 12])
                        .width(Fill)
                        .style(theme::primary_button_style),
                    button(text("Cancel").size(13).center())
                        .on_press(Message::ToggleNewChatForm)
                        .padding([6, 12])
                        .style(theme::secondary_button_style),
                ]
                .spacing(6),
            );
        }

        Some(form.into())
    } else {
        None
    };

    // ── Chat list ───────────────────────────────────────────────────
    let chat_items = chat_list.iter().fold(column![].spacing(2), |col, chat| {
        col.push(chat_item(chat, selected_id))
    });

    // ── Assemble rail ───────────────────────────────────────────────
    let mut rail = column![header].spacing(8).padding(12);

    if let Some(form) = new_chat_form {
        rail = rail.push(form);
    }

    rail = rail.push(scrollable(chat_items).height(Fill));

    // Logout button at bottom
    rail = rail.push(
        button(text("Logout").size(13).color(theme::DANGER).center())
            .on_press(Message::Logout)
            .width(Fill)
            .padding([8, 0])
            .style(|_theme: &Theme, status: button::Status| {
                let bg = match status {
                    button::Status::Hovered => theme::HOVER_BG,
                    _ => iced::Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(iced::Background::Color(bg)),
                    text_color: theme::DANGER,
                    border: iced::border::rounded(6),
                    ..Default::default()
                }
            }),
    );

    container(rail)
        .width(Length::Fixed(280.0))
        .height(Fill)
        .style(theme::rail_container_style)
        .into()
}

/// A single chat list item row.
fn chat_item<'a>(chat: &ChatSummary, selected_id: Option<&str>) -> Element<'a, Message, Theme> {
    let is_selected = selected_id == Some(chat.chat_id.as_str());

    let name = chat_display_name(chat);
    let preview = chat
        .last_message
        .as_deref()
        .unwrap_or("No messages yet");

    let timestamp_text: Element<'a, Message, Theme> = if let Some(ts) = chat.last_message_at {
        text(theme::relative_time(ts))
            .size(11)
            .color(theme::TEXT_FADED)
            .into()
    } else {
        Space::with_width(0).into()
    };

    let picture_url = chat
        .members
        .iter()
        .find(|m| !m.npub.is_empty())
        .and_then(|m| m.picture_url.as_deref());

    let avatar: Element<'a, Message, Theme> =
        avatar_circle(Some(&name), picture_url, 40.0);

    // Name + timestamp row
    let top_row = row![
        text(theme::truncate(&name, 20))
            .size(14)
            .color(theme::TEXT_PRIMARY),
        Space::with_width(Fill),
        timestamp_text,
    ]
    .align_y(Alignment::Center);

    // Preview + optional badge
    let mut bottom_row = row![text(theme::truncate(preview, 28))
        .size(12)
        .color(theme::TEXT_SECONDARY)]
    .align_y(Alignment::Center);

    if chat.unread_count > 0 {
        bottom_row = bottom_row.push(Space::with_width(Fill));
        bottom_row = bottom_row.push(
            container(
                text(chat.unread_count.to_string())
                    .size(11)
                    .color(iced::Color::WHITE)
                    .center(),
            )
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(theme::badge_container_style),
        );
    }

    let content = row![
        avatar,
        column![top_row, bottom_row].spacing(2).width(Fill),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    button(content)
        .on_press(Message::OpenChat(chat.chat_id.clone()))
        .width(Fill)
        .padding([8, 8])
        .style(theme::chat_item_style(is_selected))
        .into()
}

/// Derive a display name for a chat.
fn chat_display_name(chat: &ChatSummary) -> String {
    if chat.is_group {
        if let Some(name) = &chat.group_name {
            if !name.trim().is_empty() {
                return name.clone();
            }
        }
        return "Group".to_string();
    }

    if let Some(member) = chat.members.iter().find(|m| !m.npub.is_empty()) {
        return member
            .name
            .clone()
            .unwrap_or_else(|| short_npub(&member.npub));
    }

    "Direct chat".to_string()
}

fn short_npub(npub: &str) -> String {
    const TAIL: usize = 8;
    if npub.len() <= TAIL {
        return npub.to_string();
    }
    format!("\u{2026}{}", &npub[npub.len() - TAIL..])
}
