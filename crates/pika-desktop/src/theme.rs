use iced::widget::{button, container, text_input};
use iced::{border, Background, Border, Color, Theme};

// ── Dark palette ────────────────────────────────────────────────────────────

pub const SURFACE: Color = Color::from_rgb(0.102, 0.102, 0.180); // #1a1a2e
pub const RAIL_BG: Color = Color::from_rgb(0.086, 0.086, 0.165); // #16162a
pub const SENT_BUBBLE: Color = Color::from_rgb(0.0, 0.533, 0.8); // #0088cc
pub const RECEIVED_BUBBLE: Color = Color::from_rgb(0.165, 0.165, 0.243); // #2a2a3e
pub const TEXT_PRIMARY: Color = Color::from_rgb(0.933, 0.933, 0.953); // #eeeeF3
pub const TEXT_SECONDARY: Color = Color::from_rgb(0.600, 0.600, 0.659); // #9999a8
pub const TEXT_FADED: Color = Color::from_rgb(0.400, 0.400, 0.459); // #666675
pub const ACCENT_BLUE: Color = Color::from_rgb(0.204, 0.533, 0.961); // #3488f5
pub const BADGE_BG: Color = Color::from_rgb(0.204, 0.533, 0.961); // #3488f5
pub const AVATAR_BG: Color = Color::from_rgb(0.145, 0.216, 0.365); // #25375d
pub const SELECTION_BG: Color = Color::from_rgb(0.133, 0.133, 0.220); // #222238
pub const HOVER_BG: Color = Color::from_rgb(0.118, 0.118, 0.200); // #1e1e33
pub const INPUT_BG: Color = Color::from_rgb(0.118, 0.118, 0.200); // #1e1e33
pub const INPUT_BORDER: Color = Color::from_rgb(0.200, 0.200, 0.282); // #333348
pub const TOAST_BG: Color = Color::from_rgb(0.204, 0.533, 0.961); // #3488f5
pub const DANGER: Color = Color::from_rgb(0.906, 0.298, 0.235); // #e74c3c

// ── Reusable style functions ────────────────────────────────────────────────

pub fn bubble_sent_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(SENT_BUBBLE)),
        border: border::rounded(12),
        ..Default::default()
    }
}

pub fn bubble_received_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_PRIMARY),
        background: Some(Background::Color(RECEIVED_BUBBLE)),
        border: border::rounded(12),
        ..Default::default()
    }
}

pub fn chat_item_style(is_selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme: &Theme, status: button::Status| {
        let bg = if is_selected {
            SELECTION_BG
        } else {
            match status {
                button::Status::Hovered => HOVER_BG,
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: TEXT_PRIMARY,
            border: border::rounded(8),
            ..Default::default()
        }
    }
}

pub fn primary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Color::from_rgb(0.243, 0.573, 1.0),
        button::Status::Disabled => Color::from_rgb(0.15, 0.35, 0.65),
        _ => ACCENT_BLUE,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: Color::WHITE,
        border: border::rounded(6),
        ..Default::default()
    }
}

pub fn secondary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => HOVER_BG,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: TEXT_SECONDARY,
        border: Border {
            color: INPUT_BORDER,
            width: 1.0,
            radius: border::radius(6),
        },
        ..Default::default()
    }
}

pub fn danger_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Color::from_rgb(0.85, 0.25, 0.20),
        _ => DANGER,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: Color::WHITE,
        border: border::rounded(6),
        ..Default::default()
    }
}

pub fn dark_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => ACCENT_BLUE,
        text_input::Status::Hovered => TEXT_FADED,
        _ => INPUT_BORDER,
    };
    text_input::Style {
        background: Background::Color(INPUT_BG),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: border::radius(6),
        },
        icon: TEXT_FADED,
        placeholder: TEXT_FADED,
        value: TEXT_PRIMARY,
        selection: ACCENT_BLUE.scale_alpha(0.3),
    }
}

pub fn avatar_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(AVATAR_BG)),
        border: border::rounded(100),
        ..Default::default()
    }
}

pub fn rail_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(RAIL_BG)),
        ..Default::default()
    }
}

pub fn surface_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        ..Default::default()
    }
}

pub fn toast_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(TOAST_BG)),
        border: border::rounded(6),
        ..Default::default()
    }
}

pub fn login_card_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_PRIMARY),
        background: Some(Background::Color(RAIL_BG)),
        border: Border {
            color: INPUT_BORDER,
            width: 1.0,
            radius: border::radius(12),
        },
        ..Default::default()
    }
}

pub fn header_bar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(RAIL_BG)),
        ..Default::default()
    }
}

pub fn input_bar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(RAIL_BG)),
        border: Border {
            color: INPUT_BORDER,
            width: 0.0,
            radius: border::radius(0),
        },
        ..Default::default()
    }
}

pub fn badge_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(BADGE_BG)),
        border: border::rounded(100),
        ..Default::default()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

pub fn relative_time(unix_secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let diff = now - unix_secs;

    if diff < 60 {
        "now".to_string()
    } else if diff < 3600 {
        format!("{}m", diff / 60)
    } else if diff < 86400 {
        format!("{}h", diff / 3600)
    } else if diff < 604800 {
        format!("{}d", diff / 86400)
    } else {
        // Show abbreviated month + day for older
        let secs = unix_secs as u64;
        // Simple month/day from unix timestamp
        let days_since_epoch = secs / 86400;
        let (year, month, day) = days_to_ymd(days_since_epoch);
        let _ = year;
        let month_name = match month {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            _ => "Dec",
        };
        format!("{month_name} {day}")
    }
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Civil days algorithm (simplified)
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    }
}

pub fn truncated_npub(npub: &str) -> String {
    if npub.len() <= 20 {
        return npub.to_string();
    }
    format!("{}\u{2026}{}", &npub[..12], &npub[npub.len() - 4..])
}

pub fn truncated_npub_long(npub: &str) -> String {
    if npub.len() <= 30 {
        return npub.to_string();
    }
    format!("{}\u{2026}{}", &npub[..16], &npub[npub.len() - 8..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_adds_ellipsis() {
        assert_eq!(truncate("hello world", 6), "hello\u{2026}");
    }

    #[test]
    fn truncated_npub_short_unchanged() {
        assert_eq!(truncated_npub("npub1abcd"), "npub1abcd");
    }

    #[test]
    fn truncated_npub_long_is_compact() {
        assert_eq!(
            truncated_npub("npub1abcdefghijklmnopqrstu"),
            "npub1abcdefg\u{2026}rstu"
        );
    }

    #[test]
    fn truncated_npub_long_variant_is_compact() {
        assert_eq!(
            truncated_npub_long("npub1abcdefghijklmnopqrstuvwxyz123456"),
            "npub1abcdefghijk\u{2026}yz123456"
        );
    }

    #[test]
    fn relative_time_recent() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert_eq!(relative_time(now), "now");
        assert_eq!(relative_time(now - 120), "2m");
        assert_eq!(relative_time(now - 7200), "2h");
        assert_eq!(relative_time(now - 172800), "2d");
    }
}
