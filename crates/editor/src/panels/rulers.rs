//! Rulers down the canvas edges, and dragging guides out of them.

use super::*;

/// Horizontal and vertical rulers around the canvas. Raster rulers also
/// support pulling out guides; Design rulers use page-relative units.
pub fn rulers(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    if ws.design_mode() {
        return design_rulers(ws, cx);
    }
    let size = Workspace::RULER_SIZE;
    let zoom = ws.zoom;
    // Choose a tick spacing that stays legible at any zoom.
    let step = [
        1.0f32, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0,
    ]
    .into_iter()
    .find(|s| s * zoom >= 60.0)
    .unwrap_or(5000.0);
    let bounds = ws.canvas_bounds();
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));

    let mut h_ticks = Vec::new();
    let mut v_ticks = Vec::new();
    if w > 0.0 && zoom > 0.0 {
        let first = (ws.doc_x_at(f32::from(bounds.origin.x)) / step).floor() * step;
        let last = ws.doc_x_at(f32::from(bounds.origin.x) + w);
        let mut x = first;
        while x <= last && h_ticks.len() < 200 {
            let sx = ws.screen_x(x) - f32::from(bounds.origin.x);
            if sx >= 0.0 {
                h_ticks.push((sx, x));
            }
            x += step;
        }
        let first = (ws.doc_y_at(f32::from(bounds.origin.y)) / step).floor() * step;
        let last = ws.doc_y_at(f32::from(bounds.origin.y) + h);
        let mut y = first;
        while y <= last && v_ticks.len() < 200 {
            let sy = ws.screen_y(y) - f32::from(bounds.origin.y);
            if sy >= 0.0 {
                v_ticks.push((sy, y));
            }
            y += step;
        }
    }

    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .child(
            // Top ruler.
            div()
                .absolute()
                .top_0()
                .left(px(size))
                .right_0()
                .h(px(size))
                .bg(gpui::rgb(palette().ruler_bg))
                .border_b_1()
                .border_color(gpui::rgb(palette().panel_edge))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, ev: &MouseDownEvent, _w, cx| {
                        let y = ws.doc_y_at(f32::from(ev.position.y));
                        ws.begin_guide(true, y);
                        cx.notify();
                    }),
                )
                .children(h_ticks.into_iter().map(|(sx, value)| {
                    div().absolute().top_0().left(px(sx)).h(px(size)).child(
                        div()
                            .text_size(px(9.0))
                            .text_color(gpui::rgb(palette().text_dim))
                            .pl(px(2.0))
                            .child(format!("{value:.0}")),
                    )
                })),
        )
        .child(
            // Left ruler.
            div()
                .absolute()
                .top(px(size))
                .left_0()
                .bottom_0()
                .w(px(size))
                .bg(gpui::rgb(palette().ruler_bg))
                .border_r_1()
                .border_color(gpui::rgb(palette().panel_edge))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, ev: &MouseDownEvent, _w, cx| {
                        let x = ws.doc_x_at(f32::from(ev.position.x));
                        ws.begin_guide(false, x);
                        cx.notify();
                    }),
                )
                .children(v_ticks.into_iter().map(|(sy, value)| {
                    div()
                        .absolute()
                        .left_0()
                        .top(px(sy))
                        .w(px(size))
                        .text_size(px(9.0))
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(format!("{value:.0}"))
                })),
        )
        .child(
            // Corner square.
            div()
                .absolute()
                .top_0()
                .left_0()
                .size(px(size))
                .bg(gpui::rgb(palette().panel_bg)),
        )
        .into_any_element()
}

/// Layout rulers measure from the active page's trim, including on later
/// spreads. Guide gestures belong to the layout document.
fn design_rulers(ws: &Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    use crate::design::rulers::ticks;
    const SIZE: f32 = 32.0;
    let origin = ws.design.to_pasteboard(schist_layout::Point::ZERO);
    let scale = ws.design.view.scale;
    let unit = ws.design.ruler_unit;
    let bounds = ws.canvas_bounds();
    let horizontal = ticks(origin.x, scale, f32::from(bounds.size.width), unit);
    let vertical = ticks(origin.y, scale, f32::from(bounds.size.height), unit);
    div()
        .absolute()
        .inset_0()
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(SIZE))
                .bg(gpui::rgb(palette().ruler_bg))
                .overflow_hidden()
                 .on_mouse_down(MouseButton::Left, cx.listener(|ws, ev: &MouseDownEvent, _, cx| {
                    let at=ws.design_page_point(ev.position);
                    crate::design::guides::begin(&mut ws.design,true,at);cx.stop_propagation();cx.notify();
                }))
                .children(horizontal.into_iter().map(|tick| {
                    let length = if tick.label.is_some() { 9.0 } else { 4.0 };
                    div()
                        .absolute()
                        .left(px(tick.offset))
                        .top_0()
                        .h(px(SIZE))
                        .child(
                            div()
                                .absolute()
                                .bottom_0()
                                .w(px(1.0))
                                .h(px(length))
                                .bg(gpui::rgb(palette().text_dim)),
                        )
                        .children(tick.label.map(|label| {
                            div()
                                .pl(px(2.0))
                                .text_size(px(9.0))
                                .text_color(gpui::rgb(palette().text_dim))
                                .child(label)
                        }))
                })),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .bottom_0()
                .w(px(SIZE))
                .bg(gpui::rgb(palette().ruler_bg))
                .overflow_hidden()
                 .on_mouse_down(MouseButton::Left, cx.listener(|ws, ev: &MouseDownEvent, _, cx| {
                    let at=ws.design_page_point(ev.position);
                    crate::design::guides::begin(&mut ws.design,false,at);cx.stop_propagation();cx.notify();
                }))
                .children(vertical.into_iter().map(|tick| {
                    let length = if tick.label.is_some() { 9.0 } else { 4.0 };
                    div()
                        .absolute()
                        .top(px(tick.offset))
                        .left_0()
                        .w(px(SIZE))
                        .child(
                            div()
                                .absolute()
                                .right_0()
                                .h(px(1.0))
                                .w(px(length))
                                .bg(gpui::rgb(palette().text_dim)),
                        )
                        .children(tick.label.map(|label| {
                            div()
                                .pt(px(2.0))
                                .text_size(px(8.0))
                                .text_color(gpui::rgb(palette().text_dim))
                                .child(label)
                        }))
                })),
        )
        .child(
            div()
                .id("design-ruler-unit")
                .absolute()
                .top_0()
                .left_0()
                .size(px(SIZE))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .bg(gpui::rgb(palette().panel_bg))
                .cursor_pointer()
                .tooltip(ui::tip(t("design.ruler_unit_hint"), None))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.design.ruler_unit = ws.design.ruler_unit.next();
                    cx.notify();
                }))
                .child(t(unit.label())),
        )
        .into_any_element()
}
