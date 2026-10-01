//! Facing pages with one shared image/text frame and overlapping transparent shapes.
use schist_layout::{
    authoring::{self, Paint},
    History, Ink, Insets, LayoutDocument, Page, Rect, Spread, Story,
};
use schist_separation::{
    pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf},
    separate_page, GraphicPlacement, GraphicSource, OutputSettings, PlacedGraphic,
};
struct Pixels;
impl GraphicSource for Pixels {
    fn sample(&self, _: &schist_layout::Link, p: &GraphicPlacement) -> Option<PlacedGraphic> {
        let mut graphic = PlacedGraphic::solid(p.dest, [0.0; 4]);
        for y in 0..p.dest.height() {
            for x in 0..p.dest.width() {
                graphic.cmyk[(y * p.dest.width() + x) as usize] = if x < p.dest.width() / 2 {
                    [1.0, 1.0, 0.0, 0.0]
                } else {
                    [1.0, 0.0, 1.0, 0.0]
                };
            }
        }
        Some(graphic)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    let mut doc = LayoutDocument::new(vec![Page::new("page", 120.0, 90.0); 2]);
    doc.spreads = vec![Spread {
        pages: vec![0, 1],
        ..Spread::single(0)
    }];
    doc.inks.push(Ink::cmyk("Red", [0.0, 1.0, 1.0, 0.0]));
    for page in &mut doc.pages {
        page.bleed = Insets::uniform(6.0);
    }
    // The first shape belongs to the right page. Page-grouped serialization
    // would put it above the second shape and change the overlap's color.
    authoring::rectangle(
        &mut doc,
        &mut History::default(),
        1,
        Rect::new(-25.0, 10.0, 50.0, 20.0),
        Paint::filled("Red"),
    )
    .unwrap();
    authoring::rectangle(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(108.0, 10.0, 24.0, 20.0),
        Paint::filled("Black"),
    )
    .unwrap();
    doc.objects.last_mut().unwrap().transparency = 0.5;
    authoring::graphic_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(90.0, 36.0, 60.0, 18.0),
        "proof",
        false,
    )
    .unwrap();
    let text = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(90.0, 62.0, 90.0, 25.0),
    )
    .unwrap();
    doc.stories[text.story.0 as usize] = Story::from_text("CROSSOVER", "Body");
    let settings = OutputSettings::at(144.0);
    let separated: Vec<_> = (0..2)
        .map(|page| separate_page(&doc, page, settings, &Pixels).unwrap())
        .collect();
    let outputs: Vec<_> = (0..2)
        .map(|page| PageOutput {
            separated: &separated[page],
            trim: (120.0, 90.0),
            bleed: doc.pages[page].bleed,
            slug: Insets::ZERO,
            settings,
            imposition: Imposition::default(),
            marks: Marks::default(),
            overprint: true,
        })
        .collect();
    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    for page in &outputs {
        pages.push(write_sheet(
            &mut pdf,
            std::slice::from_ref(page),
            Imposition::default(),
        )?);
    }
    pages.push(write_sheet(
        &mut pdf,
        &outputs,
        Imposition {
            up: 2,
            marks: false,
        },
    )?);
    std::fs::write(output, pdf.finish(&pages))?;
    Ok(())
}
