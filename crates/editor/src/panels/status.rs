//! The status bar along the bottom of the window.

use super::*;
use schist_i18n::{t, tf};

pub fn support_link(cx: &mut Context<Workspace>) -> impl IntoElement {
    div()
        .id("support-schist")
        .flex()
        .items_center()
        .gap_1()
        .flex_none()
        .cursor_pointer()
        .text_size(px(ui::metrics().small_text))
        .text_color(gpui::rgb(palette().accent))
        .hover(|style| style.text_color(gpui::rgb(palette().accent_hover)))
        // The browser fonts do not contain emoji. Draw the purple heart
        // as an icon so the support control looks the same on every target.
        .child(schist_ui::icon("heart", 14.0, 0xA855F7))
        .child(t("dialog.support.title"))
        .on_click(cx.listener(|ws, _, _, cx| ws.open_modal(Modal::Support, cx)))
}

pub fn status_bar(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let title = if ws.design_mode() {
        super::design_document_name(ws)
    } else {
        ws.doc
            .as_ref()
            .map(|d| {
                if d.dirty {
                    tf!(
                        "panel.status.document_dirty",
                        title = d.title,
                        w = d.width,
                        h = d.height
                    )
                } else {
                    tf!(
                        "panel.status.document",
                        title = d.title,
                        w = d.width,
                        h = d.height
                    )
                }
            })
            .unwrap_or_else(|| t("common.no_document").to_string())
    };
    let zoom = format!("{:.0}%", ws.viewport_zoom() * 100.0);
    let brush = format!("{:.0}px", ws.editor.brush_size);
    let m = ui::metrics();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .h(px(m.status_h))
        .flex_none()
        .px_2()
        .bg(gpui::rgb(palette().status_bg))
        .border_t_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .text_size(px(m.small_text))
        .text_color(gpui::rgb(palette().text_dim))
        .child(div().min_w(px(0.0)).truncate().child(title))
        .child(zoom)
        .when(!ws.design_mode(), |d| d.child(brush))
        .child(if ws.action_recorder.recording {
            t("actions.recording_indicator")
        } else {
            ""
        })
        .child(div().flex_grow())
        .children(tool_job_progress(ws, cx))
        .child(div().min_w(px(0.0)).truncate().child(ws.status.clone()))
        .when(
            ws.tool_jobs.missing_model && ws.tool_jobs.progress().is_none(),
            |d| {
                d.child(
                    Link::new("tool-job-models", t("menu.filter.manage_models"))
                        .flex_none()
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.tool_jobs.missing_model = false;
                            ws.open_modal(Modal::ModelManager, cx);
                        })),
                )
            },
        )
        .child(support_link(cx))
}

/// A running tool edit (a removal): what it is, how far along, and a way
/// to stop it. Painting carries on while it runs, so this is the only
/// place it shows.
fn tool_job_progress(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<impl IntoElement> {
    let (name, fraction) = ws.tool_jobs.progress()?;
    Some(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .flex_none()
            .child(schist_ui::Spinner::new("tool-job-spinner").size(12.0))
            .child(tf!(
                "workspace.tool_jobs.progress",
                name = name,
                percent = (fraction * 100.0).round() as u32
            ))
            .child(schist_ui::ProgressBar::new(fraction).w(px(80.0)).h(px(4.0)))
            .child(
                Link::new("tool-job-cancel", t("common.cancel")).on_click(cx.listener(
                    |ws, _, _, cx| {
                        ws.cancel_tool_jobs(cx);
                    },
                )),
            ),
    )
}

// ===== context menus =====
