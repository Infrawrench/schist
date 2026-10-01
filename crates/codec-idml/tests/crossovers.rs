use schist_codec_idml::{export, import};
use schist_layout::{
    affine,
    authoring::{self, Paint},
    History, LayerId, LayoutDocument, Page, PageBinding, Point, Rect, Spread,
};

#[test]
fn native_spread_items_keep_stacking_and_global_geometry_across_page_ownership_changes() {
    for reverse in [false, true] {
        for count in 2..=4 {
            let mut doc = LayoutDocument::new(vec![Page::new("page", 100.0, 100.0); count]);
            doc.facing_pages = true;
            doc.page_binding = if reverse {
                PageBinding::RightToLeft
            } else {
                PageBinding::LeftToRight
            };
            let mut pages: Vec<_> = (0..count).collect();
            if reverse {
                pages.reverse();
            }
            doc.spreads = vec![Spread {
                pages,
                binding_location: Some(1),
                gutter: 0.0,
                origin: Point::ZERO,
            }];
            doc.layers.push(LayerId(1));
            for n in 0..count * 3 {
                let page = count - 1 - n % count;
                authoring::rectangle(
                    &mut doc,
                    &mut History::default(),
                    page,
                    Rect::new(-20.0 + n as f32, 10.0, 140.0, 30.0),
                    Paint::filled("Black"),
                )
                .unwrap();
                let object = doc.objects.last_mut().unwrap();
                object.name = format!("item {n}");
                object.transform = affine::Affine::skew(0.2, 0.0);
                object.transparency = 0.5;
                doc.object_layers.last_mut().unwrap().1 = LayerId((n % 2) as u32);
            }
            let expected: Vec<_> = doc
                .objects
                .iter()
                .map(|o| (o.name.clone(), doc.object_rect(o.id).unwrap()))
                .collect();
            let mut paint: Vec<_> = doc.objects.iter().collect();
            let order = doc.paint_order();
            paint.sort_by_key(|o| order[&o.id]);
            let paint: Vec<_> = paint.iter().map(|o| o.name.clone()).collect();
            for _ in 0..4 {
                doc = import::read(&export::write(&doc).bytes).unwrap().document;
                assert_eq!(
                    doc.objects.iter().map(|o| &o.name).collect::<Vec<_>>(),
                    expected.iter().map(|(name, _)| name).collect::<Vec<_>>()
                );
                for (object, (_, rect)) in doc.objects.iter().zip(&expected) {
                    let actual = doc.object_rect(object.id).unwrap();
                    for (a, b) in [actual.x, actual.y, actual.width, actual.height]
                        .into_iter()
                        .zip([rect.x, rect.y, rect.width, rect.height])
                    {
                        assert!((a - b).abs() < 0.005, "{actual:?} != {rect:?}");
                    }
                }
                let mut actual: Vec<_> = doc.objects.iter().collect();
                let order = doc.paint_order();
                actual.sort_by_key(|o| order[&o.id]);
                assert_eq!(
                    actual.iter().map(|o| o.name.clone()).collect::<Vec<_>>(),
                    paint
                );
            }
        }
    }
}
