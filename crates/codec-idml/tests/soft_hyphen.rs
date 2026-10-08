use schist_codec_idml::{container, export, import};

#[test]
fn native_discretionary_characters_keep_utf8_ranges_through_repeated_saves() {
    for encoded in ["café\u{ad}ine", "café&#xAD;ine", "café&#173;ine"] {
        let mut doc = schist_layout::blank_a4();
        schist_layout::authoring::text_frame(
            &mut doc,
            &mut schist_layout::History::default(),
            0,
            schist_layout::Rect::new(20.0, 20.0, 100.0, 100.0),
        )
        .unwrap();
        let mut package = container::read(&export::write(&doc).bytes).unwrap();
        let part = package
            .names()
            .into_iter()
            .find(|p| p.starts_with("Stories/"))
            .unwrap()
            .to_owned();
        package.insert("Resources/Styles.xml",br#"<idPkg:Styles><ParagraphStyle Self="p" Name="Body"/><CharacterStyle Self="c" Name="Protected" NoBreak="true"/></idPkg:Styles>"#.to_vec());
        package.insert(&part,format!(r#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="p"><CharacterStyleRange AppliedCharacterStyle="c"><Content>{encoded}</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes());
        let mut doc = import::read(&container::write(&package.into_parts()))
            .unwrap()
            .document;
        let source = doc.stories[0].clone();
        assert_eq!(source.text(), "café\u{ad}ine");
        assert_eq!(source.ranges.len(), 1);
        assert_eq!(source.ranges[0].start, 0);
        assert_eq!(source.ranges[0].end, source.text_len());
        for _ in 0..4 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.stories[0], source);
            assert_eq!(
                doc.styles
                    .resolve_character(&source.ranges[0].style)
                    .no_break,
                Some(true)
            );
        }
    }
}
