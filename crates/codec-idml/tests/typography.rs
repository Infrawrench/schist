use schist_codec_idml::{container, export, import, xml};
use schist_layout::{blank_a4, CharacterStyle, ParagraphStyle, Story};

fn native_story(styles: &str, story: &str) -> schist_layout::LayoutDocument {
    let mut doc = blank_a4();
    doc.stories.push(Story::from_text("", "Body"));
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    package.insert(
        "Resources/Styles.xml",
        format!("<idPkg:Styles>{styles}</idPkg:Styles>").into_bytes(),
    );
    let path = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Stories/"))
        .unwrap()
        .to_owned();
    package.insert(
        path,
        format!("<idPkg:Story><Story Self=\"SchistStory0\">{story}</Story></idPkg:Story>")
            .into_bytes(),
    );
    import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document
}

#[test]
fn local_overrides_inherit_per_property_and_never_multiply_on_save() {
    for repeats in [1, 2, 5, 9] {
        let styles = r#"<RootParagraphStyleGroup>
          <ParagraphStyle Self="p0" Name="Base / 空" FontStyle="Bold" PointSize="13" FillColor="Color/Black"><Properties><AppliedFont type="string">serif</AppliedFont></Properties></ParagraphStyle>
          <ParagraphStyle Self="p1" Name="Body / 空"><Properties><BasedOn type="object">p0</BasedOn></Properties></ParagraphStyle>
        </RootParagraphStyleGroup><RootCharacterStyleGroup>
          <CharacterStyle Self="c0" Name="Emphasis / 空" FontStyle="Italic" Underline="true"/>
        </RootCharacterStyleGroup>"#;
        let mut story = String::new();
        for index in 0..repeats {
            story.push_str(r#"<ParagraphStyleRange AppliedParagraphStyle="p1" PointSize="18" SpaceBefore="7">
              <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>é</Content></CharacterStyleRange>
              <CharacterStyleRange AppliedCharacterStyle="c0" PointSize="24" Underline="false"><Content>中</Content></CharacterStyleRange>
              <CharacterStyleRange AppliedCharacterStyle="c0" PointSize="24" Underline="false"><Content>😀</Content></CharacterStyleRange>
            </ParagraphStyleRange>"#);
            if index + 1 < repeats {
                story.push_str("<ParagraphStyleRange><CharacterStyleRange><Br/></CharacterStyleRange></ParagraphStyleRange>");
            }
        }
        let mut doc = native_story(styles, &story);
        let counts = (doc.styles.paragraphs.len(), doc.styles.characters.len());
        assert_eq!(counts, (5, 4)); // defaults, native definitions, one override each
        for _ in 0..5 {
            assert_eq!(doc.stories[0].text(), vec!["é中😀"; repeats].join("\n"));
            assert_eq!(
                (doc.styles.paragraphs.len(), doc.styles.characters.len()),
                counts
            );
            let offsets = doc.stories[0].point_offsets();
            for (index, point) in doc.stories[0].points.iter().enumerate() {
                let offset = offsets[index];
                let schist_layout::StoryPoint::Paragraph { text, style } = point else {
                    panic!()
                };
                let paragraph = doc.styles.resolve_paragraph(style);
                assert_eq!(paragraph.space_before, Some(7.0));
                assert_eq!(paragraph.family.as_deref(), Some("serif"));
                let spec = schist_layout::compose::spec_for(
                    &doc.stories[0],
                    offset,
                    offset + text.len(),
                    &doc.styles,
                    style,
                    "Default",
                    400.0,
                );
                let plain = spec.style_at(0);
                assert_eq!(plain.family, "serif");
                assert_eq!(plain.size, 18.0);
                assert!(plain.bold);
                assert!(!plain.italic);
                for byte in [2, 5] {
                    let run = spec.style_at(byte);
                    assert_eq!(run.family, "serif");
                    assert_eq!(run.size, 24.0);
                    assert!(!run.bold);
                    assert!(run.italic);
                    assert!(!run.underline);
                }
            }
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
        }
    }
}

#[test]
fn opaque_ids_disambiguate_same_named_styles_and_resolve_inheritance() {
    let styles = r#"<RootParagraphStyleGroup>
      <ParagraphStyleGroup Self="g1" Name="One"><ParagraphStyle Self="u101" Name="Body" PointSize="10"/></ParagraphStyleGroup>
      <ParagraphStyleGroup Self="g2" Name="Two"><ParagraphStyle Self="u102" Name="Body" PointSize="24"/></ParagraphStyleGroup>
      <ParagraphStyle Self="u103" Name="Child"><Properties><BasedOn type="object">u102</BasedOn></Properties></ParagraphStyle>
    </RootParagraphStyleGroup>"#;
    let story = r#"<ParagraphStyleRange AppliedParagraphStyle="u101"><CharacterStyleRange><Content>a</Content><Br/></CharacterStyleRange></ParagraphStyleRange>
      <ParagraphStyleRange AppliedParagraphStyle="u102"><CharacterStyleRange><Content>b</Content><Br/></CharacterStyleRange></ParagraphStyleRange>
      <ParagraphStyleRange AppliedParagraphStyle="u103"><CharacterStyleRange><Content>c</Content></CharacterStyleRange></ParagraphStyleRange>"#;
    let mut doc = native_story(styles, story);
    for _ in 0..4 {
        for (point, size) in doc.stories[0].points.iter().zip([10.0, 24.0, 24.0]) {
            let schist_layout::StoryPoint::Paragraph { style, .. } = point else {
                panic!()
            };
            assert_eq!(doc.styles.resolve_paragraph(style).point_size, Some(size));
        }
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}

#[test]
fn combined_font_style_keeps_both_resolved_properties() {
    for parent_bold in [false, true] {
        for parent_italic in [false, true] {
            for child_bold in [None, Some(false), Some(true)] {
                for child_italic in [None, Some(false), Some(true)] {
                    let mut doc = blank_a4();
                    doc.styles.add_character(CharacterStyle {
                        name: "Parent".into(),
                        bold: Some(parent_bold),
                        italic: Some(parent_italic),
                        ..Default::default()
                    });
                    doc.styles.add_character(CharacterStyle {
                        name: "Child".into(),
                        based_on: Some("Parent".into()),
                        bold: child_bold,
                        italic: child_italic,
                        ..Default::default()
                    });
                    let back = import::read(&export::write(&doc).bytes).unwrap().document;
                    let resolved = back.styles.resolve_character("Child");
                    assert_eq!(resolved.bold, Some(child_bold.unwrap_or(parent_bold)));
                    assert_eq!(resolved.italic, Some(child_italic.unwrap_or(parent_italic)));
                }
            }
        }
    }
}

#[test]
fn tracking_uses_each_effective_font_size_and_keeps_native_values_on_save() {
    for size in [6.0, 12.0, 24.0] {
        for tracking in [-50.0, 0.0, 150.0, 300.0] {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Body / A".into(),
                point_size: Some(size),
                tracking: Some(tracking),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Large / A".into(),
                point_size: Some(size * 2.0),
                ..Default::default()
            });
            let mut story = Story::from_text("HHHH", "Body / A");
            story.apply_style(2, 4, "Large / A");
            doc.stories.push(story);
            for _ in 0..4 {
                let spec = schist_layout::compose::spec_for(
                    &doc.stories[0],
                    0,
                    4,
                    &doc.styles,
                    "Body / A",
                    "Default",
                    400.0,
                );
                assert_eq!(spec.style_at(0).tracking, tracking * size / 1000.0);
                assert_eq!(spec.style_at(2).tracking, tracking * size * 2.0 / 1000.0);
                let bytes = export::write(&doc).bytes;
                let package = container::read(&bytes).unwrap();
                let styles = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
                assert!(styles.child("Resources").is_none());
                let paragraph = styles
                    .child("RootParagraphStyleGroup")
                    .unwrap()
                    .children_named("ParagraphStyle")
                    .find(|p| p.attr("Name") == Some("Body / A"))
                    .unwrap();
                assert_eq!(paragraph.number("Tracking"), Some(tracking));
                let fonts = xml::parse(package.text("Resources/Fonts.xml").unwrap()).unwrap();
                assert!(fonts.child("Resources").is_none());
                doc = import::read(&bytes).unwrap().document;
            }
        }
    }
}

#[test]
fn native_drop_cap_counts_inherit_and_survive_repeated_saves() {
    for (lines, characters) in [(3, 1), (4, 2), (3, 0)] {
        let styles = format!(
            r#"<RootParagraphStyleGroup>
          <ParagraphStyle Self="p0" Name="Caps" DropCapLines="{lines}" DropCapCharacters="{characters}"/>
          <ParagraphStyle Self="p1" Name="Inherited"><Properties><BasedOn type="object">p0</BasedOn></Properties></ParagraphStyle>
        </RootParagraphStyleGroup>"#
        );
        let story = r#"<ParagraphStyleRange AppliedParagraphStyle="p1"><CharacterStyleRange><Content>Éclair and letters.</Content></CharacterStyleRange></ParagraphStyleRange>"#;
        let mut doc = native_story(&styles, story);
        for _ in 0..5 {
            let paragraph = doc.styles.resolve_paragraph("Inherited");
            assert_eq!(paragraph.drop_caps_lines, Some(lines));
            assert_eq!(paragraph.drop_caps_characters, Some(characters));
            let bytes = export::write(&doc).bytes;
            let package = container::read(&bytes).unwrap();
            let xml = std::str::from_utf8(package.get("Resources/Styles.xml").unwrap()).unwrap();
            assert!(xml.contains(&format!("DropCapCharacters=\"{characters}\"")));
            doc = import::read(&bytes).unwrap().document;
        }
    }
}

#[test]
fn legacy_implicit_drop_cap_count_is_explicit_for_native_consumers() {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Initial".into(),
        drop_caps_lines: Some(3),
        ..Default::default()
    });
    doc.stories.push(Story::from_text("A paragraph", "Initial"));
    let exported = export::write(&doc);
    let package = container::read(&exported.bytes).unwrap();
    let styles =
        xml::parse(std::str::from_utf8(package.get("Resources/Styles.xml").unwrap()).unwrap())
            .unwrap();
    let style = styles
        .find_all("ParagraphStyle")
        .into_iter()
        .find(|s| s.attr("Name") == Some("Initial"))
        .unwrap();
    assert_eq!(style.attr("DropCapCharacters"), Some("1"));
    assert_eq!(
        import::read(&exported.bytes)
            .unwrap()
            .document
            .styles
            .resolve_paragraph("Initial")
            .drop_caps_characters,
        Some(1)
    );
}

#[test]
fn native_paragraph_direction_survives_inheritance_local_overrides_and_opposing_text() {
    use schist_layout::ParagraphDirection as Direction;
    for (native, expected, opposite) in [
        ("LeftToRightDirection", Direction::LeftToRight, "نص عربي"),
        (
            "RightToLeftDirection",
            Direction::RightToLeft,
            "Latin words 123",
        ),
    ] {
        let styles = format!(
            r#"<RootParagraphStyleGroup>
          <ParagraphStyle Self="base" Name="Base" ParagraphDirection="{native}"/>
          <ParagraphStyle Self="child" Name="Inherited"><Properties><BasedOn type="object">base</BasedOn></Properties></ParagraphStyle>
        </RootParagraphStyleGroup>"#
        );
        let story = format!(
            r#"<ParagraphStyleRange AppliedParagraphStyle="child"><CharacterStyleRange><Content>{opposite}</Content><Br/></CharacterStyleRange></ParagraphStyleRange>
          <ParagraphStyleRange ParagraphDirection="{native}"><CharacterStyleRange><Content>{opposite}</Content></CharacterStyleRange></ParagraphStyleRange>"#
        );
        let mut doc = native_story(&styles, &story);
        let count = doc.styles.paragraphs.len();
        for _ in 0..5 {
            assert_eq!(doc.styles.paragraphs.len(), count);
            for point in &doc.stories[0].points {
                let schist_layout::StoryPoint::Paragraph { style, .. } = point else {
                    panic!()
                };
                assert_eq!(
                    doc.styles.resolve_paragraph(style).direction,
                    Some(expected)
                );
            }
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
        }
    }
}

#[test]
fn native_story_axes_and_paragraph_direction_remain_independent_on_save() {
    use schist_layout::{ParagraphDirection, StoryDirection, StoryOrientation};
    for (column_order, expected_order) in [
        ("LeftToRightDirection", StoryDirection::LeftToRight),
        ("RightToLeftDirection", StoryDirection::RightToLeft),
    ] {
        for (orientation, expected_orientation) in [
            ("Horizontal", StoryOrientation::Horizontal),
            ("Vertical", StoryOrientation::Vertical),
        ] {
            let styles = r#"<RootParagraphStyleGroup><ParagraphStyle Self="p" Name="Explicit" ParagraphDirection="RightToLeftDirection"/></RootParagraphStyleGroup>"#;
            let story = format!(
                r#"<StoryPreference StoryDirection="{column_order}" StoryOrientation="{orientation}"/>
              <ParagraphStyleRange AppliedParagraphStyle="p"><CharacterStyleRange><Content>Latin and 日本語</Content></CharacterStyleRange></ParagraphStyleRange>"#
            );
            let mut doc = native_story(styles, &story);
            for _ in 0..5 {
                assert_eq!(doc.stories[0].prefs.direction, expected_order);
                assert_eq!(doc.stories[0].prefs.orientation, expected_orientation);
                assert_eq!(
                    doc.styles.resolve_paragraph("Explicit").direction,
                    Some(ParagraphDirection::RightToLeft)
                );
                let bytes = export::write(&doc).bytes;
                let package = container::read(&bytes).unwrap();
                let path = package
                    .names()
                    .into_iter()
                    .find(|n| n.starts_with("Stories/"))
                    .unwrap();
                let root =
                    xml::parse(std::str::from_utf8(package.get(path).unwrap()).unwrap()).unwrap();
                let native = root.find("StoryPreference").unwrap();
                assert_eq!(native.attr("StoryDirection"), Some(column_order));
                assert_eq!(native.attr("StoryOrientation"), Some(orientation));
                doc = import::read(&bytes).unwrap().document;
            }
        }
    }
}

#[test]
fn native_tint_inheritance_and_local_overrides_survive_without_style_growth() {
    for tint in [0.0, 12.5, 50.0, 100.0] {
        let styles = format!(
            r#"<RootParagraphStyleGroup>
          <ParagraphStyle Self="p0" Name="Tint base" FillTint="{tint}" StrokeTint="25"/>
          <ParagraphStyle Self="p1" Name="Tint child" FillTint="-1"><Properties><BasedOn type="object">p0</BasedOn></Properties></ParagraphStyle>
        </RootParagraphStyleGroup><RootCharacterStyleGroup>
          <CharacterStyle Self="c0" Name="Tint character" FillTint="75" StrokeTint="12.5"/>
          <CharacterStyle Self="c1" Name="Tint inherited" FillTint="-1"><Properties><BasedOn type="object">c0</BasedOn></Properties></CharacterStyle>
        </RootCharacterStyleGroup>"#
        );
        let story = r#"<ParagraphStyleRange AppliedParagraphStyle="p1" FillTint="-1">
          <CharacterStyleRange><Content>A</Content></CharacterStyleRange>
          <CharacterStyleRange AppliedCharacterStyle="c1" FillTint="-1"><Content>B</Content></CharacterStyleRange>
          <CharacterStyleRange AppliedCharacterStyle="c1" FillTint="0" StrokeTint="50"><Content>C</Content></CharacterStyleRange>
        </ParagraphStyleRange>"#;
        let mut doc = native_story(&styles, story);
        let counts = (doc.styles.paragraphs.len(), doc.styles.characters.len());
        for _ in 0..4 {
            assert_eq!(doc.styles.paragraph("Tint child").unwrap().fill_tint, None);
            assert_eq!(
                doc.styles.character("Tint inherited").unwrap().fill_tint,
                None
            );
            let para = doc.styles.resolve_paragraph("Tint child");
            assert_eq!(para.fill_tint, Some(tint / 100.0));
            assert_eq!(para.stroke_tint, Some(0.25));
            let inherited = doc.styles.resolve_character("Tint inherited");
            assert_eq!(inherited.fill_tint, Some(0.75));
            assert_eq!(inherited.stroke_tint, Some(0.125));
            let local = &doc.stories[0]
                .ranges
                .iter()
                .find(|r| r.start == 2)
                .unwrap()
                .style;
            let local = doc.styles.resolve_character(local);
            assert_eq!(local.fill_tint, Some(0.0));
            assert_eq!(local.stroke_tint, Some(0.5));
            assert_eq!(
                (doc.styles.paragraphs.len(), doc.styles.characters.len()),
                counts
            );
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
        }
    }
}
