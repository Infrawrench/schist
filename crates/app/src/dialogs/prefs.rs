//! Preferences.

use super::*;
use schist_i18n::{t, tf};

/// A rendering intent's name in the user's language.
fn intent_name(intent: schist_colormgmt::Intent) -> &'static str {
    use schist_colormgmt::Intent;
    t(match intent {
        Intent::Perceptual => "dialog.prefs.intent_perceptual",
        Intent::RelativeColorimetric => "dialog.prefs.intent_relative",
        Intent::Saturation => "dialog.prefs.intent_saturation",
        Intent::AbsoluteColorimetric => "dialog.prefs.intent_absolute",
    })
}

/// Application preferences (⌘K).
pub(super) fn preferences(
    ws: &mut Workspace,
    state: &DialogState,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let view = ws.view.clone();
    let intent = ws.color.intent;
    let keymap_path = crate::keymap::user_keymap_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| t("dialog.no_config_dir").into());

    let body = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(ui::field_row(
            t("dialog.prefs.theme"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("pref-theme"),
                    is_open: state.open_popup == Some(Popup::Field("pref-theme")),
                    current: view.theme,
                    label: view.theme.label().into(),
                    width: 150.0,
                    options: vec![
                        (
                            crate::workspace::Theme::Dark.label().into(),
                            crate::workspace::Theme::Dark,
                        ),
                        (
                            crate::workspace::Theme::Light.label().into(),
                            crate::workspace::Theme::Light,
                        ),
                    ],
                },
                |ws, theme, _cx| ws.set_theme_quiet(theme),
                cx,
            ),
        ))
        .child(ui::field_row(
            t("dialog.prefs.grid_spacing"),
            ui::num_field(
                ui::NumField {
                    id: "pref-grid",
                    value: view.grid_spacing,
                    suffix: " px",
                    step: 8.0,
                    focused: state.focused_field == Some("pref-grid"),
                    buffer: state.field_buffer.clone(),
                },
                |ws, delta| {
                    ws.view.grid_spacing = (ws.view.grid_spacing + delta).clamp(2.0, 1024.0);
                    ws.save_view_options();
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("dialog.prefs.snapping"),
            ui::checkbox(
                t("dialog.prefs.snap_to"),
                view.snap,
                |ws, _cx| {
                    ws.view.snap = !ws.view.snap;
                    ws.save_view_options();
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("dialog.prefs.rendering_intent"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("pref-intent"),
                    is_open: state.open_popup == Some(Popup::Field("pref-intent")),
                    current: intent,
                    label: intent_name(intent).into(),
                    width: 180.0,
                    options: schist_colormgmt::Intent::all()
                        .iter()
                        .map(|i| (SharedString::from(intent_name(*i)), *i))
                        .collect(),
                },
                |ws, value, _cx| {
                    ws.color.intent = value;
                    ws.rebuild_color_transforms();
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("dialog.prefs.scrolling"),
            ui::checkbox(
                t("dialog.prefs.zoom_with_scroll"),
                view.zoom_with_scroll,
                |ws, _cx| {
                    ws.view.zoom_with_scroll = !ws.view.zoom_with_scroll;
                    ws.save_view_options();
                },
                cx,
            ),
        ))
        .children(
            // The gallery is compiled out of the web build, so a switch
            // for it would be furniture there.
            (!cfg!(target_arch = "wasm32")).then(|| gallery_filter_row(&view, cx)),
        )
        .child(ui::field_row(
            t("dialog.prefs.rendering"),
            ui::checkbox(
                t("dialog.prefs.gpu_compositing"),
                view.gpu_compositing,
                |ws, cx| {
                    ws.view.gpu_compositing = !ws.view.gpu_compositing;
                    ws.save_view_options();
                    crate::workspace::init_compositor_backend(ws.view.gpu_compositing);
                    // Cached tiles and the viewport image were composited
                    // by the old backend; rebuild them on the new one.
                    ws.rebuild_after_backend_change(cx);
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("dialog.prefs.diagnostics"),
            ui::checkbox(
                t("dialog.prefs.crash_reports"),
                view.crash_reports,
                |ws, _cx| {
                    ws.view.crash_reports = !ws.view.crash_reports;
                    ws.save_view_options();
                },
                cx,
            ),
        ))
        // Only the official releases are built with a DSN. Everywhere else
        // this would be a checkbox that sends reports nowhere, so it is not
        // offered -- and the preference it would set stays false.
        .children(crate::crash::reporting_available().then(|| {
            ui::field_row(
                "",
                ui::checkbox(
                    t("dialog.prefs.crash_upload"),
                    view.crash_upload,
                    |ws, _cx| {
                        ws.view.crash_upload = !ws.view.crash_upload;
                        ws.save_view_options();
                    },
                    cx,
                ),
            )
        }))
        .children(telemetry_row(cx))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(t("dialog.prefs.diagnostics_note")),
        )
        .child(ui::field_row(
            t("dialog.prefs.updates"),
            ui::checkbox(
                t("dialog.prefs.check_updates"),
                view.check_updates,
                |ws, _cx| {
                    ws.view.check_updates = !ws.view.check_updates;
                    ws.save_view_options();
                },
                cx,
            ),
        ))
        .children(camera_sync_row(ws, cx))
        // The keymap file is for an editor on the desktop; the container
        // path it lives at on iOS is neither reachable nor readable.
        .children((!ui::touch()).then(|| {
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(tf!("dialog.prefs.keyboard_shortcuts", path = keymap_path))
        }))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(tf!(
                    "common.version_n",
                    version = crate::update::current_version()
                )),
        );

    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("common.cancel"),
            false,
            |ws, _w, cx| {
                ws.revert_preferences(cx);
                ws.close_modal(cx);
            },
            cx,
        ))
        .child(ui::button(
            t("common.done"),
            true,
            |ws, _w, cx| {
                ws.keep_preferences();
                ws.close_modal(cx);
            },
            cx,
        ));
    ui::modal_frame(t("dialog.prefs.title"), 400.0, body, actions)
}

/// The camera-roll backup, on the phones: its switch, and the way back to
/// the dialog that set it up. The row needs a cloud login to mean
/// anything, so without one it says that instead.
#[cfg(any(target_os = "ios", target_os = "android"))]
fn camera_sync_row(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<impl IntoElement> {
    let rule = &ws.view.camera_sync;
    let mut control = div().flex().flex_col().gap_1().max_w(px(260.0));
    if ws.cloud.account.is_none() {
        control = control.child(
            div()
                .text_size(px(10.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(t("dialog.prefs.camera_sync_sign_in")),
        );
    } else {
        control = control
            .child(ui::checkbox(
                t("dialog.prefs.camera_sync_enabled"),
                rule.enabled,
                |ws, cx| {
                    let enabled = !ws.view.camera_sync.enabled;
                    ws.camera_sync_set_enabled(enabled, cx);
                },
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .text_size(px(10.0))
                    .text_color(gpui::rgb(ui::palette().text_dim))
                    .children(rule.source.as_ref().map(|source| {
                        tf!(
                            "dialog.prefs.camera_sync_rule",
                            source = source.label(),
                            folder = if rule.folder_name.is_empty() {
                                t("cloud.gallery.unfiled").to_string()
                            } else {
                                rule.folder_name.clone()
                            }
                        )
                    }))
                    .child(
                        Link::new("prefs-camera-sync", t("dialog.prefs.camera_sync_change"))
                            .on_click(cx.listener(|ws, _e, _w, cx| {
                                // Leaving Preferences for the backup's
                                // dialog keeps what was changed.
                                ws.keep_preferences();
                                ws.camera_sync_prompt(cx);
                            })),
                    ),
            );
    }
    Some(ui::field_row(t("dialog.prefs.camera_sync"), control))
}
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn camera_sync_row(_ws: &Workspace, _cx: &mut Context<Workspace>) -> Option<gpui::Div> {
    None
}

/// The daily-ping switch. The preference is a marker file in the config
/// folder rather than a field in preferences.json, so that a user (or a
/// package) can opt out without ever launching the app; the switch just
/// creates and removes that file. Without a config folder there is
/// nowhere to keep it and no ping is sent, so no switch is offered.
#[cfg(not(target_arch = "wasm32"))]
fn telemetry_row(cx: &mut Context<Workspace>) -> Option<impl IntoElement> {
    let folder = crate::workspace::schist_folder()?;
    let enabled = crate::telemetry::enabled(&folder);
    Some(ui::field_row(
        "",
        ui::checkbox(
            t("dialog.prefs.telemetry"),
            enabled,
            move |_ws, _cx| {
                if let Err(err) = crate::telemetry::set_enabled(&folder, !enabled) {
                    log::warn!("could not change the telemetry opt-out: {err}");
                }
            },
            cx,
        ),
    ))
}

/// The web build has no config folder and sends no ping.
#[cfg(target_arch = "wasm32")]
fn telemetry_row(_cx: &mut Context<Workspace>) -> Option<gpui::Div> {
    None
}

/// The gallery's content-filter row. The switch only works once the
/// model that does the judging is installed; until then it is disabled,
/// with the warning above it saying what to download and a link that
/// goes straight there.
fn gallery_filter_row(
    view: &crate::workspace::ViewOptions,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let installed = schist_neural::installed("nsfw");
    let mut control = div().flex().flex_col().gap_1().max_w(px(260.0));
    if !installed {
        control = control.child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .text_size(px(10.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(t("dialog.prefs.nsfw_needs_model"))
                .child(
                    Link::new("prefs-manage-models", t("dialog.prefs.nsfw_download_link"))
                        .on_click(cx.listener(|ws, _e, _w, cx| {
                            // Leaving Preferences for the model
                            // manager keeps what was changed.
                            ws.keep_preferences();
                            ws.open_modal(Modal::ModelManager, cx);
                        })),
                ),
        );
    }
    control = control.child(if installed {
        ui::checkbox(
            t("dialog.prefs.hide_flagged"),
            view.gallery_hide_nsfw,
            |ws, _cx| {
                ws.view.gallery_hide_nsfw = !ws.view.gallery_hide_nsfw;
                ws.save_view_options();
            },
            cx,
        )
        .into_any_element()
    } else {
        // The same box, faint and listening to nothing -- still honest
        // about the stored preference, even while it cannot be changed
        // from here.
        Checkbox::new(
            "checkbox-Hide flagged photos",
            t("dialog.prefs.hide_flagged"),
            view.gallery_hide_nsfw,
        )
        .disabled(true)
        .into_any_element()
    });
    FieldRow::new(t("menu.gallery"))
        .top_aligned()
        .child(control)
}
