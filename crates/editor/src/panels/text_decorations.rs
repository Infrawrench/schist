//! Named text-style decoration controls; numeric blanks restore inheritance.
use super::*;
use crate::design::controls::{self, Target};
use schist_layout::decorations::{
    DecorationMeasure, DecorationPaint, DecorationStroke, DecorationStyle,
};

fn measure(value: Option<DecorationMeasure>) -> String {
    match value {
        None => String::new(),
        Some(DecorationMeasure::Auto) => t("design.leading_auto").to_string(),
        Some(DecorationMeasure::Points(v)) => v.to_string(),
    }
}

pub(super) fn rows(
    ws: &mut Workspace,
    target: &Target,
    styles: [(&DecorationStyle, Option<bool>); 2],
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let paragraph = matches!(target, Target::Paragraph(_));
    styles
        .into_iter()
        .enumerate()
        .map(|(index, (style, enabled))| {
            let strike = index == 1;
            let ids = match (paragraph, strike) {
                (true, false) => [
                    "design-para-underline-enabled",
                    "design-para-underline-paint",
                    "design-prop-para-underline-weight",
                    "design-prop-para-underline-offset",
                    "design-prop-para-underline-tint",
                    "design-para-underline-overprint",
                ],
                (true, true) => [
                    "design-para-strike-enabled",
                    "design-para-strike-paint",
                    "design-prop-para-strike-weight",
                    "design-prop-para-strike-offset",
                    "design-prop-para-strike-tint",
                    "design-para-strike-overprint",
                ],
                (false, false) => [
                    "design-char-underline-enabled",
                    "design-char-underline-paint",
                    "design-prop-char-underline-weight",
                    "design-prop-char-underline-offset",
                    "design-prop-char-underline-tint",
                    "design-char-underline-overprint",
                ],
                (false, true) => [
                    "design-char-strike-enabled",
                    "design-char-strike-paint",
                    "design-prop-char-strike-weight",
                    "design-prop-char-strike-offset",
                    "design-prop-char-strike-tint",
                    "design-char-strike-overprint",
                ],
            };
            let selection = |value| match value {
                None => 0,
                Some(true) => 1,
                Some(false) => 2,
            };
            let target_enabled = target.clone();
            let mut out = div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().child(t(if strike {
                    "design.strikethrough"
                } else {
                    "design.underline"
                })))
                .child(super::object_styles::picker(
                    ws,
                    ids[0],
                    vec![
                        t("design.inherited").into(),
                        t("design.enabled").into(),
                        t("design.disabled").into(),
                    ],
                    selection(enabled),
                    move |ws, index, _| {
                        controls::set_decoration_enabled(
                            &mut ws.design,
                            &target_enabled,
                            strike,
                            match index {
                                1 => Some(true),
                                2 => Some(false),
                                _ => None,
                            },
                        );
                    },
                    cx,
                ));
            let mut paints = vec![
                None,
                Some(DecorationPaint::Text),
                Some(DecorationPaint::None),
            ];
            let mut labels = vec![
                t("design.inherited").into(),
                t("design.decoration_text_color").into(),
                t("design.no_paint").into(),
            ];
            for ink in &ws.design.document.inks {
                paints.push(Some(DecorationPaint::Ink(ink.clone())));
                labels.push(ink.name.clone());
            }
            if !paints.contains(&style.paint) {
                paints.push(style.paint.clone());
                labels.push(
                    style
                        .paint
                        .as_ref()
                        .and_then(DecorationPaint::ink)
                        .map(|i| i.name.clone())
                        .unwrap_or_default(),
                );
            }
            let current = paints.iter().position(|p| *p == style.paint).unwrap_or(0);
            let target_paint = target.clone();
            out = out
                .child(div().text_xs().child(t("design.decoration_color")))
                .child(super::object_styles::picker(
                    ws,
                    ids[1],
                    labels,
                    current,
                    move |ws, index, _| {
                        if let Some(paint) = paints.get(index) {
                            controls::edit_decoration(&mut ws.design, &target_paint, strike, |d| {
                                d.paint = paint.clone()
                            });
                        }
                    },
                    cx,
                ));
            for (id, label, value) in [
                (ids[2], "design.decoration_weight", measure(style.weight)),
                (ids[3], "design.decoration_offset", measure(style.offset)),
                (
                    ids[4],
                    "design.decoration_tint",
                    style
                        .tint
                        .map(|v| (v * 100.0).to_string())
                        .unwrap_or_default(),
                ),
            ] {
                out = out.child(super::design_controls::field(
                    ws,
                    id,
                    label,
                    value,
                    target.clone(),
                    cx,
                ));
            }
            let target_overprint = target.clone();
            out.child(div().text_xs().child(t("design.overprint")))
                .child(super::object_styles::picker(
                    ws,
                    ids[5],
                    vec![
                        t("design.inherited").into(),
                        t("design.overprint").into(),
                        t("design.knockout").into(),
                    ],
                    selection(style.overprint),
                    move |ws, index, _| {
                        controls::edit_decoration(&mut ws.design, &target_overprint, strike, |d| {
                            d.overprint = match index {
                                1 => Some(true),
                                2 => Some(false),
                                _ => None,
                            }
                        });
                    },
                    cx,
                ))
                .child(pattern_and_gap(ws, target, style, strike, cx))
                .child(div().text_xs().child(t("design.decoration_dimensions")))
                .into_any_element()
        })
        .collect()
}

fn pattern_and_gap(
    ws: &mut Workspace,
    target: &Target,
    style: &DecorationStyle,
    strike: bool,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    use schist_text_engine::TextDecorationPattern;
    let ids = match (matches!(target, Target::Paragraph(_)), strike) {
        (true, false) => [
            "design-para-underline-pattern",
            "design-prop-para-underline-stripes",
            "design-para-underline-gap-paint",
            "design-prop-para-underline-gap-tint",
            "design-para-underline-gap-overprint",
            "design-prop-para-underline-dashes",
            "design-para-underline-cap",
            "design-prop-para-underline-dots",
            "design-para-underline-fitting",
        ],
        (true, true) => [
            "design-para-strike-pattern",
            "design-prop-para-strike-stripes",
            "design-para-strike-gap-paint",
            "design-prop-para-strike-gap-tint",
            "design-para-strike-gap-overprint",
            "design-prop-para-strike-dashes",
            "design-para-strike-cap",
            "design-prop-para-strike-dots",
            "design-para-strike-fitting",
        ],
        (false, false) => [
            "design-char-underline-pattern",
            "design-prop-char-underline-stripes",
            "design-char-underline-gap-paint",
            "design-prop-char-underline-gap-tint",
            "design-char-underline-gap-overprint",
            "design-prop-char-underline-dashes",
            "design-char-underline-cap",
            "design-prop-char-underline-dots",
            "design-char-underline-fitting",
        ],
        (false, true) => [
            "design-char-strike-pattern",
            "design-prop-char-strike-stripes",
            "design-char-strike-gap-paint",
            "design-prop-char-strike-gap-tint",
            "design-char-strike-gap-overprint",
            "design-prop-char-strike-dashes",
            "design-char-strike-cap",
            "design-prop-char-strike-dots",
            "design-char-strike-fitting",
        ],
    };
    let striped = DecorationStroke {
        fitting: Default::default(),
        name: t("design.decoration_striped").into(),
        pattern: TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]),
    };
    let mut strokes = vec![
        None,
        Some(DecorationStroke::solid()),
        Some(striped),
        Some(DecorationStroke {
            fitting: Default::default(),
            name: t("design.decoration_dashed").into(),
            pattern: TextDecorationPattern::Dashes(vec![6.0, 3.0].into()),
        }),
        Some(DecorationStroke {
            fitting: Default::default(),
            name: t("design.decoration_dotted").into(),
            pattern: TextDecorationPattern::Dots(vec![6.0]),
        }),
    ];
    let mut labels = vec![
        t("design.inherited").into(),
        t("design.decoration_solid").into(),
        t("design.decoration_striped").into(),
        t("design.decoration_dashed").into(),
        t("design.decoration_dotted").into(),
    ];
    for stroke in ws.design.document.all_decoration_strokes() {
        if !strokes.contains(&Some(stroke.clone())) {
            labels.push(stroke.name.clone());
            strokes.push(Some(stroke));
        }
    }
    let selected = strokes.iter().position(|s| s == &style.stroke).unwrap_or(0);
    let captured = target.clone();
    let mut out = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(t("design.decoration_pattern")))
        .child(super::object_styles::picker(
            ws,
            ids[0],
            labels,
            selected,
            move |ws, index, _| {
                if let Some(stroke) = strokes.get(index) {
                    controls::edit_decoration(&mut ws.design, &captured, strike, |d| {
                        d.stroke = stroke.clone()
                    });
                }
            },
            cx,
        ));
    let edges = style
        .stroke
        .as_ref()
        .and_then(|s| match &s.pattern {
            TextDecorationPattern::Stripes(edges) => Some(
                edges
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .unwrap_or_default();
    out = out.child(super::design_controls::field(
        ws,
        ids[1],
        "design.stripe_edges",
        edges,
        target.clone(),
        cx,
    ));
    let lengths = style
        .stroke
        .as_ref()
        .and_then(|s| match &s.pattern {
            TextDecorationPattern::Dashes(dashes) => Some(
                dashes
                    .lengths
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .unwrap_or_default();
    out = out.child(super::design_controls::field(
        ws,
        ids[5],
        "design.dash_lengths",
        lengths,
        target.clone(),
        cx,
    ));
    let spacing = style
        .stroke
        .as_ref()
        .and_then(|s| match &s.pattern {
            TextDecorationPattern::Dots(intervals) => Some(
                intervals
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .unwrap_or_default();
    out = out.child(super::design_controls::field(
        ws,
        ids[7],
        "design.dot_spacing",
        spacing,
        target.clone(),
        cx,
    ));
    if let Some(DecorationStroke {
        pattern: TextDecorationPattern::Dashes(dashes),
        ..
    }) = &style.stroke
    {
        use schist_text_engine::DecorationCap;
        let choices = [
            DecorationCap::Butt,
            DecorationCap::Round,
            DecorationCap::Projecting,
        ];
        let current = choices
            .iter()
            .position(|cap| *cap == dashes.cap)
            .unwrap_or(0);
        let captured = target.clone();
        out = out
            .child(div().text_xs().child(t("design.dash_cap")))
            .child(super::object_styles::picker(
                ws,
                ids[6],
                [
                    "design.cap_butt",
                    "design.cap_round",
                    "design.cap_projecting",
                ]
                .into_iter()
                .map(|key| t(key).into())
                .collect(),
                current,
                move |ws, index, _| {
                    if let Some(cap) = choices.get(index) {
                        controls::set_decoration_cap(&mut ws.design, &captured, strike, *cap);
                    }
                },
                cx,
            ));
    }
    if let Some(stroke) = style.stroke.as_ref().filter(|s| {
        matches!(
            s.pattern,
            TextDecorationPattern::Dashes(_) | TextDecorationPattern::Dots(_)
        )
    }) {
        use schist_text_engine::DecorationFit;
        let mut choices = vec![
            (DecorationFit::None, "design.decoration_fit_none"),
            (DecorationFit::Dashes, "design.decoration_fit_dashes"),
            (DecorationFit::Gaps, "design.decoration_fit_gaps"),
            (DecorationFit::DashesAndGaps, "design.decoration_fit_both"),
        ];
        if matches!(stroke.pattern, TextDecorationPattern::Dots(_)) {
            choices.retain(|(fit, _)| *fit != DecorationFit::Dashes);
        }
        let current = choices
            .iter()
            .position(|(fit, _)| *fit == stroke.fitting)
            .unwrap_or(0);
        let labels = choices.iter().map(|(_, key)| t(key).into()).collect();
        let captured = target.clone();
        out = out
            .child(div().text_xs().child(t("design.decoration_fitting")))
            .child(super::object_styles::picker(
                ws,
                ids[8],
                labels,
                current,
                move |ws, index, _| {
                    if let Some((fit, _)) = choices.get(index) {
                        controls::set_decoration_fitting(&mut ws.design, &captured, strike, *fit);
                    }
                },
                cx,
            ));
    }
    let mut paints = vec![
        None,
        Some(DecorationPaint::None),
        Some(DecorationPaint::Text),
    ];
    let mut labels = vec![
        t("design.inherited").into(),
        t("design.no_paint").into(),
        t("design.decoration_text_color").into(),
    ];
    for ink in &ws.design.document.inks {
        paints.push(Some(DecorationPaint::Ink(ink.clone())));
        labels.push(ink.name.clone());
    }
    if !paints.contains(&style.gap_paint) {
        paints.push(style.gap_paint.clone());
        labels.push(
            style
                .gap_paint
                .as_ref()
                .and_then(DecorationPaint::ink)
                .map(|i| i.name.clone())
                .unwrap_or_default(),
        );
    }
    let selected = paints
        .iter()
        .position(|p| p == &style.gap_paint)
        .unwrap_or(0);
    let captured = target.clone();
    out = out
        .child(div().text_xs().child(t("design.decoration_gap_color")))
        .child(super::object_styles::picker(
            ws,
            ids[2],
            labels,
            selected,
            move |ws, index, _| {
                if let Some(paint) = paints.get(index) {
                    controls::edit_decoration(&mut ws.design, &captured, strike, |d| {
                        d.gap_paint = paint.clone()
                    });
                }
            },
            cx,
        ))
        .child(super::design_controls::field(
            ws,
            ids[3],
            "design.decoration_gap_tint",
            style
                .gap_tint
                .map(|v| (v * 100.0).to_string())
                .unwrap_or_default(),
            target.clone(),
            cx,
        ));
    let captured = target.clone();
    out.child(div().text_xs().child(t("design.decoration_gap_overprint")))
        .child(super::object_styles::picker(
            ws,
            ids[4],
            vec![
                t("design.inherited").into(),
                t("design.overprint").into(),
                t("design.knockout").into(),
            ],
            match style.gap_overprint {
                None => 0,
                Some(true) => 1,
                Some(false) => 2,
            },
            move |ws, index, _| {
                controls::edit_decoration(&mut ws.design, &captured, strike, |d| {
                    d.gap_overprint = match index {
                        1 => Some(true),
                        2 => Some(false),
                        _ => None,
                    }
                });
            },
            cx,
        ))
        .into_any_element()
}
