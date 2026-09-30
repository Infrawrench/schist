use schist_layout::{authoring, History, Ink, Insets, LayoutDocument, Page, Rect};
use schist_separation::{
    drawn_plates, page_content, separate_page_without_graphics, Imposition, Marks, OutputSettings,
    PageOutput, Pdf,
};

#[test]
fn every_crop_mark_stays_outside_its_bleed_edge_at_the_requested_length() {
    for bleed in [
        Insets::ZERO,
        Insets::new(4.0, 6.0, 8.0, 10.0),
        Insets::new(9.0, 0.0, 1.0, 3.0),
    ] {
        for length in [1.0, 3.0, 7.0] {
            let marks = Marks {
                crop_length: length,
                registration: false,
                ..Marks::default()
            };
            let content = schist_separation::pdf::marks_for((60.0, 40.0), bleed, &marks);
            let lines: Vec<_> = content
                .lines()
                .filter(|line| line.ends_with(" l S"))
                .collect();
            assert_eq!(lines.len(), 8);
            for line in lines {
                let n: Vec<f32> = line
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                if n[0] == n[2] {
                    assert_eq!((n[3] - n[1]).abs(), length);
                    assert!(
                        (n[1] < -bleed.bottom && n[3] < -bleed.bottom)
                            || (n[1] > 40.0 + bleed.top && n[3] > 40.0 + bleed.top)
                    );
                } else {
                    assert_eq!(n[1], n[3]);
                    assert_eq!((n[2] - n[0]).abs(), length);
                    assert!(
                        (n[0] < -bleed.left && n[2] < -bleed.left)
                            || (n[0] > 60.0 + bleed.right && n[2] > 60.0 + bleed.right)
                    );
                }
            }
        }
    }
}

#[test]
fn asymmetric_plate_origins_and_boxes_agree_at_every_resolution_with_and_without_marks() {
    for dpi in [72.0, 144.0, 300.0] {
        for bleed in [
            Insets::ZERO,
            Insets::new(4.0, 6.0, 8.0, 10.0),
            Insets::new(9.0, 1.0, 3.0, 5.0),
        ] {
            for slug in [Insets::ZERO, Insets::new(3.0, 12.0, 1.0, 14.0)] {
                let mut doc = LayoutDocument::new(vec![Page::new("page", 60.0, 40.0)]);
                doc.pages[0].bleed = bleed;
                doc.pages[0].slug = slug;
                doc.inks.push(Ink::black());
                authoring::rectangle(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(0.0, 0.0, 60.0, 40.0),
                    authoring::Paint::filled("Black"),
                )
                .unwrap();
                let settings = OutputSettings::at(dpi);
                let separated = separate_page_without_graphics(&doc, 0, settings).unwrap();
                let rect = separated.separation.rect();
                assert_eq!(rect.left, -settings.to_pixels(bleed.left));
                assert_eq!(rect.top, -settings.to_pixels(bleed.top));
                for marks in [false, true] {
                    let page = PageOutput {
                        separated: &separated,
                        trim: (60.0, 40.0),
                        bleed,
                        slug,
                        settings,
                        imposition: Imposition { up: 1, marks },
                        marks: Marks::default(),
                        overprint: true,
                    };
                    let content = page_content(&page, &drawn_plates(&separated));
                    let line = content
                        .lines()
                        .find(|l| l.contains("cm /InkImage"))
                        .unwrap();
                    let numbers: Vec<f32> = line
                        .split_whitespace()
                        .filter_map(|v| v.parse().ok())
                        .collect();
                    let padding = if marks { 9.0 } else { 0.0 };
                    let left = (bleed.left + padding).max(slug.left);
                    let bottom = (bleed.bottom + padding).max(slug.bottom);
                    assert!(
                        (numbers[4] - (rect.left as f32 / settings.scale() + left)).abs() < 0.001
                    );
                    assert!(
                        (numbers[5] - (40.0 + bottom - rect.bottom as f32 / settings.scale()))
                            .abs()
                            < 0.001
                    );
                    let mut pdf = Pdf::new();
                    let id = schist_separation::pdf::write_page(&mut pdf, &page).unwrap();
                    let bytes = pdf.finish(&[id]);
                    let text = String::from_utf8_lossy(&bytes);
                    assert!(text.contains(&format!(
                        "/TrimBox [{left:.2} {bottom:.2} {:.2} {:.2}]",
                        left + 60.0,
                        bottom + 40.0
                    )));
                }
            }
        }
    }
}

#[test]
fn invalid_bleed_or_slug_edges_are_rejected_before_writing_a_pdf_page() {
    let doc = LayoutDocument::default();
    let settings = OutputSettings::at(72.0);
    let separated = separate_page_without_graphics(&doc, 0, settings).unwrap();
    for edge in 0..8 {
        for value in [-1.0, f32::NAN, f32::INFINITY] {
            let mut page = PageOutput {
                separated: &separated,
                trim: (60.0, 40.0),
                bleed: Insets::ZERO,
                slug: Insets::ZERO,
                settings,
                imposition: Imposition::default(),
                marks: Marks::default(),
                overprint: true,
            };
            let offsets = if edge < 4 {
                &mut page.bleed
            } else {
                &mut page.slug
            };
            *[
                &mut offsets.top,
                &mut offsets.right,
                &mut offsets.bottom,
                &mut offsets.left,
            ][edge % 4] = value;
            assert!(schist_separation::pdf::write_page(&mut Pdf::new(), &page).is_err());
        }
    }
}
