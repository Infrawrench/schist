#[path = "../examples/support/footnotes.rs"]
mod proof;

#[test]
fn footnotes_match_independent_frames_and_rules_in_every_plate_at_multiple_resolutions() {
    match_independent_frames(proof::document);
}

#[test]
fn spanning_notes_match_full_width_independent_text_and_rules_in_every_plate() {
    match_independent_frames(proof::spanning_document);
}

#[test]
fn continued_notes_match_independent_fragments_and_rules_in_every_plate() {
    compare_plates(
        &proof::continuing_document(false),
        &proof::continuing_document(true),
    );
}

fn match_independent_frames(document: fn(bool) -> schist_layout::LayoutDocument) {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for (columns, balanced) in [1, 2, 3]
        .into_iter()
        .flat_map(|columns| [false, true].map(|balanced| (columns, balanced)))
    {
        let mut actual = document(false);
        actual.footnotes.straddle.get_or_insert(false);
        for object in &mut actual.objects {
            if let schist_layout::LayoutObject::TextFrame {
                columns: count,
                balance_columns,
                gutter,
                ..
            } = &mut object.object
            {
                *count = columns;
                *balance_columns = Some(balanced);
                *gutter = 5.0;
            }
        }
        let expected = document(true);
        compare_plates(&actual, &expected);
    }
}

fn compare_plates(
    actual: &schist_layout::LayoutDocument,
    expected: &schist_layout::LayoutDocument,
) {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for dpi in [72.0, 144.0, 216.0] {
        for page in 0..actual.pages.len() {
            let settings = schist_separation::OutputSettings::at(dpi);
            let a =
                schist_separation::separate_page_without_graphics(actual, page, settings).unwrap();
            let b = schist_separation::separate_page_without_graphics(expected, page, settings)
                .unwrap();
            assert!(
                !a.report
                    .findings
                    .iter()
                    .any(|f| f.severity == schist_separation::Severity::Error),
                "{:?}",
                a.report
            );
            assert_eq!(a.separation.plates().len(), b.separation.plates().len());
            for (index, (plate, other)) in a
                .separation
                .plates()
                .iter()
                .zip(b.separation.plates())
                .enumerate()
            {
                let first = plate
                    .data
                    .iter()
                    .zip(&other.data)
                    .enumerate()
                    .find(|(_, (a, b))| a != b);
                assert!(
                    first.is_none(),
                    "page={page}, dpi={dpi}, plate={index}, difference={first:?}"
                );
            }
        }
    }
}

#[test]
fn unsupported_note_policies_and_overset_notes_remain_preflight_errors() {
    use schist_separation::{
        separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
    };
    for unsupported in [false, true] {
        let mut doc = proof::document(false);
        if unsupported {
            doc.footnotes.first_baseline =
                Some(schist_layout::footnotes::FootnoteFirstBaseline::CapHeight);
        } else {
            doc.objects[0].bounds.height = 10.0;
        }
        let key = if unsupported {
            "design.preflight_story_structure"
        } else {
            "design.preflight_overset"
        };
        let expected = if unsupported {
            schist_i18n::tf!(key, name = doc.objects[0].name, count = 1)
        } else {
            schist_i18n::tf!(key, name = doc.objects[0].name)
        };
        for output in [
            separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
            separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &NaiveBuild),
        ] {
            assert!(output
                .unwrap()
                .report
                .findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.message == expected));
        }
    }
}

#[test]
fn note_preflight_uses_its_own_paragraph_context_and_numbering_sequence() {
    use schist_layout::{
        lists::{ListKind, ListStyle, ListTab, NumberingList},
        ParagraphStyle, Story,
    };
    use schist_separation::{
        separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
    };
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for case in ["tabs", "local list", "cross-story list"] {
        let mut doc = proof::document(false);
        let mut style = ParagraphStyle {
            name: "Note context".into(),
            based_on: Some("Note".into()),
            ..Default::default()
        };
        if case == "tabs" {
            style.list.tabs = Some(vec![ListTab {
                position: 30.0,
                alignment: "UnsupportedAlignment".into(),
                alignment_character: String::new(),
                leader: String::new(),
            }]);
        } else {
            style.left_indent = Some(20.0);
            style.first_line_indent = Some(-20.0);
            style.list = ListStyle {
                kind: Some(ListKind::Numbered),
                list: Some("notes".into()),
                ..Default::default()
            };
            doc.styles.numbering_lists.push(NumberingList {
                id: "notes".into(),
                across_stories: case == "cross-story list",
                ..Default::default()
            });
        }
        doc.styles.add_paragraph(style);
        let note = doc.stories[0].structures[0].footnote.as_mut().unwrap();
        note.story = Story::from_text("Alpha\tBeta", "Note context");
        note.story.push_paragraph("Gamma", "Note context");
        note.markers.clear();
        let frame = schist_layout::compose::compose_object(&doc, &doc.objects[0]).unwrap();
        assert!(!frame.lost);
        assert_eq!(frame.unrendered_structures, 0);
        assert_eq!(frame.footnotes.len(), 1);
        if case == "local list" {
            let markers: Vec<_> = frame.footnotes[0]
                .lines
                .iter()
                .filter(|line| line.generated.is_some())
                .map(|line| line.projected.as_ref().unwrap().spec.text.as_str())
                .collect();
            assert_eq!(markers, ["1.", "2."]);
        }
        for output in [
            separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
            separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &NaiveBuild),
        ] {
            let report = output.unwrap().report;
            let errors: Vec<_> = report
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .map(|f| f.message.as_str())
                .collect();
            let expected = match case {
                "tabs" => Some(schist_i18n::tf!(
                    "design.idml_tabs_unsupported",
                    value = "TabList"
                )),
                "cross-story list" => Some(schist_i18n::tf!(
                    "design.idml_list_unsupported",
                    value = "ContinueNumbersAcrossStories.MissingFrame"
                )),
                _ => None,
            };
            assert_eq!(
                errors,
                expected.as_deref().into_iter().collect::<Vec<_>>(),
                "{case}"
            );
            assert!(
                !report
                    .findings
                    .iter()
                    .any(|f| { f.message == schist_i18n::t("design.idml_cross_story_order") }),
                "{case}: {report:?}"
            );
        }
    }
}

#[test]
fn unfinished_note_after_main_eof_is_overset_in_both_preflight_paths() {
    use schist_layout::{compose::compose_story, StoryId};
    use schist_separation::{
        separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
    };
    let mut doc = proof::continuing_document(false);
    doc.objects.truncate(1);
    let frame = compose_story(&doc, StoryId(0)).frames.remove(0);
    assert_eq!(frame.consumed_to, doc.stories[0].text_len());
    assert!(!frame.footnotes.is_empty());
    assert_eq!(frame.unrendered_structures, 0);
    assert!(frame.lost);
    let expected = schist_i18n::tf!("design.preflight_overset", name = doc.objects[0].name);
    for result in [
        separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
        separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &NaiveBuild),
    ] {
        assert!(result
            .unwrap()
            .report
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Error && finding.message == expected));
    }
}
