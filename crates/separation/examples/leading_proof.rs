//! Flowed mixed-size text versus separately placed lines at known baseline
//! distances, including a blank line. Each pair must render identically.
use schist_layout::{
    authoring, styles::Leading, CharacterStyle, History, LayoutDocument, Page, ParagraphStyle,
    Rect, Story, WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 320.0, 320.0); 12]);
    doc.styles.add_character(CharacterStyle {
        name: "Large".into(),
        point_size: Some(40.0),
        ..Default::default()
    });
    for page in 0..12 {
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalRightToLeft,
            WritingMode::VerticalLeftToRight,
        ][page / 4];
        let automatic = page % 4 >= 2;
        let reference = page % 2 == 1;
        let name = format!("Style {page}");
        let parent = format!("Parent {page}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: parent.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(20.0),
            leading: Some(Leading::Points(50.0)),
            auto_leading: Some(150.0),
            writing_mode: Some(mode),
            features: vec![("liga".into(), true)],
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            based_on: Some(parent),
            leading: automatic.then_some(Leading::Auto),
            ..Default::default()
        });
        if !reference {
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                page,
                Rect::new(20.0, 20.0, 280.0, 280.0),
            )
            .unwrap();
            let mut story = Story::from_text("HH\nHH\n\nHH", &name);
            story.apply_style(3, 5, "Large");
            doc.stories[frame.story.0 as usize] = story;
        } else {
            let metrics = |size| {
                schist_text_engine::measure(&schist_text_engine::TextSpec {
                    text: "HH".into(),
                    family: "IBM Plex Sans".into(),
                    size,
                    ..Default::default()
                })
                .unwrap()
            };
            let regular = metrics(20.0);
            let mut distance = 0.0;
            for (index, size) in [20.0, 40.0, 20.0, 20.0].into_iter().enumerate() {
                if index > 0 {
                    distance += if automatic { size * 1.5 } else { 50.0 };
                }
                if index == 2 {
                    continue;
                } // The blank still advances the following baseline.
                let cell = metrics(size);
                let style = format!("Reference {page} {index}");
                doc.styles.add_paragraph(ParagraphStyle {
                    name: style.clone(),
                    based_on: Some(name.clone()),
                    point_size: Some(size),
                    ..Default::default()
                });
                let bounds = match mode {
                    WritingMode::Horizontal => Rect::new(
                        20.0,
                        20.0 + regular.first_baseline + distance - cell.first_baseline,
                        280.0,
                        80.0,
                    ),
                    WritingMode::VerticalRightToLeft => {
                        let center = 300.0 - regular.height / 2.0 - distance;
                        Rect::new(center + cell.height / 2.0 - 80.0, 20.0, 80.0, 280.0)
                    }
                    WritingMode::VerticalLeftToRight => {
                        let center = 20.0 + regular.height / 2.0 + distance;
                        Rect::new(center - cell.height / 2.0, 20.0, 80.0, 280.0)
                    }
                };
                let frame =
                    authoring::text_frame(&mut doc, &mut History::default(), page, bounds).unwrap();
                doc.stories[frame.story.0 as usize] = Story::from_text("HH", style);
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
                trim: (320.0, 320.0),
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
