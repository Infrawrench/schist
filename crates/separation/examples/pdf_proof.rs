//! Small deterministic plate proof for independent PDF renderer checks.
use schist_layout::{authoring, blank_a4, History, Ink, Rect};
use schist_separation::{
    separate_page, GraphicPlacement, GraphicSource, OutputSettings, PlacedGraphic,
};
struct ProofPixels;
impl GraphicSource for ProofPixels {
    fn sample(&self, _: &schist_layout::Link, p: &GraphicPlacement) -> Option<PlacedGraphic> {
        let mut graphic = PlacedGraphic::solid(p.dest, [0.0; 4]);
        let frame = Rect::new(0.0, 0.0, p.dest.width() as f32, p.dest.height() as f32);
        let mapping = schist_layout::graphics::ImageMapping::new(frame, frame, p.image_transform)?;
        for y in 0..p.dest.height() {
            for x in 0..p.dest.width() {
                let Some(at) =
                    mapping.source_at(schist_layout::Point::new(x as f32 + 0.5, y as f32 + 0.5))
                else {
                    graphic.coverage[(y * p.dest.width() + x) as usize] = 0;
                    continue;
                };
                let index = (at.x >= frame.width / 2.0) as usize
                    + 2 * (at.y >= frame.height / 2.0) as usize;
                graphic.cmyk[(y * p.dest.width() + x) as usize] = [
                    [0.0, 1.0, 1.0, 0.0],
                    [1.0, 1.0, 0.0, 0.0],
                    [1.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ][index];
            }
        }
        Some(graphic)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).expect("output PDF path");
    let mut doc = blank_a4();
    doc.pages[0].width = 180.0;
    doc.pages[0].height = 120.0;
    doc.pages[0].bleed = (6.0).into();
    let colors = [
        Ink::process("Red", [1.0, 0.0, 0.0]),
        Ink::process("Blue", [0.0, 0.0, 1.0]),
        Ink::black(),
        Ink::spot("Spot Grün / 色", [70.0, -45.0, 35.0]),
    ];
    for (i, ink) in colors.into_iter().enumerate() {
        doc.inks.push(ink.clone());
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(10.0 + i as f32 * 40.0, 10.0, 30.0, 40.0),
            authoring::Paint::filled(ink.name),
        )
        .unwrap();
    }
    let text = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(8.0, 72.0, 76.0, 28.0),
    )
    .unwrap();
    doc.stories[text.story.0 as usize] = schist_layout::Story::from_text("Affine", "Body");
    doc.objects
        .iter_mut()
        .find(|o| o.id == text.object)
        .unwrap()
        .transform = schist_layout::affine::Affine::rotate(-0.26);
    let mut link = schist_layout::Link::new("proof-pixels");
    link.present = true;
    let graphic = authoring::graphic_frame_with_link(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(110.0, 68.0, 26.0, 32.0),
        link,
        false,
    )
    .unwrap();
    doc.objects
        .iter_mut()
        .find(|o| o.id == graphic)
        .unwrap()
        .transform = schist_layout::affine::Affine::skew(0.5, 0.0);
    let mut link = schist_layout::Link::new("inner-proof");
    link.present = true;
    let graphic = authoring::graphic_frame_with_link(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(65.0, 77.0, 28.0, 28.0),
        link,
        false,
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == graphic).unwrap();
    let schist_layout::LayoutObject::GraphicFrame {
        image_transform, ..
    } = &mut object.object
    else {
        unreachable!()
    };
    *image_transform =
        schist_layout::affine::Affine::rotate(std::f32::consts::FRAC_PI_4).around(0.5, 0.5);
    let mut link = schist_layout::Link::new("curved-clip-proof");
    link.present = true;
    let graphic = authoring::graphic_frame_with_link(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(155.0, 65.0, 22.0, 42.0),
        link,
        false,
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == graphic).unwrap();
    let schist_layout::LayoutObject::GraphicFrame { clip_path, .. } = &mut object.object else {
        unreachable!()
    };
    let mut clip = schist_layout::ShapePath::ellipse(1.0, 1.0);
    let mut hole = schist_layout::ShapePath::ellipse(0.4, 0.4);
    hole.map_points(|p| p + schist_layout::Point::new(0.3, 0.3));
    hole.subpaths[0].points.reverse();
    hole.subpaths[0].handles.reverse();
    for h in &mut hole.subpaths[0].handles {
        std::mem::swap(&mut h.incoming, &mut h.outgoing);
    }
    clip.subpaths.extend(hole.subpaths);
    *clip_path = Some(clip);
    let settings = OutputSettings::at(144.0);
    let page = separate_page(&doc, 0, settings, &ProofPixels).unwrap();
    let bytes =
        schist_separation::pdf::write_document(&[page], &[(180.0, 120.0)], &[6.0], settings)?;
    std::fs::write(path, bytes)?;
    Ok(())
}
