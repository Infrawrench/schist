use schist_layout::{
    authoring, CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story,
    WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

#[test]
fn decorated_text_paints_each_translucent_ink_once_even_where_lines_cross_glyphs() {
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        for opacity in [0.125, 0.5, 1.0] {
            for overprint in [false, true] {
                let mut doc = LayoutDocument::new(vec![Page::new("1", 220.0, 220.0)]);
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Decorated".into(),
                    point_size: Some(40.0),
                    underline: Some(true),
                    strikethrough: Some(true),
                    writing_mode: Some(mode),
                    ..Default::default()
                });
                for (name, ink) in [
                    ("Cyan", Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
                    ("Magenta", Ink::cmyk("Magenta", [0.0, 1.0, 0.0, 0.0])),
                ] {
                    doc.styles.add_character(CharacterStyle {
                        name: name.into(),
                        fill: Some(ink),
                        opacity: Some(opacity),
                        overprint_fill: Some(overprint),
                        ..Default::default()
                    });
                }
                let frame = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(10.0, 10.0, 200.0, 200.0),
                )
                .unwrap();
                let mut story = Story::from_text("HH HH", "Decorated");
                story.apply_style(0, 3, "Cyan");
                story.apply_style(3, 5, "Magenta");
                doc.stories[frame.story.0 as usize] = story;
                let out =
                    separate_page_without_graphics(&doc, 0, OutputSettings::at(144.0)).unwrap();
                for channel in [0, 1] {
                    let data = &out
                        .separation
                        .plate(out.plan.process[channel])
                        .unwrap()
                        .data;
                    assert!(
                        data.iter().all(|v| *v <= opacity + 1e-6),
                        "decoration repainted its glyph at {opacity}"
                    );
                    assert!(data.iter().filter(|v| (**v - opacity).abs() < 1e-6).count() > 100);
                }
            }
        }
    }
}
