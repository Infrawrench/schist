//! A small turning activity indicator.

use crate::palette;
use gpui::{
    percentage, px, Animation, AnimationExt as _, App, ElementId, IntoElement, RenderOnce, Styled,
    Transformation, Window,
};

/// A 14 px ring that turns once every 900 ms while something is on its
/// way. Drawn from `icons/loading.svg`, in the accent unless told
/// otherwise.
#[derive(gpui::IntoElement)]
pub struct Spinner {
    id: ElementId,
    size: f32,
    color: Option<u32>,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Spinner {
            id: id.into(),
            size: 14.0,
            color: None,
        }
    }

    /// The ring's diameter, in pixels.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        gpui::svg()
            .path("icons/loading.svg")
            .size(px(self.size))
            .flex_none()
            .text_color(gpui::rgb(self.color.unwrap_or(palette().accent)))
            .with_animation(
                self.id,
                Animation::new(std::time::Duration::from_millis(900)).repeat(),
                |icon, delta| icon.with_transformation(Transformation::rotate(percentage(delta))),
            )
    }
}
