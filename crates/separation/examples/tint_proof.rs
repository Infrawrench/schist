//! Direct process/spot tints, text inheritance, and tint versus opacity.
use schist_layout::{
    authoring::{self, Paint},
    CharacterStyle, History, Ink, Insets, LayoutDocument, LayoutObject, Page, Rect, Story,
};
use schist_separation::{
    pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf},
    separate_page_without_graphics, OutputSettings,
};
fn patch(doc: &mut LayoutDocument, name: &str, x: f32, y: f32, tint: f32, opacity: f32) {
    authoring::rectangle(
        doc,
        &mut History::default(),
        0,
        Rect::new(x, y, 30.0, 25.0),
        Paint::filled(name),
    )
    .unwrap();
    let object = doc.objects.last_mut().unwrap();
    object.transparency = opacity;
    let LayoutObject::Shape { tints, .. } = &mut object.object else {
        unreachable!()
    };
    tints.fill = tint;
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    let mut doc = LayoutDocument::new(vec![Page::new("1", 200.0, 170.0)]);
    doc.inks.push(Ink::cmyk("Red", [0.0, 1.0, 1.0, 0.0]));
    let mut spot = Ink::process("Spot green", [0.0, 0.6, 0.0]);
    spot.spot = true;
    doc.inks.push(spot);
    for (i, tint) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
        let process_name = format!("Black {i}");
        let spot_name = format!("Spot green {i}");
        doc.inks.push(
            doc.ink("Black")
                .unwrap()
                .named_tint(&process_name, tint)
                .unwrap(),
        );
        doc.inks.push(
            doc.ink("Spot green")
                .unwrap()
                .named_tint(&spot_name, tint)
                .unwrap(),
        );
        patch(
            &mut doc,
            if i % 2 == 0 { "Black" } else { &process_name },
            10.0 + i as f32 * 36.0,
            10.0,
            tint,
            1.0,
        );
        patch(
            &mut doc,
            if i % 2 == 0 { &spot_name } else { "Spot green" },
            10.0 + i as f32 * 36.0,
            45.0,
            tint,
            1.0,
        );
    }
    for (i, (tint, opacity, overprint)) in [
        (0.0, 1.0, false),
        (0.25, 1.0, false),
        (1.0, 0.25, false),
        (0.25, 1.0, true),
    ]
    .into_iter()
    .enumerate()
    {
        let x = 10.0 + i as f32 * 45.0;
        patch(&mut doc, "Red", x, 80.0, 1.0, 1.0);
        patch(&mut doc, "Black", x, 80.0, tint, opacity);
        doc.objects.last_mut().unwrap().overprint = overprint;
    }
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(10.0, 115.0, 180.0, 40.0),
    )
    .unwrap();
    let style = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|s| s.name == "Body")
        .unwrap();
    style.point_size = Some(28.0);
    style.fill = Some(Ink::black().named_tint("Black quarter", 0.25).unwrap());
    doc.styles.add_character(CharacterStyle {
        name: "Full".into(),
        fill_tint: Some(1.0),
        fill: Some(Ink::black()),
        ..Default::default()
    });
    let mut story = Story::from_text("HHHHHH", "Body");
    story.apply_style(3, 6, "Full");
    doc.stories[frame.story.0 as usize] = story;
    let settings = OutputSettings::at(144.0);
    let separated = separate_page_without_graphics(&doc, 0, settings).unwrap();
    let page = PageOutput {
        separated: &separated,
        trim: (200.0, 170.0),
        bleed: Insets::ZERO,
        slug: Insets::ZERO,
        settings,
        imposition: Imposition::default(),
        marks: Marks::default(),
        overprint: true,
    };
    let mut pdf = Pdf::new();
    let id = write_sheet(&mut pdf, &[page], Imposition::default())?;
    std::fs::write(output, pdf.finish(&[id]))?;
    Ok(())
}
