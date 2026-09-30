use schist_layout::{
    authoring, blank_a4,
    compose::{compose_story, line_spec},
    grid::{GridMode, GridSettings},
    CharacterStyle, History, LayoutObject, ParagraphStyle, Rect, Story,
};

#[test]
fn baselines_follow_each_frames_page_grid_with_mixed_fonts_and_empty_paragraphs() {
    for mode in [GridMode::SnapToGrid, GridMode::LinesPerGrid] {
        for columns in [1, 2, 3] {
            let mut doc = blank_a4();
            doc.grids.document = GridSettings {
                mode,
                baseline_count: 6.0,
                ..Default::default()
            };
            doc.pages[0].margins.top = 23.25;
            let mut other = doc.pages[0].clone();
            other.margins.top = 37.5;
            doc.add_page(other);
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Plain".into(),
                point_size: Some(11.0),
                leading: Some(15.0),
                keep_lines: Some(1),
                space_after: Some(3.0),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Large".into(),
                point_size: Some(22.0),
                leading: Some(29.0),
                ..Default::default()
            });
            for page in 0..2 {
                let mut history = History::default();
                let frame = authoring::text_frame(
                    &mut doc,
                    &mut history,
                    page,
                    Rect::new(10.0, 13.7, 360.0, 450.0),
                )
                .unwrap();
                let mut story =
                    Story::from_text("Small text followed by much larger words.", "Plain");
                story.apply_style(19, 39, "Large");
                story.push_paragraph("", "Plain");
                story.push_paragraph("Another paragraph with ordinary letters.", "Plain");
                *doc.story_mut(frame.story) = story;
                if let LayoutObject::TextFrame { columns: count, .. } =
                    &mut doc.objects.last_mut().unwrap().object
                {
                    *count = columns;
                }
                let thread = compose_story(&doc, frame.story);
                assert!(!thread.has_overflow());
                assert!(thread.lines().any(|l| l.forced_break));
                let mut previous = None;
                for line in thread.lines() {
                    let spec = line_spec(line, doc.story(frame.story).unwrap(), &doc);
                    let metrics = schist_text_engine::measure(&spec).unwrap();
                    let baseline = line.bounds.y + metrics.first_baseline;
                    let phase = (baseline - doc.pages[page].margins.top) / 12.0;
                    assert!(
                        (phase - phase.round()).abs() < 0.0001,
                        "page {page}, {columns} columns, {mode:?}: baseline {baseline}"
                    );
                    assert!(baseline >= doc.pages[page].margins.top);
                    assert!(line.bounds.y >= 13.7 - 0.0001);
                    assert!(line.bounds.bottom() <= 463.7 + 0.0001);
                    if let Some((x, bottom, last_baseline)) = previous {
                        if x == line.bounds.x {
                            assert!(line.bounds.y >= bottom - 0.001);
                            assert!(baseline > last_baseline);
                        }
                    }
                    previous = Some((line.bounds.x, line.bounds.bottom(), baseline));
                }
            }
        }
    }
}

#[test]
fn grid_phase_consumes_room_including_for_blank_lines() {
    for text in ["Word", ""] {
        let mut doc = blank_a4();
        doc.grids.document.mode = GridMode::SnapToGrid;
        doc.grids.document.baseline_count = 3.0;
        doc.pages[0].margins.top = 30.0;
        let mut history = History::default();
        let frame =
            authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 0.0, 100.0, 20.0))
                .unwrap();
        *doc.story_mut(frame.story) = Story::from_text(text, "Body");
        let thread = compose_story(&doc, frame.story);
        assert_eq!(thread.lines().count(), 0, "first grid baseline cannot fit");
        assert_eq!(thread.has_overflow(), !text.is_empty());
        doc.objects.last_mut().unwrap().bounds.height = 70.0;
        let thread = compose_story(&doc, frame.story);
        assert_eq!(thread.lines().count(), 1);
        assert!(!thread.has_overflow());
    }
}

#[test]
fn a_named_frame_grid_controls_the_actual_baselines() {
    let mut doc = blank_a4();
    doc.pages[0].margins.top = 23.25;
    doc.grids.document = GridSettings {
        mode: GridMode::SnapToGrid,
        baseline_count: 6.0,
        ..Default::default()
    };
    doc.grids.set_named(
        "Caption",
        GridSettings {
            mode: GridMode::SnapToGrid,
            baseline_count: 4.0,
            ..Default::default()
        },
    );
    for (name, interval) in [("Body", 12.0), ("Caption", 18.0)] {
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(10.0, 29.0, 200.0, 400.0),
        )
        .unwrap();
        doc.objects.last_mut().unwrap().name = name.into();
        *doc.story_mut(frame.story) =
            Story::from_text("Words on the baseline grid. ".repeat(10), "Body");
        let thread = compose_story(&doc, frame.story);
        assert!(!thread.has_overflow());
        for line in thread.lines() {
            let phase = (line.baseline - doc.pages[0].margins.top) / interval;
            assert!(
                (phase - phase.round()).abs() < 0.001,
                "{name}: {}",
                line.baseline
            );
        }
    }
}
