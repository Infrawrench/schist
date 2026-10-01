//! Paired inherited frame paints versus independent fill/content/stroke objects.
use schist_layout::{authoring, object_styles, *};
use schist_separation::{GraphicPlacement, GraphicSource, PlacedGraphic};

struct Image;
impl GraphicSource for Image {
    fn sample(&self, _: &Link, placement: &GraphicPlacement) -> Option<PlacedGraphic> {
        Some(PlacedGraphic::solid(placement.dest, [0.0, 0.65, 0.0, 0.0]))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).expect("output PDF path");
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 8]);
    let fill = Ink::cmyk("Frame cyan", [0.8, 0.0, 0.0, 0.0]);
    let stroke = Ink::spot("Frame border", [45.0, 55.0, 25.0]);
    doc.styles.objects.push(ObjectStyle {
        name: "Frame".into(),
        enable_fill: Some(true),
        enable_stroke: Some(true),
        paint: ObjectPaint {
            fill: Some(Paint::Ink(fill.clone())),
            stroke: Some(Paint::Ink(stroke.clone())),
            stroke_width: Some(10.0),
            fill_tint: Some(0.6),
            stroke_tint: Some(0.75),
            overprint_stroke: Some(true),
            ..Default::default()
        },
        ..Default::default()
    });
    doc.styles.objects.push(ObjectStyle {
        name: "Child".into(),
        based_on: Some("Frame".into()),
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Proof".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(20.0),
        leading: Some(styles::Leading::Points(30.0)),
        ..Default::default()
    });
    for page in 0..8 {
        let graphic = page / 4 == 1;
        let curved = (page / 2) % 2 == 1;
        let rect = Rect::new(35.0, 35.0, 105.0, 105.0);
        let mut history = History::default();
        let id = if graphic {
            authoring::graphic_frame(&mut doc, &mut history, page, rect, "image.png", false)
                .unwrap()
        } else {
            let frame = authoring::text_frame(&mut doc, &mut history, page, rect).unwrap();
            doc.stories[frame.story.0 as usize] = Story::from_text("Frame\npaint", "Proof");
            frame.object
        };
        object_styles::apply_style(&mut doc, &mut history, &[id], Some("Child"));
        let object = doc.objects.last_mut().unwrap();
        object.transform = schist_core::Affine {
            a: 0.9,
            b: 0.15,
            c: 0.2,
            d: 0.9,
            tx: 0.0,
            ty: 0.0,
        };
        object.transparency = 0.8;
        if curved {
            if let LayoutObject::GraphicFrame { clip_path, .. } = &mut object.object {
                *clip_path = Some(ShapePath::ellipse(1.0, 1.0));
            } else {
                object.appearance.outline = Some(ShapePath::ellipse(1.0, 1.0));
            }
        }
        if page % 2 == 1 {
            let mut content = doc.objects.pop().unwrap();
            content.appearance = Default::default();
            let path = if curved {
                {
                    // Use the same authored normalized contour. Building a new
                    // ellipse at another size can round one handle differently.
                    let mut path = ShapePath::ellipse(1.0, 1.0);
                    path.map_points(|p| Point::new(p.x * rect.width, p.y * rect.height));
                    path
                }
            } else {
                authoring::path_for(authoring::ShapeKind::Rectangle, rect.width, rect.height)
            };
            let mut background = content.clone();
            background.id = ObjectId::next();
            background.object = LayoutObject::Shape {
                path: path.clone(),
                fill: Some(fill.clone()),
                stroke: None,
                stroke_width: 0.0,
                fill_overprint: false,
                stroke_overprint: false,
                tints: PaintTints {
                    fill: 0.6,
                    stroke: 1.0,
                },
            };
            let mut border = content.clone();
            border.id = ObjectId::next();
            border.object = LayoutObject::Shape {
                path,
                fill: None,
                stroke: Some(stroke.clone()),
                stroke_width: 10.0,
                fill_overprint: false,
                stroke_overprint: true,
                tints: PaintTints {
                    fill: 1.0,
                    stroke: 0.75,
                },
            };
            doc.objects.extend([background, content, border]);
        }
    }
    use schist_separation::pdf::{write_sheet, Imposition, Marks, PageOutput, Pdf};
    let settings = schist_separation::OutputSettings::at(144.0);
    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    for page in 0..8 {
        let separated = schist_separation::separate_page(&doc, page, settings, &Image)
            .ok_or("proof separation failed")?;
        pages.push(write_sheet(
            &mut pdf,
            &[PageOutput {
                separated: &separated,
                trim: (200.0, 200.0),
                bleed: Insets::ZERO,
                slug: Insets::ZERO,
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
