use super::*;
use crate::printing::{Drag, Editor, Preset};
use gpui::StyledImage;

fn edit(ws: &mut Workspace, f: impl FnOnce(&mut Editor)) {
    ws.update_modal(|modal| {
        if let Modal::Printing { editor } = modal {
            if !editor.running {
                editor.error = None;
                editor.notice = None;
                f(editor);
            }
        }
    });
}
fn history(ws: &mut Workspace, undo: bool) {
    edit(ws, |e| e.history(undo));
}
fn drag_start(ws: &mut Workspace, index: usize, position: gpui::Point<gpui::Pixels>, resize: bool) {
    edit(ws, |e| {
        if let Some(placed) = e.options.layout.get(index).copied() {
            e.remember();
            e.selected = Some(index);
            e.drag = Some(Drag {
                index,
                start: position,
                frame: placed.frame,
                resize,
            });
        }
    });
}
fn drag_move(ws: &mut Workspace, ev: &gpui::MouseMoveEvent) {
    edit(ws, |e| {
        if ev.pressed_button != Some(gpui::MouseButton::Left) {
            e.drag = None;
            return;
        }
        let (Some(drag), Some(bounds)) = (e.drag, e.preview_bounds) else {
            return;
        };
        let paper = e.options.page_mm();
        let dx = f32::from(ev.position.x - drag.start.x) * paper.0
            / f32::from(bounds.size.width).max(1.0);
        let dy = f32::from(ev.position.y - drag.start.y) * paper.1
            / f32::from(bounds.size.height).max(1.0);
        if let Some(placed) = e.options.layout.get_mut(drag.index) {
            placed.frame = if drag.resize {
                drag.frame.resized(dx, dy, paper)
            } else {
                drag.frame.moved(dx, dy, paper)
            };
            e.preset = Preset::Custom;
        }
    });
}

pub(super) fn printing_dialog(
    ws: &mut Workspace,
    state: &DialogState,
    editor: Editor,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let o = &editor.options;
    let paper = o.page_mm();
    let scale = (400.0 / paper.0).min(480.0 / paper.1);
    let mut settings = div()
        .id("print-settings")
        .flex()
        .flex_col()
        .gap_2()
        .w(px(310.0))
        .flex_none()
        .max_h(px(500.0))
        .overflow_y_scroll()
        .child(ui::field_row(
            t("printing.preset"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("print-preset"),
                    is_open: state.open_popup == Some(Popup::Field("print-preset")),
                    current: editor.preset,
                    label: editor.preset.label().into(),
                    width: 180.0,
                    options: Preset::ALL
                        .into_iter()
                        .map(|p| (p.label().into(), p))
                        .collect(),
                },
                |ws, value, _| edit(ws, |e| e.apply_preset(value)),
                cx,
            ),
        ))
        .child(ui::button(
            t("printing.add_images"),
            false,
            |ws, _, cx| ws.add_print_images(cx),
            cx,
        ));
    for (id, label, value, values) in [
        (
            "print-paper",
            "printing.paper",
            usize::from(o.letter),
            vec![
                (t("printing.a4").to_string(), 0),
                (t("printing.letter").to_string(), 1),
            ],
        ),
        (
            "print-margin",
            "printing.margin",
            o.margin as usize,
            [5, 10, 15, 20, 25, 30]
                .into_iter()
                .map(|n| (schist_i18n::tf!("printing.margin_value", value = n), n))
                .collect(),
        ),
        (
            "print-dpi",
            "printing.dpi",
            o.dpi as usize,
            [72, 150, 300, 600]
                .into_iter()
                .map(|n| (n.to_string(), n))
                .collect(),
        ),
    ] {
        let label_value = values
            .iter()
            .find(|(_, n)| *n == value)
            .map(|(s, _)| s.clone())
            .unwrap_or_default();
        settings = settings.child(ui::field_row(
            t(label),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field(id),
                    is_open: state.open_popup == Some(Popup::Field(id)),
                    current: value,
                    label: label_value.into(),
                    width: 180.0,
                    options: values.into_iter().map(|(s, n)| (s.into(), n)).collect(),
                },
                move |ws, value, _| {
                    edit(ws, |e| match id {
                        "print-paper" => {
                            let old = e.options.page_mm();
                            e.options.letter = value == 1;
                            e.change_paper(old);
                        }
                        "print-margin" => {
                            e.options.margin = value as f32;
                            e.apply_preset(e.preset);
                        }
                        "print-dpi" => e.options.dpi = value as u32,
                        _ => {}
                    })
                },
                cx,
            ),
        ));
    }
    settings = settings
        .child(ui::checkbox(
            t("printing.landscape"),
            o.landscape,
            |ws, _| {
                edit(ws, |e| {
                    let old = e.options.page_mm();
                    e.options.landscape = !e.options.landscape;
                    e.change_paper(old);
                })
            },
            cx,
        ))
        .child(ui::checkbox(
            t("printing.captions"),
            o.captions,
            |ws, _| edit(ws, |e| e.options.captions = !e.options.captions),
            cx,
        ))
        .child(
            div()
                .flex()
                .gap_2()
                .child(ui::button(
                    t("common.undo"),
                    false,
                    |ws, _, _| history(ws, true),
                    cx,
                ))
                .child(ui::button(
                    t("common.redo"),
                    false,
                    |ws, _, _| history(ws, false),
                    cx,
                )),
        )
        .child(div().text_size(px(11.0)).child(t("printing.layout_help")));
    if let Some(index) = editor.selected.filter(|i| *i < o.layout.len()) {
        let frame = o.layout[index].frame;
        settings = settings.child(div().text_size(px(12.0)).child(t("printing.selection")));
        for (id, label, value, max) in [
            ("print-x", "common.x", frame.x, paper.0 - frame.width),
            ("print-y", "common.y", frame.y, paper.1 - frame.height),
            (
                "print-width",
                "common.width",
                frame.width,
                paper.0 - frame.x,
            ),
            (
                "print-height",
                "common.height",
                frame.height,
                paper.1 - frame.y,
            ),
        ] {
            let min = if id == "print-x" || id == "print-y" {
                0.0
            } else {
                8.0
            };
            let span = (max - min).max(0.001);
            settings = settings.child(ui::field_row(
                t(label),
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::slider_track(
                        id,
                        ((value - min) / span).clamp(0.0, 1.0),
                        115.0,
                        move |ws, r, _| {
                            edit(ws, |e| {
                                // A numeric adjustment participates in layout undo too.
                                e.remember();
                                if let Some(p) = e.options.layout.get_mut(index) {
                                    let v = min + r * span;
                                    match id {
                                        "print-x" => p.frame.x = v,
                                        "print-y" => p.frame.y = v,
                                        "print-width" => p.frame.width = v,
                                        "print-height" => p.frame.height = v,
                                        _ => {}
                                    }
                                    e.preset = Preset::Custom;
                                }
                            })
                        },
                        cx,
                    ))
                    .child(
                        div()
                            .w(px(56.0))
                            .text_size(px(11.0))
                            .child(SharedString::from(schist_i18n::tf!(
                                "printing.margin_value",
                                value = format!("{value:.1}")
                            ))),
                    ),
            ));
        }
        settings = settings.child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(ui::button(
                    t("common.duplicate"),
                    false,
                    move |ws, _, _| {
                        edit(ws, |e| {
                            if e.options.layout.len() >= crate::printing::MAX_ITEMS {
                                return;
                            }
                            if let Some(mut p) = e.options.layout.get(index).copied() {
                                e.remember();
                                p.frame = p.frame.moved(5.0, 5.0, e.options.page_mm());
                                e.options.layout.push(p);
                                e.selected = Some(e.options.layout.len() - 1);
                                e.preset = Preset::Custom;
                            }
                        })
                    },
                    cx,
                ))
                .child(ui::button(
                    t("common.remove"),
                    false,
                    move |ws, _, _| {
                        edit(ws, |e| {
                            if index < e.options.layout.len() {
                                e.remember();
                                e.options.layout.remove(index);
                                e.selected = None;
                                e.preset = Preset::Custom;
                            }
                        })
                    },
                    cx,
                ))
                .child(ui::button(
                    t("printing.bring_front"),
                    false,
                    move |ws, _, _| {
                        edit(ws, |e| {
                            if index < e.options.layout.len() {
                                e.remember();
                                let p = e.options.layout.remove(index);
                                e.options.layout.push(p);
                                e.selected = Some(e.options.layout.len() - 1);
                            }
                        })
                    },
                    cx,
                )),
        );
    }
    settings = settings.child(ui::checkbox(
        t("printing.actual_size"),
        o.actual_size,
        |ws, _| edit(ws, |e| e.options.actual_size = !e.options.actual_size),
        cx,
    ));
    let entity = cx.entity();
    let mut page = div()
        .id("print-page")
        .relative()
        .flex_none()
        .overflow_hidden()
        .w(px(paper.0 * scale))
        .h(px(paper.1 * scale))
        .bg(gpui::rgb(0xffffff))
        .child(
            gpui::canvas(
                move |bounds, _, cx| {
                    entity.update(cx, |ws, _| {
                        if let Some(Modal::Printing { editor }) = &mut ws.modal {
                            editor.preview_bounds = Some(bounds);
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .left(px(o.margin * scale))
                .top(px(o.margin * scale))
                .w(px((paper.0 - 2.0 * o.margin) * scale))
                .h(px((paper.1 - 2.0 * o.margin) * scale))
                .border_1()
                .border_color(gpui::rgb(0xe1e4e8)),
        );
    for (index, placed) in o
        .layout
        .iter()
        .enumerate()
        .filter(|(_, p)| p.page == editor.page)
    {
        let Some(item) = editor.photos.get(placed.source) else {
            continue;
        };
        let frame = placed.frame;
        let selected = editor.selected == Some(index);
        let mut tile = div()
            .id(("print-image", index))
            .absolute()
            .left(px(frame.x * scale))
            .top(px(frame.y * scale))
            .w(px(frame.width * scale))
            .h(px(frame.height * scale))
            .p(px(2.0 * scale))
            .flex()
            .flex_col()
            .items_center()
            .overflow_hidden()
            .cursor_pointer()
            .border_1()
            .border_color(gpui::rgb(if selected { 0x307bce } else { 0xd6d6d6 }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |ws, ev: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    drag_start(ws, index, ev.position, false);
                    cx.notify();
                }),
            );
        tile = tile.child(match &item.preview {
            Some(image) if o.actual_size && item.dimensions.is_some() => {
                let (w, h, dpi) = item.dimensions.unwrap();
                let dpi = if dpi.is_finite() && dpi > 0.0 {
                    dpi
                } else {
                    72.0
                };
                div()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::img(image.clone())
                            .flex_none()
                            .w(px(w as f32 / dpi * 25.4 * scale))
                            .h(px(h as f32 / dpi * 25.4 * scale))
                            .object_fit(gpui::ObjectFit::Contain),
                    )
                    .into_any_element()
            }
            Some(image) => gpui::img(image.clone())
                .w_full()
                .flex_1()
                .min_h(px(0.0))
                .object_fit(gpui::ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .flex_1()
                .text_size(px(11.0))
                .text_color(gpui::rgb(0x333333))
                .child(item.name.clone())
                .into_any_element(),
        });
        if o.captions {
            tile = tile.child(
                div()
                    .w_full()
                    .flex_none()
                    .mt(px(2.0 * scale))
                    .text_color(gpui::rgb(0x111111))
                    .text_size(px(8.0 * 25.4 / 72.0 * scale))
                    .children(
                        item.caption
                            .lines()
                            .map(|line| div().child(line.to_owned())),
                    ),
            );
        }
        if selected && !editor.running {
            tile = tile.child(
                div()
                    .id(("print-resize", index))
                    .absolute()
                    .right(px(0.0))
                    .bottom(px(0.0))
                    .w(px(12.0))
                    .h(px(12.0))
                    .bg(gpui::rgb(0x307bce))
                    .cursor_pointer()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |ws, ev: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            drag_start(ws, index, ev.position, true);
                            cx.notify();
                        }),
                    ),
            );
        }
        page = page.child(tile);
    }
    let pages = o.pages(editor.photos.len()).max(1);
    let canvas = div()
        .flex()
        .flex_col()
        .gap_2()
        .items_center()
        .child(page)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(ui::button(
                    t("common.back"),
                    false,
                    |ws, _, _| {
                        edit(ws, |e| {
                            e.page = e.page.saturating_sub(1);
                            e.selected = None;
                        })
                    },
                    cx,
                ))
                .child(SharedString::from(schist_i18n::tf!(
                    "printing.page_position",
                    page = editor.page + 1,
                    pages = pages
                )))
                .child(ui::button(
                    t("common.next"),
                    false,
                    |ws, _, _| {
                        edit(ws, |e| {
                            e.page =
                                (e.page + 1).min(e.options.pages(e.photos.len()).saturating_sub(1));
                            e.selected = None;
                        })
                    },
                    cx,
                ))
                .child(ui::button(
                    t("printing.add_page"),
                    false,
                    |ws, _, _| {
                        edit(ws, |e| {
                            let pages = e.options.pages(e.photos.len());
                            if pages < crate::printing::MAX_ITEMS {
                                e.remember();
                                e.options.page_count = pages + 1;
                                e.page = pages;
                                e.selected = None;
                            }
                        })
                    },
                    cx,
                )),
        );
    let bank = div()
        .id("print-sources")
        .flex()
        .flex_col()
        .gap_2()
        .w(px(90.0))
        .flex_none()
        .max_h(px(480.0))
        .overflow_y_scroll()
        .child(t("common.images"))
        .children(editor.photos.iter().enumerate().map(|(index, item)| {
            let image = match &item.preview {
                Some(image) => gpui::img(image.clone())
                    .w_full()
                    .h(px(58.0))
                    .object_fit(gpui::ObjectFit::Contain)
                    .into_any_element(),
                None => div()
                    .h(px(58.0))
                    .child(t("printing.loading_preview"))
                    .into_any_element(),
            };
            div()
                .id(("print-source", index))
                .p_1()
                .rounded_sm()
                .cursor_pointer()
                .border_1()
                .border_color(gpui::rgb(ui::palette().panel_edge))
                .child(image)
                .child(
                    div()
                        .w_full()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(10.0))
                        .child(item.name.clone()),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    edit(ws, |e| e.add(index));
                    cx.notify();
                }))
        }));
    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .on_mouse_move(cx.listener(|ws, ev: &gpui::MouseMoveEvent, _, cx| {
            let dragging =
                matches!(&ws.modal,Some(Modal::Printing{editor}) if editor.drag.is_some());
            if dragging {
                drag_move(ws, ev);
                cx.notify();
            }
        }))
        .on_mouse_up(
            gpui::MouseButton::Left,
            cx.listener(|ws, _, _, cx| {
                edit(ws, |e| e.drag = None);
                cx.notify();
            }),
        )
        .on_mouse_up_out(
            gpui::MouseButton::Left,
            cx.listener(|ws, _, _, cx| {
                edit(ws, |e| e.drag = None);
                cx.notify();
            }),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .child(settings)
                .child(canvas)
                .child(bank),
        )
        .child(div().text_size(px(11.0)).child(t("printing.color_note")))
        .children(
            editor
                .loading
                .then(|| div().child(t("printing.loading_preview"))),
        )
        .children(editor.running.then(|| div().child(t("printing.running"))))
        .children(editor.notice.map(|notice| div().child(notice)))
        .children(editor.error.map(|error| div().child(error)));
    let mut actions = div().flex().gap_2().child(ui::button(
        t("common.cancel"),
        false,
        |ws, _, cx| ws.close_modal(cx),
        cx,
    ));
    if !editor.running && !editor.loading {
        actions = actions.child(ui::button(
            t("printing.save_pdf"),
            true,
            |ws, w, cx| ws.run_printing(false, w, cx),
            cx,
        ));
        #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
        {
            actions = actions.child(ui::button(
                t("printing.open_print"),
                false,
                |ws, w, cx| ws.run_printing(true, w, cx),
                cx,
            ));
        }
    }
    ui::modal_frame(t("printing.layout_title"), 970.0, body, actions)
}
