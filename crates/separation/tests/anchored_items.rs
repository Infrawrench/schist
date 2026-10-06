use schist_layout::anchored::{self, AnchoredItem, AnchoredPosition};
use schist_layout::{
    authoring, authoring::ShapeKind, blank_a4, compose::compose_story, History, Ink,
    LayoutDocument, ParagraphStyle, Rect, Story, StoryId, StoryStructure, WritingMode,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings, Severity};

fn document(position: AnchoredPosition, rotation: f32) -> LayoutDocument {
    styled(position, rotation, "Body")
}

fn styled(position: AnchoredPosition, rotation: f32, style: &str) -> LayoutDocument {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Vertical".into(),
        writing_mode: Some(WritingMode::VerticalRightToLeft),
        ..Default::default()
    });
    doc.inks.push(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]));
    let mut scratch = doc.clone();
    let shape = authoring::shape(
        &mut scratch,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 40.0, 24.0),
        ShapeKind::Ellipse,
        authoring::Paint::filled("Cyan"),
    )
    .unwrap();
    let object = scratch.objects.into_iter().find(|o| o.id == shape).unwrap();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(60.0, 80.0, 380.0, 300.0),
    )
    .unwrap();
    let before = "Black text with art ";
    let mut story = Story::from_text(format!("{before} inline, then more black text."), style);
    story.structures.push(StoryStructure {
        at: Some(before.len()),
        kind: "Oval".into(),
        payload: "<Oval />".into(),
        control: None,
        footnote: None,
        table: None,
        anchored: Some(Box::new(AnchoredItem {
            position,
            y_offset: 1.0,
            object,
            members: Vec::new(),
            placement: Default::default(),
        })),
    });
    doc.stories[frame.story.0 as usize] = story;
    doc.objects
        .iter_mut()
        .find(|o| o.id == frame.object)
        .unwrap()
        .rotation = rotation;
    doc
}

#[test]
fn item_ink_lands_where_it_is_placed_in_every_position() {
    for (position, rotation) in [
        (AnchoredPosition::Inline, 0.0),
        (AnchoredPosition::Inline, 12.0),
        (AnchoredPosition::AboveLine, 0.0),
        (AnchoredPosition::AboveLine, 12.0),
        (AnchoredPosition::Anchored, 0.0),
        (AnchoredPosition::Anchored, 12.0),
    ] {
        let doc = document(position, rotation);
        let lines: Vec<_> = compose_story(&doc, StoryId(0)).lines().cloned().collect();
        let [placed] = &anchored::placements(&doc, &doc.stories[0], &doc.objects[0], &lines)[..]
        else {
            panic!()
        };
        let area = placed.visual_bounds();
        for dpi in [72.0, 144.0] {
            let result = separate_page(&doc, 0, OutputSettings::at(dpi), &NoGraphics).unwrap();
            let plate = result.separation.plate(result.plan.process[0]).unwrap();
            let scale = dpi / 72.0;
            let (mut inside, mut outside) = (0, 0);
            for y in 0..(doc.pages[0].height * scale) as i32 {
                for x in 0..(doc.pages[0].width * scale) as i32 {
                    if plate.at(x, y) <= 0.02 {
                        continue;
                    }
                    let p = ((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale);
                    // One point of slack covers antialiasing.
                    if p.0 > area.x - 1.0
                        && p.0 < area.right() + 1.0
                        && p.1 > area.y - 1.0
                        && p.1 < area.bottom() + 1.0
                    {
                        inside += 1;
                    } else {
                        outside += 1;
                    }
                }
            }
            assert!(inside > 100, "{position:?} {rotation} {dpi}: {inside}");
            assert_eq!(outside, 0, "{position:?} {rotation} {dpi}");
            assert!(!result
                .report
                .findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.message.contains("structure")));
        }
    }
}

#[test]
fn items_in_vertical_text_stay_reported_and_unpainted() {
    let doc = styled(AnchoredPosition::Inline, 0.0, "Vertical");
    let result = separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    let plate = result.separation.plate(result.plan.process[0]).unwrap();
    assert!(plate.data.iter().all(|v| *v <= 0.02));
    let expected = schist_i18n::tf!(
        "design.preflight_story_structure",
        name = doc.objects[0].name,
        count = 1
    );
    assert!(
        result.report.findings.iter().any(|f| f.message == expected),
        "{:?}",
        result.report.findings
    );
}

/// Native review: output scaled the line's text but not its box, so a line
/// raised by a tall item set its text at the wrong height at other resolutions.
#[test]
fn a_raised_lines_text_sits_on_its_baseline_at_every_resolution() {
    let mut doc = document(AnchoredPosition::Inline, 0.0);
    // A tall item: 60pt above the baseline.
    let item = doc.stories[0].structures[0].anchored.as_mut().unwrap();
    item.object.bounds.height = 60.0;
    if let schist_layout::LayoutObject::Shape { path, .. } = &mut item.object.object {
        path.map_points(|p| schist_layout::Point::new(p.x, p.y * 2.5));
    }
    let lines: Vec<_> = compose_story(&doc, StoryId(0)).lines().cloned().collect();
    let line = &lines[0];
    let [placed] = &anchored::placements(&doc, &doc.stories[0], &doc.objects[0], &lines)[..] else {
        panic!()
    };
    let right = placed.visual_bounds().right();
    let mut bottoms = Vec::new();
    for dpi in [72.0, 144.0] {
        let result = separate_page(&doc, 0, OutputSettings::at(dpi), &NoGraphics).unwrap();
        let black = result.separation.plate(result.plan.process[3]).unwrap();
        let scale = dpi / 72.0;
        // The lowest text ink right of the item, within the raised line.
        let mut bottom = f32::NEG_INFINITY;
        for y in (line.bounds.y * scale) as i32..(line.bounds.bottom() * scale) as i32 {
            for x in ((right + 2.0) * scale) as i32..(line.bounds.right() * scale) as i32 {
                if black.at(x, y) > 0.3 {
                    bottom = bottom.max((y as f32 + 1.0) / scale);
                }
            }
        }
        assert!(bottom.is_finite(), "{dpi}");
        // Descenders reach a few points below the baseline, never far above it.
        assert!(
            bottom > line.baseline - 1.0 && bottom < line.baseline + 6.0,
            "{dpi}: {bottom} {}",
            line.baseline
        );
        bottoms.push(bottom);
    }
    assert!((bottoms[0] - bottoms[1]).abs() < 1.5, "{bottoms:?}");
}

/// An anchored text frame's story paints inside the item, and text it cannot
/// hold is reported as overset under the item's name.
#[test]
fn an_anchored_text_frame_paints_its_story_and_reports_overset() {
    for (inner, lost) in [("Inner".to_owned(), false), ("words ".repeat(80), true)] {
        let mut doc = blank_a4();
        let host = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(60.0, 80.0, 380.0, 300.0),
        )
        .unwrap();
        let boxed = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(0.0, 0.0, 70.0, 30.0),
        )
        .unwrap();
        let mut object = doc.object(boxed.object).unwrap().clone();
        object.name = "Boxed note".into();
        doc.objects.retain(|o| o.id != boxed.object);
        doc.stories[boxed.story.0 as usize] = Story::from_text(inner, "Body");
        let before = "Text ";
        let mut story = Story::from_text(format!("{before} and more."), "Body");
        story.structures.push(StoryStructure {
            at: Some(before.len()),
            kind: "TextFrame".into(),
            payload: "<TextFrame />".into(),
            control: None,
            footnote: None,
            table: None,
            anchored: Some(Box::new(AnchoredItem::inline(object))),
        });
        doc.stories[host.story.0 as usize] = story;
        let lines: Vec<_> = compose_story(&doc, StoryId(0)).lines().cloned().collect();
        let [placed] = &anchored::placements(&doc, &doc.stories[0], &doc.objects[0], &lines)[..]
        else {
            panic!()
        };
        let area = placed.visual_bounds();
        let result = separate_page(&doc, 0, OutputSettings::at(144.0), &NoGraphics).unwrap();
        let black = result.separation.plate(result.plan.process[3]).unwrap();
        let mut inside = 0;
        for y in (area.y * 2.0) as i32..(area.bottom() * 2.0) as i32 {
            for x in (area.x * 2.0) as i32..(area.right() * 2.0) as i32 {
                if black.at(x, y) > 0.3 {
                    inside += 1;
                }
            }
        }
        assert!(inside > 20, "{lost}: {inside}");
        let overset = schist_i18n::tf!("design.preflight_overset", name = "Boxed note");
        assert_eq!(
            result.report.findings.iter().any(|f| f.message == overset),
            lost,
            "{:?}",
            result.report.findings
        );
    }
}

/// A custom item's wrap moves only its own story's later lines; a frame of
/// another story it reaches is told its text does not wrap.
#[test]
fn custom_wrap_reaching_another_story_is_reported_there() {
    use schist_layout::anchored::{
        AnchorPoint, HorizontalAlignment, HorizontalReference, Placement, VerticalReference,
    };
    let mut doc = blank_a4();
    doc.inks.push(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]));
    let mut scratch = doc.clone();
    let shape = authoring::shape(
        &mut scratch,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 60.0, 60.0),
        ShapeKind::Rectangle,
        authoring::Paint::filled("Cyan"),
    )
    .unwrap();
    let mut object = scratch.objects.into_iter().find(|o| o.id == shape).unwrap();
    object.appearance.text_wrap = Some(schist_layout::text_wrap::TextWrap {
        mode: schist_layout::text_wrap::WrapMode::BoundingBox,
        ..Default::default()
    });
    let host = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 80.0, 200.0, 300.0),
    )
    .unwrap();
    let other = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(260.0, 80.0, 200.0, 300.0),
    )
    .unwrap();
    for (id, name) in [(host.object, "Host"), (other.object, "Neighbour")] {
        doc.objects.iter_mut().find(|o| o.id == id).unwrap().name = name.into();
    }
    let words = "Words to fill the frame so that its lines run past the item. ".repeat(6);
    let mut story = Story::from_text(words.clone(), "Body");
    story.structures.push(StoryStructure {
        at: Some(10),
        kind: "Rectangle".into(),
        payload: "<Rectangle />".into(),
        control: None,
        footnote: None,
        table: None,
        anchored: Some(Box::new(AnchoredItem {
            position: AnchoredPosition::Anchored,
            y_offset: 0.0,
            // Its top left 30 pt right of the host frame, over the neighbour.
            placement: Placement {
                anchor_point: AnchorPoint::TopLeft,
                horizontal_reference: HorizontalReference::TextFrame,
                horizontal_alignment: HorizontalAlignment::Right,
                vertical_reference: VerticalReference::LineBaseline,
                x_offset: 30.0,
                ..Default::default()
            },
            object,
            members: Vec::new(),
        })),
    });
    doc.stories[host.story.0 as usize] = story;
    doc.stories[other.story.0 as usize] = Story::from_text(words, "Body");
    let result = separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    let warned = |name: &str| {
        let message = schist_i18n::tf!("design.preflight_wrap_ignored", name = name);
        result.report.findings.iter().any(|f| f.message == message)
    };
    assert!(warned("Neighbour"), "{:?}", result.report.findings);
    assert!(!warned("Host"));
}

/// A table's fills, edges and cell text all reach the plates, inside its grid.
#[test]
fn a_tables_fill_edges_and_cell_text_paint_inside_its_grid() {
    use schist_layout::tables::{
        CellEdge, CellJustification, CellPaint, Table, TableCell, TableRow,
    };
    let mut doc = blank_a4();
    let cyan = Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]);
    doc.inks.push(cyan.clone());
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(60.0, 80.0, 380.0, 300.0),
    )
    .unwrap();
    let edge = CellEdge {
        weight: 1.0,
        paint: Some(CellPaint {
            ink: schist_layout::Ink::black(),
            tint: 1.0,
        }),
    };
    let mut cells = Vec::new();
    for column in 0..2 {
        doc.stories.push(Story::from_text("Cell text", "Body"));
        cells.push(TableCell {
            column,
            row: 0,
            columns: 1,
            rows: 1,
            story: StoryId((doc.stories.len() - 1) as u32),
            fill: (column == 0).then(|| CellPaint {
                ink: cyan.clone(),
                tint: 1.0,
            }),
            insets: schist_layout::Insets::uniform(4.0),
            justification: CellJustification::Top,
            edges: [edge.clone(), edge.clone(), edge.clone(), edge.clone()],
        });
    }
    let table = Table {
        header_rows: 0,
        footer_rows: 0,
        rows: vec![TableRow {
            height: 40.0,
            minimum: 40.0,
            maximum: None,
            auto_grow: true,
        }],
        columns: vec![100.0, 100.0],
        cells,
    };
    let mut host = Story::from_text("", "Body");
    host.structures.push(StoryStructure {
        at: Some(0),
        kind: "Table".into(),
        payload: "<Table />".into(),
        control: None,
        footnote: None,
        table: Some(Box::new(table)),
        anchored: None,
    });
    doc.stories[frame.story.0 as usize] = host;
    let result = separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    let ink = |plate: usize, x: f32, y: f32| {
        result
            .separation
            .plate(plate)
            .unwrap()
            .at(x as i32, y as i32)
    };
    let (cyan, black) = (result.plan.process[0], result.plan.process[3]);
    // The first cell is filled cyan; the second is not.
    assert!(ink(cyan, 110.0, 115.0) > 0.9);
    assert!(ink(cyan, 210.0, 115.0) < 0.02);
    // Edges: the outer left stroke and the shared vertical edge.
    assert!(ink(black, 60.0, 110.0) > 0.4);
    assert!(ink(black, 160.0, 110.0) > 0.4);
    // Cell text inside the second cell.
    let mut text = 0;
    for y in 80..121 {
        for x in 165..260 {
            if ink(black, x as f32, y as f32) > 0.3 {
                text += 1;
            }
        }
    }
    assert!(text > 20, "{text}");
    assert!(!result
        .report
        .findings
        .iter()
        .any(|f| f.message.contains("structure")));
}
