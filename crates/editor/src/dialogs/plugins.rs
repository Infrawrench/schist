//! The plug-in manager, including the Photoshop plug-in section.

use super::*;
use schist_i18n::{t, tf};

/// The third-party plugin manager: what loaded, what didn't and why, and
/// per-plugin enable/disable.
pub(super) fn plugin_manager(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let dir = schist_plugin_host_wasm::PluginManager::plugin_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| t("dialog.no_config_dir").into());
    let rows: Vec<gpui::AnyElement> = ws
        .plugins
        .entries
        .iter()
        .map(|entry| {
            let id = entry.id.clone();
            let enabled = entry.enabled;
            let kind = t(match &entry.kind {
                Some(schist_plugin_host_wasm::abi::PluginKind::Filter) => {
                    "dialog.plugins.kind_filter"
                }
                Some(schist_plugin_host_wasm::abi::PluginKind::Codec) => {
                    "dialog.plugins.kind_format"
                }
                None => "dialog.plugins.kind_unavailable",
            });
            let detail = match &entry.error {
                Some(err) => err.to_string(),
                None => tf!("dialog.plugins.detail", kind = kind, id = entry.id),
            };
            let failed = entry.error.is_some();
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(12.0)).child(entry.name.clone()))
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(gpui::rgb(if failed {
                                    0xD08770
                                } else {
                                    ui::palette().text_dim
                                }))
                                .child(detail),
                        ),
                )
                .child(if failed {
                    div().into_any_element()
                } else {
                    ui::checkbox(
                        if enabled {
                            t("common.enabled")
                        } else {
                            t("common.disabled")
                        },
                        enabled,
                        move |ws, _cx| {
                            let id = id.clone();
                            ws.pending_plugin_toggle = Some((id, !enabled));
                        },
                        cx,
                    )
                    .into_any_element()
                })
                .into_any_element()
        })
        .collect();

    let body = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(tf!("dialog.plugins.load_from", dir = dir)),
        )
        .when(rows.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(12.0))
                    .py_2()
                    .child(t("dialog.plugins.none")),
            )
        })
        .children(rows)
        .children(photoshop_section(ws, cx));

    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("dialog.plugins.install_ellipsis"),
            false,
            crate::keymap::install_plugin_dialog,
            cx,
        ))
        .child(ui::button(
            t("common.close"),
            true,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ));
    ui::modal_frame(t("dialog.plugins.title"), 420.0, body, actions)
}

/// The Photoshop plug-in half of the manager.
///
/// Everything discovered is listed, including what this machine cannot
/// run: a Windows filter with no Wine installed is the case people
/// actually hit, and the reason is more use than an absence. Only the
/// runnable ones carry a switch, because disabling something that was
/// never offered would say nothing.
pub(super) fn photoshop_section(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let manager = &ws.photoshop_plugins;
    if manager.entries.is_empty() {
        return Vec::new();
    }
    let folders = manager
        .dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let mut out: Vec<gpui::AnyElement> = vec![div()
        .flex()
        .flex_col()
        .pt_2()
        .child(
            div()
                .text_size(px(12.0))
                .child(t("dialog.plugins.photoshop_heading")),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(tf!("dialog.plugins.loaded_from", folders = folders)),
        )
        .into_any_element()];

    for entry in &manager.entries {
        let id = entry.id.clone();
        let enabled = entry.enabled;
        let detail = match &entry.blocker {
            Some(why) => tf!(
                "dialog.plugins.ps_detail",
                architecture = entry.architecture,
                detail = why
            ),
            None => tf!(
                "dialog.plugins.ps_detail",
                architecture = entry.architecture,
                detail = entry.id
            ),
        };
        let blocked = entry.blocker.is_some();
        out.push(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        // Bounded so a reason wraps inside the dialog: the
                        // one that matters most names a missing helper by
                        // its full path, which is wider than the modal.
                        .w(px(290.0))
                        .child(div().text_size(px(12.0)).child(entry.name.clone()))
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(gpui::rgb(if blocked {
                                    0xD08770
                                } else {
                                    ui::palette().text_dim
                                }))
                                .child(detail),
                        ),
                )
                .child(if blocked {
                    div().into_any_element()
                } else {
                    ui::checkbox(
                        if enabled {
                            t("common.enabled")
                        } else {
                            t("common.disabled")
                        },
                        enabled,
                        move |ws, _cx| {
                            let id = id.clone();
                            ws.pending_plugin_toggle = Some((id, !enabled));
                        },
                        cx,
                    )
                    .into_any_element()
                })
                .into_any_element(),
        );
    }
    out
}
