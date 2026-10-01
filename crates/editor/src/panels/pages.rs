//! The Pages panel: Design Mode's list of a document's pages.
//!
//! A page layout editor is navigated by page, so this is the first thing
//! in the dock rather than a palette that happens to be available. Each
//! row is a schematic thumbnail using the same frame geometry as the
//! pasteboard. It shows the page's arrangement as clipped frame bounds.

use gpui::{div, px, rgb, Context, IntoElement, Window};

use schist_layout::pasteboard::{pasteboard, Display, PasteboardView};
use schist_layout::Page;

use super::*;

/// The width a page thumbnail is drawn at, in pixels.
///
/// Small enough for a page to be recognisable and a spread to fit beside
/// it, large enough that a reader can see whether a frame is square.
const THUMB_WIDTH: f32 = 64.0;

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
    let label = schist_i18n::tf!("design.page_count", count = count);
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
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(rgb(palette().text_dim))
                            .child(label),
                    )
                    .child(
                        IconButton::new("design-add-page", "plus")
                            .tooltip(t("design.add_page"), None)
                            .on_click(cx.listener(move |ws, _, window, cx| {
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
                            })),
                    )
                    .child(
                        IconButton::new("design-remove-page", "trash")
                            .tooltip(t("design.remove_page"), None)
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
            .child(div().flex().flex_col().gap_1().children(rows))
            .child(super::design_dock::section(
                ws,
                "page-setup",
                "design.page_options",
                vec![setup],
                cx,
            ))
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
        ("design-prop-bleed-top", "design.bleed_top_all"),
        ("design-prop-bleed-bottom", "design.bleed_bottom_all"),
        ("design-prop-bleed-inside", "design.bleed_inside_all"),
        ("design-prop-bleed-outside", "design.bleed_outside_all"),
        ("design-prop-slug-top", "design.slug_top_all"),
        ("design-prop-slug-bottom", "design.slug_bottom_all"),
        ("design-prop-slug-inside", "design.slug_inside_all"),
        ("design-prop-slug-outside", "design.slug_outside_all"),
        ("design-prop-margin-top", "design.margin_top"),
        ("design-prop-margin-right", "design.margin_right"),
        ("design-prop-margin-bottom", "design.margin_bottom"),
        ("design-prop-margin-left", "design.margin_left"),
    ]
    .into_iter()
    .map(|(id, label)| {
        let pages = if id.starts_with("design-prop-bleed-") || id.starts_with("design-prop-slug-") {
            (0..ws.design.document.pages.len()).collect()
        } else {
            vec![current]
        };
        let value = format!(
            "{:.2}",
            page_property(id)
                .unwrap()
                .value_for(&ws.design.document, current)
        );
        super::design_controls::field(ws, id, label, value, Target::Pages(pages), cx)
    })
    .collect::<Vec<_>>();
    let (section_page, section) = ws.design.document.section_at(current);
    let numbering = [
        ("1, 2, 3", schist_layout::NumberStyle::Arabic),
        ("i, ii, iii", schist_layout::NumberStyle::RomanLower),
        ("I, II, III", schist_layout::NumberStyle::RomanUpper),
        ("a, b, c", schist_layout::NumberStyle::AlphaLower),
        ("A, B, C", schist_layout::NumberStyle::AlphaUpper),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (label, style))| {
        Button::new(("design-number-style", index), label).on_click(cx.listener(
            move |ws, _, _, cx| {
                ws.commit_focused_field();
                schist_layout::numbering::edit_section(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    section_page,
                    |section| section.style = style,
                );
                cx.notify();
            },
        ))
    })
    .collect::<Vec<_>>();
    let fields = [
        (
            "design-prop-number-start",
            "design.number_start",
            section.start.to_string(),
        ),
        (
            "design-prop-number-prefix",
            "design.number_prefix",
            section.prefix.clone(),
        ),
        (
            "design-prop-section-name",
            "design.section_name",
            section.name.clone(),
        ),
        (
            "design-prop-section-marker",
            "design.section_marker",
            section.marker.clone(),
        ),
    ]
    .into_iter()
    .map(|(id, label, value)| {
        super::design_controls::field(ws, id, label, value, Target::Section(section_page), cx)
    })
    .collect::<Vec<_>>();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(t("design.page_setup_points")))
        .children(rows)
        .child(div().text_xs().child(t("design.section_numbering")))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("design-section-new", t("design.section_start_here"))
                        .disabled(current == section_page)
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.commit_focused_field();
                            let (_, mut section) = ws.design.document.section_at(current);
                            section.continue_numbering = true;
                            schist_layout::numbering::set_section(
                                &mut ws.design.document,
                                &mut ws.design.history,
                                current,
                                Some(section),
                            );
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("design-section-remove", t("design.section_remove"))
                        .disabled(current == 0 || page.section.is_none())
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.commit_focused_field();
                            schist_layout::numbering::set_section(
                                &mut ws.design.document,
                                &mut ws.design.history,
                                current,
                                None,
                            );
                            cx.notify();
                        })),
                ),
        )
        .children(fields)
        .child(
            Button::new(
                "design-section-continue",
                t(if section.continue_numbering {
                    "design.section_restart"
                } else {
                    "design.section_continue"
                }),
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.commit_focused_field();
                schist_layout::numbering::edit_section(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    section_page,
                    |section| section.continue_numbering = !section.continue_numbering,
                );
                cx.notify();
            })),
        )
        .child(
            Button::new(
                "design-section-prefix",
                t(if section.include_prefix {
                    "design.section_hide_prefix"
                } else {
                    "design.section_show_prefix"
                }),
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.commit_focused_field();
                schist_layout::numbering::edit_section(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    section_page,
                    |section| section.include_prefix = !section.include_prefix,
                );
                cx.notify();
            })),
        )
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
            row =
                row.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(div().text_sm().child(number.clone()))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    IconButton::new(
                                        ("page-hidden", index),
                                        if page.hidden { "eye-off" } else { "eye" },
                                    )
                                    .consume_press()
                                    .tooltip(
                                        t(if page.hidden {
                                            "design.show"
                                        } else {
                                            "design.hide"
                                        }),
                                        None,
                                    )
                                    .on_click(cx.listener(
                                        move |ws, _, _, cx| {
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
                                        },
                                    )),
                                )
                                .child(
                                    Button::bare(("page-parent", index))
                                        .child(div().min_w_0().truncate().text_xs().child(
                                            parent.map(|p| p.name.clone()).unwrap_or_else(|| {
                                                t("design.no_parent").to_string()
                                            }),
                                        ))
                                        .min_w_0()
                                        .flex_1()
                                        .consume_press()
                                        .ghost()
                                        .tooltip(t("design.change_parent"), None)
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
/// A paper rectangle with page-local frame bounds. The thumbnail represents
/// artwork schematically instead of rasterizing its text or graphics.
fn thumbnail(
    plan: Option<schist_layout::pasteboard::Pasteboard>,
    width: f32,
    height: f32,
) -> impl IntoElement {
    let plan = plan.and_then(|p| p.pages.into_iter().next());
    let frames = plan.as_ref().map(thumbnail_frames).unwrap_or_default();
    // Parent and page-owned frames use the same schematic outline.
    div()
        .relative()
        .overflow_hidden()
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

fn thumbnail_frames(page: &schist_layout::pasteboard::PagePlan) -> Vec<schist_layout::Rect> {
    page.objects
        .iter()
        .filter_map(|object| match object {
            // Every object has one interaction frame. Empty text frames also
            // have a separate paint outline, which must not be drawn twice.
            Display::Frame {
                rect, transform, ..
            } => {
                let rect = schist_layout::affine::bounds(*transform, *rect);
                Some(schist_layout::Rect::new(
                    rect.x - page.page.trim.x,
                    rect.y - page.page.trim.y,
                    rect.width,
                    rect.height,
                ))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::{blank_a4, LayoutDocument, Page};

    #[test]
    fn thumbnail_frames_are_page_local_for_every_page_origin_and_zoom() {
        let mut doc = blank_a4();
        let mut history = schist_layout::History::default();
        for index in 0..4 {
            if index > 0 {
                let page = doc.pages[0].clone();
                assert!(schist_layout::structure::add_page(
                    &mut doc,
                    &mut history,
                    index - 1,
                    page
                ));
            }
            schist_layout::authoring::text_frame(
                &mut doc,
                &mut history,
                index,
                schist_layout::Rect::new(16.0, 32.0, 128.0, 64.0),
            )
            .unwrap();
        }
        for page in 0..doc.pages.len() {
            for scale in [0.125, 0.5, 1.0, 2.0] {
                for origin in [
                    schist_layout::Point::ZERO,
                    schist_layout::Point::new(-512.0, 128.0),
                ] {
                    let plan = pasteboard(
                        &doc,
                        &PasteboardView {
                            page: Some(page),
                            scale,
                            origin,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let frames = thumbnail_frames(&plan.pages[0]);
                    assert_eq!(frames.len(), 1);
                    let expected = [16.0 * scale, 32.0 * scale, 128.0 * scale, 64.0 * scale];
                    let rect = frames[0];
                    for (actual, expected) in [rect.x, rect.y, rect.width, rect.height]
                        .into_iter()
                        .zip(expected)
                    {
                        assert!(
                            (actual - expected).abs() < 0.001,
                            "page {page}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }

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
