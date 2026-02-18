use iced::widget::{container, image, text};
use iced::{Alignment, Element, Length, Theme};

use crate::theme;

/// Renders a circular avatar. Shows the profile picture if a local file:// URL
/// is available, otherwise falls back to an initials circle.
pub fn avatar_circle<'a, M: 'a>(
    name: Option<&str>,
    picture_url: Option<&str>,
    size: f32,
) -> Element<'a, M, Theme> {
    // Try to use a cached profile picture (file:// URL from core).
    if let Some(path) = picture_url.and_then(local_path_from_url) {
        if let Some(handle) = load_circular_image(&path, size as u32) {
            return image(handle)
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .into();
        }
    }

    // Fallback: initial letter circle.
    let initial = name
        .and_then(|n| n.trim().chars().next())
        .unwrap_or('?')
        .to_uppercase()
        .to_string();

    container(
        text(initial)
            .size(size * 0.45)
            .color(theme::TEXT_PRIMARY)
            .center(),
    )
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(theme::avatar_container_style)
    .into()
}

/// Load an image, resize it, and apply a circular alpha mask.
fn load_circular_image(path: &str, size: u32) -> Option<image::Handle> {
    let bytes = std::fs::read(path).ok()?;
    let img = ::image::load_from_memory(&bytes).ok()?;
    let img = img.resize_to_fill(size, size, ::image::imageops::FilterType::Lanczos3);
    let mut rgba = img.into_rgba8();

    // Apply circular mask: set alpha to 0 for pixels outside the circle.
    let center = size as f32 / 2.0;
    let radius = center;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center + 0.5;
            let dy = y as f32 - center + 0.5;
            if dx * dx + dy * dy > radius * radius {
                rgba.get_pixel_mut(x, y).0[3] = 0;
            }
        }
    }

    Some(image::Handle::from_rgba(size, size, rgba.into_raw()))
}

/// Extract a local filesystem path from a `file://` URL.
fn local_path_from_url(url: &str) -> Option<String> {
    url.strip_prefix("file://").map(|s| s.to_string())
}
