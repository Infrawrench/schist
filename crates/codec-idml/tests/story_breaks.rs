use schist_codec_idml::{container, export, import, xml};
use schist_layout::{blank_a4, Story, StoryPoint, StyleRange};

#[test]
fn soft_paragraph_and_structural_breaks_keep_their_identity_and_byte_offsets() {
    for forced in [
        StoryPoint::ColumnBreak,
        StoryPoint::FrameBreak,
        StoryPoint::PageBreak,
    ] {
        let mut doc = blank_a4();
        let mut story = Story::default();
        story.push_paragraph("é\nsoft", "Body");
        story.points.push(forced);
        let (start, end) = story.push_paragraph("中😀", "Body");
        story.ranges.push(StyleRange::new(start, end, "Bold"));
        story.push_paragraph("last", "Body");
        story.push_paragraph("", "Body");
        doc.stories.push(story.clone());
        for _ in 0..5 {
            let bytes = export::write(&doc).bytes;
            let package = container::read(&bytes).unwrap();
            let path = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Stories/"))
                .unwrap();
            let root = xml::parse(package.text(path).unwrap()).unwrap();
            assert!(root.find_all("Content").iter().any(|c| c.text == "é\nsoft"));
            assert!(root
                .find_all("CharacterStyleRange")
                .iter()
                .any(|r| r.attr("ParagraphBreakType").is_some()));
            doc = import::read(&bytes).unwrap().document;
            assert_eq!(doc.stories[0], story);
            for range in &doc.stories[0].ranges {
                assert_eq!(doc.stories[0].slice(range.start, range.end), "中😀");
            }
        }
    }
}

#[test]
fn one_native_style_range_can_hold_several_paragraphs() {
    let mut doc = blank_a4();
    doc.stories.push(Story::from_text("", "Body"));
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let path = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Stories/"))
        .unwrap()
        .to_owned();
    package.insert(path, br#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Bold"><Content>ABC</Content><Br/><Content>DEF</Content><Br/><Content>GHI</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#.to_vec());
    let doc = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    assert_eq!(doc.stories[0].points.len(), 3);
    assert_eq!(doc.stories[0].text(), "ABC\nDEF\nGHI");
    for (range, expected) in doc.stories[0].ranges.iter().zip(["ABC", "DEF", "GHI"]) {
        assert_eq!(doc.stories[0].slice(range.start, range.end), expected);
    }
}
