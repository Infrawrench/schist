//! The panel sliders and what each one reads and writes.

use super::*;

#[derive(Clone, Copy, PartialEq)]
pub enum SliderTarget {
    /// A control the active tool declared for itself, mapped from the
    /// slider's 0..=1 ratio into the option's own range.
    ToolOption {
        key: &'static str,
        min: f32,
        max: f32,
    },
    BrushSize,
    BrushHardness,
    ToolOpacity,
    LayerOpacity(LayerId),
    ForegroundR,
    ForegroundG,
    ForegroundB,
}

pub(super) fn slider_get(ws: &Workspace, target: SliderTarget) -> f32 {
    match target {
        SliderTarget::ToolOption { key, min, max } => {
            let v = ws
                .registry
                .tools()
                .find(|t| t.id() == ws.editor.active_tool)
                .and_then(|t| t.options().into_iter().find(|o| o.key == key))
                .map(|o| o.value.num())
                .unwrap_or(min);
            ((v - min) / (max - min).max(1e-6)).clamp(0.0, 1.0)
        }
        SliderTarget::BrushSize => ((ws.editor.brush_size - 1.0) / 299.0).clamp(0.0, 1.0),
        SliderTarget::BrushHardness => ws.editor.brush_hardness,
        SliderTarget::ToolOpacity => ws.editor.tool_opacity,
        SliderTarget::LayerOpacity(id) => ws
            .doc
            .as_ref()
            .and_then(|d| d.tree.find(id))
            .map(|l| l.opacity)
            .unwrap_or(1.0),
        SliderTarget::ForegroundR => ws.editor.foreground.r,
        SliderTarget::ForegroundG => ws.editor.foreground.g,
        SliderTarget::ForegroundB => ws.editor.foreground.b,
    }
}

pub(super) fn slider_set(
    ws: &mut Workspace,
    target: SliderTarget,
    ratio: f32,
    cx: &mut Context<Workspace>,
) {
    match target {
        SliderTarget::ToolOption { key, min, max } => ws.set_tool_option(
            key,
            schist_plugin_api::OptionValue::Num(min + ratio * (max - min)),
            cx,
        ),
        SliderTarget::BrushSize => ws.editor.brush_size = 1.0 + ratio * 299.0,
        SliderTarget::BrushHardness => ws.editor.brush_hardness = ratio,
        SliderTarget::ToolOpacity => ws.editor.tool_opacity = ratio,
        SliderTarget::LayerOpacity(id) => ws.set_layer_opacity_live(id, ratio),
        SliderTarget::ForegroundR => ws.editor.foreground.r = ratio,
        SliderTarget::ForegroundG => ws.editor.foreground.g = ratio,
        SliderTarget::ForegroundB => ws.editor.foreground.b = ratio,
    }
    if matches!(target, SliderTarget::LayerOpacity(_)) {
        ws.after_change(cx);
    } else {
        cx.notify();
    }
}

/// A horizontal slider with its label and readout.
pub(super) fn slider(
    id: &'static str,
    label: &'static str,
    display: String,
    target: SliderTarget,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    slider_impl(id, label, display, target, false, ws, cx)
}

/// The same slider filling whatever row it is in: what the touch chrome's
/// popup strip shows.
pub(super) fn slider_stretch(
    id: &'static str,
    label: &'static str,
    display: String,
    target: SliderTarget,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    slider_impl(id, label, display, target, true, ws, cx)
}

fn slider_impl(
    id: &'static str,
    label: &'static str,
    display: String,
    target: SliderTarget,
    stretch: bool,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let ratio = slider_get(ws, target);
    let m = ui::metrics();
    // The kit's slider draws the desktop's filled bar or the touch
    // chrome's thumb, records its own bounds and drag, and claims a
    // finger's drag so it does not scroll.
    let track = Slider::new(id, ratio)
        .when(stretch, |s| s.flex_grow().min_w(px(0.0)))
        .when(!stretch, |s| s.w(px(m.slider_w)))
        .on_change(cx.listener(move |ws, r, _w, cx| slider_set(ws, target, *r, cx)))
        // The kit hands back the ratio the drag began at, which is what
        // the layer-opacity undo entry wants.
        .on_release(cx.listener(move |ws, before, _w, cx| {
            if let SliderTarget::LayerOpacity(layer) = target {
                ws.commit_layer_opacity(layer, *before, cx);
            }
        }));
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .when(stretch, |d| d.flex_grow().gap_3());
    if !label.is_empty() {
        row = row.child(
            div()
                .text_size(px(m.small_text))
                .text_color(gpui::rgb(palette().text_dim))
                .child(label),
        );
    }
    row.child(track).child(
        div()
            // "180 px" is wider than the old 34px slot. Keep quantities
            // on one line, including at the largest three-digit values.
            .w(px(m.small_text * 4.0))
            .flex_none()
            .whitespace_nowrap()
            .text_size(px(m.small_text))
            .child(display),
    )
}

// ===== document tabs =====
