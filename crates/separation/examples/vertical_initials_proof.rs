#[path = "support/vertical_initials.rs"]
mod proof;
use schist_layout::WritingMode;
use schist_separation::{separate_page_without_graphics, OutputSettings};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    proof::register_font();
    use schist_separation::pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf};
    let settings = OutputSettings::at(144.0);
    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    for spot in [false, true] {
        for axis in [
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ] {
            for latin in [false, true] {
                for reference in [false, true] {
                    let doc = proof::document(reference, spot, axis, latin);
                    let separated = separate_page_without_graphics(&doc, 0, settings)
                        .ok_or("separation failed")?;
                    pages.push(write_sheet(
                        &mut pdf,
                        &[PageOutput {
                            separated: &separated,
                            trim: (360.0, 260.0),
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
