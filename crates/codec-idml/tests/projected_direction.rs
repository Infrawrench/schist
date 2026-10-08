#[path = "../../separation/examples/support/projected_direction.rs"]
mod proof;
use schist_codec_idml::{container, export, import};
use schist_layout::{compose, ParagraphDirection, StoryId};
use schist_text_engine::ParagraphDirection as EngineDirection;

#[test]
fn resolved_display_direction_never_replaces_the_authored_automatic_policy_on_save() {
    proof::register_font();
    for rtl in [false, true] {
        for initial in [false, true] {
            for policy in [None, Some(ParagraphDirection::Auto)] {
                let mut doc = proof::document(false, rtl, initial);
                doc.styles
                    .paragraphs
                    .iter_mut()
                    .find(|style| style.name == "Source")
                    .unwrap()
                    .direction = policy;
                let text = doc.stories[0].text();
                let note = doc.stories[0].structures[0]
                    .footnote
                    .as_ref()
                    .unwrap()
                    .story
                    .text();
                for _ in 0..4 {
                    let before = doc.clone();
                    let thread = compose::compose_story(&doc, StoryId(0));
                    assert!(!thread.has_overflow());
                    for line in thread.frames.iter().flat_map(|frame| frame.all_lines()) {
                        assert_eq!(
                            compose::line_spec(line, &doc.stories[0], &doc).direction,
                            if rtl {
                                EngineDirection::RightToLeft
                            } else {
                                EngineDirection::LeftToRight
                            }
                        );
                    }
                    assert_eq!(doc, before);
                    let written = export::write(&doc);
                    let package = container::read(&written.bytes).unwrap();
                    assert!(!package
                        .text("Resources/Styles.xml")
                        .unwrap()
                        .contains("Schist generated footnote paragraph"));
                    doc = import::read(&written.bytes).unwrap().document;
                    assert_eq!(doc.styles.paragraph("Source").unwrap().direction, policy);
                    assert_eq!(doc.stories[0].text(), text);
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
    }
}
