//! Monochrome line icons, tinted at render time.

use gpui::{px, svg, IntoElement, Styled as _};

/// The icon called `name`, `size` pixels square, tinted `color`.
///
/// Icons are SVGs the host application serves from its asset source as
/// `icons/<name>.svg`; the kit only names them. Their fill is the text
/// colour, which is why a single set works in both themes.
pub fn icon(name: &str, size: f32, color: u32) -> impl IntoElement {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .text_color(gpui::rgb(color))
}
