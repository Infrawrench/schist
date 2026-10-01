//! Paired pages expose baseline offsets independently of the Schist renderer.
use schist_layout::{
    authoring, styles::BaselineShift, CharacterStyle, History, Ink, LayoutDocument, Page,
    ParagraphStyle, Rect, Story, WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec(),
    );
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 8]);
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
    for page in 0..8 {
        let (mode, shift, rotation) = match page / 2 {
            0 => (WritingMode::Horizontal, 8.0, 0.0),
            1 => (WritingMode::VerticalRightToLeft, 8.0, 0.0),
            2 => (WritingMode::VerticalLeftToRight, -8.0, 0.0),
            _ => (WritingMode::Horizontal, 8.0, 90.0),
        };
        let name = format!("Style {page}");
        let vertical = mode != WritingMode::Horizontal;
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some(
                if vertical {
                    "Noto Sans CJK JP"
                } else {
                    "IBM Plex Sans"
                }
                .into(),
            ),
            point_size: Some(24.0),
            underline: Some(true),
            strikethrough: Some(true),
            writing_mode: Some(mode),
            baseline_shift: Some(BaselineShift::Offset(if page % 2 == 0 {
                0.0
            } else {
                shift
            })),
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            page,
            Rect::new(40.0, 40.0, 120.0, 120.0),
        )
        .unwrap();
        doc.objects
            .iter_mut()
            .find(|o| o.id == frame.object)
            .unwrap()
            .rotation = rotation;
        let text = if vertical { "日本 H H" } else { "HH HH" };
        let split = if vertical { 6 } else { 3 };
        let mut story = Story::from_text(text, &name);
        story.apply_style(0, split, "Cyan");
        story.apply_style(split, text.len(), "Magenta");
        doc.stories[frame.story.0 as usize] = story;
    }
    use schist_separation::pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf};
    let settings = OutputSettings::at(144.0);
    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    for page in 0..8 {
        let separated = separate_page_without_graphics(&doc, page, settings)
            .ok_or("proof page could not separate")?;
        let output = PageOutput {
            separated: &separated,
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
