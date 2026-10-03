use schist_codec_idml::{container, export, import, xml};
use schist_layout::{authoring, History, LayoutDocument, Rect};

fn package(yes: &str, no: &str) -> Vec<u8> {
    let mut doc = schist_layout::blank_a4();
    authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 200.0, 200.0),
    )
    .unwrap();
    let mut parts = container::read(&export::write(&doc).bytes).unwrap();
    parts.insert(
        "Resources/Styles.xml",
        format!(
            r#"<idPkg:Styles>
      <ParagraphStyle Self="p" Name="true" Hyphenation="{yes}" NoBreak="{no}"
       Underline="{yes}" StrikeThru="{no}" OverprintFill="{yes}" OverprintStroke="{no}"
       KeepLinesTogether="{yes}" KeepAllLinesTogether="{no}" KeepWithPrevious="{yes}"
       NumberingContinue="{no}" NumberingApplyRestartPolicy="{yes}"
       Ligatures="{yes}" OTFHVKana="{no}" OTFProportionalMetrics="{yes}"
       UnderlineOverprint="{yes}" UnderlineGapOverprint="{no}" />
      <CharacterStyle Self="c" Name="false" NoBreak="{yes}" Underline="{no}"
       StrikeThru="{yes}" OverprintFill="{no}" OverprintStroke="{yes}" Ligatures="{no}"
       OTFHVKana="{yes}" OTFProportionalMetrics="{no}" />
      <ObjectStyle Self="o" Name="Object" EnableFill="{yes}" EnableStroke="{no}"
       EnableStrokeAndCornerOptions="{yes}" EnableTextFrameFootnoteOptions="{no}"
       EnableTextFrameGeneralOptions="{yes}" OverprintFill="{no}" OverprintStroke="{yes}">
       <TextFramePreference VerticalBalanceColumns="{yes}" />
       <TextFrameFootnoteOptionsObject EnableOverrides="{yes}" SpanFootnotesAcross="{no}" />
      </ObjectStyle></idPkg:Styles>"#
        )
        .into_bytes(),
    );
    parts.insert("Resources/Preferences.xml", format!(r#"<idPkg:Preferences>
      <DocumentPreference FacingPages="{yes}" DocumentBleedUniformSize="{yes}"
       DocumentBleedTopOffset="4" DocumentBleedBottomOffset="7" DocumentBleedInsideOrLeftOffset="9" DocumentBleedOutsideOrRightOffset="11"
       DocumentSlugUniformSize="{no}" SlugTopOffset="13" SlugBottomOffset="15" SlugInsideOrLeftOffset="17" SlugRightOrOutsideOffset="19" />
      <TextFramePreference VerticalBalanceColumns="{yes}" />
      <FootnoteOption NoSplitting="{yes}" EnableStraddling="{no}" RuleOn="{yes}" RuleOverprint="{no}" />
      </idPkg:Preferences>"#).into_bytes());
    let names = parts
        .names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for path in names {
        if path.starts_with("Stories/") {
            parts.insert(path, br#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="p"><CharacterStyleRange AppliedCharacterStyle="c"><Content>true false 1 0</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#.to_vec());
        } else if path.starts_with("Spreads/") {
            parts.insert(path, format!(r#"<idPkg:Spread><Spread Self="s" PageCount="1" ShowMasterItems="{no}">
              <Page Self="SchistPage0" GeometricBounds="0 0 842 595" ItemTransform="1 0 0 1 0 0" AppliedMaster="master">
              <Guide Self="guide" Orientation="Horizontal" Location="35" Locked="{yes}" /></Page>
              <Group Locked="{yes}" Visible="{no}"><Polygon Self="shape" Name="true" ItemLayer="SchistLayer0"><Properties><PathGeometry EvenOdd="{yes}"><GeometryPathType PathOpen="{yes}"><PathPointArray><PathPointType Anchor="5 5"/><PathPointType Anchor="50 5"/><PathPointType Anchor="50 50"/></PathPointArray></GeometryPathType></PathGeometry></Properties></Polygon></Group>
              <TextFrame Self="frame" ParentStory="SchistStory0" ItemLayer="SchistLayer0" AppliedObjectStyle="o" Locked="{no}" Visible="{yes}" GeometricBounds="20 20 220 220"><TextFramePreference VerticalBalanceColumns="{no}"/><TextFrameFootnoteOptionsObject EnableOverrides="{no}" SpanFootnotesAcross="{yes}" /></TextFrame>
              </Spread></idPkg:Spread>"#).into_bytes());
        }
    }
    parts.insert("MasterSpreads/Base.xml", br#"<idPkg:MasterSpread><MasterSpread Self="base" Name="Base"><Page Self="base-page" GeometricBounds="0 0 842 595"/></MasterSpread></idPkg:MasterSpread>"#.to_vec());
    parts.insert("MasterSpreads/Child.xml", format!(r#"<idPkg:MasterSpread><MasterSpread Self="master" Name="Child" ShowMasterItems="{yes}"><Page Self="master-page" GeometricBounds="0 0 842 595" AppliedMaster="base"/></MasterSpread></idPkg:MasterSpread>"#).into_bytes());
    let root = parts
        .text("designmap.xml")
        .unwrap()
        .replace("Visible=\"true\"", &format!("Visible=\"{no}\""))
        .replace("Locked=\"false\"", &format!("Locked=\"{yes}\""))
        .replace(
            "ContinueNumbering=\"false\"",
            &format!("ContinueNumbering=\"{no}\""),
        )
        .replace(
            "IncludeSectionPrefix=\"false\"",
            &format!("IncludeSectionPrefix=\"{yes}\""),
        ).replace("</Document>", &format!(r#"<idPkg:MasterSpread src="MasterSpreads/Base.xml"/><idPkg:MasterSpread src="MasterSpreads/Child.xml"/><NumberingList Self="sequence" Name="true" ContinueNumbersAcrossStories="{yes}" ContinueNumbersAcrossDocuments="{no}"/><TextFrameFootnoteOptionsObject EnableOverrides="{yes}" SpanFootnotesAcross="{no}"/></Document>"#));
    parts.insert("designmap.xml", root.into_bytes());
    container::write(&parts.into_parts())
}

fn same(a: &LayoutDocument, b: &LayoutDocument) {
    assert_eq!(a.styles, b.styles);
    assert_eq!(a.stories, b.stories);
    assert_eq!(a.pages, b.pages);
    assert_eq!(a.parents, b.parents);
    assert_eq!(a.facing_pages, b.facing_pages);
    assert_eq!(a.balance_columns_default, b.balance_columns_default);
    assert_eq!(a.frame_footnote_defaults, b.frame_footnote_defaults);
    assert_eq!(a.footnotes, b.footnotes);
    assert_eq!(a.layers, b.layers);
    assert_eq!(a.layer_properties, b.layer_properties);
    assert_eq!(a.objects.len(), b.objects.len());
    for (a, b) in a.objects.iter().zip(&b.objects) {
        // Import allocates fresh ObjectIds; compare the actual retained values.
        assert_eq!(
            (
                &a.name,
                a.bounds,
                a.hidden,
                a.locked,
                &a.object,
                &a.appearance
            ),
            (
                &b.name,
                b.bounds,
                b.hidden,
                b.locked,
                &b.object,
                &b.appearance
            )
        );
    }
}

#[test]
fn legal_boolean_spellings_preserve_supported_native_settings_on_every_save() {
    for reverse in [false, true] {
        for (yes, no) in [
            ("true", "false"),
            ("1", "0"),
            (" true ", " false "),
            ("\t1\r\n", "\n0\t"),
        ] {
            let (yes, no) = if reverse { (no, yes) } else { (yes, no) };
            let mut expected = import::read(&package(
                if reverse { "false" } else { "true" },
                if reverse { "true" } else { "false" },
            ))
            .unwrap();
            let mut actual = import::read(&package(yes, no)).unwrap();
            assert_eq!(actual.document.objects.len(), 2);
            assert_eq!(actual.document.pages[0].guides.len(), 1);
            assert_eq!(actual.document.parents.len(), 2);
            assert_eq!(actual.document.styles.numbering_lists.len(), 1);
            assert_eq!(actual.document.stories[0].text(), "true false 1 0");
            for _ in 0..4 {
                same(&actual.document, &expected.document);
                assert_eq!(actual.report.skipped, expected.report.skipped);
                actual = import::read(&export::write(&actual.document).bytes).unwrap();
                expected = import::read(&export::write(&expected.document).bytes).unwrap();
            }
        }
    }
}

#[test]
fn boolean_lexical_space_collapses_only_xml_whitespace() {
    for (literal, expected) in [("true", true), ("1", true), ("false", false), ("0", false)] {
        for left in ["", " ", "\t", "\r", "\n", " \t\r\n"] {
            for right in ["", " ", "\t", "\r", "\n", " \t\r\n"] {
                let value = format!("{left}{literal}{right}");
                assert_eq!(xml::parse_boolean(&value), Some(expected));
                let element =
                    xml::parse(&format!("<Flag Value=\"{value}\" Name=\"{value}\"/>")).unwrap();
                assert_eq!(element.boolean("Value"), Some(expected));
                assert_eq!(element.boolean("Missing"), None);
            }
        }
    }
    for value in [
        "",
        " ",
        "TRUE",
        "False",
        "yes",
        "on",
        "-1",
        "2",
        "1.0",
        "+1",
        "tr ue",
        "0 1",
        "\u{a0}true\u{a0}",
        "\u{2003}0",
    ] {
        assert_eq!(xml::parse_boolean(value), None, "{value:?}");
    }
}

#[test]
fn invalid_booleans_keep_existing_diagnostics_in_each_typed_reader() {
    for raw in ["TRUE", "", "2", "\u{a0}false"] {
        let read = import::read(&package(raw, raw)).unwrap();
        for property in [
            "NoBreak",
            "KeepLinesTogether",
            "VerticalBalanceColumns",
            "NoSplitting",
            "RuleOn",
            "EnableOverrides",
        ] {
            let warning = schist_i18n::tf!(
                "design.idml_text_preference_invalid",
                property = property,
                value = raw
            );
            assert!(read.report.skipped.contains(&warning), "{property}/{raw:?}");
        }
        for property in ["Ligatures", "OTFHVKana", "OTFProportionalMetrics"] {
            let warning = schist_i18n::tf!(
                "design.idml_feature_invalid",
                property = property,
                value = raw
            );
            assert!(read.report.skipped.contains(&warning), "{property}/{raw:?}");
        }
        for property in [
            "NumberingContinue",
            "NumberingApplyRestartPolicy",
            "ContinueNumbersAcrossStories",
        ] {
            let warning = schist_i18n::tf!("design.idml_list_unsupported", value = property);
            assert!(read.report.skipped.contains(&warning), "{property}/{raw:?}");
        }
        assert!(read
            .document
            .styles
            .paragraphs
            .iter()
            .find(|p| p.name == "true")
            .unwrap()
            .no_break
            .is_none());
        assert_eq!(read.document.stories[0].text(), "true false 1 0");
    }
}
