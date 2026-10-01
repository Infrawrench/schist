use schist_layout::{
    authoring, compose,
    lists::{ListStyle, ListTab},
    History, LayoutDocument, Page, ParagraphStyle, Rect, Story,
};
use schist_separation::{separate_page_without_graphics, OutputSettings, Severity};

#[test]
fn tab_preflight_follows_used_settings_and_the_full_paragraph_context() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for (alignment, leader, expected) in [
        ("LeftAlign", "", false),
        ("LeftAlign", ".", true),
        ("RightAlign", "", true),
        ("CharacterAlign", "", true),
    ] {
        let mut doc = LayoutDocument::new(vec![Page::new("proof", 240.0, 160.0)]);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Tabs".into(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(12.0),
            drop_caps_lines: Some(2),
            drop_caps_characters: Some(1),
            list: ListStyle {
                tabs: Some(vec![ListTab {
                    position: 80.0,
                    alignment: alignment.into(),
                    alignment_character: ".".into(),
                    leader: leader.into(),
                }]),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 200.0, 120.0),
        )
        .unwrap();
        // The continued line starts with a tab; the enlarged initial is A.
        // Diagnostics must consult the paragraph, not mistake this line's tab
        // for the paragraph's drop-cap character.
        doc.stories[frame.story.0 as usize] = Story::from_text("A word\u{2028}\tH words", "Tabs");
        let flow = compose::compose_story(&doc, frame.story);
        assert!(!flow.has_overflow());
        assert!(flow.lines().any(|line| compose::line_spec(
            line,
            &doc.stories[frame.story.0 as usize],
            &doc
        )
        .text
        .starts_with('\t')));
        let result = separate_page_without_graphics(&doc, 0, OutputSettings::at(36.0)).unwrap();
        let message = schist_i18n::tf!("design.idml_tabs_unsupported", value = "TabList");
        assert_eq!(
            result
                .report
                .findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.message == message),
            expected
        );
        assert!(!result
            .report
            .findings
            .iter()
            .any(|f| f.message.contains("DropCapCharacters + TabList")));
        doc.stories[frame.story.0 as usize] = Story::from_text("A word without tabs", "Tabs");
        let result = separate_page_without_graphics(&doc, 0, OutputSettings::at(36.0)).unwrap();
        assert!(!result.report.findings.iter().any(|f| f.message == message));
    }
}

#[path = "../examples/support/tabs.rs"]
mod proof;
#[test]
fn tabbed_fields_match_independent_frames_across_writing_modes_sizes_indents_and_dpi() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let actual = proof::document(false);
    let expected = proof::document(true);
    for dpi in [72.0, 144.0, 216.0] {
        for page in 0..actual.pages.len() {
            let settings = schist_separation::OutputSettings::at(dpi);
            let a =
                schist_separation::separate_page_without_graphics(&actual, page, settings).unwrap();
            let b = schist_separation::separate_page_without_graphics(&expected, page, settings)
                .unwrap();
            assert_eq!(a.separation.plates().len(), b.separation.plates().len());
            for (index, (plate, other)) in a
                .separation
                .plates()
                .iter()
                .zip(b.separation.plates())
                .enumerate()
            {
                assert_eq!(plate.data.len(), other.data.len());
                let difference = plate
                    .data
                    .iter()
                    .zip(&other.data)
                    .enumerate()
                    .find(|(_, (a, b))| a != b);
                assert!(
                    difference.is_none(),
                    "page={page},dpi={dpi},plate={index},first={difference:?}"
                );
            }
            assert!(a
                .separation
                .plates()
                .iter()
                .any(|p| p.data.iter().any(|v| *v > 0.1)));
        }
    }
}
