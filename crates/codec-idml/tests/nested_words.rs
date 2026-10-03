#[path = "../../separation/examples/support/nested_words.rs"]
mod proof;
use schist_codec_idml::{container, export, import};
use schist_layout::{compose, StoryId};

#[test]
fn native_saves_preserve_word_rules_and_source_without_projected_aliases() {
    proof::register_font();
    for case in 0..proof::CASES {
        let mut doc = proof::document(false, case);
        let source = doc.stories[0].clone();
        let rules = doc
            .styles
            .paragraph("Source")
            .unwrap()
            .nested_styles
            .clone();
        for _ in 0..3 {
            let before = doc.clone();
            let rendered: Vec<_> = compose::compose_story(&doc, StoryId(0))
                .frames
                .iter()
                .flat_map(|f| f.all_lines())
                .map(|line| {
                    (
                        compose::line_spec(line, &doc.stories[0], &doc),
                        compose::line_paint_styles(line, &doc.stories[0], &doc),
                    )
                })
                .collect();
            let written = export::write(&doc);
            assert_eq!(doc, before);
            assert!(!written
                .warnings
                .iter()
                .any(|w| w.contains("AllNestedStyles")));
            let package = container::read(&written.bytes).unwrap();
            assert!(!package
                .text("Resources/Styles.xml")
                .unwrap()
                .contains("Schist generated"));
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(doc.styles.paragraph("Source").unwrap().nested_styles, rules);
            assert_eq!(doc.stories[0].text(), source.text());
            assert_eq!(doc.stories[0].ranges, source.ranges);
            let restored: Vec<_> = compose::compose_story(&doc, StoryId(0))
                .frames
                .iter()
                .flat_map(|f| f.all_lines())
                .map(|line| {
                    (
                        compose::line_spec(line, &doc.stories[0], &doc),
                        compose::line_paint_styles(line, &doc.stories[0], &doc),
                    )
                })
                .collect();
            assert_eq!(restored, rendered, "case {case}");
        }
    }
}

#[test]
fn observed_native_word_records_restart_after_each_paragraph_break() {
    // Public LeonidB specimen: two ordered AnyWord rules, one native range
    // containing five Content elements separated by Br. Text/metrics below
    // are our own; the unlicensed original is not redistributed.
    use schist_layout::nested_styles::{CharacterStyle, Delimiter, NestedStyle};
    use schist_layout::{authoring, History, Rect, StoryPoint};
    let texts = [
        ("First", "second", "remaining source"),
        ("Élan", "café", "suite"),
        ("E\u{301}cho", "B", "tail"),
        ("漢字", "次語", "本文"),
        ("left,right", "next!", "end"),
    ];
    let mut doc = schist_layout::blank_a4();
    authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 200.0, 200.0),
    )
    .unwrap();
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let rules = ["Regular", "Bold"].map(|name| format!(r#"<ListItem type="record"><AppliedCharacterStyle type="object">CharacterStyle/{name}</AppliedCharacterStyle><Delimiter type="enumeration">AnyWord</Delimiter><Repetition type="long">1</Repetition><Inclusive type="boolean">true</Inclusive></ListItem>"#)).concat();
    package.insert("Resources/Styles.xml", format!(r#"<idPkg:Styles><RootCharacterStyleGroup><CharacterStyle Self="CharacterStyle/Regular" Name="Regular" PointSize="11"/><CharacterStyle Self="CharacterStyle/Bold" Name="Bold" PointSize="20"/></RootCharacterStyleGroup><ParagraphStyle Self="p" Name="Policy" PointSize="9"><Properties><AllNestedStyles type="list">{rules}</AllNestedStyles></Properties></ParagraphStyle></idPkg:Styles>"#).into_bytes());
    let contents = texts
        .iter()
        .map(|(first, second, tail)| format!("<Content>{first} {second} {tail}</Content>"))
        .collect::<Vec<_>>()
        .join("<Br/>");
    let path = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Stories/"))
        .unwrap()
        .to_owned();
    package.insert(path, format!(r#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="p"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">{contents}</CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    assert!(!imported
        .report
        .skipped
        .iter()
        .any(|m| m.contains("AllNestedStyles")));
    doc = imported.document;
    let source = doc.stories[0].clone();
    for _ in 0..4 {
        let story = &doc.stories[0];
        assert_eq!(story, &source);
        assert!(story.ranges.is_empty());
        assert_eq!(story.points.len(), texts.len());
        let resolved = doc.styles.resolve_paragraph("Policy");
        assert_eq!(
            resolved.nested_styles,
            Some(
                ["Regular", "Bold"]
                    .map(|name| NestedStyle {
                        character_style: CharacterStyle::Named(name.into()),
                        delimiter: Delimiter::Enumeration("AnyWord".into()),
                        repetition: 1,
                        inclusive: true,
                    })
                    .to_vec()
            )
        );
        for ((point, offset), (first, second, tail)) in
            story.points.iter().zip(story.point_offsets()).zip(texts)
        {
            let StoryPoint::Paragraph { text, style } = point else {
                panic!("paragraph expected")
            };
            assert_eq!(text, &format!("{first} {second} {tail}"));
            for start in schist_text_engine::grapheme_boundaries(text) {
                let spec = compose::spec_for(
                    story,
                    offset + start,
                    offset + text.len(),
                    &doc.styles,
                    style,
                    "Default",
                    200.0,
                );
                for (at, _) in spec.text.char_indices() {
                    let original = start + at;
                    let size = if original < first.len() + 1 {
                        11.0
                    } else if original < first.len() + second.len() + 2 {
                        20.0
                    } else {
                        9.0
                    };
                    assert_eq!(spec.style_at(at).size, size, "{text:?}/{start}/{at}");
                }
            }
        }
        let exported = export::write(&doc);
        assert!(!exported
            .warnings
            .iter()
            .any(|m| m.contains("AllNestedStyles")));
        doc = import::read(&exported.bytes).unwrap().document;
    }
}
