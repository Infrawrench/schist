use schist_layout::{
    authoring, authoring::ShapeKind, blank_a4, text_shape, History, Ink, LayoutDocument,
    LayoutObject, ParagraphStyle, Rect, Story,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings};

const TEXT: &str = "Type set inside a circle prints inside the circle, and none of its ink \
spills past the outline or into the frame's inset, whatever the output resolution.";

/// A 280pt circle at (150, 150), turned into a text frame.
fn document(inset: f32) -> LayoutDocument {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Shaped".into(),
        point_size: Some(13.0),
        fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
        ..Default::default()
    });
    let mut history = History::default();
    let id = authoring::shape(
        &mut doc,
        &mut history,
        0,
        Rect::new(150.0, 150.0, 280.0, 280.0),
        ShapeKind::Ellipse,
        authoring::Paint::none(),
    )
    .unwrap();
    let frame = text_shape::attach(&mut doc, &mut history, id).unwrap();
    let mut story = Story::new();
    for _ in 0..8 {
        story.push_paragraph(TEXT, "Shaped");
    }
    doc.stories[frame.story.0 as usize] = story;
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    let LayoutObject::TextFrame { insets, .. } = &mut object.object else {
        panic!()
    };
    *insets = schist_layout::Insets::uniform(inset);
    doc
}

#[test]
fn shaped_frame_ink_stays_inside_the_outline_and_inset() {
    for dpi in [72.0, 144.0] {
        for inset in [0.0, 12.0] {
            let doc = document(inset);
            let result = separate_page(&doc, 0, OutputSettings::at(dpi), &NoGraphics).unwrap();
            let plate = result.separation.plate(result.plan.process[0]).unwrap();
            let scale = dpi / 72.0;
            let (center, radius) = ((290.0, 290.0), 140.0 - inset);
            let (mut inked, mut outside) = (0, 0);
            for y in (110.0 * scale) as i32..(470.0 * scale) as i32 {
                for x in (110.0 * scale) as i32..(470.0 * scale) as i32 {
                    if plate.at(x, y) <= 0.02 {
                        continue;
                    }
                    inked += 1;
                    let p = ((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale);
                    // One point of slack covers antialiasing and flattening.
                    if ((p.0 - center.0).powi(2) + (p.1 - center.1).powi(2)).sqrt() > radius + 1.0 {
                        outside += 1;
                    }
                }
            }
            assert!(inked > 1000, "dpi {dpi} inset {inset}: {inked}");
            assert_eq!(outside, 0, "dpi {dpi} inset {inset}");
        }
    }
}
