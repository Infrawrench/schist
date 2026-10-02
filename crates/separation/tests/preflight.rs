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

#[test]
fn preflight_detects_a_missing_named_face_even_when_its_family_is_present() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(10.0, 10.0, 400.0, 80.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = schist_layout::Story::from_text("HéH", "Body");
    let name = "Schist missing style 57321";
    doc.styles.add_character(schist_layout::CharacterStyle {
        name: "Face".into(),
        family: Some("IBM Plex Sans".into()),
        font_style: Some(name.into()),
        ..Default::default()
    });
    let check = |doc: &schist_layout::LayoutDocument| {
        separate_page(doc, 0, OutputSettings::at(36.0), &NoGraphics)
            .unwrap()
            .report
    };
    assert!(
        check(&doc).is_printable(),
        "unused styles do not fail preflight"
    );
    doc.stories[frame.story.0 as usize].apply_style(1, 3, "Face");
    for _ in 0..2 {
        let report = check(&doc);
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error
                    && f.message.contains(name)
                    && f.message.contains("IBM Plex Sans"))
                .count(),
            1
        );
    }
    doc.styles
        .characters
        .iter_mut()
        .find(|s| s.name == "Face")
        .unwrap()
        .font_style = Some("Regular".into());
    assert!(check(&doc).is_printable());
}

#[test]
fn visible_generated_markers_contribute_their_own_font_to_preflight() {
    use schist_layout::{
        lists::{ListKind, ListStyle},
        CharacterStyle, ParagraphStyle, Story,
    };
    for kind in [ListKind::None, ListKind::Bullet, ListKind::Numbered] {
        let mut doc = blank_a4();
        let missing = "Schist nonexistent marker family 740521";
        doc.styles.add_character(CharacterStyle {
            name: "Marker".into(),
            family: Some(missing.into()),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "List".into(),
            left_indent: Some(30.0),
            first_line_indent: Some(-30.0),
            list: ListStyle {
                kind: Some(kind),
                bullet_character_style: Some("Marker".into()),
                numbering_character_style: Some("Marker".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 200.0, 100.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("Visible", "List");
        let page = separate_page(&doc, 0, OutputSettings::at(36.0), &NoGraphics).unwrap();
        assert_eq!(
            page.report
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error && f.message.contains(missing))
                .count(),
            usize::from(kind != ListKind::None)
        );
    }
}

#[test]
fn unsupported_visible_list_markers_are_errors_even_when_the_body_fits() {
    use schist_layout::{
        lists::{ListKind, ListStyle, NumberingList},
        ParagraphStyle, Story,
    };
    for property in [
        "NumberingLevel",
        "NumberingExpression.MissingLevel",
        "NumberingRestartPolicies",
        "NumberingFormat",
        "NumberingFormat.RomanRange",
        "ContinueNumbersAcrossStories.UnknownCreationOrder",
    ] {
        let mut doc = blank_a4();
        doc.styles.numbering_lists.push(NumberingList {
            id: "sequence".into(),
            across_stories: property.starts_with("ContinueNumbersAcrossStories"),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "List".into(),
            list: ListStyle {
                kind: Some(ListKind::Numbered),
                list: Some("sequence".into()),
                level: Some(match property {
                    "NumberingLevel" => 10,
                    "NumberingExpression.MissingLevel" => 2,
                    _ => 1,
                }),
                expression: (property == "NumberingExpression.MissingLevel")
                    .then(|| "^1.^#^t".into()),
                restart_policy: (property == "NumberingRestartPolicies").then(|| {
                    schist_layout::lists::RestartPolicy {
                        policy: "AfterSpecificLevel".into(),
                        lower: 1,
                        upper: 0,
                    }
                }),
                start: Some(if property == "NumberingFormat.RomanRange" {
                    3999
                } else {
                    1
                }),
                format: match property {
                    "NumberingFormat" => Some("Custom".into()),
                    "NumberingFormat.RomanRange" => {
                        Some(schist_layout::list_numbering::CounterFormat::UpperRoman.native())
                    }
                    _ => None,
                },
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 200.0, 100.0),
        )
        .unwrap();
        if property.starts_with("ContinueNumbersAcrossStories") {
            // Unlabelled imports have no evidence for this sequence's order.
            doc.creation_order.clear();
        }
        doc.stories[frame.story.0 as usize] = Story::from_text("Visible", "List");
        if property == "NumberingFormat.RomanRange" {
            doc.stories[frame.story.0 as usize].push_paragraph("Continued", "List");
        }
        let page = separate_page(&doc, 0, OutputSettings::at(36.0), &NoGraphics).unwrap();
        assert_eq!(
            page.report
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error && f.message.contains(property))
                .count(),
            1
        );
        assert!(!page.report.is_printable());
    }
}

#[test]
fn unrendered_structures_fail_both_preflight_paths_even_when_the_body_fits() {
    for text in ["", "Fits"] {
        for legacy in [false, true] {
            for at in [Some(0), None] {
                let mut doc = blank_a4();
                doc.pages[0] = schist_layout::Page::new("1", 100.0, 100.0);
                let frame = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(10.0, 10.0, 80.0, 80.0),
                )
                .unwrap();
                doc.stories[frame.story.0 as usize] = schist_layout::Story::from_text(text, "Body");
                if legacy {
                    doc.stories[frame.story.0 as usize].points.push(
                        schist_layout::StoryPoint::Other {
                            kind: "Table".into(),
                            payload: "raw".into(),
                        },
                    );
                } else {
                    doc.stories[frame.story.0 as usize].structures.push(
                        schist_layout::StoryStructure {
                            at,
                            kind: "Footnote".into(),
                            payload: "raw".into(),
                            footnote: None,
                        },
                    );
                }
                for dpi in [36.0, 72.0] {
                    let settings = OutputSettings::at(dpi);
                    for separated in [
                        separate_page(&doc, 0, settings, &NoGraphics),
                        schist_separation::separate_page_built(
                            &doc,
                            0,
                            settings,
                            &NoGraphics,
                            &schist_separation::NaiveBuild,
                        ),
                    ] {
                        let report = separated.unwrap().report;
                        let message = schist_i18n::tf!(
                            "design.preflight_story_structure",
                            name = doc.object(frame.object).unwrap().name,
                            count = 1
                        );
                        assert_eq!(
                            report
                                .findings
                                .iter()
                                .filter(|f| f.severity == Severity::Error && f.message == message)
                                .count(),
                            1
                        );
                        assert!(!report.is_printable());
                    }
                }
            }
        }
    }
}
