//! Inline text that acts when clicked.

use crate::{palette, ClickHandler};
use gpui::{
    div, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Refineable as _, RenderOnce, SharedString, StatefulInteractiveElement as _, StyleRefinement,
    Styled, Window,
};

/// A run of accent-coloured text that opens a URL or runs a handler on
/// release. Inherits its size from the text around it.
#[derive(gpui::IntoElement)]
pub struct Link {
    id: ElementId,
    label: SharedString,
    style: StyleRefinement,
    on_click: Option<ClickHandler>,
}

impl Link {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Link {
            id: id.into(),
            label: label.into(),
            style: StyleRefinement::default(),
            on_click: None,
        }
    }

    /// Open `url` in the user's browser.
    pub fn url(self, url: impl Into<String>) -> Self {
        let url = url.into();
        self.on_click(move |_e, _window, cx| cx.open_url(&url))
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for Link {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Link {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut el = div().text_color(gpui::rgb(p.accent));
        el.style().refine(&self.style);
        let mut el = el
            .id(self.id)
            .cursor_pointer()
            .hover(|s| s.text_color(gpui::rgb(p.accent_hover)));
        if let Some(on_click) = self.on_click {
            el = el.on_click(on_click);
        }
        el.child(self.label)
    }
}
