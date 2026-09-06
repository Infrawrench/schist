//! A colour well.

use crate::{palette, ClickHandler};
use gpui::{
    div, px, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, Refineable as _,
    RenderOnce, StatefulInteractiveElement as _, StyleRefinement, Styled, Window,
};

/// A square of one colour that opens a picker, or takes the colour,
/// when clicked. 18 px with a hairline edge that brightens under the
/// pointer; the modifiers held are on the event for "alt sets the
/// background" conventions.
#[derive(gpui::IntoElement)]
pub struct Swatch {
    id: ElementId,
    color: gpui::Rgba,
    style: StyleRefinement,
    on_click: Option<ClickHandler>,
}

impl Swatch {
    pub fn new(id: impl Into<ElementId>, color: impl Into<gpui::Rgba>) -> Self {
        Swatch {
            id: id.into(),
            color: color.into(),
            style: StyleRefinement::default(),
            on_click: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for Swatch {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Swatch {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut el = div()
            .flex_none()
            .size(px(18.0))
            .rounded_sm()
            .border_1()
            .border_color(gpui::rgb(p.edge))
            .bg(self.color);
        el.style().refine(&self.style);
        let mut el = el.id(self.id);
        if let Some(on_click) = self.on_click {
            el = el
                .cursor_pointer()
                .hover(|s| s.border_color(gpui::rgb(p.text)))
                .on_click(on_click);
        }
        el
    }
}
