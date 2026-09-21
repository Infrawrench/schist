//! Comparison chrome shared with the local gallery, independent of image storage.
use crate::pal;
use gpui::{
    canvas, div, img, px, Bounds, Context, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, Point, RenderImage, Styled as _,
};
use schist_gallery::culling::{CompareCamera, PhotoCulling};
use schist_i18n::t;
use schist_ui::Button;
use std::sync::Arc;

pub enum CompareAction {
    Close,
    Fit,
    ActualSize,
    ZoomOut,
    ZoomIn,
}

pub struct ComparisonToolbar {
    pub title: &'static str,
    pub zoom: f32,
    pub actual_size_enabled: bool,
}

pub fn toolbar<T: 'static>(
    state: ComparisonToolbar,
    action: fn(&mut T, CompareAction, &mut Context<T>),
    cx: &mut Context<T>,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .items_center()
        .px_2()
        .py_1()
        .bg(gpui::rgb(pal().chrome_bg))
        .border_b_1()
        .border_color(gpui::rgb(pal().chrome_edge))
        .child(
            Button::new("compare-close", t("common.close"))
                .ghost()
                .px_2()
                .on_click(
                    cx.listener(move |host, _, _, cx| action(host, CompareAction::Close, cx)),
                ),
        )
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(state.title),
        )
        .child(div().flex_grow())
        .child(
            Button::new("compare-fit", t("menu.view.fit_on_screen"))
                .ghost()
                .px_2()
                .on_click(cx.listener(move |host, _, _, cx| action(host, CompareAction::Fit, cx))),
        )
        .child(
            Button::new("compare-actual", t("menu.view.actual_size"))
                .disabled(!state.actual_size_enabled)
                .ghost()
                .px_2()
                .on_click(
                    cx.listener(move |host, _, _, cx| action(host, CompareAction::ActualSize, cx)),
                ),
        )
        .child(
            Button::new("compare-out", "−")
                .ghost()
                .w(px(24.0))
                .px_0()
                .on_click(
                    cx.listener(move |host, _, _, cx| action(host, CompareAction::ZoomOut, cx)),
                ),
        )
        .child(
            div()
                .min_w(px(40.0))
                .text_center()
                .text_color(gpui::rgb(pal().text_dim))
                .child(format!("{:.2}×", state.zoom)),
        )
        .child(
            Button::new("compare-in", "+")
                .ghost()
                .w(px(24.0))
                .px_0()
                .on_click(
                    cx.listener(move |host, _, _, cx| action(host, CompareAction::ZoomIn, cx)),
                ),
        )
        .into_any_element()
}

pub fn pane_row() -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .p_2()
        .gap_2()
        .flex_grow()
        .min_h(px(0.0))
        .min_w(px(0.0))
}

pub struct ComparisonPane {
    pub title: String,
    pub tooltip: String,
    pub active: bool,
    pub image: Option<(Arc<RenderImage>, [f32; 2])>,
    pub preview: bool,
    pub camera: CompareCamera,
    pub bounds: Bounds<Pixels>,
    pub message: Option<String>,
    pub culling: Option<PhotoCulling>,
    pub overlay: Option<gpui::AnyElement>,
}

pub struct ComparisonActions<T: 'static> {
    pub select: fn(&mut T, usize, Option<Point<Pixels>>, &mut Context<T>),
    pub drag: fn(&mut T, Point<Pixels>, bool, &mut Context<T>),
    pub release: fn(&mut T, &mut Context<T>),
    pub zoom: fn(&mut T, f32, &mut Context<T>),
    pub bounds: fn(&mut T, usize, Bounds<Pixels>, &mut Context<T>),
}

pub fn pane<T: 'static>(
    index: usize,
    state: ComparisonPane,
    actions: ComparisonActions<T>,
    cx: &mut Context<T>,
) -> gpui::AnyElement {
    let entity = cx.entity();
    let mut picture = div()
        .id(("compare-picture", index))
        .relative()
        .flex_grow()
        .min_h(px(0.0))
        .overflow_hidden()
        .bg(gpui::rgb(0x161616))
        .cursor(gpui::CursorStyle::OpenHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |host, ev: &gpui::MouseDownEvent, _, cx| {
                (actions.select)(host, index, Some(ev.position), cx);
            }),
        )
        .on_mouse_move(cx.listener(move |host, ev: &gpui::MouseMoveEvent, _, cx| {
            (actions.drag)(
                host,
                ev.position,
                ev.pressed_button == Some(MouseButton::Left),
                cx,
            );
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |host, _, _, cx| (actions.release)(host, cx)),
        )
        .on_scroll_wheel(
            cx.listener(move |host, ev: &gpui::ScrollWheelEvent, _, cx| {
                let dy = match ev.delta {
                    gpui::ScrollDelta::Pixels(p) => f32::from(p.y),
                    gpui::ScrollDelta::Lines(l) => l.y * 40.0,
                };
                (actions.zoom)(host, (dy * 0.008).exp(), cx);
                cx.stop_propagation();
            }),
        )
        .child(
            canvas(
                move |bounds, _, cx| {
                    entity.update(cx, |host, cx| (actions.bounds)(host, index, bounds, cx));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
    if let Some((image, dimensions)) = &state.image {
        let area = state.bounds.size;
        let [x, y, w, h] = state
            .camera
            .image_rect(*dimensions, [area.width.into(), area.height.into()]);
        picture = picture.child(
            img(image.clone())
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h)),
        );
        if state.preview {
            picture = picture.child(
                div()
                    .absolute()
                    .top_1()
                    .left_1()
                    .text_color(gpui::rgb(0xFFFFFF))
                    .child(t("common.preview")),
            );
        }
    } else {
        picture = picture.child(
            div().p_3().text_color(gpui::rgb(0xFFFFFF)).child(
                state
                    .message
                    .clone()
                    .unwrap_or_else(|| t("common.loading").into()),
            ),
        );
    }
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .rounded_sm()
        .overflow_hidden()
        .border_1()
        .border_color(gpui::rgb(if state.active {
            pal().select_border
        } else {
            pal().chrome_edge
        }))
        .child(
            Button::bare(("compare-name", index))
                .ghost()
                .h(px(28.0))
                .w_full()
                .min_w(px(0.0))
                .px_2()
                .justify_start()
                .bg(gpui::rgb(if state.active {
                    pal().select_fill
                } else {
                    pal().chrome_bg
                }))
                .tooltip(state.tooltip, None)
                .child(
                    div()
                        .flex_none()
                        .text_color(gpui::rgb(pal().text_dim))
                        .child((index + 1).to_string()),
                )
                .child(div().flex_1().min_w(px(0.0)).truncate().child(state.title))
                .on_click(cx.listener(move |host, _, _, cx| {
                    (actions.select)(host, index, None, cx);
                })),
        )
        .child(
            picture
                .children(state.culling.and_then(crate::culling::badge))
                .children(state.overlay),
        )
        .into_any_element()
}
