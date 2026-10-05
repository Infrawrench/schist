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
