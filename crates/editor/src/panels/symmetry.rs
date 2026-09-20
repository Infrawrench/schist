//! Symmetry and pattern painting controls beside the brush controls.
use super::*;
use schist_plugin_api::SymmetryMode;

const POPUP: Popup = Popup::Field("paint-symmetry");

pub(super) fn controls(ws: &Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let open = ws.open_popup == Some(POPUP);
    div()
        .relative()
        .child(
            DropdownButton::new("paint-symmetry", t("tool.brush.symmetry")).on_press(cx.listener(
                |ws, _e, _w, cx| {
                    ws.commit_focused_field();
                    ws.toggle_popup(POPUP, cx);
                },
            )),
        )
        .children(open.then(|| deferred(popover(ws, cx))))
        .into_any_element()
}

fn popover(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let symmetry = ws.editor.paint_symmetry;
    Popover::new("paint-symmetry-popup")
        .top(px(ui::metrics().icon_button + 6.0))
        .left_0()
        .w(px(310.0))
        .p_3()
        .gap_2()
        .on_dismiss(cx.listener(|ws, _e, _w, cx| ws.close_popup(cx)))
        .child(
            div().flex().flex_wrap().gap_1().children(
                [
                    (SymmetryMode::None, "common.none"),
                    (SymmetryMode::Vertical, "common.vertical"),
                    (SymmetryMode::Horizontal, "common.horizontal"),
                    (SymmetryMode::Radial, "tool.gradient.choice.radial"),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, (mode, key))| {
                    Button::new(("symmetry-mode", i), t(key))
                        .active(symmetry.mode == mode)
                        .on_click(cx.listener(move |ws, _e, _w, cx| {
                            ws.editor.paint_symmetry.mode = mode;
                            ws.editor.symmetry_positioning = false;
                            cx.notify();
                        }))
                }),
            ),
        )
        .children((symmetry.mode == SymmetryMode::Radial).then(|| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(t("tool.gradient.choice.radial"))
                .child(
                    Slider::new(
                        "symmetry-segments",
                        (symmetry.segments.clamp(2, 24) - 2) as f32 / 22.0,
                    )
                    .flex_grow()
                    .on_change(cx.listener(|ws, ratio: &f32, _w, cx| {
                        ws.editor.paint_symmetry.segments = (2.0 + ratio * 22.0).round() as u8;
                        cx.notify();
                    })),
                )
                .child(symmetry.segments.to_string())
        }))
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("symmetry-position", t("common.position"))
                        .disabled(symmetry.mode == SymmetryMode::None)
                        .active(ws.editor.symmetry_positioning)
                        .on_click(cx.listener(|ws, _e, _w, cx| {
                            ws.editor.symmetry_positioning = true;
                            ws.close_popup(cx);
                        })),
                )
                .child(
                    Button::new("symmetry-center", t("common.center"))
                        .disabled(symmetry.mode == SymmetryMode::None)
                        .on_click(cx.listener(|ws, _e, _w, cx| {
                            ws.editor.paint_symmetry.center = [0.5, 0.5];
                            ws.editor.symmetry_positioning = false;
                            cx.notify();
                        })),
                ),
        )
        .child(
            Button::new("seamless-preview", t("tool.brush.seamless_preview"))
                .active(ws.editor.seamless_painting)
                .on_click(cx.listener(|ws, _e, _w, cx| {
                    ws.editor.seamless_painting = !ws.editor.seamless_painting;
                    ws.close_popup(cx);
                })),
        )
}
