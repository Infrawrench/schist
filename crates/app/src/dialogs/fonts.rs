//! The missing-fonts prompt.

use super::*;
use schist_i18n::{t, tf};

/// Fonts the open document names that this system doesn't have.
///
/// Substituting silently would keep the file readable while quietly
/// changing every glyph width and line break, so we say what is missing
/// and offer to fetch it. Nothing is requested until a button is pressed.
pub(super) fn missing_fonts(
    ws: &Workspace,
    fonts: &[crate::fonts::MissingFont],
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let busy = ws.font_downloads.clone();
    let rows: Vec<gpui::AnyElement> = fonts
        .iter()
        .map(|font| {
            let family = font.family.clone();
            let target = font.target().to_string();
            let downloading = busy.contains(&family);
            let action: gpui::AnyElement = if downloading {
                div()
                    .text_size(px(11.0))
                    .text_color(gpui::rgb(ui::palette().text_dim))
                    .child("\u{2026}")
                    .into_any_element()
            } else {
                let label = match font.substitute {
                    Some(sub) => tf!("dialog.fonts.install_named", name = sub),
                    None => t("common.download").to_string(),
                };
                let (f, t) = (family.clone(), target.clone());
                ui::button(
                    SharedString::from(label),
                    true,
                    move |ws, _w, cx| ws.download_font(f.clone(), t.clone(), cx),
                    cx,
                )
                .into_any_element()
            };
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    // Fixed rather than flex-grown, like the model rows:
                    // a grown column sizes to its content and clips.
                    div()
                        .flex()
                        .flex_col()
                        .w(px(392.0))
                        .flex_none()
                        .child(
                            div()
                                .text_size(px(12.0))
                                .child(SharedString::from(font.family.clone())),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(gpui::rgb(ui::palette().text_dim))
                                .child(SharedString::from(font.detail())),
                        ),
                )
                .child(action)
                .into_any_element()
        })
        .collect();

    // What the dialog says when it has nothing to offer: no document, or
    // a document whose every font is already here.
    let preamble: SharedString = if ws.doc.is_none() {
        t("dialog.fonts.no_document").into()
    } else if fonts.is_empty() {
        t("dialog.fonts.all_installed").into()
    } else {
        t("dialog.fonts.missing_intro").into()
    };

    let body = div()
        .id("missing-fonts-body")
        .flex()
        .flex_col()
        .gap_1()
        .w_full()
        .max_h(px(360.0))
        .overflow_y_scroll()
        .child(
            div()
                .pb_1()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(preamble),
        )
        .children(rows)
        .when(!fonts.is_empty(), |body| {
            body.child(
                div()
                    .pt_2()
                    .text_size(px(11.0))
                    .text_color(gpui::rgb(ui::palette().text_dim))
                    .child(SharedString::from(tf!(
                        "dialog.fonts.footer",
                        dir = schist_text_engine::font_dir()
                            .map(|d| d.display().to_string())
                            .unwrap_or_else(|| t("dialog.fonts.user_font_dir").into())
                    ))),
            )
        });
    let actions = div().flex().flex_row().gap_2().child(ui::button(
        t("common.close"),
        true,
        |ws, _w, cx| ws.close_modal(cx),
        cx,
    ));
    ui::modal_frame(t("dialog.fonts.title"), 560.0, body, actions)
}
