//! Tab fields compared with ordinary frames at independently fixed positions.
use schist_layout::{
    authoring,
    lists::{ListStyle, ListTab},
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story, StyleRange,
    WritingMode,
};

pub fn document(reference: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 72]);
    for case in 0_usize..72 {
        let leader_case = (case >= 48).then_some(case.saturating_sub(48));
        let alignment = ["LeftAlign", "RightAlign", "CenterAlign", "CharacterAlign"]
            [leader_case.map_or(case / 12, |c| c / 6)];
        let fields = if alignment == "LeftAlign" {
            ["A", "H", "é"]
        } else {
            ["A", "12.34", "é,15"]
        };
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ][leader_case.map_or(case % 12 / 4, |c| c % 6 / 2)];
        let stops = if case < 12 {
            [48.0, 96.0]
        } else {
            [48.0, 108.0]
        };
        let indent = if case.is_multiple_of(2) { 0.0 } else { 12.0 };
        let size = if leader_case.map_or(case % 4 < 2, |c| c.is_multiple_of(2)) {
            12.0
        } else {
            18.0
        };
        let leader = leader_case.map_or("", |c| [". ", "_ ", "é— "][c % 6 / 2]);
        let name = format!("Tabs {case}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(size),
            writing_mode: Some(mode),
            leading: Some(schist_layout::styles::Leading::Points(24.0)),
            first_line_indent: Some(if reference { 0.0 } else { indent }),
            list: ListStyle {
                tabs: (!reference).then(|| {
                    stops
                        .into_iter()
                        .enumerate()
                        .map(|(index, offset)| {
                            // Independently shape ordinary text, then choose
                            // a ruler stop whose anchor lands at a fixed frame.
                            // No tab geometry is used to construct the control.
                            let character = ['.', ','][index];
                            let text = fields[index + 1];
                            let spec = schist_text_engine::TextSpec {
                                text: text.into(),
                                family: "IBM Plex Sans".into(),
                                size,
                                direction: schist_text_engine::ParagraphDirection::LeftToRight,
                                writing_mode: match mode {
                                    WritingMode::Horizontal => {
                                        schist_text_engine::WritingMode::Horizontal
                                    }
                                    WritingMode::VerticalLeftToRight => {
                                        schist_text_engine::WritingMode::VerticalLr
                                    }
                                    WritingMode::VerticalRightToLeft => {
                                        schist_text_engine::WritingMode::VerticalRl
                                    }
                                },
                                ..Default::default()
                            };
                            let width = schist_text_engine::measure(&spec).unwrap().width;
                            let anchor = match alignment {
                                "RightAlign" => width,
                                "CenterAlign" => width / 2.0,
                                "CharacterAlign" => {
                                    let caret = schist_text_engine::caret_at(
                                        &spec,
                                        text.find(character).unwrap(),
                                    )
                                    .unwrap();
                                    if mode == WritingMode::Horizontal {
                                        caret.x
                                    } else {
                                        caret.top
                                    }
                                }
                                _ => 0.0,
                            };
                            ListTab {
                                position: offset + anchor,
                                alignment: alignment.into(),
                                alignment_character: character.into(),
                                leader: leader.into(),
                            }
                        })
                        .collect()
                }),
                ..Default::default()
            },
            ..Default::default()
        });
        for field in 0..3 {
            doc.styles.add_character(CharacterStyle {
                name: format!("Ink {case}/{field}"),
                fill: Some(if field == 1 {
                    Ink::spot("Tab spot", [45.0, 60.0, 30.0])
                } else {
                    Ink::cmyk("Tab cyan", [1.0, 0.0, 0.0, 0.0])
                }),
                fill_tint: Some(0.65),
                opacity: Some(0.7),
                stroke: Some(Ink::cmyk("Tab magenta", [0.0, 1.0, 0.0, 0.0])),
                stroke_weight: Some(0.35),
                stroke_tint: Some(0.75),
                overprint_fill: Some(case.is_multiple_of(2)),
                overprint_stroke: Some(!case.is_multiple_of(2)),
                ..Default::default()
            });
        }
        let leader_style = format!("Leader {case}");
        doc.styles.add_character(CharacterStyle {
            name: leader_style.clone(),
            fill: Some(Ink::spot("Leader spot", [65.0, 10.0, 70.0])),
            fill_tint: Some(0.6),
            opacity: Some(0.7),
            stroke: Some(Ink::cmyk("Tab magenta", [0.0, 1.0, 0.0, 0.0])),
            stroke_weight: Some(0.35),
            stroke_tint: Some(0.75),
            baseline_shift: Some(schist_layout::styles::BaselineShift::Offset(2.0)),
            overprint_fill: Some(case.is_multiple_of(2)),
            overprint_stroke: Some(!case.is_multiple_of(2)),
            ..Default::default()
        });
        if reference {
            for (field, text) in fields.iter().enumerate() {
                let offset = [indent, stops[0], stops[1]][field];
                let bounds = if mode == WritingMode::Horizontal {
                    Rect::new(20.0 + offset, 20.0, 160.0 - offset, 160.0)
                } else {
                    Rect::new(20.0, 20.0 + offset, 160.0, 160.0 - offset)
                };
                let frame =
                    authoring::text_frame(&mut doc, &mut History::default(), case, bounds).unwrap();
                let mut story = Story::from_text(*text, &name);
                story.ranges.push(StyleRange::new(
                    0,
                    text.len(),
                    format!("Ink {case}/{field}"),
                ));
                doc.stories[frame.story.0 as usize] = story;
            }
            if !leader.is_empty() {
                // Enumerate complete ordinary-text units backwards from each
                // independently fixed field edge. No tab stop implementation
                // or generated leader geometry constructs the reference.
                let spec = |text: &str| schist_text_engine::TextSpec {
                    text: text.into(),
                    family: "IBM Plex Sans".into(),
                    size,
                    direction: schist_text_engine::ParagraphDirection::LeftToRight,
                    writing_mode: match mode {
                        WritingMode::Horizontal => schist_text_engine::WritingMode::Horizontal,
                        WritingMode::VerticalLeftToRight => {
                            schist_text_engine::WritingMode::VerticalLr
                        }
                        WritingMode::VerticalRightToLeft => {
                            schist_text_engine::WritingMode::VerticalRl
                        }
                    },
                    ..Default::default()
                };
                let period = schist_text_engine::measure(&spec(leader)).unwrap().width;
                for field in 0..2 {
                    let previous = [indent, stops[0]][field]
                        + schist_text_engine::measure(&spec(fields[field]))
                            .unwrap()
                            .width;
                    let mut offset = f64::from(stops[field]);
                    let mut count = 0;
                    while offset - f64::from(period) >= f64::from(previous) {
                        offset -= f64::from(period);
                        count += 1;
                    }
                    if count == 0 {
                        continue;
                    }
                    // Frame placement rounds before rasterization. A tracked
                    // zero-width character preserves the independently chosen
                    // fractional position inside an ordinary text run instead.
                    assert_eq!(
                        schist_text_engine::measure(&spec("\u{200b}"))
                            .unwrap()
                            .width,
                        0.0
                    );
                    let padding = format!("Leader position {case}/{field}");
                    doc.styles.add_character(CharacterStyle {
                        name: padding.clone(),
                        tracking: Some(offset as f32 * 1000.0 / size),
                        ..Default::default()
                    });
                    let bounds = Rect::new(20.0, 20.0, 160.0, 160.0);
                    let frame =
                        authoring::text_frame(&mut doc, &mut History::default(), case, bounds)
                            .unwrap();
                    let text = format!("\u{200b}{}", leader.repeat(count));
                    let mut story = Story::from_text(&text, &name);
                    story
                        .ranges
                        .push(StyleRange::new(3, text.len(), &leader_style));
                    story.ranges.push(StyleRange::new(0, 3, padding));
                    doc.stories[frame.story.0 as usize] = story;
                }
            }
        } else {
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                case,
                Rect::new(20.0, 20.0, 160.0, 160.0),
            )
            .unwrap();
            let mut story = Story::from_text(fields.join("\t"), &name);
            let mut from = 0;
            for (field, text) in fields.iter().enumerate() {
                story.ranges.push(StyleRange::new(
                    from,
                    from + text.len(),
                    format!("Ink {case}/{field}"),
                ));
                if !leader.is_empty() && field < 2 {
                    story.ranges.push(StyleRange::new(
                        from + text.len(),
                        from + text.len() + 1,
                        &leader_style,
                    ));
                }
                from += text.len() + 1;
            }
            doc.stories[frame.story.0 as usize] = story;
        }
    }
    rtl_cases(&mut doc, reference);
    collision_cases(&mut doc, reference);
    path_cases(&mut doc, reference);
    doc
}

/// The independent horizontal RTL controls already place ordinary fields and
/// leaders at fixed distances. Copy those controls onto common baselines;
/// neither their positions nor their stories come from Design composition.
fn path_cases(doc: &mut LayoutDocument, reference: bool) {
    use schist_layout::{BezierHandles, LayoutObject, ObjectId, Point, ShapePath, SubPath};
    for alignment in 0..4 {
        for sample in 0_usize..4 {
            let size = if sample < 2 { 12.0 } else { 18.0 };
            let indent = if sample.is_multiple_of(2) { 0.0 } else { 12.0 };
            let widths = ["אב", "12.34", "56,78"].map(|text| {
                schist_text_engine::measure(&schist_text_engine::TextSpec {
                    text: text.into(),
                    family: if text == "אב" {
                        "Noto Sans Hebrew"
                    } else {
                        "IBM Plex Sans"
                    }
                    .into(),
                    size,
                    direction: schist_text_engine::ParagraphDirection::LeftToRight,
                    ..Default::default()
                })
                .unwrap()
                .width
            });
            // Use literal bracket distances on curves. Encoding a position as
            // tracking and recovering it at another size introduces round-off
            // before the nonlinear path projection. No tab geometry is used.
            let starts = [
                160.0 - indent - widths[0],
                70.0,
                10.0,
                70.0 + widths[1],
                10.0 + widths[2],
            ];
            let source = 72 + alignment * 12 + sample;
            let objects: Vec<_> = doc
                .objects
                .iter()
                .filter(|object| object.page == source)
                .cloned()
                .collect();
            for curve in 0..4 {
                let page = doc.pages.len();
                doc.pages.push(Page::new("Path tab proof", 200.0, 200.0));
                let (points, handles) = match curve {
                    0 => (
                        vec![Point::new(20.0, 60.0), Point::new(180.0, 60.0)],
                        vec![],
                    ),
                    1 => (
                        vec![Point::new(70.0, 20.0), Point::new(70.0, 180.0)],
                        vec![],
                    ),
                    2 => (
                        vec![Point::new(150.0, 180.0), Point::new(150.0, 20.0)],
                        vec![],
                    ),
                    _ => (
                        vec![Point::new(20.0, 100.0), Point::new(180.0, 100.0)],
                        vec![
                            BezierHandles {
                                outgoing: Some(Point::new(50.0, -20.0)),
                                ..Default::default()
                            },
                            BezierHandles {
                                incoming: Some(Point::new(150.0, 220.0)),
                                ..Default::default()
                            },
                        ],
                    ),
                };
                let mut path = ShapePath {
                    subpaths: vec![SubPath {
                        points,
                        handles,
                        closed: false,
                    }],
                    even_odd: false,
                };
                let bounds = path.bounds();
                path.map_points(|point| point - Point::new(bounds.x, bounds.y));
                for (index, original) in objects.iter().enumerate() {
                    let mut object = original.clone();
                    let LayoutObject::TextFrame {
                        story, text_path, ..
                    } = &mut object.object
                    else {
                        unreachable!()
                    };
                    let mut copied = doc.story(*story).unwrap().clone();
                    if reference {
                        let schist_layout::StoryPoint::Paragraph { text, .. } =
                            &mut copied.points[0]
                        else {
                            unreachable!()
                        };
                        assert!(text.starts_with('\u{200b}'));
                        text.drain(..3);
                        copied.ranges.retain(|range| range.end > 3);
                        for range in &mut copied.ranges {
                            range.start = range.start.saturating_sub(3);
                            range.end -= 3;
                        }
                    }
                    *story = doc.add_story(copied);
                    *text_path = Some(schist_layout::text_path::PathText {
                        path: path.clone(),
                        start: if reference { starts[index] } else { 0.0 },
                        end: Some(160.0),
                    });
                    object.id = ObjectId::next();
                    object.page = page;
                    object.bounds = bounds;
                    doc.objects.push(object);
                }
            }
        }
    }
}

/// Touching fields are controlled by ordinary text at the prefix's measured
/// end. An ahead-of-pen stop collides; its leader must produce no extra ink.
fn collision_cases(doc: &mut LayoutDocument, reference: bool) {
    for case in 0_usize..18 {
        let page = doc.pages.len();
        doc.pages
            .push(Page::new("Colliding tab proof", 200.0, 200.0));
        let alignment = ["RightAlign", "CenterAlign", "CharacterAlign"][case / 6];
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ][case % 6 / 2];
        let writing_mode = match mode {
            WritingMode::Horizontal => schist_text_engine::WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight => schist_text_engine::WritingMode::VerticalLr,
            WritingMode::VerticalRightToLeft => schist_text_engine::WritingMode::VerticalRl,
        };
        let size = if case.is_multiple_of(2) { 12.0 } else { 18.0 };
        let pen = schist_text_engine::measure(&schist_text_engine::TextSpec {
            text: "WWWW".into(),
            family: "IBM Plex Sans".into(),
            size,
            writing_mode,
            ..Default::default()
        })
        .unwrap()
        .width;
        let name = format!("Collision {case}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(size),
            writing_mode: Some(mode),
            leading: Some(schist_layout::styles::Leading::Points(24.0)),
            list: ListStyle {
                tabs: (!reference).then(|| {
                    vec![ListTab {
                        position: pen + 0.25,
                        alignment: alignment.into(),
                        alignment_character: ".".into(),
                        leader: ".".into(),
                    }]
                }),
                ..Default::default()
            },
            ..Default::default()
        });
        let inks: Vec<_> = (0..2)
            .map(|field| {
                let name = format!("Collision ink {case}/{field}");
                doc.styles.add_character(CharacterStyle {
                    name: name.clone(),
                    fill: Some(if field == 0 {
                        Ink::cmyk("Collision cyan", [1.0, 0.0, 0.0, 0.0])
                    } else {
                        Ink::spot("Collision spot", [45.0, 60.0, 30.0])
                    }),
                    fill_tint: Some(0.65),
                    opacity: Some(0.7),
                    stroke: Some(Ink::cmyk("Collision magenta", [0.0, 1.0, 0.0, 0.0])),
                    stroke_weight: Some(0.35),
                    overprint_fill: Some(case.is_multiple_of(2)),
                    overprint_stroke: Some(!case.is_multiple_of(2)),
                    ..Default::default()
                });
                name
            })
            .collect();
        let bounds = Rect::new(20.0, 20.0, 160.0, 160.0);
        if reference {
            for (field, text) in ["WWWW", "12.34"].into_iter().enumerate() {
                let padding = format!("Collision padding {case}/{field}");
                doc.styles.add_character(CharacterStyle {
                    name: padding.clone(),
                    tracking: Some(if field == 0 { 0.0 } else { pen * 1000.0 / size }),
                    ..Default::default()
                });
                let frame =
                    authoring::text_frame(doc, &mut History::default(), page, bounds).unwrap();
                let text = format!("\u{200b}{text}");
                let mut story = Story::from_text(&text, &name);
                story.ranges.push(StyleRange::new(0, 3, padding));
                story
                    .ranges
                    .push(StyleRange::new(3, text.len(), &inks[field]));
                doc.stories[frame.story.0 as usize] = story;
            }
        } else {
            let frame = authoring::text_frame(doc, &mut History::default(), page, bounds).unwrap();
            let mut story = Story::from_text("WWWW\t12.34", &name);
            story.ranges.push(StyleRange::new(0, 4, &inks[0]));
            story.ranges.push(StyleRange::new(4, 5, &inks[1]));
            story.ranges.push(StyleRange::new(5, 10, &inks[1]));
            doc.stories[frame.story.0 as usize] = story;
        }
    }
}

/// Controls use independently positioned ordinary text with tracked zero-width
/// padding. No tab geometry constructs their placements or repeated leader ink.
fn rtl_cases(doc: &mut LayoutDocument, reference: bool) {
    use schist_layout::styles::{Align, ParagraphDirection};
    schist_text_engine::add_font_data(
        include_bytes!("../../../../web/fonts/NotoSansHebrew-Regular.ttf").to_vec(),
    );
    for case in 0_usize..48 {
        let page = doc.pages.len();
        doc.pages.push(Page::new("RTL tab proof", 200.0, 200.0));
        let alignment = ["LeftAlign", "RightAlign", "CenterAlign", "CharacterAlign"][case / 12];
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ][case % 12 / 4];
        let writing_mode = match mode {
            WritingMode::Horizontal => schist_text_engine::WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight => schist_text_engine::WritingMode::VerticalLr,
            WritingMode::VerticalRightToLeft => schist_text_engine::WritingMode::VerticalRl,
        };
        let reverse = mode == WritingMode::Horizontal;
        let size = if case % 4 < 2 { 12.0 } else { 18.0 };
        let indent = if case.is_multiple_of(2) { 0.0 } else { 12.0 };
        let fields = ["אב", "12.34", "56,78"];
        let plain = |text: &str| schist_text_engine::TextSpec {
            text: text.into(),
            family: if text == "אב" {
                "Noto Sans Hebrew"
            } else {
                "IBM Plex Sans"
            }
            .into(),
            size,
            writing_mode,
            direction: schist_text_engine::ParagraphDirection::LeftToRight,
            ..Default::default()
        };
        let widths = fields.map(|text| schist_text_engine::measure(&plain(text)).unwrap().width);
        let positions = if reverse {
            [160.0 - indent - widths[0], 70.0, 10.0]
        } else {
            [indent, 48.0, 108.0]
        };
        let name = format!("RTL tabs {case}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("Noto Sans Hebrew".into()),
            point_size: Some(size),
            writing_mode: Some(mode),
            leading: Some(schist_layout::styles::Leading::Points(24.0)),
            direction: Some(if reference {
                ParagraphDirection::LeftToRight
            } else if case.is_multiple_of(2) {
                ParagraphDirection::RightToLeft
            } else {
                ParagraphDirection::Auto
            }),
            align: Some(if reverse && !reference {
                Align::Right
            } else {
                Align::Left
            }),
            first_line_indent: Some(if reference { 0.0 } else { indent }),
            list: ListStyle {
                tabs: (!reference).then(|| {
                    (0..2)
                        .map(|i| {
                            let character = ['.', ','][i];
                            let anchor = match alignment {
                                "LeftAlign" => 0.0,
                                "RightAlign" => widths[i + 1],
                                "CenterAlign" => widths[i + 1] / 2.0,
                                _ => {
                                    let caret = schist_text_engine::caret_at(
                                        &plain(fields[i + 1]),
                                        fields[i + 1].find(character).unwrap(),
                                    )
                                    .unwrap();
                                    if reverse {
                                        caret.x
                                    } else {
                                        caret.top
                                    }
                                }
                            };
                            ListTab {
                                position: if reverse {
                                    160.0 - positions[i + 1] - anchor
                                } else {
                                    positions[i + 1] + anchor
                                },
                                alignment: alignment.into(),
                                alignment_character: character.into(),
                                leader: ".".into(),
                            }
                        })
                        .collect()
                }),
                ..Default::default()
            },
            ..Default::default()
        });
        let inks: Vec<_> = (0..4)
            .map(|field| {
                let name = format!("RTL ink {case}/{field}");
                doc.styles.add_character(CharacterStyle {
                    name: name.clone(),
                    family: Some(
                        if field == 0 {
                            "Noto Sans Hebrew"
                        } else {
                            "IBM Plex Sans"
                        }
                        .into(),
                    ),
                    fill: Some(if field == 1 {
                        Ink::spot("RTL tab spot", [45.0, 60.0, 30.0])
                    } else {
                        Ink::cmyk("RTL tab cyan", [1.0, 0.0, 0.0, 0.0])
                    }),
                    fill_tint: Some(0.65),
                    opacity: Some(0.7),
                    stroke: Some(Ink::cmyk("RTL tab magenta", [0.0, 1.0, 0.0, 0.0])),
                    stroke_weight: Some(0.35),
                    stroke_tint: Some(0.75),
                    baseline_shift: (field == 3)
                        .then_some(schist_layout::styles::BaselineShift::Offset(2.0)),
                    overprint_fill: Some(case.is_multiple_of(2)),
                    overprint_stroke: Some(!case.is_multiple_of(2)),
                    ..Default::default()
                });
                name
            })
            .collect();
        if reference {
            let mut place = |offset: f32, text: &str, ink: &str| {
                assert_eq!(
                    schist_text_engine::measure(&plain("\u{200b}"))
                        .unwrap()
                        .width,
                    0.0
                );
                let padding = format!("RTL padding {case}/{}", doc.styles.characters.len());
                doc.styles.add_character(CharacterStyle {
                    name: padding.clone(),
                    tracking: Some(offset * 1000.0 / size),
                    ..Default::default()
                });
                let frame = authoring::text_frame(
                    doc,
                    &mut History::default(),
                    page,
                    Rect::new(20.0, 20.0, 160.0, 160.0),
                )
                .unwrap();
                let text = format!("\u{200b}{text}");
                let mut story = Story::from_text(&text, &name);
                story.ranges.push(StyleRange::new(0, 3, padding));
                story.ranges.push(StyleRange::new(3, text.len(), ink));
                doc.stories[frame.story.0 as usize] = story;
            };
            for i in 0..3 {
                place(positions[i], fields[i], &inks[i]);
            }
            let period = schist_text_engine::measure(&plain(".")).unwrap().width;
            for i in 0..2 {
                let (start, end) = if reverse {
                    (positions[i + 1] + widths[i + 1], positions[i])
                } else {
                    (positions[i] + widths[i], positions[i + 1])
                };
                let count =
                    ((f64::from(end) - f64::from(start)) / f64::from(period)).floor() as usize;
                assert!(
                    (schist_text_engine::measure(&plain(&".".repeat(count)))
                        .unwrap()
                        .width
                        - count as f32 * period)
                        .abs()
                        < 0.002
                );
                let offset = if reverse {
                    start
                } else {
                    end - count as f32 * period
                };
                if count > 0 {
                    place(offset, &".".repeat(count), &inks[3]);
                }
            }
        } else {
            let frame = authoring::text_frame(
                doc,
                &mut History::default(),
                page,
                Rect::new(20.0, 20.0, 160.0, 160.0),
            )
            .unwrap();
            let mut story = Story::from_text(fields.join("\t"), &name);
            let mut start = 0;
            for i in 0..3 {
                story
                    .ranges
                    .push(StyleRange::new(start, start + fields[i].len(), &inks[i]));
                if i < 2 {
                    story.ranges.push(StyleRange::new(
                        start + fields[i].len(),
                        start + fields[i].len() + 1,
                        &inks[3],
                    ));
                }
                start += fields[i].len() + 1;
            }
            doc.stories[frame.story.0 as usize] = story;
        }
    }
}
