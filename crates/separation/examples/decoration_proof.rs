//! Paired undecorated/decorated text for independent PDF pixel comparisons.
use schist_layout::{
    authoring, CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story,
    WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 4]);
    for (name, ink) in [
        ("Cyan", Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
        ("Magenta", Ink::cmyk("Magenta", [0.0, 1.0, 0.0, 0.0])),
    ] {
        doc.styles.add_character(CharacterStyle {
            name: name.into(),
            fill: Some(ink),
            opacity: Some(0.5),
            ..Default::default()
        });
    }
    for page in 0..4 {
        let decorated = page % 2 == 1;
        let modes = if page < 2 {
            vec![WritingMode::Horizontal]
        } else {
            vec![
                WritingMode::VerticalRightToLeft,
                WritingMode::VerticalLeftToRight,
            ]
        };
        for (i, mode) in modes.into_iter().enumerate() {
            let name = format!("Style {page} {i}");
            doc.styles.add_paragraph(ParagraphStyle {
                name: name.clone(),
                family: Some("IBM Plex Sans".into()),
                point_size: Some(40.0),
                underline: Some(decorated),
                strikethrough: Some(decorated),
                writing_mode: Some(mode),
                ..Default::default()
            });
            let rect = if page < 2 {
                Rect::new(10.0, 30.0, 180.0, 100.0)
            } else {
                Rect::new(15.0 + i as f32 * 90.0, 15.0, 80.0, 170.0)
            };
            let frame =
                authoring::text_frame(&mut doc, &mut History::default(), page, rect).unwrap();
            let mut story = Story::from_text("HH HH", &name);
            story.apply_style(0, 3, "Cyan");
            story.apply_style(3, 5, "Magenta");
            doc.stories[frame.story.0 as usize] = story;
        }
    }
    let settings = OutputSettings::at(144.0);
    let separated: Vec<_> = (0..4)
        .map(|page| separate_page_without_graphics(&doc, page, settings).unwrap())
        .collect();
    use schist_separation::pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf};
    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    for page in &separated {
        let output = PageOutput {
            separated: page,
            trim: (200.0, 200.0),
            bleed: schist_layout::Insets::ZERO,
            slug: schist_layout::Insets::ZERO,
            settings,
            imposition: Imposition::default(),
            marks: Marks::default(),
            overprint: true,
        };
        pages.push(write_sheet(&mut pdf, &[output], Imposition::default())?);
    }
    std::fs::write(output, pdf.finish(&pages))?;
    Ok(())
}
