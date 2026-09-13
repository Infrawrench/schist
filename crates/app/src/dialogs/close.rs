//! Confirming a close with unsaved changes.

use super::*;
use schist_i18n::{t, tf};

/// "Save changes before closing?" for the active tab. Save falls back to
/// the Save As dialog for never-saved documents; the tab then stays open
/// (now clean) rather than chaining a close onto an async file prompt.
pub(super) fn confirm_close_tab(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let title = ws
        .doc
        .as_ref()
        .map(|d| d.title.clone())
        .unwrap_or_else(|| t("common.untitled").into());
    ui::modal_frame(
        t("common.unsaved_changes"),
        380.0,
        div()
            .text_size(px(12.0))
            .child(tf!("common.unsaved_changes_prompt", name = title)),
        div()
            .flex()
            .flex_row()
            .gap_2()
            .child(ui::button(
                t("common.dont_save"),
                false,
                |ws, _window, cx| {
                    ws.close_modal(cx);
                    let index = ws.active_tab();
                    ws.close_tab(index, cx);
                    ws.resume_quit(cx);
                },
                cx,
            ))
            .child(ui::button(
                t("common.cancel"),
                false,
                |ws, _window, cx| {
                    ws.cancel_quit();
                    ws.close_modal(cx);
                },
                cx,
            ))
            .child(ui::button(
                t("dialog.save_ellipsis"),
                true,
                |ws, window, cx| {
                    ws.close_modal(cx);
                    // The Save As prompt is async: it returns with the
                    // document still dirty and finishes later, so the
                    // close has to be pending rather than conditional.
                    // Answering "Save…" used to save an Untitled document
                    // and leave its tab open.
                    ws.close_tab_after_save();
                    ws.save_current(window, cx);
                    if ws.has_pending_save() {
                        if ws
                            .doc
                            .as_ref()
                            .is_some_and(|doc| ws.cloud.docs.contains_key(&doc.id))
                        {
                            // The cloud acknowledgement resumes the pending quit.
                            return;
                        }
                        // Still waiting on a file prompt. Do not hold a
                        // quit open across it; the tab closes when the
                        // save lands.
                        ws.cancel_quit();
                    } else {
                        // Saved synchronously, so the tab has gone.
                        ws.resume_quit(cx);
                    }
                },
                cx,
            )),
    )
}
