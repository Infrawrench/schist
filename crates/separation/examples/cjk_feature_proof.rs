//! Each writing mode has off, native directional defaults and explicit-tag
//! control pages. On/control pixels match; real proportional metrics differ from off.
use schist_layout::{
    authoring, directional_features::DirectionalFeatures, History, LayoutDocument, Page,
    ParagraphStyle, Rect, Story, WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec(),
    );
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 9]);
    for page in 0..9 {
        let name = format!("Style {page}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("Noto Sans CJK JP".into()),
            point_size: Some(22.0),
            leading: Some(schist_layout::styles::Leading::Points(44.0)),
            writing_mode: Some(
                [
                    WritingMode::Horizontal,
                    WritingMode::VerticalRightToLeft,
                    WritingMode::VerticalLeftToRight,
                ][page / 3],
            ),
            directional_features: if page % 3 == 2 {
                DirectionalFeatures::default()
            } else {
                DirectionalFeatures {
                    kana: Some(page % 3 == 1),
                    proportional_metrics: Some(page % 3 == 1),
                }
            },
            features: if page % 3 == 2 {
                vec![
                    ("palt".into(), page / 3 == 0),
                    ("vpal".into(), page / 3 != 0),
                    ("hkna".into(), page / 3 == 0),
                    ("vkna".into(), page / 3 != 0),
                ]
            } else {
                Vec::new()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            page,
            Rect::new(15.0, 15.0, 170.0, 170.0),
        )
        .unwrap();
        let story = Story::from_text("かなカナ。、\nかなカナ。、", &name);
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
