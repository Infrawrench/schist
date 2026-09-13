//! File ▸ New: presets and the new-document dialog.

use super::*;
use schist_i18n::{t, tf};

/// (label key, width, height, ppi) rows of the File ▸ New preset
/// dropdown, matched against the dialog's current values to show which
/// is selected. The key names the preset in the catalog; `t` gives the
/// label.
pub(super) const NEW_DOC_PRESETS: &[(&str, u32, u32, f32)] = &[
    ("dialog.new_doc.preset_default", 1280, 800, 72.0),
    ("dialog.new_doc.preset_hd", 1920, 1080, 72.0),
    ("dialog.new_doc.preset_4k", 3840, 2160, 72.0),
    ("dialog.new_doc.preset_square", 1080, 1080, 72.0),
    ("dialog.new_doc.preset_a4", 2480, 3508, 300.0),
    ("dialog.new_doc.preset_us_letter", 2550, 3300, 300.0),
];

/// A colour mode's name in the user's language.
fn mode_name(mode: ColorMode) -> &'static str {
    t(match mode {
        ColorMode::Rgb => "common.rgb",
        ColorMode::Grayscale => "common.grayscale",
        ColorMode::Cmyk => "common.cmyk",
        ColorMode::Lab => "common.lab",
        ColorMode::Indexed => "common.indexed",
    })
}

/// File ▸ New: the preset picker. One card per common size — a click
/// creates the document on the spot — and Custom… opens the full
/// dialog below for everything else.
pub(super) fn new_file_picker(cx: &mut Context<Workspace>) -> impl IntoElement {
    let mut cards = div().flex().flex_row().flex_wrap().gap_2();
    for &(key, width, height, ppi) in NEW_DOC_PRESETS {
        cards = cards.child(preset_card(key, width, height, ppi, cx));
    }
    cards = cards.child(
        div()
            .flex()
            .items_center()
            .justify_center()
            .w(px(150.0))
            .h(px(56.0))
            .rounded_md()
            .border_1()
            .border_color(gpui::rgb(ui::palette().edge))
            .text_size(px(12.0))
            .text_color(gpui::rgb(ui::palette().text_dim))
            .id("new-doc-custom")
            .cursor_pointer()
            .hover(|s| s.border_color(gpui::rgb(ui::palette().accent)))
            .on_click(cx.listener(|ws, _e, _w, cx| {
                ws.open_new_document_dialog(cx);
            }))
            .child(t("dialog.new_doc.custom_ellipsis")),
    );
    let actions = div().flex().flex_row().gap_2().child(ui::button(
        "Cancel",
        false,
        |ws, _w, cx| ws.close_modal(cx),
        cx,
    ));
    ui::modal_frame(t("dialog.new_doc.picker_title"), 520.0, cards, actions)
}

/// A preset card: click it and the document exists. `key` is the
/// preset's catalog key, which also serves as the card's element id.
fn preset_card(
    key: &'static str,
    width: u32,
    height: u32,
    ppi: f32,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .justify_center()
        .w(px(150.0))
        .h(px(56.0))
        .px_3()
        .rounded_md()
        .bg(gpui::rgb(ui::palette().control_bg))
        .border_1()
        .border_color(gpui::rgb(ui::palette().edge))
        .id(key)
        .cursor_pointer()
        .hover(|s| s.border_color(gpui::rgb(ui::palette().accent)))
        .on_click(cx.listener(move |ws, _e, _w, cx| {
            ws.close_modal(cx);
            ws.create_document(
                "",
                width,
                height,
                ppi,
                ColorMode::Rgb,
                Depth::Eight,
                crate::workspace::NewDocBackground::White,
            );
            cx.notify();
        }))
        .child(div().text_size(px(12.0)).child(t(key)))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(tf!("common.dimensions_px", w = width, h = height)),
        )
}

/// The full dialog, asked before anything is created, as Photoshop does.
pub(super) fn new_document_dialog(
    state: &DialogState,
    modal: Modal,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let Modal::NewDocument {
        name,
        width,
        height,
        resolution,
        mode,
        depth,
        background,
    } = modal
    else {
        unreachable!("render dispatches only Modal::NewDocument here");
    };

    let name_focused = state.focused_field == Some("new-doc-name");
    let committed = name.clone();
    let shown_name = if name_focused && !state.field_buffer.is_empty() {
        state.field_buffer.clone()
    } else {
        name.clone()
    };

    let preset = NEW_DOC_PRESETS
        .iter()
        .position(|(_, w, h, r)| *w == width && *h == height && (*r - resolution).abs() < 0.5);
    let preset_label: SharedString = preset
        .map(|i| t(NEW_DOC_PRESETS[i].0).into())
        .unwrap_or_else(|| t("common.custom").into());
    let preset_options: Vec<(SharedString, usize)> = NEW_DOC_PRESETS
        .iter()
        .enumerate()
        .map(|(i, (key, ..))| (SharedString::from(t(key)), i))
        .collect();

    let depth_label = |d: Depth| {
        tf!(
            "common.bits_per_channel",
            n = match d {
                Depth::Eight => 8,
                Depth::Sixteen => 16,
                Depth::ThirtyTwo => 32,
            }
        )
    };
    let mode_options: Vec<(SharedString, ColorMode)> = [
        ColorMode::Rgb,
        ColorMode::Grayscale,
        ColorMode::Cmyk,
        ColorMode::Lab,
    ]
    .into_iter()
    .map(|m| (SharedString::from(mode_name(m)), m))
    .collect();
    let background_options: Vec<(SharedString, NewDocBackground)> = [
        NewDocBackground::White,
        NewDocBackground::BackgroundColor,
        NewDocBackground::Black,
        NewDocBackground::Transparent,
    ]
    .into_iter()
    .map(|b| (SharedString::from(b.label()), b))
    .collect();

    // Uncompressed pixel size, the way Photoshop's dialog reports it.
    let bytes = width as u64
        * height as u64
        * (mode.channels() as u64 + 1)
        * depth.bytes_per_channel() as u64;
    let size = if bytes < 1 << 20 {
        tf!(
            "common.kilobytes",
            n = format!("{:.0}", bytes as f64 / (1u64 << 10) as f64)
        )
    } else if bytes < 1 << 30 {
        tf!(
            "common.megabytes",
            n = format!("{:.1}", bytes as f64 / (1u64 << 20) as f64)
        )
    } else {
        tf!(
            "common.gigabytes",
            n = format!("{:.2}", bytes as f64 / (1u64 << 30) as f64)
        )
    };

    let body = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(ui::field_row(
            t("common.name"),
            TextInput::new("new-doc-name", shown_name.clone())
                .cursor(if state.field_buffer.is_empty() {
                    shown_name.len()
                } else {
                    state.field_cursor.min(shown_name.len())
                })
                .selection(state.field_selection.clone())
                .active(name_focused)
                .caret_on(state.caret_on)
                .w(px(200.0))
                .on_focus(cx.listener(move |ws, press: &ui::TextPress, _w, cx| {
                    ws.press_field("new-doc-name", committed.clone(), press);
                    cx.notify();
                }))
                .on_select_to(cx.listener(|ws, offset: &usize, _w, cx| {
                    ws.drag_field("new-doc-name", *offset);
                    cx.notify();
                })),
        ))
        .child(ui::field_row(
            t("dialog.new_doc.preset"),
            ui::dropdown(
                &state.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("new-doc-preset"),
                    is_open: state.open_popup == Some(Popup::Field("new-doc-preset")),
                    current: preset.unwrap_or(usize::MAX),
                    label: preset_label,
                    width: 200.0,
                    options: preset_options,
                },
                |ws, index, _cx| {
                    let (_, w, h, r) = NEW_DOC_PRESETS[index];
                    ws.update_modal(|m| {
                        if let Modal::NewDocument {
                            width,
                            height,
                            resolution,
                            ..
                        } = m
                        {
                            *width = w;
                            *height = h;
                            *resolution = r;
                        }
                    });
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("common.width"),
            ui::num_field(
                ui::NumField {
                    id: "new-doc-w",
                    value: width as f32,
                    suffix: " px",
                    step: 10.0,
                    focused: state.focused_field == Some("new-doc-w"),
                    buffer: state.field_buffer.clone(),
                },
                |ws, delta| {
                    ws.update_modal(|m| {
                        if let Modal::NewDocument { width, .. } = m {
                            *width = (*width as f32 + delta).clamp(1.0, 30000.0) as u32;
                        }
                    });
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("common.height"),
            ui::num_field(
                ui::NumField {
                    id: "new-doc-h",
                    value: height as f32,
                    suffix: " px",
                    step: 10.0,
                    focused: state.focused_field == Some("new-doc-h"),
                    buffer: state.field_buffer.clone(),
                },
                |ws, delta| {
                    ws.update_modal(|m| {
                        if let Modal::NewDocument { height, .. } = m {
                            *height = (*height as f32 + delta).clamp(1.0, 30000.0) as u32;
                        }
                    });
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("common.resolution"),
            ui::num_field(
                ui::NumField {
                    id: "new-doc-dpi",
                    value: resolution,
                    suffix: " ppi",
                    step: 1.0,
                    focused: state.focused_field == Some("new-doc-dpi"),
                    buffer: state.field_buffer.clone(),
                },
                |ws, delta| {
                    ws.update_modal(|m| {
                        if let Modal::NewDocument { resolution, .. } = m {
                            *resolution = (*resolution + delta).max(1.0);
                        }
                    });
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("common.color_mode"),
            ui::dropdown(
                &state.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("new-doc-mode"),
                    is_open: state.open_popup == Some(Popup::Field("new-doc-mode")),
                    current: mode,
                    label: mode_name(mode).into(),
                    width: 150.0,
                    options: mode_options,
                },
                |ws, value, _cx| {
                    ws.update_modal(|m| {
                        if let Modal::NewDocument { mode, .. } = m {
                            *mode = value;
                        }
                    });
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("common.bit_depth"),
            ui::dropdown(
                &state.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("new-doc-depth"),
                    is_open: state.open_popup == Some(Popup::Field("new-doc-depth")),
                    current: depth,
                    label: (depth_label(depth)).into(),
                    width: 150.0,
                    options: [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo]
                        .into_iter()
                        .map(|d| (SharedString::from(depth_label(d)), d))
                        .collect(),
                },
                |ws, value, _cx| {
                    ws.update_modal(|m| {
                        if let Modal::NewDocument { depth, .. } = m {
                            *depth = value;
                        }
                    });
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("common.background"),
            ui::dropdown(
                &state.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("new-doc-bg"),
                    is_open: state.open_popup == Some(Popup::Field("new-doc-bg")),
                    current: background,
                    label: background.label().into(),
                    width: 150.0,
                    options: background_options,
                },
                |ws, value, _cx| {
                    ws.update_modal(|m| {
                        if let Modal::NewDocument { background, .. } = m {
                            *background = value;
                        }
                    });
                },
                cx,
            ),
        ))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(tf!(
                    "dialog.new_doc.summary",
                    w = width,
                    h = height,
                    ppi = format!("{resolution:.0}"),
                    size = size
                )),
        );

    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("common.cancel"),
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .child(ui::button(
            t("dialog.new_doc.create"),
            true,
            |ws, _w, cx| {
                let Some(Modal::NewDocument {
                    name,
                    width,
                    height,
                    resolution,
                    mode,
                    depth,
                    background,
                }) = ws.modal.clone()
                else {
                    return;
                };
                ws.close_modal(cx);
                ws.create_document(&name, width, height, resolution, mode, depth, background);
                ws.status = tf!("dialog.new_doc.created", w = width, h = height).into();
                cx.notify();
            },
            cx,
        ));

    ui::modal_frame(t("dialog.new_doc.title"), 400.0, body, actions)
}
