//! Unequal bleed/slug edges expose origin errors that symmetric proofs hide.
use schist_layout::{authoring, History, Ink, Insets, LayoutDocument, Page, Rect};
use schist_separation::{
    pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf},
    separate_page_without_graphics, OutputSettings,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).expect("output PDF path");
    let settings = OutputSettings::at(72.0);
    let mut doc = LayoutDocument::new(vec![Page::new("1", 60.0, 40.0); 2]);
    doc.pages[0].bleed = Insets::new(4.0, 6.0, 8.0, 10.0);
    doc.pages[0].slug = Insets::new(3.0, 12.0, 1.0, 14.0);
    doc.pages[1].bleed = Insets::new(8.0, 10.0, 4.0, 6.0);
    doc.pages[1].slug = Insets::new(1.0, 14.0, 3.0, 12.0);
    doc.inks.push(Ink::process("Red", [1.0, 0.0, 0.0]));
    for i in 0..2 {
        let bounds = doc.pages[i].bleed_rect();
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            i,
            bounds,
            authoring::Paint::filled("Red"),
        )
        .unwrap();
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            i,
            Rect::new(0.0, 0.0, 60.0, 40.0),
            authoring::Paint::filled("Black"),
        )
        .unwrap();
    }
    let separated: Vec<_> = (0..2)
        .map(|i| separate_page_without_graphics(&doc, i, settings).unwrap())
        .collect();
    let outputs: Vec<_> = (0..2)
        .map(|i| PageOutput {
            separated: &separated[i],
            trim: (60.0, 40.0),
            bleed: doc.pages[i].bleed,
            slug: doc.pages[i].slug,
            settings,
            imposition: Imposition::default(),
            marks: Marks::default(),
            overprint: true,
        })
        .collect();
    let mut pdf = Pdf::new();
    let first = write_sheet(&mut pdf, &outputs[..1], Imposition::default())?;
    let second = write_sheet(
        &mut pdf,
        &outputs,
        Imposition {
            up: 2,
            marks: false,
        },
    )?;
    std::fs::write(path, pdf.finish(&[first, second]))?;
    Ok(())
}
