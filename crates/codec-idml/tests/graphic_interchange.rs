use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    authoring, blank_a4, GraphicFit, GraphicInfo, History, LayoutObject, Link, Rect,
};

#[test]
fn all_fitting_modes_keep_their_link_crop_and_embedded_bytes() {
    for embedded in [false, true] {
        for fit in [
            GraphicFit::Fill,
            GraphicFit::Contain,
            GraphicFit::Original,
            GraphicFit::Stretch,
        ] {
            for crop in [
                None,
                Some(Rect::new(0.1, 0.2, 0.6, 0.7)),
                Some(Rect::new(-0.5, -0.25, 2.0, 1.5)),
            ] {
                let mut doc = blank_a4();
                let mut source = Link::new("/art/café 100% #1.psd");
                source.info = Some(GraphicInfo {
                    width: 1200,
                    height: 800,
                    dpi: 300.0,
                });
                if embedded {
                    doc.assets.insert(
                        source.path.clone(),
                        std::sync::Arc::new((0..=255).collect()),
                    );
                }
                let id = authoring::graphic_frame_with_link(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(15.0, 20.0, 140.0, 160.0),
                    source,
                    embedded,
                )
                .unwrap();
                let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
                let LayoutObject::GraphicFrame {
                    fit: f,
                    crop: c,
                    scale,
                    ..
                } = &mut object.object
                else {
                    unreachable!()
                };
                *f = fit;
                *c = crop;
                *scale = 1.25;
                let bytes = export::write(&doc).bytes;
                let again = import::read(&bytes).unwrap().document;
                assert_eq!(
                    again.objects[0].object, doc.objects[0].object,
                    "{fit:?}, {crop:?}"
                );
                assert_eq!(again.objects[0].bounds, doc.objects[0].bounds);
                assert_eq!(again.assets, doc.assets);

                // Verify the public structure, independently of our reader.
                let package = container::open(&bytes).unwrap();
                let spread = package
                    .names()
                    .into_iter()
                    .find(|name| name.starts_with("Spreads/"))
                    .unwrap();
                let root = xml::parse(package.text(spread).unwrap()).unwrap();
                let frame = root.find("Rectangle").unwrap();
                assert_eq!(frame.attr("ContentType"), Some("GraphicType"));
                let image = frame.child("Image").unwrap();
                assert!(image.child("Link").is_some());
                assert!(image
                    .child("Properties")
                    .unwrap()
                    .child("GraphicBounds")
                    .is_some());
                assert_eq!(image.find("Contents").is_some(), embedded);
            }
        }
    }
}

#[test]
fn published_image_fixture_keeps_real_frames_and_embedded_sources() {
    let before = import::read(include_bytes!("../../../fixtures/idml/images.idml")).unwrap();
    let frames: Vec<_> = before
        .document
        .objects
        .iter()
        .filter(|o| matches!(o.object, LayoutObject::GraphicFrame { .. }))
        .collect();
    assert!(
        !frames.is_empty(),
        "image frames must not be imported as unfilled shapes"
    );
    assert!(!before.document.assets.is_empty());
    for object in frames {
        let LayoutObject::GraphicFrame { link, embedded, .. } = &object.object else {
            unreachable!()
        };
        if *embedded {
            let bytes = before.document.assets.get(&link.path).unwrap();
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }
    let after = import::read(&export::write(&before.document).bytes)
        .unwrap()
        .document;
    assert_eq!(after.assets, before.document.assets);
    for (before, after) in before.document.objects.iter().zip(&after.objects) {
        if matches!(before.object, LayoutObject::GraphicFrame { .. }) {
            assert_eq!(before.object, after.object);
        }
    }
}
