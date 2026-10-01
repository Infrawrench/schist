use schist_layout::{affine::Affine, authoring, blank_a4, History, Link, Rect, Story};
use schist_separation::{
    separate_page, GraphicPlacement, GraphicSource, NoGraphics, OutputSettings, PlacedGraphic,
};

struct Native;
const INK: [f32; 4] = [0.17, 0.42, 0.63, 0.91];
impl GraphicSource for Native {
    fn sample(&self, _: &Link, p: &GraphicPlacement) -> Option<PlacedGraphic> {
        let mut graphic = PlacedGraphic::solid(p.dest, INK);
        for (index, alpha) in graphic.coverage.iter_mut().enumerate() {
            *alpha = [0, 64, 128, 255][index % 4];
        }
        Some(graphic)
    }
}

#[test]
fn graphic_affines_keep_native_ink_values_at_every_nontransparent_pixel() {
    for matrix in [
        Affine::scale(-1.0, 1.0),
        Affine::skew(0.6, -0.2),
        Affine::rotate(0.71),
        Affine::scale(1.6, 0.7),
    ] {
        for dpi in [72.0, 144.0, 300.0] {
            let mut doc = blank_a4();
            let mut history = History::default();
            let id = authoring::graphic_frame(
                &mut doc,
                &mut history,
                0,
                Rect::new(100.0, 100.0, 32.0, 24.0),
                "art",
                false,
            )
            .unwrap();
            doc.objects[0].transform = matrix;
            let object = doc.object(id).unwrap();
            let graphic = schist_separation::raster::graphic_coverage(
                object,
                OutputSettings::at(dpi),
                &doc.pages[0],
                &Native,
            )
            .unwrap();
            let mut covered = 0;
            for (color, alpha) in graphic.cmyk.iter().zip(&graphic.coverage) {
                if *alpha == 0 {
                    continue;
                }
                covered += 1;
                for (actual, expected) in color.iter().zip(INK) {
                    assert!((*actual - expected).abs() < 1e-6);
                }
            }
            assert!(covered > 0);
        }
    }
}

#[test]
fn text_plates_rotate_the_composed_glyphs_without_reflow_or_loss() {
    let mut doc = blank_a4();
    doc.pages[0] = schist_layout::Page::new("1", 120.0, 120.0);
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(70.0, 20.0, 40.0, 50.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("Hello é 世界", "Body");
    let settings = OutputSettings::at(72.0);
    let before = separate_page(&doc, 0, settings, &NoGraphics).unwrap();
    doc.objects[0].transform = Affine {
        a: 0.0,
        b: 1.0,
        c: -1.0,
        d: 0.0,
        tx: 0.0,
        ty: 0.0,
    };
    let matrix = doc.objects[0].content_transform();
    let after = separate_page(&doc, 0, settings, &NoGraphics).unwrap();
    let a = before.separation.plate(before.plan.process[3]).unwrap();
    let b = after.separation.plate(after.plan.process[3]).unwrap();
    let mut seen = 0;
    for y in a.rect.top..a.rect.bottom {
        for x in a.rect.left..a.rect.right {
            let value = a.at(x, y);
            if value <= 0.0 {
                continue;
            }
            let (dx, dy) = matrix.apply(x as f32 + 0.5, y as f32 + 0.5);
            assert!((value - b.at(dx.floor() as i32, dy.floor() as i32)).abs() < 0.0001);
            seen += 1;
        }
    }
    assert!(seen > 10);
    let sum = |data: &[f32]| data.iter().sum::<f32>();
    assert!((sum(&a.data) - sum(&b.data)).abs() < 0.01);
}

#[test]
fn collapsed_frames_are_preflight_errors_instead_of_silent_print_success() {
    let mut doc = blank_a4();
    authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(10.0, 10.0, 30.0, 30.0),
    )
    .unwrap();
    for matrix in [Affine::scale(0.0, 1.0), Affine::translate(f32::NAN, 0.0)] {
        doc.objects[0].transform = matrix;
        let result = separate_page(&doc, 0, OutputSettings::at(36.0), &NoGraphics).unwrap();
        assert!(!result.report.is_printable());
        assert!(result.report.findings.iter().any(|f| f.message
            == schist_i18n::tf!("design.preflight_transform", name = doc.objects[0].name)));
    }
}

#[test]
fn inner_image_transform_reaches_sampling_and_effective_resolution_checks() {
    struct Source(Affine);
    impl GraphicSource for Source {
        fn sample(&self, _: &Link, p: &GraphicPlacement) -> Option<PlacedGraphic> {
            assert_eq!(p.image_transform, self.0);
            Some(PlacedGraphic::solid(p.dest, [0.0; 4]))
        }
    }
    for (matrix, expected) in [
        (Affine::rotate(0.7).around(0.5, 0.5), 100.0_f32),
        (Affine::scale(-1.0, 1.0).around(0.5, 0.5), 100.0),
        (Affine::scale(2.0, 0.5), 50.0),
        (
            Affine {
                c: 1.0,
                ..Affine::IDENTITY
            },
            61.8034,
        ),
    ] {
        let mut doc = blank_a4();
        doc.pages[0] = schist_layout::Page::new("1", 100.0, 100.0);
        let mut link = Link::new("art");
        link.present = true;
        link.info = Some(schist_layout::GraphicInfo {
            width: 100,
            height: 100,
            dpi: 72.0,
        });
        authoring::graphic_frame_with_link(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(10.0, 10.0, 72.0, 72.0),
            link,
            false,
        )
        .unwrap();
        let schist_layout::LayoutObject::GraphicFrame {
            image_transform, ..
        } = &mut doc.objects[0].object
        else {
            panic!()
        };
        *image_transform = matrix;
        for dpi in [36.0, 144.0] {
            let result = separate_page(&doc, 0, OutputSettings::at(dpi), &Source(matrix)).unwrap();
            assert!(result.report.findings.iter().any(|f| f.message
                == schist_i18n::tf!(
                    "design.preflight_low_resolution",
                    name = doc.objects[0].name,
                    dpi = expected.round()
                )));
        }
    }
}

#[test]
fn invalid_inner_image_transforms_are_preflight_errors() {
    for matrix in [Affine::scale(0.0, 1.0), Affine::translate(f32::NAN, 0.0)] {
        let mut doc = blank_a4();
        authoring::graphic_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(10.0, 10.0, 30.0, 20.0),
            "art",
            false,
        )
        .unwrap();
        let schist_layout::LayoutObject::GraphicFrame {
            image_transform, ..
        } = &mut doc.objects[0].object
        else {
            panic!()
        };
        *image_transform = matrix;
        let result = separate_page(&doc, 0, OutputSettings::at(36.0), &NoGraphics).unwrap();
        assert!(!result.report.is_printable());
        assert!(result.report.findings.iter().any(|f| f.message
            == schist_i18n::tf!("design.preflight_transform", name = doc.objects[0].name)));
    }
}

#[test]
fn curved_clipping_multiplies_existing_alpha_without_reseparating_native_ink() {
    use schist_layout::{affine, LayoutObject, Point, ShapePath};
    for matrix in [
        Affine::IDENTITY,
        Affine::rotate(0.71),
        Affine::skew(0.5, -0.3),
        Affine::scale(-1.2, 0.8),
    ] {
        for dpi in [72.0, 144.0, 300.0] {
            let mut doc = blank_a4();
            authoring::graphic_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(100.0, 100.0, 32.0, 24.0),
                "art",
                false,
            )
            .unwrap();
            let object = &mut doc.objects[0];
            object.transform = matrix;
            let settings = OutputSettings::at(dpi);
            let before = schist_separation::raster::graphic_coverage(
                object,
                settings,
                &doc.pages[0],
                &Native,
            )
            .unwrap();
            let LayoutObject::GraphicFrame { clip_path, .. } = &mut object.object else {
                panic!()
            };
            *clip_path = Some(ShapePath::ellipse(1.0, 1.0));
            let after = schist_separation::raster::graphic_coverage(
                object,
                settings,
                &doc.pages[0],
                &Native,
            )
            .unwrap();
            assert_eq!(after.rect, before.rect);
            assert_eq!(after.cmyk, before.cmyk);
            let inverse = affine::inverse(affine::in_view(
                object.content_transform(),
                settings.scale(),
                Point::ZERO,
            ))
            .unwrap();
            let rect = settings.to_pixels_rect(object.bounds);
            let mut removed = 0;
            let mut retained = 0;
            for y in after.rect.top..after.rect.bottom {
                for x in after.rect.left..after.rect.right {
                    let i =
                        ((y - after.rect.top) * after.rect.width() + x - after.rect.left) as usize;
                    assert!(after.coverage[i] <= before.coverage[i]);
                    let p = affine::point(inverse, Point::new(x as f32 + 0.5, y as f32 + 0.5));
                    let radius = ((p.x - rect.left as f32) / rect.width() as f32 * 2.0 - 1.0)
                        .hypot((p.y - rect.top as f32) / rect.height() as f32 * 2.0 - 1.0);
                    if radius > 1.2 {
                        assert_eq!(after.coverage[i], 0);
                    }
                    if radius < 0.8 {
                        assert_eq!(after.coverage[i], before.coverage[i]);
                    }
                    removed += usize::from(before.coverage[i] > 0 && after.coverage[i] == 0);
                    retained += usize::from(after.coverage[i] > 0);
                }
            }
            assert!(removed > 10 && retained > 100);
        }
    }
}
