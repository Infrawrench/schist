//! Five colored pages make reading order and incomplete sheets observable.
use schist_layout::{authoring, History, Ink, LayoutDocument, Page, Rect};
use schist_separation::{
    pdf::{write_document_imposed, Imposition},
    separate_page_without_graphics, OutputSettings,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).expect("output PDF path");
    let up = std::env::args().nth(2).expect("pages per sheet").parse()?;
    let settings = OutputSettings::at(72.0);
    let mut pages = Vec::new();
    for rgb in [
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    ] {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 60.0, 40.0)]);
        doc.inks.push(Ink::process("Patch", rgb));
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(0.0, 0.0, 60.0, 40.0),
            authoring::Paint::filled("Patch"),
        )
        .unwrap();
        pages.push(separate_page_without_graphics(&doc, 0, settings).unwrap());
    }
    let bytes = write_document_imposed(
        &pages,
        &[(60.0, 40.0); 5],
        &[0.0; 5],
        settings,
        Imposition { up, marks: false },
    )?;
    std::fs::write(path, bytes)?;
    Ok(())
}
