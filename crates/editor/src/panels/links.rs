//! The Links panel: the files behind the placed graphics, and which of
//! them have gone missing.
//!
//! A placed graphic is a link and a frame, and the link is the half that
//! rots. The frame stays on the page looking exactly as it did, the pixels
//! are gone, and a prepress check is the first place a user finds out — at
//! the worst possible moment, which is on the plate.
//!
//! So the links are listed with their state, and the missing ones say so.
//! A missing file is not an error to hide behind a placeholder icon: it is
//! the single most important thing this panel has to communicate.

use gpui::{div, px, rgb, Context, IntoElement};

use schist_layout::{Link, ObjectId};

use super::*;

/// The links panel.
///
/// `None` when there is no document, or when it places nothing. A document
/// of only text has no links, and a panel saying so is just clutter.
pub(super) fn links_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() {
        return None;
    }
    let rows = link_rows(ws, cx);
    let missing = ws
        .design
        .document
        .objects
        .iter()
        .filter(|placed| {
            matches!(&placed.object, schist_layout::LayoutObject::GraphicFrame { link, .. } if ws.design.graphics.sources.get(&link.path).is_some_and(Result::is_err))
        })
        .count();
    if rows.is_empty() {
        return None;
    }
    let mut header = div()
        .flex()
        .justify_between()
        .text_xs()
        .text_color(rgb(palette().text_dim))
        .child(schist_i18n::tn!("design.link_count", rows.len() as u64));
    if missing > 0 {
        // In the warning colour, because a missing link is a problem the
        // user has to act on and not a fact to file away.
        header = header.child(
            div().text_color(rgb(palette().warning)).child(schist_i18n::tn!(
                "design.unavailable_link_count",
                missing as u64
            )),
        );
    }
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
            .child(header)
            .child(Button::new("design-links-refresh", t("design.refresh_links"))
                .disabled(ws.design.graphics_busy)
                .on_click(cx.listener(|ws, _, _, cx| ws.refresh_design_graphics(cx))))
            .child(div().flex().flex_col().gap_1().children(rows))
            .into_any_element(),
    )
}

/// One row per placed graphic: its file and what state that file is in.
fn link_rows(ws: &Workspace, cx: &mut Context<Workspace>) -> Vec<gpui::AnyElement> {
    let document = &ws.design.document;
    let selected = &ws.design.selection;
    document
        .objects
        .iter()
        .filter_map(|placed| {
            let schist_layout::LayoutObject::GraphicFrame { link, embedded, .. } =
                &placed.object
            else {
                return None;
            };
            // The link knows whether the file is there; whether it is
            // embedded lives on the frame, because a link and the frame
            // holding it are different things.
            Some((placed.id, placed.page, link, *embedded))
        })
        .map(|(object, page, link, embedded)| {
            let is_selected = selected.contains(&object);
            let decoded = ws.design.graphics.sources.get(&link.path);
            let missing = decoded.is_some_and(Result::is_err) || (decoded.is_none() && !link.present);
            let error = decoded.and_then(|result| result.as_ref().err()).cloned();
            let name = file_name_of(link);
            let mut row = div()
                .id(("link-row", object.0))
                .flex()
                .flex_col()
                .p_1()
                .rounded_sm()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .child(name)
                                // The file name is the long part and the
                                // panel is narrow, so it is cut rather than
                                // allowed to push the state off the row.
                                .overflow_hidden()
                                .text_ellipsis(),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(if !missing {
                                    palette().text_dim
                                } else {
                                    palette().warning
                                }))
                                .child(if missing {
                                    t("design.unavailable_link").to_string()
                                } else if embedded {
                                    t("design.embedded").to_string()
                                } else {
                                    t("design.linked").to_string()
                                }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette().text_dim))
                        .child(schist_i18n::tn!("design.link_on_page", (page + 1) as u64)),
                );
            row = row.child(Button::new(("design-relink", object.0), t("design.relink"))
                .disabled(ws.design.graphics_busy || document.object_locked(object))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.pick_design_graphic(crate::workspace::design_graphics::Destination::Relink(object), cx);
                    cx.stop_propagation();
                })));
            if let Some(error) = error { row = row.child(div().text_xs().text_color(rgb(palette().warning)).child(error)); }
            if is_selected {
                row = row.bg(rgb(palette().selection_bg));
            }
            row.hover(move |s| {
                if is_selected {
                    s
                } else {
                    s.bg(rgb(palette().hover))
                }
            })
            .on_click(cx.listener(move |ws, _ev, _window, cx| {
                ws.select_link(object, page, cx);
            }))
            .into_any_element()
        })
        .collect()
}

/// The last component of a link's path.
///
/// The whole path would overflow a narrow panel, and the part that
/// distinguishes one file from another is the file name, not the folder it
/// sits in.
fn file_name_of(link: &Link) -> String {
    link.path
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(&link.path)
        .to_string()
}

impl Workspace {
    /// Select the frame behind a link and bring its page into view.
    pub fn select_link(&mut self, object: ObjectId, page: usize, cx: &mut Context<Self>) {
        self.commit_focused_field();
        self.design.typing = None;
        self.design.selection = vec![object];
        if self.design.current_page() != page {
            self.design.page = Some(page);
            self.design.view.page = Some(page);
            self.refit_design = true;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(path: &str) -> Link {
        Link::new(path)
    }

    #[test]
    fn a_file_is_named_by_the_last_part_of_its_path() {
        assert_eq!(file_name_of(&link("/a/b/photo.png")), "photo.png");
        assert_eq!(file_name_of(&link("C:\\work\\photo.png")), "photo.png");
    }

    #[test]
    fn a_bare_name_survives_being_treated_as_a_path() {
        assert_eq!(file_name_of(&link("photo.png")), "photo.png");
    }

    #[test]
    fn a_trailing_separator_does_not_make_the_name_empty() {
        // `rsplit` on a path with a trailing slash ends with an empty
        // component, and a row labelled with nothing is worse than one
        // showing the folder.
        assert_eq!(file_name_of(&link("/a/b/photo.png/")), "photo.png");
        assert_eq!(file_name_of(&link("/")), "/");
    }
}
