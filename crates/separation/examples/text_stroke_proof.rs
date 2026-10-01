//! Each pair compares styled fill/stroke text with two independently placed
//! text objects, in all writing modes, both stroke alignments and all joins.
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
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 12]);
    doc.styles.add_character(CharacterStyle {
        name: "Default".into(),
        opacity: Some(0.7),
        ..Default::default()
    });
    for case in 0..6 {
        let name = format!("Case {case}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(30.0),
            leading: Some(schist_layout::styles::Leading::Points(48.0)),
            writing_mode: Some(
                [
                    WritingMode::Horizontal,
                    WritingMode::VerticalRightToLeft,
                    WritingMode::VerticalLeftToRight,
                ][case / 2],
            ),
            fill: Some(Ink::cmyk("Text cyan", [1.0, 0.0, 0.0, 0.0])),
            stroke: Some(Ink::spot("Outline spot", [45.0, 60.0, 30.0])),
            stroke_weight: Some(2.5),
            stroke_outside: Some(case % 2 == 1),
            stroke_join: Some(
                [
                    schist_text_engine::TextStrokeJoin::Miter,
                    schist_text_engine::TextStrokeJoin::Round,
                    schist_text_engine::TextStrokeJoin::Bevel,
                ][case % 3],
            ),
            stroke_miter_limit: Some(if case % 2 == 0 { 0.0 } else { 8.0 }),
            fill_tint: Some(0.85),
            stroke_tint: Some(0.8),
            overprint_stroke: Some(case % 2 == 1),
            ..Default::default()
        });
        for (suffix, fill_disabled, stroke_disabled) in
            [("fill", false, true), ("stroke", true, false)]
        {
            doc.styles.add_paragraph(ParagraphStyle {
                name: format!("{name} {suffix}"),
                based_on: Some(name.clone()),
                fill_disabled,
                stroke_disabled,
                ..Default::default()
            });
        }
        for reference in [false, true] {
            let page = case * 2 + usize::from(reference);
            let styles = if reference {
                vec![format!("{name} fill"), format!("{name} stroke")]
            } else {
                vec![name.clone()]
            };
            for style in styles {
                let frame = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    page,
                    Rect::new(25.0, 25.0, 140.0, 140.0),
                )
                .unwrap();
                doc.stories[frame.story.0 as usize] = Story::from_text("AVA AV\nType", &style);
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
    use schist_separation::pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf};
    let settings = OutputSettings::at(144.0);
    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    for page in 0..12 {
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
