//! The Layer Style dialog.
//!
//! Laid out like Photoshop's: the effects list down the left with a
//! checkbox each, the selected effect's settings on the right. Edits write
//! straight onto the layer so the canvas previews them live, and OK records
//! one history entry for the whole session (Cancel puts the old style
//! back), the same shape the adjustment dialogs use.

use crate::dialogs::{param_slider, stroke_position_name, SliderSpec};
use crate::ui;
use crate::workspace::{ColorTarget, Modal, Popup, Workspace};
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled,
};
use schist_color::Rgba;
use schist_core::{
    BevelStyle_, BlendMode, GradientShape, LayerId, LayerStyle, StrokePosition, Technique,
};
use schist_i18n::t;
use schist_ui::{Checkbox, ListItem, Swatch};

/// The effects, in the order Photoshop lists them: each one's key in
/// the style, and the catalog key of its name.
pub const EFFECTS: &[(&str, &str)] = &[
    ("bevel", "dialog.style.bevel"),
    ("stroke", "dialog.style.stroke"),
    ("inner_shadow", "dialog.style.inner_shadow"),
    ("inner_glow", "dialog.style.inner_glow"),
    ("satin", "dialog.style.satin"),
    ("color_overlay", "dialog.style.color_overlay"),
    ("gradient_overlay", "dialog.style.gradient_overlay"),
    ("outer_glow", "dialog.style.outer_glow"),
    ("drop_shadow", "dialog.style.drop_shadow"),
    // Affinity's own, at the bottom of its panel: not a decoration
    // around the layer but a softening of the layer itself.
    ("blur", "dialog.style.blur"),
];

fn enabled(style: &LayerStyle, key: &str) -> bool {
    match key {
        "bevel" => style.bevel.enabled,
        "stroke" => style.stroke.enabled,
        "inner_shadow" => style.inner_shadow.enabled,
        "inner_glow" => style.inner_glow.enabled,
        "satin" => style.satin.enabled,
        "color_overlay" => style.color_overlay.enabled,
        "gradient_overlay" => style.gradient_overlay.enabled,
        "outer_glow" => style.outer_glow.enabled,
        "drop_shadow" => style.drop_shadow.enabled,
        "blur" => style.blur.enabled,
        _ => false,
    }
}

fn set_enabled(style: &mut LayerStyle, key: &str, on: bool) {
    match key {
        "bevel" => style.bevel.enabled = on,
        "stroke" => style.stroke.enabled = on,
        "inner_shadow" => style.inner_shadow.enabled = on,
        "inner_glow" => style.inner_glow.enabled = on,
        "satin" => style.satin.enabled = on,
        "color_overlay" => style.color_overlay.enabled = on,
        "gradient_overlay" => style.gradient_overlay.enabled = on,
        "outer_glow" => style.outer_glow.enabled = on,
        "drop_shadow" => style.drop_shadow.enabled = on,
        "blur" => style.blur.enabled = on,
        _ => {}
    }
}

/// Every numeric control in the dialog, addressed by `<effect>.<field>`.
/// Keeping them in one table means the widgets, the setter and the layout
/// cannot drift apart. `label` is the catalog key of the row's label.
struct Field {
    key: &'static str,
    label: &'static str,
    min: f32,
    max: f32,
    suffix: &'static str,
}

const fn f(
    key: &'static str,
    label: &'static str,
    min: f32,
    max: f32,
    suffix: &'static str,
) -> Field {
    Field {
        key,
        label,
        min,
        max,
        suffix,
    }
}

fn fields(effect: &str) -> &'static [Field] {
    const SHADOW: &[Field] = &[
        f("opacity", "common.opacity", 0.0, 100.0, "%"),
        f("angle", "common.angle", -180.0, 180.0, "\u{b0}"),
        f("distance", "common.distance", 0.0, 250.0, " px"),
        f("spread", "common.spread", 0.0, 100.0, "%"),
        f("size", "common.size", 0.0, 250.0, " px"),
    ];
    const GLOW: &[Field] = &[
        f("opacity", "common.opacity", 0.0, 100.0, "%"),
        f("spread", "common.spread", 0.0, 100.0, "%"),
        f("size", "common.size", 0.0, 250.0, " px"),
    ];
    const STROKE: &[Field] = &[
        f("size", "common.size", 1.0, 250.0, " px"),
        f("opacity", "common.opacity", 0.0, 100.0, "%"),
    ];
    const OVERLAY: &[Field] = &[f("opacity", "common.opacity", 0.0, 100.0, "%")];
    const GRADIENT: &[Field] = &[
        f("opacity", "common.opacity", 0.0, 100.0, "%"),
        f("angle", "common.angle", -180.0, 180.0, "\u{b0}"),
        f("scale", "common.scale", 0.1, 5.0, "\u{d7}"),
    ];
    const SATIN: &[Field] = &[
        f("opacity", "common.opacity", 0.0, 100.0, "%"),
        f("angle", "common.angle", -180.0, 180.0, "\u{b0}"),
        f("distance", "common.distance", 0.0, 250.0, " px"),
        f("size", "common.size", 0.0, 250.0, " px"),
    ];
    const BEVEL: &[Field] = &[
        f("size", "common.size", 0.0, 250.0, " px"),
        f("soften", "dialog.style.soften", 0.0, 16.0, " px"),
        f("depth", "dialog.style.depth", 0.0, 10.0, "\u{d7}"),
        f("angle", "common.angle", -180.0, 180.0, "\u{b0}"),
        f("altitude", "dialog.style.altitude", 0.0, 90.0, "\u{b0}"),
        f(
            "highlight_opacity",
            "dialog.style.highlight",
            0.0,
            100.0,
            "%",
        ),
        f("shadow_opacity", "dialog.style.shadow", 0.0, 100.0, "%"),
    ];
    const BLUR: &[Field] = &[f("radius", "common.radius", 0.0, 250.0, " px")];
    match effect {
        "drop_shadow" | "inner_shadow" => SHADOW,
        "outer_glow" | "inner_glow" => GLOW,
        "stroke" => STROKE,
        "color_overlay" => OVERLAY,
        "gradient_overlay" => GRADIENT,
        "satin" => SATIN,
        "bevel" => BEVEL,
        "blur" => BLUR,
        _ => &[],
    }
}

/// Percentages are stored 0..=1 but shown 0..=100.
fn is_percent(suffix: &str) -> bool {
    suffix == "%"
}

fn get(style: &LayerStyle, effect: &str, field: &str) -> f32 {
    match (effect, field) {
        ("drop_shadow", f) => shadow_get(&style.drop_shadow.settings, f),
        ("inner_shadow", f) => shadow_get(&style.inner_shadow.settings, f),
        ("outer_glow", f) => glow_get(&style.outer_glow.settings, f),
        ("inner_glow", f) => glow_get(&style.inner_glow.settings, f),
        ("stroke", "size") => style.stroke.settings.size,
        ("stroke", "opacity") => style.stroke.settings.opacity,
        ("color_overlay", "opacity") => style.color_overlay.settings.opacity,
        ("gradient_overlay", "opacity") => style.gradient_overlay.settings.opacity,
        ("gradient_overlay", "angle") => style.gradient_overlay.settings.angle,
        ("gradient_overlay", "scale") => style.gradient_overlay.settings.scale,
        ("satin", "opacity") => style.satin.settings.opacity,
        ("satin", "angle") => style.satin.settings.angle,
        ("satin", "distance") => style.satin.settings.distance,
        ("satin", "size") => style.satin.settings.size,
        ("bevel", "size") => style.bevel.settings.size,
        ("bevel", "soften") => style.bevel.settings.soften,
        ("bevel", "depth") => style.bevel.settings.depth,
        ("bevel", "angle") => style.bevel.settings.angle,
        ("bevel", "altitude") => style.bevel.settings.altitude,
        ("bevel", "highlight_opacity") => style.bevel.settings.highlight_opacity,
        ("bevel", "shadow_opacity") => style.bevel.settings.shadow_opacity,
        ("blur", "radius") => style.blur.settings.radius,
        _ => 0.0,
    }
}

fn shadow_get(s: &schist_core::ShadowStyle, field: &str) -> f32 {
    match field {
        "opacity" => s.opacity,
        "angle" => s.angle,
        "distance" => s.distance,
        "spread" => s.spread,
        "size" => s.size,
        _ => 0.0,
    }
}

fn glow_get(g: &schist_core::GlowStyle, field: &str) -> f32 {
    match field {
        "opacity" => g.opacity,
        "spread" => g.spread,
        "size" => g.size,
        _ => 0.0,
    }
}

fn set(style: &mut LayerStyle, effect: &str, field: &str, v: f32) {
    match effect {
        "drop_shadow" => shadow_set(&mut style.drop_shadow.settings, field, v),
        "inner_shadow" => shadow_set(&mut style.inner_shadow.settings, field, v),
        "outer_glow" => glow_set(&mut style.outer_glow.settings, field, v),
        "inner_glow" => glow_set(&mut style.inner_glow.settings, field, v),
        "stroke" => match field {
            "size" => style.stroke.settings.size = v,
            "opacity" => style.stroke.settings.opacity = v,
            _ => {}
        },
        "blur" => {
            if field == "radius" {
                style.blur.settings.radius = v;
            }
        }
        "color_overlay" => {
            if field == "opacity" {
                style.color_overlay.settings.opacity = v;
            }
        }
        "gradient_overlay" => match field {
            "opacity" => style.gradient_overlay.settings.opacity = v,
            "angle" => style.gradient_overlay.settings.angle = v,
            "scale" => style.gradient_overlay.settings.scale = v,
            _ => {}
        },
        "satin" => match field {
            "opacity" => style.satin.settings.opacity = v,
            "angle" => style.satin.settings.angle = v,
            "distance" => style.satin.settings.distance = v,
            "size" => style.satin.settings.size = v,
            _ => {}
        },
        "bevel" => match field {
            "size" => style.bevel.settings.size = v,
            "soften" => style.bevel.settings.soften = v,
            "depth" => style.bevel.settings.depth = v,
            "angle" => style.bevel.settings.angle = v,
            "altitude" => style.bevel.settings.altitude = v,
            "highlight_opacity" => style.bevel.settings.highlight_opacity = v,
            "shadow_opacity" => style.bevel.settings.shadow_opacity = v,
            _ => {}
        },
        _ => {}
    }
}

fn shadow_set(s: &mut schist_core::ShadowStyle, field: &str, v: f32) {
    match field {
        "opacity" => s.opacity = v,
        "angle" => s.angle = v,
        "distance" => s.distance = v,
        "spread" => s.spread = v,
        "size" => s.size = v,
        _ => {}
    }
}

fn glow_set(g: &mut schist_core::GlowStyle, field: &str, v: f32) {
    match field {
        "opacity" => g.opacity = v,
        "spread" => g.spread = v,
        "size" => g.size = v,
        _ => {}
    }
}

fn color_of(style: &LayerStyle, effect: &str) -> Option<Rgba> {
    Some(match effect {
        "drop_shadow" => style.drop_shadow.settings.color,
        "inner_shadow" => style.inner_shadow.settings.color,
        "outer_glow" => style.outer_glow.settings.color,
        "inner_glow" => style.inner_glow.settings.color,
        "stroke" => style.stroke.settings.color,
        "color_overlay" => style.color_overlay.settings.color,
        "satin" => style.satin.settings.color,
        _ => return None,
    })
}

/// The display name of a colour this dialog owns, for the Color Picker's
/// title bar.
pub fn effect_label(effect: &str) -> &'static str {
    t(match effect {
        "gradient_overlay.from" => "dialog.style.gradient_from",
        "gradient_overlay.to" => "dialog.style.gradient_to",
        _ => EFFECTS
            .iter()
            .find(|(key, _)| *key == effect)
            .map(|(_, label)| *label)
            .unwrap_or("dialog.style.title"),
    })
}

/// A bevel style's name.
fn bevel_style_name(style: BevelStyle_) -> &'static str {
    t(match style {
        BevelStyle_::OuterBevel => "dialog.style.outer_bevel",
        BevelStyle_::InnerBevel => "dialog.style.inner_bevel",
        BevelStyle_::Emboss => "dialog.style.emboss",
        BevelStyle_::PillowEmboss => "dialog.style.pillow_emboss",
    })
}

/// A gradient shape's name.
fn gradient_shape_name(shape: GradientShape) -> &'static str {
    t(match shape {
        GradientShape::Linear => "dialog.style.linear",
        GradientShape::Radial => "dialog.style.radial",
    })
}

pub fn set_color(style: &mut LayerStyle, effect: &str, c: Rgba) {
    match effect {
        "drop_shadow" => style.drop_shadow.settings.color = c,
        "inner_shadow" => style.inner_shadow.settings.color = c,
        "outer_glow" => style.outer_glow.settings.color = c,
        "inner_glow" => style.inner_glow.settings.color = c,
        "stroke" => style.stroke.settings.color = c,
        "color_overlay" => style.color_overlay.settings.color = c,
        "satin" => style.satin.settings.color = c,
        // The gradient's two ends are not "the effect's colour", so they
        // get their own keys rather than a slot in `color_of`.
        "gradient_overlay.from" => style.gradient_overlay.settings.from = c,
        "gradient_overlay.to" => style.gradient_overlay.settings.to = c,
        _ => {}
    }
}

/// A colour swatch that opens the Color Picker on `key`. The Layer Style
/// dialog stays open underneath it and takes the colour on OK.
fn color_swatch(
    id: &'static str,
    key: &'static str,
    c: Rgba,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    Swatch::new(id, gpui::rgb(rgb_of(c))).on_click(cx.listener(move |ws, _e, _w, cx| {
        ws.open_color_picker_on(ColorTarget::StyleEffect(key), c, cx);
    }))
}

fn blend_of(style: &LayerStyle, effect: &str) -> Option<BlendMode> {
    Some(match effect {
        "drop_shadow" => style.drop_shadow.settings.blend,
        "inner_shadow" => style.inner_shadow.settings.blend,
        "outer_glow" => style.outer_glow.settings.blend,
        "inner_glow" => style.inner_glow.settings.blend,
        "stroke" => style.stroke.settings.blend,
        "color_overlay" => style.color_overlay.settings.blend,
        "gradient_overlay" => style.gradient_overlay.settings.blend,
        "satin" => style.satin.settings.blend,
        _ => return None,
    })
}

fn set_blend(style: &mut LayerStyle, effect: &str, m: BlendMode) {
    match effect {
        "drop_shadow" => style.drop_shadow.settings.blend = m,
        "inner_shadow" => style.inner_shadow.settings.blend = m,
        "outer_glow" => style.outer_glow.settings.blend = m,
        "inner_glow" => style.inner_glow.settings.blend = m,
        "stroke" => style.stroke.settings.blend = m,
        "color_overlay" => style.color_overlay.settings.blend = m,
        "gradient_overlay" => style.gradient_overlay.settings.blend = m,
        "satin" => style.satin.settings.blend = m,
        _ => {}
    }
}

/// Mutate the open dialog's style and push it onto the layer so the canvas
/// updates on the next frame.
fn edit(ws: &mut Workspace, cx: &mut Context<Workspace>, f: impl FnOnce(&mut LayerStyle)) {
    let mut next = None;
    ws.update_modal(|m| {
        if let Modal::LayerStyle { style, layer, .. } = m {
            f(style);
            next = Some((*layer, **style));
        }
    });
    if let Some((layer, style)) = next {
        ws.preview_layer_style(layer, style, cx);
    }
}

pub fn render(
    ws: &mut Workspace,
    layer: LayerId,
    style: LayerStyle,
    active: &'static str,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let _ = layer;
    let list = div()
        .flex()
        .flex_col()
        .w(px(150.0))
        .flex_none()
        .gap(px(1.0))
        .children(EFFECTS.iter().map(|(key, label)| {
            let key = *key;
            let on = enabled(&style, key);
            let selected = key == active;
            ListItem::new(key)
                .gap_2()
                .h(px(22.0))
                .rounded_sm()
                .when_selected(selected)
                .child(
                    Checkbox::new(key, "", on).on_change(cx.listener(move |ws, _on, _w, cx| {
                        edit(ws, cx, |s| set_enabled(s, key, !on));
                    })),
                )
                .child(div().flex_grow().child(SharedString::from(t(label))))
                .on_click(cx.listener(move |ws, _e, _w, cx| {
                    ws.update_modal(|m| {
                        if let Modal::LayerStyle { active, .. } = m {
                            *active = key;
                        }
                    });
                    cx.notify();
                }))
        }));

    let mut settings = div().flex().flex_col().gap_1().flex_grow();
    let title = EFFECTS
        .iter()
        .find(|(k, _)| *k == active)
        .map(|(_, l)| t(l))
        .unwrap_or("");
    settings = settings.child(
        div()
            .text_size(px(12.0))
            .pb_1()
            .child(SharedString::from(title)),
    );

    if let Some(mode) = blend_of(&style, active) {
        settings = settings.child(ui::field_row(
            t("common.blend"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("fx-blend"),
                    is_open: ws.open_popup == Some(Popup::Field("fx-blend")),
                    current: mode,
                    label: ui::blend_mode_name(mode).into(),
                    width: 150.0,
                    options: BlendMode::layer_modes()
                        .iter()
                        .map(|m| (SharedString::from(ui::blend_mode_name(*m)), *m))
                        .collect(),
                },
                move |ws, m, _cx| {
                    // Dropdowns fire without a context; the preview catches
                    // up on the next edit or on OK.
                    ws.update_modal(|md| {
                        if let Modal::LayerStyle { style, .. } = md {
                            set_blend(style, active, m);
                        }
                    });
                    ws.restyle_from_modal();
                },
                cx,
            ),
        ));
    }

    if let Some(c) = color_of(&style, active) {
        settings = settings.child(ui::field_row(
            t("common.color"),
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(color_swatch("style-color-swatch", active, c, cx))
                .child(ui::button(
                    t("dialog.use_foreground"),
                    false,
                    move |ws, _w, cx| {
                        let fg = ws.editor.foreground;
                        edit(ws, cx, |s| set_color(s, active, fg));
                    },
                    cx,
                )),
        ));
    }

    for field in fields(active) {
        let pct = is_percent(field.suffix);
        let raw = get(&style, active, field.key);
        let key = field.key;
        settings = settings.child(param_slider(
            SliderSpec {
                id: key,
                label: t(field.label),
                value: if pct { raw * 100.0 } else { raw },
                min: field.min,
                max: field.max,
                suffix: field.suffix,
                ..Default::default()
            },
            move |ws, v, cx| {
                let v = if pct { v / 100.0 } else { v };
                edit(ws, cx, |s| set(s, active, key, v));
            },
            cx,
        ));
    }

    // The handful of controls that are not numeric.
    settings = match active {
        "stroke" => {
            let pos = style.stroke.settings.position;
            settings.child(ui::field_row(
                t("common.position"),
                ui::dropdown(
                    &ws.dropdown,
                    ui::Dropdown {
                        popup: Popup::Field("fx-stroke-pos"),
                        is_open: ws.open_popup == Some(Popup::Field("fx-stroke-pos")),
                        current: pos,
                        label: stroke_position_name(pos).into(),
                        width: 150.0,
                        options: vec![
                            (
                                stroke_position_name(StrokePosition::Outside).into(),
                                StrokePosition::Outside,
                            ),
                            (
                                stroke_position_name(StrokePosition::Inside).into(),
                                StrokePosition::Inside,
                            ),
                            (
                                stroke_position_name(StrokePosition::Center).into(),
                                StrokePosition::Center,
                            ),
                        ],
                    },
                    move |ws, p, _cx| {
                        ws.update_modal(|md| {
                            if let Modal::LayerStyle { style, .. } = md {
                                style.stroke.settings.position = p;
                            }
                        });
                        ws.restyle_from_modal();
                    },
                    cx,
                ),
            ))
        }
        "bevel" => {
            let st = style.bevel.settings.style;
            settings.child(ui::field_row(
                t("common.style"),
                ui::dropdown(
                    &ws.dropdown,
                    ui::Dropdown {
                        popup: Popup::Field("fx-bevel-style"),
                        is_open: ws.open_popup == Some(Popup::Field("fx-bevel-style")),
                        current: st,
                        label: bevel_style_name(st).into(),
                        width: 150.0,
                        options: [
                            BevelStyle_::OuterBevel,
                            BevelStyle_::InnerBevel,
                            BevelStyle_::Emboss,
                            BevelStyle_::PillowEmboss,
                        ]
                        .into_iter()
                        .map(|s| (bevel_style_name(s).into(), s))
                        .collect(),
                    },
                    move |ws, v, _cx| {
                        ws.update_modal(|md| {
                            if let Modal::LayerStyle { style, .. } = md {
                                style.bevel.settings.style = v;
                            }
                        });
                        ws.restyle_from_modal();
                    },
                    cx,
                ),
            ))
        }
        "gradient_overlay" => {
            let shape = style.gradient_overlay.settings.shape;
            let rev = style.gradient_overlay.settings.reverse;
            settings
                .child(ui::field_row(
                    t("common.shape"),
                    ui::dropdown(
                        &ws.dropdown,
                        ui::Dropdown {
                            popup: Popup::Field("fx-grad-shape"),
                            is_open: ws.open_popup == Some(Popup::Field("fx-grad-shape")),
                            current: shape,
                            label: gradient_shape_name(shape).into(),
                            width: 150.0,
                            options: [GradientShape::Linear, GradientShape::Radial]
                                .into_iter()
                                .map(|s| (gradient_shape_name(s).into(), s))
                                .collect(),
                        },
                        move |ws, v, _cx| {
                            ws.update_modal(|md| {
                                if let Modal::LayerStyle { style, .. } = md {
                                    style.gradient_overlay.settings.shape = v;
                                }
                            });
                            ws.restyle_from_modal();
                        },
                        cx,
                    ),
                ))
                .child(ui::checkbox(
                    t("dialog.style.reverse"),
                    rev,
                    move |ws, cx| {
                        edit(ws, cx, |s| {
                            s.gradient_overlay.settings.reverse = !rev;
                        });
                    },
                    cx,
                ))
                .child(ui::field_row(
                    t("dialog.style.from_to"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(color_swatch(
                            "style-gradient-from",
                            "gradient_overlay.from",
                            style.gradient_overlay.settings.from,
                            cx,
                        ))
                        .child(ui::button(
                            t("dialog.style.from_fg"),
                            false,
                            move |ws, _w, cx| {
                                let fg = ws.editor.foreground;
                                edit(ws, cx, |s| s.gradient_overlay.settings.from = fg);
                            },
                            cx,
                        ))
                        .child(color_swatch(
                            "style-gradient-to",
                            "gradient_overlay.to",
                            style.gradient_overlay.settings.to,
                            cx,
                        ))
                        .child(ui::button(
                            t("dialog.style.to_fg"),
                            false,
                            move |ws, _w, cx| {
                                let fg = ws.editor.foreground;
                                edit(ws, cx, |s| s.gradient_overlay.settings.to = fg);
                            },
                            cx,
                        )),
                ))
        }
        "outer_glow" | "inner_glow" => {
            let inner = active == "inner_glow";
            let tech = if inner {
                style.inner_glow.settings.technique
            } else {
                style.outer_glow.settings.technique
            };
            let precise = tech == Technique::Precise;
            settings.child(ui::checkbox(
                t("dialog.style.precise"),
                precise,
                move |ws, cx| {
                    let next = if precise {
                        Technique::Softer
                    } else {
                        Technique::Precise
                    };
                    edit(ws, cx, |s| {
                        if inner {
                            s.inner_glow.settings.technique = next;
                        } else {
                            s.outer_glow.settings.technique = next;
                        }
                    });
                },
                cx,
            ))
        }
        "drop_shadow" => {
            let knock = style.drop_shadow.settings.knockout;
            settings.child(ui::checkbox(
                t("dialog.style.knockout"),
                knock,
                move |ws, cx| {
                    edit(ws, cx, |s| s.drop_shadow.settings.knockout = !knock);
                },
                cx,
            ))
        }
        "satin" => {
            let inv = style.satin.settings.invert;
            settings.child(ui::checkbox(
                t("common.invert"),
                inv,
                move |ws, cx| {
                    edit(ws, cx, |s| s.satin.settings.invert = !inv);
                },
                cx,
            ))
        }
        _ => settings,
    };

    let body = div()
        .id("layer-style-body")
        .flex()
        .flex_row()
        .gap_3()
        .h(px(330.0))
        .overflow_y_scroll()
        .child(list)
        .child(settings);

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
            t("common.ok"),
            true,
            move |ws, _w, cx| ws.commit_layer_style(cx),
            cx,
        ));
    ui::modal_frame(t("dialog.style.title"), 620.0, body, actions)
}

fn rgb_of(c: Rgba) -> u32 {
    let q = |v: f32| ((v.clamp(0.0, 1.0) * 255.0).round() as u32) & 0xFF;
    (q(c.r) << 16) | (q(c.g) << 8) | q(c.b)
}

/// Highlight for the selected row in the effects list.
trait Selected: Styled + Sized {
    fn when_selected(self, on: bool) -> Self {
        if on {
            self.bg(gpui::rgb(ui::palette().selection_bg))
        } else {
            self
        }
    }
}

impl<T: Styled + Sized> Selected for T {}
