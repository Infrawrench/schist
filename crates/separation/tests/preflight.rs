use schist_layout::{authoring, blank_a4, threading, History, Rect};
use schist_separation::{separate_page, NoGraphics, OutputSettings, Severity};

#[test]
fn effective_resolution_follows_crop_and_scale_not_output_dpi() {
    struct Pixels;
    impl schist_separation::GraphicSource for Pixels {
        fn sample(
            &self,
            _: &schist_layout::Link,
            p: &schist_separation::GraphicPlacement,
        ) -> Option<schist_separation::PlacedGraphic> {
            Some(schist_separation::PlacedGraphic::solid(p.dest, [0.0; 4]))
        }
    }
    for scale in [0.5, 1.0, 2.0] {
        for crop in [0.25, 1.0] {
            let mut doc = blank_a4();
            let mut history = History::default();
            let mut link = schist_layout::Link::new("pixels");
            link.present = true;
            link.info = Some(schist_layout::GraphicInfo {
                width: 100,
                height: 100,
                dpi: 72.0,
            });
            let id = authoring::graphic_frame_with_link(
                &mut doc,
                &mut history,
                0,
                Rect::new(10.0, 10.0, 72.0, 72.0),
                link,
                false,
            )
            .unwrap();
            let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
            let schist_layout::LayoutObject::GraphicFrame {
                fit,
                crop: rect,
                scale: factor,
                ..
            } = &mut object.object
            else {
                panic!()
            };
            *fit = schist_layout::GraphicFit::Stretch;
            *rect = Some(Rect::new(0.0, 0.0, crop, crop));
            *factor = scale;
            object.name = "Artwork".into();
            let expected = 100.0 * crop / scale;
            for dpi in [36.0, 144.0] {
                let result = separate_page(&doc, 0, OutputSettings::at(dpi), &Pixels).unwrap();
                let message = schist_i18n::tf!(
                    "design.preflight_low_resolution",
                    name = "Artwork",
                    dpi = expected.round()
                );
                assert_eq!(
                    result
                        .report
                        .findings
                        .iter()
                        .any(|f| f.severity == Severity::Warning && f.message == message),
                    expected < 150.0
                );
            }
        }
    }
}

#[test]
fn reserved_process_names_cannot_silently_claim_a_spot_plate() {
    for name in ["Cyan", "Magenta", "Yellow", "Black", "All", "None"] {
        let mut doc = blank_a4();
        doc.pages[0] = schist_layout::Page::new("1", 20.0, 20.0);
        doc.inks
            .push(schist_layout::Ink::spot(name, [50.0, 20.0, 10.0]));
        let separated = separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
        assert!(!separated.report.is_printable());
        let result = schist_separation::pdf::write_document(
            &[separated],
            &[(20.0, 20.0)],
            &[0.0],
            OutputSettings::at(72.0),
        );
        assert!(result.is_err());
    }
}

#[test]
fn overset_is_reported_only_where_the_thread_actually_loses_text() {
    for count in 2..6 {
        let mut doc = blank_a4();
        let mut history = History::default();
        let frames: Vec<_> = (0..count)
            .map(|i| {
                authoring::text_frame(
                    &mut doc,
                    &mut history,
                    0,
                    Rect::new(10.0, i as f32 * 30.0, 180.0, 20.0),
                )
                .unwrap()
            })
            .collect();
        authoring::set_text(
            &mut doc,
            &mut history,
            frames[0].story,
            "Text that continues beyond the frame. ".repeat(20),
        );
        for i in 1..count {
            assert!(threading::link(
                &mut doc,
                &mut history,
                frames[i - 1].object,
                frames[i].object
            ));
        }
        for (i, frame) in frames.iter().enumerate() {
            doc.objects
                .iter_mut()
                .find(|o| o.id == frame.object)
                .unwrap()
                .name = format!("frame{i}");
        }
        let last = format!("frame{}", count - 1);
        for dpi in [36.0, 72.0] {
            let result = separate_page(&doc, 0, OutputSettings::at(dpi), &NoGraphics).unwrap();
            let findings: Vec<_> = result
                .report
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .collect();
            assert_eq!(findings.len(), 1);
            assert!(findings[0].message.contains(&last));
        }
        authoring::set_text(&mut doc, &mut history, frames[0].story, "Fits");
        assert!(
            separate_page(&doc, 0, OutputSettings::at(36.0), &NoGraphics)
                .unwrap()
                .report
                .is_printable()
        );
    }
}

#[test]
fn font_checks_follow_visible_text_and_do_not_flag_unused_styles() {
    let mut doc = blank_a4();
    let mut history = History::default();
    let frame = authoring::text_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(10.0, 10.0, 400.0, 80.0),
    )
    .unwrap();
    authoring::set_text(&mut doc, &mut history, frame.story, "Visible text");
    let name = "Schist nonexistent family 894721";
    doc.styles.add_character(schist_layout::CharacterStyle {
        name: "missing".into(),
        family: Some(name.into()),
        ..Default::default()
    });
    assert!(
        separate_page(&doc, 0, OutputSettings::at(36.0), &NoGraphics)
            .unwrap()
            .report
            .is_printable()
    );
    doc.stories[frame.story.0 as usize].apply_style(0, 7, "missing");
    let page = separate_page(&doc, 0, OutputSettings::at(36.0), &NoGraphics).unwrap();
    assert_eq!(
        page.report
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error && f.message.contains(name))
            .count(),
        1
    );
}
