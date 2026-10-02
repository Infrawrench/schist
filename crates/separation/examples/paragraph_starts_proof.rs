#[path = "support/paragraph_starts.rs"]
mod proof;
use schist_separation::{separate_page_without_graphics, OutputSettings};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    use schist_separation::pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf};
    let settings = OutputSettings::at(144.0);
    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    for columns in [1, 2] {
        for reverse in [false, true] {
            let actual = proof::document(false, columns, reverse);
            let expected = proof::document(true, columns, reverse);
            for page in 0..3 {
                for doc in [&actual, &expected] {
                    let separated = separate_page_without_graphics(doc, page, settings)
                        .ok_or("separation failed")?;
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
            }
        }
    }
    std::fs::write(output, pdf.finish(&pages))?;
    Ok(())
}
