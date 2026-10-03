#[path = "../../separation/examples/support/projected_initials.rs"]
mod proof;
use schist_codec_idml::{container, export, import};

#[test]
fn composed_reference_counts_never_leak_into_native_styles_or_authored_source() {
    proof::register_font();
    for middle in [false, true] {
        let mut doc = proof::document(false, true, middle);
        let main = doc.stories[0].text();
        let note = doc.stories[0].structures[0]
            .footnote
            .as_ref()
            .unwrap()
            .story
            .text();
        let anchor = doc.stories[0].structures[0].at;
        for _ in 0..4 {
            let before = doc.clone();
            let thread = schist_layout::compose::compose_story(&doc, schist_layout::StoryId(0));
            assert!(!thread.has_overflow());
            let initial = thread.lines().find(|line| line.initial.is_some()).unwrap();
            assert_eq!((initial.start, initial.end), (0, "E\u{301}x".len()));
            assert_eq!(doc, before);
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            assert!(!package
                .text("Resources/Styles.xml")
                .unwrap()
                .contains("Schist generated footnote paragraph"));
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(
                doc.styles.resolve_paragraph("Main").drop_caps_characters,
                Some(2)
            );
            assert_eq!(
                doc.styles.resolve_paragraph("Note").drop_caps_characters,
                Some(1)
            );
            assert_eq!(doc.stories[0].text(), main);
            assert_eq!(doc.stories[0].structures[0].at, anchor);
            assert_eq!(
                doc.stories[0].structures[0]
                    .footnote
                    .as_ref()
                    .unwrap()
                    .story
                    .text(),
                note
            );
        }
    }
}
