//! The Pages panel: Design Mode's list of a document's pages.
//!
//! A page layout editor is navigated by page, so this is the first thing
//! in the dock rather than a palette that happens to be available. Each
//! row is a live thumbnail drawn from the same pasteboard plan the canvas
//! paints, which is the only way a thumbnail can be trusted: a second
//! renderer for the same document is a second answer, and the two would
//! disagree the first time a layout rule changed.

use gpui::{div, px, rgb, Context, IntoElement, Window};

use schist_layout::pasteboard::{pasteboard, Display, PasteboardView};
use schist_layout::Page;

use super::*;

/// The width a page thumbnail is drawn at, in pixels.
///
/// Small enough for a page to be recognisable and a spread to fit beside
/// it, large enough that a reader can see whether a frame is square.
const THUMB_WIDTH: f32 = 96.0;

/// The pages panel.
///
/// `None` when there is no document to list, so a workspace with nothing
/// open does not show an empty dock section.
pub(super) fn pages_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() {
        return None;
    }
    let document = &ws.design.document;
    if document.pages.is_empty() {
        return None;
    }
    let rows = page_rows(ws, cx);
    let current = ws.design.current_page();
    let count = document.pages.len();
    let label = schist_i18n::tn!("design.page_count", count as u64);
    let setup = page_setup(ws, cx);
    Some(
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_h(px(0.0))
            .p_2()
            .gap_1()
            .border_t_1()
            .border_color(rgb(palette().panel_edge))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette().text_dim))
                    .child(label),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new("design-add-page", t("design.add_page")).on_click(cx.listener(
                            move |ws, _, window, cx| {
                                ws.commit_focused_field();
                                ws.design.cancel_gesture();
                                let page = ws.design.document.pages[current].clone();
                                if schist_layout::structure::add_page(
                                    &mut ws.design.document,
                                    &mut ws.design.history,
                                    current,
                                    page,
                                ) {
                                    ws.show_page(current + 1, window, cx);
                                }
                            },
                        )),
                    )
                    .child(
                        Button::new("design-remove-page", t("design.remove_page"))
                            .disabled(count <= 1)
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                ws.commit_focused_field();
                                ws.design.cancel_gesture();
                                if schist_layout::structure::remove_page(
                                    &mut ws.design.document,
                                    &mut ws.design.history,
                                    current,
                                ) {
                                    let page = current.min(ws.design.document.pages.len() - 1);
                                    ws.design.page = Some(page);
                                    ws.design.view.page = Some(page);
                                    ws.design.selection.clear();
                                    ws.design.typing = None;
                                    ws.refit_design = true;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .child(setup)
            .child(div().flex().flex_col().gap_1().children(rows))
            .into_any_element(),
    )
}

fn page_setup(ws: &Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    use crate::design::controls::{page_property, Target};
    let current = ws.design.current_page();
    let page = &ws.design.document.pages[current];
    let rows = [
        ("design-prop-page-width", "design.width"),
        ("design-prop-page-height", "design.height"),
        ("design-prop-bleed", "design.bleed_all"),
        ("design-prop-slug", "design.slug_all"),
        ("design-prop-margin-top", "design.margin_top"),
        ("design-prop-margin-right", "design.margin_right"),
        ("design-prop-margin-bottom", "design.margin_bottom"),
        ("design-prop-margin-left", "design.margin_left"),
    ]
    .into_iter()
    .map(|(id, label)| {
        let pages = if matches!(id, "design-prop-bleed" | "design-prop-slug") {
            (0..ws.design.document.pages.len()).collect()
        } else {
            vec![current]
        };
        let value = format!("{:.2}", page_property(id).unwrap().value(page));
        super::design_controls::field(ws, id, label, value, Target::Pages(pages), cx)
    })
    .collect::<Vec<_>>();
    let numbering = [
        ("1, 2, 3", "Arabic"),
        ("i, ii, iii", "RomanLower"),
        ("I, II, III", "RomanUpper"),
        ("a, b, c", "AlphaLower"),
        ("A, B, C", "AlphaUpper"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (label, style))| {
        Button::new(("design-number-style", index), label).on_click(cx.listener(
            move |ws, _, _, cx| {
                ws.commit_focused_field();
                schist_layout::properties::edit_settings(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    |settings| settings.page_number_style = style.into(),
                );
                cx.notify();
            },
        ))
    })
    .collect::<Vec<_>>();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(t("design.page_setup_points")))
        .children(rows)
        .child(super::design_controls::field(
            ws,
            "design-prop-number-start",
            "design.number_start",
            ws.design.document.page_number_start.to_string(),
            Target::Document,
            cx,
        ))
        .child(super::design_controls::field(
            ws,
            "design-prop-number-prefix",
            "design.number_prefix",
            ws.design.document.page_number_prefix.clone(),
            Target::Document,
            cx,
        ))
        .child(div().flex().flex_wrap().gap_1().children(numbering))
        .into_any_element()
}

/// One row per page, with its number and a thumbnail.
fn page_rows(ws: &Workspace, cx: &mut Context<Workspace>) -> Vec<gpui::AnyElement> {
    let document = &ws.design.document;
    let current = ws.design.current_page();
    document
        .pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let number = document.page_number(index);
            let selected = index == current;
            let plan = page_plan(ws, index);
            let height = thumbnail_height(document.pages.get(index), THUMB_WIDTH);
            let mut row = div()
                .id(("page-row", index))
                .flex()
                .items_center()
                .gap_2()
                .p_1()
                .rounded_sm()
                .child(thumbnail(plan, THUMB_WIDTH, height));
            if selected {
                row = row.bg(rgb(palette().selection_bg));
            }
            let parent = page.master.and_then(|id| document.parents.get(id));
            row = row.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_sm().child(number.clone()))
                    .child(
                        div().text_xs().overflow_hidden().text_ellipsis().child(
                            parent
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| t("design.no_parent").to_string()),
                        ),
                    )
                    .child(
                        Button::new(
                            ("page-hidden", index),
                            t(if page.hidden {
                                "design.show"
                            } else {
                                "design.hide"
                            }),
                        )
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            cx.stop_propagation();
                            ws.commit_focused_field();
                            ws.design.cancel_gesture();
                            schist_layout::structure::toggle_page_hidden(
                                &mut ws.design.document,
                                &mut ws.design.history,
                                index,
                            );
                            ws.refit_design = true;
                            cx.notify();
                        })),
                    )
                    .child(
                        Button::new(("page-parent", index), t("design.change_parent"))
                            .disabled(document.parents.is_empty())
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                cx.stop_propagation();
                                ws.commit_focused_field();
                                ws.design.cancel_gesture();
                                let count = ws.design.document.parents.len();
                                let next = ws.design.document.pages[index]
                                    .master
                                    .map(|i| i + 1)
                                    .or(Some(0))
                                    .filter(|i| *i < count);
                                schist_layout::structure::set_parent(
                                    &mut ws.design.document,
                                    &mut ws.design.history,
                                    index,
                                    next,
                                );
                                cx.notify();
                            })),
                    ),
            );
            row.on_drag(
                PageDrag {
                    index,
                    label: number.clone().into(),
                },
                |drag, _, _, cx| cx.new(|_| PageDragPreview(drag.label.clone())),
            )
            .drag_over::<PageDrag>(|style, _, _, _| {
                style.border_t_2().border_color(rgb(palette().accent))
            })
            .on_drop(cx.listener(move |ws, drag: &PageDrag, window, cx| {
                ws.commit_focused_field();
                ws.design.cancel_gesture();
                if schist_layout::structure::move_page(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    drag.index,
                    index,
                ) {
                    ws.design.page = None;
                    ws.show_page(index, window, cx);
                }
            }))
            .hover(move |s| {
                if selected {
                    s
                } else {
                    s.bg(rgb(palette().hover))
                }
            })
            .on_click(cx.listener(move |ws, _ev, window, cx| {
                ws.show_page(index, window, cx);
            }))
            .into_any_element()
        })
        .collect()
}

/// Show a page, or the spread it is on.
impl Workspace {
    /// Bring a page into view.
    pub fn show_page(&mut self, index: usize, _window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.design.document.pages.len() || self.design.page == Some(index) {
            return;
        }
        self.commit_focused_field();
        self.design.cancel_gesture();
        self.design.page = Some(index);
        // The view's own page follows the panel, so a hit test and the
        // painter can never disagree about which page is showing.
        self.design.view.page = Some(index);
        // The view is fitted to the page it last showed, so it has to be
        // refitted for this one rather than showing a zoom from elsewhere.
        self.refit_design = true;
        self.design.selection.clear();
        self.design.typing = None;
        cx.notify();
    }
}

/// The plan for one page alone, at thumbnail scale.
fn page_plan(ws: &Workspace, index: usize) -> Option<schist_layout::pasteboard::Pasteboard> {
    let page = ws.design.document.pages.get(index)?;
    let view = PasteboardView {
        scale: THUMB_WIDTH / page.width.max(1.0),
        page: Some(index),
        ..PasteboardView::default()
    };
    pasteboard(&ws.design.document, &view)
}

/// A thumbnail's height, which follows the page's aspect ratio.
///
/// A thumbnail that stretched a tall page to a fixed height would make a
/// spread and a single page look the same size, which is the one thing a
/// page list has to get right.
fn thumbnail_height(page: Option<&Page>, fallback: f32) -> f32 {
    let Some(page) = page.filter(|p| p.width > 0.0 && p.height > 0.0) else {
        return fallback;
    };
    THUMB_WIDTH * page.height / page.width
}

/// One page's thumbnail.
///
/// A paper rectangle with the frames on it. Deliberately not a raster
/// render: compositing a page's text for a 96-pixel thumbnail would cost
/// more than the whole canvas, and the point of a thumbnail is the page's
/// arrangement, which boxes convey.
fn thumbnail(
    plan: Option<schist_layout::pasteboard::Pasteboard>,
    width: f32,
    height: f32,
) -> impl IntoElement {
    let plan = plan.and_then(|p| p.pages.into_iter().next());
    let frames: Vec<schist_layout::Rect> = plan
        .as_ref()
        .map(|page| {
            page.objects
                .iter()
                .filter_map(|object| match object {
                    Display::Frame { rect, .. } | Display::EmptyFrame { rect, .. } => Some(*rect),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    // A parent's items and a page's own are drawn the same at this size:
    // the difference is a one-pixel dash nobody can see.
    div()
        .relative()
        .w(px(width))
        .h(px(height))
        .flex_none()
        .border_1()
        .border_color(rgb(palette().edge))
        .bg(rgb(0xFDFDFB))
        .children(frames.into_iter().map(|rect| {
            div()
                .absolute()
                .left(px(rect.x))
                .top(px(rect.y))
                .w(px(rect.width.max(1.0)))
                .h(px(rect.height.max(1.0)))
                .border_1()
                .border_color(rgb(0x4A9FD8))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::{blank_a4, LayoutDocument, Page};

    #[test]
    fn a_thumbnail_keeps_the_page_aspect_ratio() {
        // A4 is taller than wide, so its thumbnail is taller than it is
        // wide, which is how a reader recognises it at a glance.
        let a4 = thumbnail_height(Some(&Page::a4()), THUMB_WIDTH);
        assert!(a4 > THUMB_WIDTH, "{a4} should exceed {THUMB_WIDTH}");
        // And the ratio is the page's own, not a guess.
        let page = Page::a4();
        assert!((a4 - THUMB_WIDTH * page.height / page.width).abs() < 0.01);
    }

    #[test]
    fn a_landscape_page_gets_a_landscape_thumbnail() {
        let mut page = Page::a4();
        page.width = 841.0;
        page.height = 595.0;
        assert!(thumbnail_height(Some(&page), THUMB_WIDTH) < THUMB_WIDTH);
    }

    #[test]
    fn a_degenerate_page_falls_back_instead_of_dividing_by_zero() {
        let mut page = Page::a4();
        page.height = 0.0;
        assert_eq!(thumbnail_height(Some(&page), THUMB_WIDTH), THUMB_WIDTH);
        page.height = 100.0;
        page.width = 0.0;
        assert_eq!(thumbnail_height(Some(&page), THUMB_WIDTH), THUMB_WIDTH);
        // And a page that is not there at all.
        assert_eq!(thumbnail_height(None, THUMB_WIDTH), THUMB_WIDTH);
    }

    #[test]
    fn a_thumbnail_view_scales_the_page_to_the_thumbnail_width() {
        let document: LayoutDocument = blank_a4();
        let page = document.pages[0].clone();
        let view = PasteboardView {
            scale: THUMB_WIDTH / page.width,
            page: Some(0),
            ..PasteboardView::default()
        };
        let plan = pasteboard(&document, &view).expect("a page has a plan");
        assert_eq!(plan.pages.len(), 1, "a thumbnail is one page");
        // The trim box comes out at the thumbnail's width, which is the
        // whole point of the scale.
        assert!((plan.pages[0].page.trim.width - THUMB_WIDTH).abs() < 0.01);
    }
}

#[derive(Clone)]
struct PageDrag {
    index: usize,
    label: SharedString,
}
struct PageDragPreview(SharedString);
impl gpui::Render for PageDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_2()
            .bg(rgb(palette().panel_bg))
            .child(self.0.clone())
    }
}
