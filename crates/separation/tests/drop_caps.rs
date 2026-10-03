use schist_layout::{authoring, History, ParagraphStyle, Rect, Story};
use schist_separation::{separate_page_without_graphics, OutputSettings, Severity};

#[test]
fn preflight_reports_only_used_active_native_initial_flags_without_losing_their_text() {
    for active in [false, true] {
        for used in [false, true] {
            let mut doc = schist_layout::blank_a4();
            let made = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(20.0, 20.0, 200.0, 200.0),
            )
            .unwrap();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Initial".into(),
                drop_caps_lines: Some(if active { 3 } else { 0 }),
                drop_caps_characters: Some(1),
                drop_caps_detail: Some(0),
                ..Default::default()
            });
            doc.stories[made.story.0 as usize] = Story::from_text(
                "Alpha beta gamma delta epsilon zeta eta theta iota.",
                if used { "Initial" } else { "Body" },
            );
            let saved = doc.clone();
            let page = separate_page_without_graphics(&doc, 0, OutputSettings::at(72.0)).unwrap();
            let issues = page
                .report
                .findings
                .iter()
                .filter(|finding| finding.message.contains("DropcapDetail"))
                .collect::<Vec<_>>();
            assert_eq!(issues.len(), usize::from(active && used));
            assert!(issues.iter().all(|issue| issue.severity == Severity::Error));
            assert_eq!(doc, saved);
        }
    }
}

#[test]
fn preflight_reports_only_active_enlarged_initials_on_paths_in_both_separation_modes() {
    use schist_layout::{text_path, Point, ShapePath, SubPath};
    use schist_separation::{separate_page, separate_page_built, NaiveBuild, NoGraphics};
    for path in [false, true] {
        for lines in [0, 1, 3] {
            for count in [0, 1] {
                let mut doc = schist_layout::blank_a4();
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Initial".into(),
                    drop_caps_lines: Some(lines),
                    drop_caps_characters: Some(count),
                    ..Default::default()
                });
                let made = if path {
                    let shape = authoring::path_shape(
                        &mut doc,
                        &mut History::default(),
                        0,
                        ShapePath {
                            subpaths: vec![SubPath {
                                points: vec![Point::new(20.0, 40.0), Point::new(250.0, 40.0)],
                                ..Default::default()
                            }],
                            even_odd: false,
                        },
                        authoring::Paint::none(),
                    )
                    .unwrap();
                    text_path::attach(&mut doc, &mut History::default(), shape).unwrap()
                } else {
                    authoring::text_frame(
                        &mut doc,
                        &mut History::default(),
                        0,
                        Rect::new(20.0, 20.0, 250.0, 250.0),
                    )
                    .unwrap()
                };
                doc.stories[made.story.0 as usize] = Story::from_text("Alpha beta", "Initial");
                let saved = doc.clone();
                for page in [
                    separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
                    separate_page_built(
                        &doc,
                        0,
                        OutputSettings::at(72.0),
                        &NoGraphics,
                        &NaiveBuild,
                    ),
                ] {
                    let page = page.unwrap();
                    let issues: Vec<_> = page
                        .report
                        .findings
                        .iter()
                        .filter(|f| f.message.contains("TextPath + DropCapLines"))
                        .collect();
                    assert_eq!(
                        issues.len(),
                        usize::from(path && lines > 1 && count > 0),
                        "{path}/{lines}/{count}"
                    );
                    assert!(issues.iter().all(|f| f.severity == Severity::Error));
                }
                assert_eq!(doc, saved);
            }
        }
    }
}
