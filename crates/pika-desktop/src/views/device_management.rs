use iced::widget::{button, column, container, row, rule, scrollable, text, toggler, Space};
use iced::{Alignment, Element, Fill, Theme};
use pika_core::DeviceInfo;

use crate::theme;
use crate::Message;

/// Device management screen shown in the center pane.
pub fn device_management_view<'a>(
    devices: &'a [DeviceInfo],
    pending_devices: &'a [DeviceInfo],
    auto_add_devices: bool,
) -> Element<'a, Message, Theme> {
    let mut content = column![].spacing(16).padding([24, 32]).width(Fill);

    // ── Header ────────────────────────────────────────────────────────
    content = content.push(text("Devices").size(22).color(theme::TEXT_PRIMARY));

    // ── Auto-add toggle ───────────────────────────────────────────────
    let toggle_row = row![
        column![
            text("Auto-add new devices")
                .size(14)
                .color(theme::TEXT_PRIMARY),
            text("Automatically detect and invite new devices to all groups.")
                .size(12)
                .color(theme::TEXT_SECONDARY),
        ]
        .spacing(4)
        .width(Fill),
        toggler(auto_add_devices).on_toggle(|_| Message::ToggleAutoAddDevices),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    content = content.push(toggle_row);
    content = content.push(rule::horizontal(1));

    // ── Pending devices ──────────────────────────────────────────────
    if !pending_devices.is_empty() {
        let header_row = row![
            text(format!("Pending Devices ({})", pending_devices.len()))
                .size(16)
                .color(theme::TEXT_PRIMARY),
            Space::new().width(Fill),
            button(text("Accept All").size(12).center())
                .on_press(Message::AcceptAllPendingDevices)
                .padding([6, 14])
                .style(theme::primary_button_style),
            button(text("Reject All").size(12).center())
                .on_press(Message::RejectAllPendingDevices)
                .padding([6, 14])
                .style(theme::danger_button_style),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        content = content.push(header_row);

        let pending_list = pending_devices
            .iter()
            .fold(column![].spacing(4), |col, device| {
                col.push(pending_device_row(device))
            });
        content = content.push(pending_list);
        content = content.push(rule::horizontal(1));
    }

    // ── Device list header ────────────────────────────────────────────
    content = content.push(
        text(format!("My Devices ({})", devices.len()))
            .size(16)
            .color(theme::TEXT_PRIMARY),
    );

    // ── Device list ───────────────────────────────────────────────────
    if devices.is_empty() {
        content = content.push(text("No devices found").size(14).color(theme::TEXT_FADED));
    } else {
        let device_list = devices.iter().fold(column![].spacing(4), |col, device| {
            col.push(device_row(device))
        });
        content = content.push(scrollable(device_list).height(Fill).width(Fill));
    }

    // ── Close button ──────────────────────────────────────────────────
    content = content.push(
        container(
            button(text("Close").size(13).color(theme::TEXT_SECONDARY).center())
                .on_press(Message::CloseDeviceManagement)
                .padding([8, 20])
                .style(theme::secondary_button_style),
        )
        .width(Fill)
        .align_x(Alignment::Center),
    );

    container(content)
        .width(Fill)
        .height(Fill)
        .style(theme::surface_style)
        .into()
}

fn pending_device_row<'a>(device: &'a DeviceInfo) -> Element<'a, Message, Theme> {
    let name = format!("Device {}", device.fingerprint);
    let ts = relative_time(device.published_at);
    let fp = device.fingerprint.clone();
    let fp2 = device.fingerprint.clone();

    let row_content = row![
        column![
            text(name).size(14).color(theme::TEXT_PRIMARY),
            text(format!("Published: {ts}"))
                .size(12)
                .color(theme::TEXT_FADED),
        ]
        .spacing(2),
        Space::new().width(Fill),
        button(text("Accept").size(12).center())
            .on_press(Message::AcceptPendingDevice(fp))
            .padding([4, 12])
            .style(theme::primary_button_style),
        button(text("Reject").size(12).center())
            .on_press(Message::RejectPendingDevice(fp2))
            .padding([4, 12])
            .style(theme::danger_button_style),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(row_content).width(Fill).padding([8, 12]).into()
}

fn device_row<'a>(device: &'a DeviceInfo) -> Element<'a, Message, Theme> {
    let name = format!("Device {}", device.fingerprint);
    let ts = relative_time(device.published_at);

    let mut name_row = row![text(name).size(14).color(theme::TEXT_PRIMARY),]
        .spacing(8)
        .align_y(Alignment::Center);

    if device.is_current_device {
        name_row = name_row.push(
            container(
                text("This device")
                    .size(11)
                    .color(iced::Color::WHITE)
                    .center(),
            )
            .padding([2, 8])
            .style(|_: &Theme| container::Style {
                background: Some(iced::Background::Color(theme::ACCENT_BLUE)),
                border: iced::border::rounded(10),
                ..Default::default()
            }),
        );
    }

    let row_content = row![
        column![
            name_row,
            text(format!("Published: {ts}"))
                .size(12)
                .color(theme::TEXT_FADED),
        ]
        .spacing(2),
        Space::new().width(Fill),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    container(row_content).width(Fill).padding([8, 12]).into()
}

fn relative_time(published_at: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let dt = UNIX_EPOCH + Duration::from_secs(published_at as u64);
    let days_ago = std::time::SystemTime::now()
        .duration_since(dt)
        .unwrap_or_default()
        .as_secs()
        / 86400;
    if days_ago == 0 {
        "today".to_string()
    } else if days_ago == 1 {
        "yesterday".to_string()
    } else if days_ago < 30 {
        format!("{days_ago} days ago")
    } else {
        let secs = dt.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        format!("epoch {secs}")
    }
}
