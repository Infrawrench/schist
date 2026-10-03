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
