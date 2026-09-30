//! Native script preferences scale glyphs without shrinking line spacing.
use schist_layout::{
    authoring,
    styles::{BaselineShift, TextPosition, TextPreferences},
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story, WritingMode,
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
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 9]);
    doc.styles.text_preferences = TextPreferences {
        superscript_size: 50.0,
        superscript_position: 50.0,
        subscript_size: 50.0,
        subscript_position: 50.0,
    };
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
    for page in 0..9 {
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalRightToLeft,
            WritingMode::VerticalLeftToRight,
        ][page / 3];
        let position = [
            TextPosition::Normal,
            TextPosition::Superscript,
            TextPosition::Subscript,
        ][page % 3];
        let vertical = mode != WritingMode::Horizontal;
        let name = format!("Style {page}");
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
            leading: Some(schist_layout::styles::Leading::Points(40.0)),
            writing_mode: Some(mode),
            position: Some(position),
            baseline_shift: Some(BaselineShift::Offset(3.0)),
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            page,
            Rect::new(60.0, 60.0, 100.0, 100.0),
        )
        .unwrap();
        let text = if vertical { "日本\n日本" } else { "HH\nHH" };
        let split = if vertical { 6 } else { 2 };
        let mut story = Story::from_text(text, &name);
        story.apply_style(0, split, "Cyan");
        story.apply_style(split + 1, text.len(), "Magenta");
        doc.stories[frame.story.0 as usize] = story;
    }
    use schist_separation::pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf};
    let settings = OutputSettings::at(144.0);
    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    for page in 0..9 {
        let separated = separate_page_without_graphics(&doc, page, settings)
            .ok_or("proof page could not separate")?;
        pages.push(write_sheet(
            &mut pdf,
            &[PageOutput {
                separated: &separated,
                trim: (200.0, 200.0),
                bleed: schist_layout::Insets::ZERO,
                slug: schist_layout::Insets::ZERO,
                settings,
                imposition: Imposition::default(),
                marks: Marks::default(),
                overprint: true,
            }],
            Imposition::default(),
        )?);
    }
    std::fs::write(output, pdf.finish(&pages))?;
    Ok(())
}
