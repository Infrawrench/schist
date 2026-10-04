use schist_layout::text_wrap::{ContourType, TextWrap, WrapMode};
use schist_layout::{
    authoring, authoring::ShapeKind, blank_a4, History, Ink, Insets, LayoutDocument,
    ParagraphStyle, Rect, Story, WritingMode,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings, Severity};

const TEXT: &str = "Ink never lands where an object asks for room; the words flow around it \
on both sides and continue below without losing a single character of the story.";

fn document(style: ParagraphStyle) -> LayoutDocument {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Wrapped".into(),
        point_size: Some(14.0),
        fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
        ..style
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 40.0, 420.0, 600.0),
    )
    .unwrap();
    let mut story = Story::new();
    for _ in 0..6 {
        story.push_paragraph(TEXT, "Wrapped");
    }
    doc.stories[frame.story.0 as usize] = story;
    doc
}

fn obstacle(doc: &mut LayoutDocument, kind: ShapeKind, rect: Rect, wrap: TextWrap) {
    let id = authoring::shape(
        doc,
        &mut History::default(),
        0,
        rect,
        kind,
        authoring::Paint::none(),
    )
    .unwrap();
    doc.objects
        .iter_mut()
        .find(|o| o.id == id)
        .unwrap()
        .appearance
        .text_wrap = Some(wrap);
}

fn cyan(doc: &LayoutDocument, dpi: f32) -> (Vec<f32>, i32, i32, f32) {
    let result = separate_page(doc, 0, OutputSettings::at(dpi), &NoGraphics).unwrap();
    let plate = result.separation.plate(result.plan.process[0]).unwrap();
    let scale = dpi / 72.0;
    let width = (doc.pages[0].width * scale).ceil() as i32;
    let height = (doc.pages[0].height * scale).ceil() as i32;
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            out.push(plate.at(x, y));
        }
    }
    (out, width, height, scale)
}

#[test]
fn no_text_ink_lands_inside_bounding_box_or_contour_wrap_zones() {
    for dpi in [72.0, 144.0] {
        for contour in [false, true] {
            let mut doc = document(ParagraphStyle::default());
            let rect = Rect::new(180.0, 160.0, 140.0, 140.0);
            let offset = 6.0;
            obstacle(
                &mut doc,
                if contour {
                    ShapeKind::Ellipse
                } else {
                    ShapeKind::Rectangle
                },
                rect,
                TextWrap {
                    mode: if contour {
                        WrapMode::Contour
                    } else {
                        WrapMode::BoundingBox
                    },
                    offsets: Insets::uniform(offset),
                    contour: contour.then_some(ContourType::SameAsClipping),
                    ..Default::default()
                },
            );
            let (plate, width, height, scale) = cyan(&doc, dpi);
            let center = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            let (mut left, mut right, mut inside) = (0, 0, 0);
            for y in 0..height {
                for x in 0..width {
                    if plate[(y * width + x) as usize] <= 0.02 {
                        continue;
                    }
                    let p = ((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale);
                    // One point of slack covers antialiasing and flattening.
                    let blocked = if contour {
                        ((p.0 - center.0).powi(2) + (p.1 - center.1).powi(2)).sqrt()
                            < rect.width / 2.0 + offset - 1.0
                    } else {
                        p.0 > rect.x - offset + 1.0
                            && p.0 < rect.right() + offset - 1.0
                            && p.1 > rect.y - offset + 1.0
                            && p.1 < rect.bottom() + offset - 1.0
                    };
                    if blocked {
                        inside += 1;
                    }
                    if p.1 > rect.y + 20.0 && p.1 < rect.bottom() - 20.0 {
                        if p.0 < rect.x {
                            left += 1;
                        } else if p.0 > rect.right() {
                            right += 1;
                        }
                    }
                }
            }
            assert_eq!(inside, 0, "dpi {dpi} contour {contour}");
            assert!(left > 0 && right > 0, "text uses both sides");
        }
    }
}

#[test]
fn preflight_reports_wrap_it_could_not_apply_exactly() {
    let rect = Rect::new(60.0, 60.0, 100.0, 100.0);
    let warnings = |doc: &LayoutDocument| {
        separate_page(doc, 0, OutputSettings::at(36.0), &NoGraphics)
            .unwrap()
            .report
            .findings
            .into_iter()
            .filter(|f| f.severity == Severity::Warning)
            .map(|f| f.message)
            .collect::<Vec<_>>()
    };
    let doc = document(ParagraphStyle::default());
    let name = doc.objects[0].name.clone();
    let ignored = schist_i18n::tf!("design.preflight_wrap_ignored", name = name);
    let approximated = schist_i18n::tf!("design.preflight_wrap_approximated", name = name);
    let clean = warnings(&doc);
    let mut wrapped = doc.clone();
    obstacle(
        &mut wrapped,
        ShapeKind::Rectangle,
        rect,
        TextWrap {
            mode: WrapMode::BoundingBox,
            ..Default::default()
        },
    );
    assert_eq!(warnings(&wrapped), clean);

    let mut vertical = document(ParagraphStyle {
        writing_mode: Some(WritingMode::VerticalRightToLeft),
        ..Default::default()
    });
    obstacle(
        &mut vertical,
        ShapeKind::Rectangle,
        rect,
        TextWrap {
            mode: WrapMode::BoundingBox,
            ..Default::default()
        },
    );
    assert!(warnings(&vertical).iter().any(|m| m == &ignored));

    let mut pixels = doc.clone();
    obstacle(
        &mut pixels,
        ShapeKind::Ellipse,
        rect,
        TextWrap {
            mode: WrapMode::Contour,
            contour: Some(ContourType::DetectEdges),
            ..Default::default()
        },
    );
    assert!(warnings(&pixels).iter().any(|m| m == &approximated));
}
