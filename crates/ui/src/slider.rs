//! A horizontal slider, and the progress bar that shares its track.

use crate::{metrics, palette, touch};
use gpui::{
    div, px, App, Bounds, DispatchPhase, ElementId, Entity, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point,
    Refineable as _, RenderOnce, StyleRefinement, Styled, Window,
};
use std::cell::RefCell;
use std::rc::Rc;

/// The two colours of a track: its bed, and the filled part.
#[derive(Clone, Copy, Debug)]
pub struct TrackColors {
    pub track: u32,
    pub fill: u32,
}

impl TrackColors {
    fn slider() -> Self {
        let p = palette();
        TrackColors {
            track: p.field_bg,
            fill: p.accent,
        }
    }

    fn progress() -> Self {
        let p = palette();
        TrackColors {
            track: p.button_bg,
            fill: p.accent,
        }
    }
}

/// A handler told a 0..=1 ratio.
type RatioHandler = Rc<dyn Fn(&f32, &mut Window, &mut App) + 'static>;

/// A drag in progress: the ratio it began at, kept from frame to frame
/// in the element's own state.
#[derive(Default)]
struct Drag {
    started_at: Option<f32>,
}

/// What the layout pass leaves for the press handler: the drag state
/// and the track's bounds this frame.
type DragSlot = Rc<RefCell<Option<(Entity<Drag>, Bounds<Pixels>)>>>;

/// A track that reports a 0..=1 ratio. 72×12 px by default, filled in
/// the accent up to the ratio it shows; on a touch screen it looks like
/// the system's, a thin bar with a round thumb to put a finger on,
/// inside a target-height hit area (see [`metrics`]).
///
/// A press anywhere on the track jumps to it and starts a drag; the
/// drag follows the pointer even once it leaves the track, and ends on
/// release wherever the pointer is. [`Slider::on_change`] fires for
/// every movement, [`Slider::on_release`] once at the end with the
/// ratio the drag began at, which is what an undo entry wants.
#[derive(gpui::IntoElement)]
pub struct Slider {
    id: ElementId,
    ratio: f32,
    colors: Option<TrackColors>,
    style: StyleRefinement,
    on_change: Option<RatioHandler>,
    on_release: Option<RatioHandler>,
}

impl Slider {
    pub fn new(id: impl Into<ElementId>, ratio: f32) -> Self {
        Slider {
            id: id.into(),
            ratio: ratio.clamp(0.0, 1.0),
            colors: None,
            style: StyleRefinement::default(),
            on_change: None,
            on_release: None,
        }
    }

    /// Colours of the caller's own choosing.
    pub fn colors(mut self, colors: TrackColors) -> Self {
        self.colors = Some(colors);
        self
    }

    /// Told the new ratio on the press and on every movement of the
    /// drag that follows.
    pub fn on_change(mut self, handler: impl Fn(&f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// Told the ratio the drag began at when the button comes back up.
    pub fn on_release(mut self, handler: impl Fn(&f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_release = Some(Rc::new(handler));
        self
    }
}

impl Styled for Slider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// Where along `bounds` a window position falls, 0..=1.
fn ratio_at(bounds: &Bounds<Pixels>, at: Point<Pixels>) -> Option<f32> {
    let w = f32::from(bounds.size.width);
    if w <= 0.0 {
        return None;
    }
    Some(((f32::from(at.x) - f32::from(bounds.origin.x)) / w).clamp(0.0, 1.0))
}

impl RenderOnce for Slider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let colors = self.colors.unwrap_or_else(TrackColors::slider);
        let ratio = self.ratio;
        // The track's bounds and drag state are learnt while the frame
        // is laid out, after this closure has built the press handler;
        // a shared slot carries them across.
        let slot: DragSlot = Default::default();
        let on_change = self.on_change;
        let on_release = self.on_release;

        let canvas = {
            let slot = slot.clone();
            let on_change = on_change.clone();
            gpui::canvas(
                move |bounds, window, cx| {
                    let drag = window.use_keyed_state("drag", cx, |_, _| Drag::default());
                    *slot.borrow_mut() = Some((drag.clone(), bounds));
                    drag
                },
                move |bounds, drag: Entity<Drag>, window, _cx| {
                    // Movement and release are watched window-wide, so
                    // the drag survives the pointer leaving the track.
                    let moving = drag.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble
                            || ev.pressed_button != Some(MouseButton::Left)
                            || moving.read(cx).started_at.is_none()
                        {
                            return;
                        }
                        if let (Some(r), Some(on_change)) =
                            (ratio_at(&bounds, ev.position), &on_change)
                        {
                            on_change(&r, window, cx);
                        }
                    });
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble || ev.button != MouseButton::Left {
                            return;
                        }
                        let started_at = drag.update(cx, |d, _| d.started_at.take());
                        if let (Some(from), Some(on_release)) = (started_at, &on_release) {
                            on_release(&from, window, cx);
                        }
                    });
                },
            )
            .absolute()
            .size_full()
        };

        let m = metrics();
        let mut el = div()
            .relative()
            .flex_none()
            .w(px(m.slider_w))
            .h(px(m.slider_h))
            .rounded_sm();
        if !touch() {
            el = el.bg(gpui::rgb(colors.track));
        }
        el.style().refine(&self.style);
        let mut el = el.id(self.id);
        if touch() {
            // A thin bar down the middle of the hit area, and a white
            // thumb whose left edge runs from 0 to (width - thumb).
            let thumb = m.slider_h;
            let bar_h = 6.0;
            el = el
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px((m.slider_h - bar_h) / 2.0))
                        .h(px(bar_h))
                        .rounded_full()
                        .bg(gpui::rgb(colors.track))
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(gpui::relative(ratio))
                                .rounded_full()
                                .bg(gpui::rgb(colors.fill)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(gpui::relative(ratio))
                        .ml(px(-thumb * ratio))
                        .size(px(thumb))
                        .rounded_full()
                        .bg(gpui::rgb(0xFFFFFF))
                        .shadow_sm(),
                );
        } else {
            el = el.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(gpui::relative(ratio))
                    .rounded_sm()
                    .bg(gpui::rgb(colors.fill)),
            );
        }
        el.child(canvas)
            .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
                let Some((drag, bounds)) = slot.borrow().clone() else {
                    return;
                };
                // A finger on the slider drags it; it must not scroll.
                window.claim_touch_drag();
                drag.update(cx, |d, _| d.started_at = Some(ratio));
                if let (Some(r), Some(on_change)) = (ratio_at(&bounds, ev.position), &on_change) {
                    on_change(&r, window, cx);
                }
            })
    }
}

/// A bar filled to a ratio: 4 px high, as wide as its row.
#[derive(gpui::IntoElement)]
pub struct ProgressBar {
    ratio: f32,
    colors: Option<TrackColors>,
    style: StyleRefinement,
}

impl ProgressBar {
    pub fn new(ratio: f32) -> Self {
        ProgressBar {
            ratio: ratio.clamp(0.0, 1.0),
            colors: None,
            style: StyleRefinement::default(),
        }
    }

    /// Colours of the caller's own choosing.
    pub fn colors(mut self, colors: TrackColors) -> Self {
        self.colors = Some(colors);
        self
    }
}

impl Styled for ProgressBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let colors = self.colors.unwrap_or_else(TrackColors::progress);
        let mut el = div()
            .w_full()
            .h(px(4.0))
            .rounded_sm()
            .bg(gpui::rgb(colors.track));
        el.style().refine(&self.style);
        el.child(
            div()
                .h_full()
                .w(gpui::relative(self.ratio))
                .rounded_sm()
                .bg(gpui::rgb(colors.fill)),
        )
    }
}
