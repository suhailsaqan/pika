use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Fill, Theme};
use pika_core::FollowListEntry;

use crate::theme;
use crate::views::avatar::avatar_circle;
use crate::Message;

/// "New Group" screen shown in the center pane.
pub fn new_group_chat_view<'a>(
    follow_list: &'a [FollowListEntry],
    group_name: &str,
    new_chat_input: &str,
    selected_members: &[String],
    creating_chat: bool,
    fetching_follow_list: bool,
    search_query: &str,
    avatar_cache: &mut super::avatar::AvatarCache,
) -> Element<'a, Message, Theme> {
    let mut content = column![].spacing(16).padding([24, 32]).width(Fill);

    // ── Header ──────────────────────────────────────────────────────
    content = content.push(text("New Group").size(22).color(theme::TEXT_PRIMARY));

    // ── Group name ──────────────────────────────────────────────────
    let mut group_name_field = text_input("Group name\u{2026}", group_name)
        .padding(10)
        .style(theme::dark_input_style);
    if !creating_chat {
        group_name_field = group_name_field.on_input(Message::GroupNameChanged);
    }
    content = content.push(group_name_field);

    // ── Selected members chips ──────────────────────────────────────
    if !selected_members.is_empty() {
        let mut chips_row = row![text("Selected:").size(13).color(theme::TEXT_SECONDARY),]
            .spacing(6)
            .align_y(Alignment::Center);

        for npub in selected_members {
            let label = follow_list
                .iter()
                .find(|e| e.npub == *npub)
                .and_then(|e| e.name.clone())
                .unwrap_or_else(|| theme::truncated_npub(npub));
            let npub_clone = npub.clone();
            let mut chip = button(
                text(format!("{label} \u{00d7}"))
                    .size(12)
                    .color(theme::TEXT_PRIMARY),
            )
            .padding([4, 8])
            .style(theme::secondary_button_style);
            if !creating_chat {
                chip = chip.on_press(Message::ToggleGroupMember(npub_clone));
            }
            chips_row = chips_row.push(chip);
        }

        content = content.push(chips_row);
    }

    // ── Manual npub entry ───────────────────────────────────────────
    let mut npub_field = text_input("npub1\u{2026} or hex pubkey", new_chat_input)
        .padding(10)
        .width(Fill)
        .style(theme::dark_input_style);
    if !creating_chat {
        npub_field = npub_field
            .on_input(Message::NewChatChanged)
            .on_submit(Message::AddManualGroupMember);
    }

    let add_btn = button(text("Add").size(14).center())
        .on_press_maybe(if creating_chat || new_chat_input.trim().is_empty() {
            None
        } else {
            Some(Message::AddManualGroupMember)
        })
        .padding([10, 20])
        .style(theme::secondary_button_style);

    let input_row = row![npub_field, add_btn]
        .spacing(8)
        .align_y(Alignment::Center);

    content = content.push(input_row);

    // ── Search bar ──────────────────────────────────────────────────
    let mut search_field = text_input("Search follows\u{2026}", search_query)
        .padding(8)
        .style(theme::dark_input_style);
    if !creating_chat {
        search_field = search_field.on_input(Message::NewChatSearchChanged);
    }
    content = content.push(search_field);

    // ── Follow list ─────────────────────────────────────────────────
    let header_row = row![
        text("Follows").size(14).color(theme::TEXT_SECONDARY),
        Space::new().width(Fill),
        if fetching_follow_list {
            text("Loading\u{2026}").size(12).color(theme::TEXT_FADED)
        } else {
            text(format!("{}", follow_list.len()))
                .size(12)
                .color(theme::TEXT_FADED)
        },
    ]
    .align_y(Alignment::Center);
    content = content.push(header_row);

    if follow_list.is_empty() && !fetching_follow_list {
        content = content.push(
            container(
                text("No follows found")
                    .size(14)
                    .color(theme::TEXT_FADED)
                    .center(),
            )
            .width(Fill)
            .padding([20, 0]),
        );
    } else {
        let list = follow_list.iter().fold(column![].spacing(2), |col, entry| {
            let is_selected = selected_members.contains(&entry.npub);
            col.push(follow_row_selectable(
                entry,
                is_selected,
                creating_chat,
                avatar_cache,
            ))
        });

        content = content.push(scrollable(list).height(Fill).width(Fill));
    }

    // ── Create Group button ─────────────────────────────────────────
    let can_create = !selected_members.is_empty() && !creating_chat;

    let create_btn = if creating_chat {
        button(
            text("Creating\u{2026}")
                .size(14)
                .color(theme::TEXT_FADED)
                .width(Fill)
                .center(),
        )
        .width(Fill)
        .padding([12, 0])
        .style(theme::secondary_button_style)
    } else {
        let mut btn = button(text("Create Group").size(14).width(Fill).center())
            .width(Fill)
            .padding([12, 0])
            .style(theme::primary_button_style);
        if can_create {
            btn = btn.on_press(Message::CreateGroup);
        }
        btn
    };

    content = content.push(create_btn);

    container(content)
        .width(Fill)
        .height(Fill)
        .style(theme::surface_style)
        .into()
}

/// A follow row with a checkbox indicator for multi-select.
fn follow_row_selectable<'a>(
    entry: &'a FollowListEntry,
    is_selected: bool,
    disabled: bool,
    avatar_cache: &mut super::avatar::AvatarCache,
) -> Element<'a, Message, Theme> {
    let name = entry.name.as_deref().unwrap_or("");
    let display_name = if name.is_empty() {
        theme::truncated_npub(&entry.npub)
    } else {
        name.to_string()
    };

    let check = if is_selected { "\u{2611}" } else { "\u{2610}" };

    let avatar: Element<'_, Message, Theme> = avatar_circle(
        Some(&display_name),
        entry.picture_url.as_deref(),
        40.0,
        avatar_cache,
    );

    let mut info = column![text(theme::truncate(&display_name, 30))
        .size(14)
        .color(theme::TEXT_PRIMARY),]
    .spacing(2);

    if !name.is_empty() {
        info = info.push(
            text(theme::truncated_npub(&entry.npub))
                .size(11)
                .color(theme::TEXT_FADED),
        );
    }

    let row_content = row![
        text(check).size(18).color(if is_selected {
            theme::ACCENT_BLUE
        } else {
            theme::TEXT_FADED
        }),
        avatar,
        info,
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let npub = entry.npub.clone();
    let mut btn = button(row_content).width(Fill).padding([8, 12]).style(
        |_: &Theme, status: button::Status| {
            let bg = match status {
                button::Status::Hovered => theme::HOVER_BG,
                _ => iced::Color::TRANSPARENT,
            };
            button::Style {
                background: Some(iced::Background::Color(bg)),
                text_color: theme::TEXT_PRIMARY,
                border: iced::border::rounded(8),
                ..Default::default()
            }
        },
    );

    if !disabled {
        btn = btn.on_press(Message::ToggleGroupMember(npub));
    }

    btn.into()
}
