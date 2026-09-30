//! Typography proof for independent rendering, including real drop-cap ink.
use schist_layout::{
    authoring, blank_a4, CharacterStyle, History, Ink, ParagraphStyle, Rect, Story,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).expect("output PDF path");
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec(),
    );
    let mut doc = blank_a4();
    doc.pages[0] = schist_layout::Page::new("1", 360.0, 240.0);
    doc.pages[0].margins.top = 20.0;
    doc.grids.document.mode = schist_layout::grid::GridMode::SnapToGrid;
    doc.grids.document.baseline_count = 72.0 / 14.0;
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Caps".into(),
        family: Some(schist_text_engine::default_family()),
        point_size: Some(11.0),
        leading: Some(schist_layout::styles::Leading::Points(14.0)),
        drop_caps_lines: Some(3),
        drop_caps_characters: Some(1),
        keep_lines: Some(1),
        space_after: Some(7.0),
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Caps RTL".into(),
        based_on: Some("Caps".into()),
        direction: Some(schist_layout::ParagraphDirection::RightToLeft),
        align: Some(schist_layout::styles::Align::Right),
        drop_caps_characters: Some(2),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Red".into(),
        fill: Some(Ink::cmyk("Red", [0.0, 1.0, 1.0, 0.0])),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Blue".into(),
        fill: Some(Ink::cmyk("Blue", [1.0, 1.0, 0.0, 0.0])),
        ..Default::default()
    });
    let guide = Ink::process("Guide", [0.88; 3]);
    doc.inks.push(guide.clone());
    for y in doc.grids.document.baselines(&doc.pages[0]) {
        let mut path = authoring::path_for(authoring::ShapeKind::Rectangle, 336.0, 0.25);
        path.map_points(|p| schist_layout::Point::new(p.x + 12.0, p.y + y));
        authoring::path_shape(
            &mut doc,
            &mut History::default(),
            0,
            path,
            authoring::Paint::filled(guide.name.clone()),
        )
        .unwrap();
    }
    for (x, style, ink, prefix) in [
        (20.0, "Caps", "Red", "A"),
        (190.0, "Caps RTL", "Blue", "Éc"),
    ] {
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(x, 23.7, 150.0, 195.0),
        )
        .unwrap();
        let mut story = Story::from_text(format!("{prefix} clear opening initial shares three lines with this paragraph and keeps all its words."), style);
        story.apply_style(0, prefix.len(), ink);
        if x < 100.0 {
            story.push_paragraph("Both paragraphs retain their own enlarged initial, with the body text resting on the page grid.", style);
            let start = story.point_offsets()[1];
            story.apply_style(start, start + 1, ink);
        }
        *doc.story_mut(frame.story) = story;
        assert!(!schist_layout::compose::compose_story(&doc, frame.story).has_overflow());
    }
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Vertical".into(),
        family: Some("Noto Sans CJK JP".into()),
        point_size: Some(11.0),
        leading: Some(schist_layout::styles::Leading::Points(15.0)),
        keep_lines: Some(1),
        ..Default::default()
    });
    let vertical = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(190.0, 135.0, 150.0, 75.0),
    )
    .unwrap();
    let story = doc.story_mut(vertical.story);
    *story = Story::from_text("縦書きの日本語。ABCと数字123。".repeat(2), "Vertical");
    story.prefs.orientation = schist_layout::StoryOrientation::Vertical;
    assert!(!schist_layout::compose::compose_story(&doc, vertical.story).has_overflow());
    for (name, mode) in [
        ("Mixed horizontal", schist_layout::WritingMode::Horizontal),
        (
            "Mixed vertical",
            schist_layout::WritingMode::VerticalRightToLeft,
        ),
    ] {
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.into(),
            based_on: Some("Vertical".into()),
            writing_mode: Some(mode),
            ..Default::default()
        });
    }
    let mixed = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 160.0, 150.0, 55.0),
    )
    .unwrap();
    let story = doc.story_mut(mixed.story);
    *story = Story::from_text("横書き", "Mixed horizontal");
    story.push_paragraph("縦書き", "Mixed vertical");
    story.push_paragraph("ABC", "Mixed horizontal");
    assert!(!schist_layout::compose::compose_story(&doc, mixed.story).has_overflow());
    let settings = OutputSettings::at(144.0);
    let page = separate_page(&doc, 0, settings, &NoGraphics).unwrap();
    let bytes =
        schist_separation::pdf::write_document(&[page], &[(360.0, 240.0)], &[0.0], settings)?;
    std::fs::write(path, bytes)?;
    Ok(())
}
