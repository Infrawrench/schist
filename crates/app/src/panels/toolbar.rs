//! The toolbar, its flyouts, and the tool options bar above the canvas.

use super::*;

/// One toolbar slot: its group, the icon of the tool currently showing,
/// whether that tool is active, whether the group has more than one tool,
/// and the name and shortcut for its hover label.
pub(super) type ToolSlot = (
    &'static str,
    &'static str,
    bool,
    bool,
    String,
    Option<SharedString>,
);

pub fn tool_options_bar(
    ws: &mut Workspace,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let tool_id = ws.editor.active_tool;
    if tool_id == "type" {
        return super::typography::type_options_bar(ws, cx).into_any_element();
    }
    let (tool_icon, tool_name) = ws
        .registry
        .tools()
        .find(|t| t.id() == tool_id)
        .map(|t| (t.icon(), t.name()))
        .unwrap_or(("move", "Move"));
    let is_paint = matches!(tool_id, "brush" | "pencil" | "eraser");

    let m = ui::metrics();
    let mut bar = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        // Tighter on touch: the controls are already larger, and a phone
        // has no width to give away.
        .gap_x(px(if ui::touch() { 8.0 } else { 16.0 }))
        .gap_y_1()
        .min_h(px(m.options_bar_h))
        .w_full()
        .min_w_0()
        .flex_none()
        .px_3()
        .py_1()
        .bg(gpui::rgb(palette().panel_bg))
        .border_b_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .when(!ui::touch(), |d| d.w(px(130.0)))
                .flex_none()
                .child(icon(tool_icon, m.tool_icon - 1.0, palette().text))
                .child(div().text_size(px(m.row_text)).child(tool_name)),
        );
    // On a touch screen each slider is a button naming its value, and a
    // tap opens the slider in a popover under it; the count says which
    // side to hang the popover from.
    let mut bar_sliders: usize = 0;
    if is_paint {
        bar = bar
            .child(option_slider(
                &mut bar_sliders,
                "opt-size",
                "Size",
                format!("{:.0}px", ws.editor.brush_size),
                SliderTarget::BrushSize,
                ws,
                cx,
            ))
            .child(option_slider(
                &mut bar_sliders,
                "opt-hard",
                "Hardness",
                format!("{:.0}%", ws.editor.brush_hardness * 100.0),
                SliderTarget::BrushHardness,
                ws,
                cx,
            ));
    }
    if tool_id == "note" {
        bar = bar.child(note_options(ws, cx));
    }
    bar = bar.child(option_slider(
        &mut bar_sliders,
        "opt-opacity",
        "Opacity",
        format!("{:.0}%", ws.editor.tool_opacity * 100.0),
        SliderTarget::ToolOpacity,
        ws,
        cx,
    ));
    // Whatever else the active tool asked for.
    for opt in ws
        .registry
        .tools()
        .find(|t| t.id() == tool_id)
        .map(|t| t.options())
        .unwrap_or_default()
    {
        // Keep each control together when a tool has more options than
        // one row can show. Type has its own compact bar and panel.
        bar = bar.child(div().flex_none().child(tool_option_control(
            &mut bar_sliders,
            ws,
            opt,
            cx,
        )));
    }
    if ui::touch() {
        bar = bar
            .child(div().flex_grow())
            .children(ui::touch().then(|| save_to_photos_button(ws, cx)))
            .child(side_panels_toggle(ws, window, cx));
    }
    bar.into_any_element()
}

/// The touch chrome's panel button. On an iPad it folds the panel column
/// away, which is the difference between a canvas and a wide one; on a
/// phone it switches the body between the canvas and the panels, and
/// its icon names the one it will switch to.
/// On iOS, the camera roll is a tap away from the canvas: the flattened
/// document goes to Photos (the same item as File ▸ Save to Photos).
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
fn save_to_photos_button(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let m = ui::metrics();
    IconButton::new("save-to-photos", "save-photos")
        .size(m.icon_button)
        .icon_size(m.icon_button_icon)
        .disabled(ws.doc.is_none())
        .tooltip("Save to Photos", None)
        .on_click(
            cx.listener(|ws, _e, window, cx| run_app_item(ws, AppItem::SaveToPhotos, window, cx)),
        )
}

fn side_panels_toggle(
    ws: &Workspace,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let m = ui::metrics();
    let shown = ws.side_panels_shown(window);
    let compact = ui::compact(window);
    let icon = if compact && shown {
        "artboard"
    } else {
        "navigator"
    };
    IconButton::new("side-panels-toggle", icon)
        .size(m.icon_button)
        .icon_size(m.icon_button_icon)
        .active(shown && !compact)
        .on_click(cx.listener(|ws, _e, window, cx| ws.toggle_side_panels(window, cx)))
}

/// A slider in the tool options bar: the slider itself on the desktop,
/// and on a touch screen a button carrying its label and value that
/// opens the slider in a popover hanging from it. The first two hang
/// from their left edge and the rest from their right, which keeps the
/// popover on a phone's screen whichever end of the bar its button is.
fn option_slider(
    bar_sliders: &mut usize,
    id: &'static str,
    label: &'static str,
    display: String,
    target: SliderTarget,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    if !ui::touch() {
        return slider(id, label, display, target, ws, cx).into_any_element();
    }
    let popup = Popup::Slider(id);
    let is_open = ws.open_popup == Some(popup);
    let m = ui::metrics();
    let index = *bar_sliders;
    *bar_sliders += 1;
    let mut button = div()
        .id(SharedString::from(format!("opt-slider-{id}")))
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .h(px(m.icon_button))
        .px_2()
        .rounded_sm()
        .bg(gpui::rgb(palette().field_bg))
        .text_size(px(m.small_text))
        .when_active(is_open)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _e, _w, cx| ws.toggle_popup(popup, cx)),
        )
        .child(
            div()
                .text_color(gpui::rgb(if is_open {
                    palette().accent_text
                } else {
                    palette().text_dim
                }))
                .child(label),
        )
        .child(display.clone());
    if is_open {
        button = button.child(deferred(
            div()
                .absolute()
                .top(px(m.icon_button + 6.0))
                .when(index < 2, |d| d.left_0())
                .when(index >= 2, |d| d.right_0())
                .w(px(280.0))
                .flex()
                .flex_row()
                .items_center()
                .p_3()
                .bg(gpui::rgb(palette().popup_bg))
                .text_color(gpui::rgb(palette().text))
                .border_1()
                .border_color(gpui::rgb(palette().edge))
                .rounded_md()
                .shadow_lg()
                .occlude()
                .on_mouse_down_out(cx.listener(|ws, _e, _w, cx| ws.close_popup(cx)))
                .child(slider_stretch(id, label, display, target, ws, cx)),
        ));
    }
    button.into_any_element()
}

/// Render one plugin-declared option. The shell knows the three kinds, not
/// the tools. `bar_sliders` counts the bar's sliders so far, for the touch
/// chrome's popovers.
fn tool_option_control(
    bar_sliders: &mut usize,
    ws: &Workspace,
    opt: schist_plugin_api::ToolOption,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    use schist_plugin_api::OptionKind;
    let key = opt.key;
    match opt.kind {
        OptionKind::Slider { min, max, suffix } => {
            let v = opt.value.num();
            // Coarse ranges read better without decimals.
            let display = if max - min > 20.0 {
                format!("{v:.0}{suffix}")
            } else {
                format!("{v:.1}{suffix}")
            };
            option_slider(
                bar_sliders,
                key,
                opt.label,
                display,
                SliderTarget::ToolOption { key, min, max },
                ws,
                cx,
            )
        }
        OptionKind::Toggle => {
            let on = opt.value.bool();
            ui::checkbox(
                opt.label,
                on,
                move |ws, cx| {
                    ws.set_tool_option(key, schist_plugin_api::OptionValue::Bool(!on), cx)
                },
                cx,
            )
            .into_any_element()
        }
        OptionKind::Choice(labels) => {
            let current = opt.value.index().min(labels.len().saturating_sub(1));
            // Wide enough for the longest thing it can say, so a dropdown
            // never has to truncate its own value.
            let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
            let width = (longest as f32 * 6.2 + 34.0).clamp(80.0, 210.0);
            let spec = ui::Dropdown {
                popup: Popup::Field(key),
                is_open: ws.open_popup == Some(Popup::Field(key)),
                current,
                label: labels.get(current).copied().unwrap_or("").into(),
                width,
                options: labels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| (SharedString::from(*l), i))
                    .collect(),
            };
            let on_select = move |ws: &mut Workspace, i, cx: &mut Context<Workspace>| {
                ws.set_tool_option(key, schist_plugin_api::OptionValue::Choice(i), cx)
            };
            // The font menu's rows are font names, so show each in itself.
            let control = if key == "type-family" {
                ui::font_dropdown(&ws.dropdown, spec, on_select, cx).into_any_element()
            } else {
                ui::dropdown(&ws.dropdown, spec, on_select, cx).into_any_element()
            };
            // Sliders carry their own label; a dropdown does not, and an
            // unlabelled one reading "Point Sample" does not say what it
            // is choosing.
            if opt.label.is_empty() {
                return control.into_any_element();
            }
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(opt.label),
                )
                .child(control)
                .into_any_element()
        }
    }
}

pub fn toolbar(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let active = ws.editor.active_tool;
    // One slot per group, showing whichever tool that group last used —
    // Photoshop's nested tools, so twenty tools take eleven slots.
    let slots: Vec<ToolSlot> = ws
        .tool_groups
        .clone()
        .into_iter()
        .map(|(group, tools)| {
            let shown = ws.group_tool(group);
            let (icon, name, key) = ws
                .registry
                .tool_mut(shown)
                .map(|t| (t.icon(), t.name().to_string(), t.shortcut()))
                .unwrap_or(("move", "Move".into(), None));
            let hint = key.map(|k| SharedString::from(k.to_uppercase()));
            (
                group,
                icon,
                tools.contains(&active),
                tools.len() > 1,
                name,
                hint,
            )
        })
        .collect();

    let m = ui::metrics();
    // Scrolls when the window is shorter than the tool list (a phone,
    // with 44pt slots): the slots keep their size and the column moves,
    // rather than every slot shrinking to fit.
    div()
        .id("toolbar")
        .flex()
        .flex_col()
        .w(px(m.toolbar_w))
        .flex_none()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .items_center()
        .bg(gpui::rgb(palette().panel_bg))
        .border_r_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .pt_1()
        .children(slots.into_iter().map(
            |(group, icon_name, is_active, has_siblings, name, hint)| {
                div()
                    .id(SharedString::from(format!("tool-slot-{group}")))
                    .relative()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(m.tool_slot))
                    .my(px(1.0))
                    .rounded_sm()
                    .cursor_pointer()
                    .tooltip(ui::tip(name, hint))
                    .when_active(is_active)
                    .hover(move |s| {
                        if is_active {
                            s
                        } else {
                            s.bg(gpui::rgb(palette().hover))
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                            ws.press_tool_group(group, ev.position, cx);
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |ws, _ev, _w, cx| {
                            ws.release_tool_group(group, cx);
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(move |ws, _ev, _w, _cx| {
                            ws.abandon_tool_press(group);
                        }),
                    )
                    // Right-click opens the flyout immediately, for
                    // people who don't want to wait out the hold.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                            ws.open_tool_flyout(group, ev.position, cx);
                        }),
                    )
                    .child(icon(
                        icon_name,
                        m.tool_icon,
                        if is_active {
                            palette().accent_text
                        } else {
                            palette().text
                        },
                    ))
                    .children(has_siblings.then(|| {
                        // The corner mark that means "more tools here".
                        div()
                            .absolute()
                            .right(px(2.0))
                            .bottom(px(2.0))
                            .size(px(4.0))
                            .bg(gpui::rgb(if is_active {
                                palette().accent_text
                            } else {
                                palette().text_dim
                            }))
                    }))
            },
        ))
        .child(color_wells(ws, cx))
}

/// The flyout listing a group's tools, opened by holding or right-clicking
/// its toolbar slot.
pub fn tool_flyout(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Option<gpui::AnyElement> {
    let (group, position) = ws.tool_flyout?;
    let active = ws.editor.active_tool;
    let shortcut = ws
        .group_shortcut(group)
        .map(|s| s.to_uppercase())
        .unwrap_or_default();
    let tools: Vec<&'static str> = ws
        .tool_groups
        .iter()
        .find(|(g, _)| *g == group)
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    let rows: Vec<gpui::AnyElement> = tools
        .into_iter()
        .map(|id| {
            let (name, icon_name) = ws
                .registry
                .tool_mut(id)
                .map(|t| (t.name(), t.icon()))
                .unwrap_or((id, "move"));
            let selected = id == active;
            let shortcut = shortcut.clone();
            ListItem::new(id)
                .gap_2()
                .selected(selected)
                .on_click(cx.listener(move |ws, _e, _w, cx| {
                    ws.close_tool_flyout(cx);
                    ws.activate_tool(id, cx);
                }))
                .child(icon(icon_name, 14.0, palette().text))
                .child(div().flex_grow().text_size(px(12.0)).child(name))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(shortcut),
                )
                .into_any_element()
        })
        .collect();

    Some(
        deferred(
            div()
                .absolute()
                // Sits just right of the toolbar, level with the slot.
                .left(px(42.0))
                .top(px(f32::from(position.y) - 12.0))
                .w(px(200.0))
                .py_1()
                .bg(gpui::rgb(palette().popup_bg))
                .text_color(gpui::rgb(palette().text))
                .border_1()
                .border_color(gpui::rgb(palette().edge))
                .rounded_sm()
                .shadow_lg()
                .occlude()
                .on_mouse_down_out(cx.listener(|ws, _e, _w, cx| ws.close_tool_flyout(cx)))
                .children(rows),
        )
        .into_any_element(),
    )
}
