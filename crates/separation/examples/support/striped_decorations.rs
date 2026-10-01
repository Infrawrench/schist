//! A striped line and its independently placed solid-band reference.
use schist_layout::{
    authoring,
    decorations::{
        DecorationMeasure as Measure, DecorationPaint as Paint, DecorationStroke, DecorationStyle,
    },
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story, WritingMode,
};
use schist_text_engine::TextDecorationPattern;

pub fn document() -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 12]);
    doc.styles.add_character(CharacterStyle {
        name: "Default".into(),
        opacity: Some(0.7),
        ..Default::default()
    });
    for case in 0..6 {
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalRightToLeft,
            WritingMode::VerticalLeftToRight,
        ][case / 2];
        let name = format!("Case {case}");
        let weight = if case % 2 == 0 { 4.0 } else { 7.5 };
        let underline = DecorationStyle {
            stroke: Some(DecorationStroke {
                fitting: Default::default(),
                name: "Double".into(),
                pattern: TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]),
            }),
            paint: Some(Paint::Ink(Ink::spot("Line spot", [45.0, 60.0, 30.0]))),
            gap_paint: Some(Paint::Ink(Ink::cmyk("Gap cyan", [1.0, 0.0, 0.0, 0.0]))),
            weight: Some(Measure::Points(weight)),
            offset: Some(Measure::Points(7.0)),
            tint: Some(0.8),
            gap_tint: Some(0.65),
            overprint: Some(case % 2 == 1),
            gap_overprint: Some(case % 2 == 0),
        };
        let strike = DecorationStyle {
            offset: Some(Measure::Points(8.0)),
            ..underline.clone()
        };
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(30.0),
            leading: Some(schist_layout::styles::Leading::Points(48.0)),
            writing_mode: Some(mode),
            fill: Some(Ink::cmyk("Glyph yellow", [0.0, 0.0, 1.0, 0.0])),
            fill_tint: Some(0.85),
            underline: Some(true),
            strikethrough: Some(true),
            underline_style: underline.clone(),
            strike_style: strike.clone(),
            ..Default::default()
        });
        let mut references = Vec::new();
        for is_strike in [false, true] {
            if is_strike {
                let fill = format!("{name} fill");
                doc.styles.add_paragraph(ParagraphStyle {
                    name: fill.clone(),
                    based_on: Some(name.clone()),
                    underline: Some(false),
                    strikethrough: Some(false),
                    ..Default::default()
                });
                references.push(fill);
            }
            let original = if is_strike { &strike } else { &underline };
            // Each band has an independent absolute width and baseline offset;
            // no stripe pattern is used by the reference text objects.
            for (band, (a, b, gap)) in [(0.25, 0.75, true), (0.0, 0.25, false), (0.75, 1.0, false)]
                .into_iter()
                .enumerate()
            {
                let delta = weight * ((a + b) / 2.0 - 0.5);
                let sign = if mode == WritingMode::VerticalLeftToRight
                    || (mode == WritingMode::Horizontal && is_strike)
                {
                    -1.0
                } else {
                    1.0
                };
                let settings = DecorationStyle {
                    stroke: Some(DecorationStroke::solid()),
                    weight: Some(Measure::Points(weight * (b - a))),
                    offset: Some(Measure::Points(
                        original.offset.unwrap().points().unwrap() + sign * delta,
                    )),
                    paint: if gap {
                        original.gap_paint.clone()
                    } else {
                        original.paint.clone()
                    },
                    tint: if gap {
                        original.gap_tint
                    } else {
                        original.tint
                    },
                    overprint: if gap {
                        original.gap_overprint
                    } else {
                        original.overprint
                    },
                    gap_paint: Some(Paint::None),
                    ..Default::default()
                };
                let reference = format!("{name} {is_strike} band {band}");
                doc.styles.add_paragraph(ParagraphStyle {
                    name: reference.clone(),
                    based_on: Some(name.clone()),
                    fill_disabled: true,
                    underline: Some(!is_strike),
                    strikethrough: Some(is_strike),
                    underline_style: settings.clone(),
                    strike_style: settings,
                    ..Default::default()
                });
                references.push(reference);
            }
        }
        for reference in [false, true] {
            let styles = if reference {
                references.clone()
            } else {
                vec![name.clone()]
            };
            for style in styles {
                let frame = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    case * 2 + usize::from(reference),
                    Rect::new(25.0, 25.0, 140.0, 140.0),
                )
                .unwrap();
                doc.stories[frame.story.0 as usize] = Story::from_text("HéH AV\nType", &style);
                doc.objects.last_mut().unwrap().transform = schist_core::Affine {
                    a: 0.9,
                    b: 0.12,
                    c: 0.15,
                    d: 0.9,
                    tx: 0.0,
                    ty: 0.0,
                };
            }
        }
    }
    doc
}
