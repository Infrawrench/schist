use schist_codec_idml::{container, export, import};
use schist_layout::text_variables::{
    ChangeCase, MatchStyle, RunningHeader, TextVariable, VariableKind,
};
use schist_layout::{blank_a4, CharacterStyle, LayoutDocument, ParagraphStyle};

fn header(id: &str, style: MatchStyle, last: bool, case: ChangeCase, delete: bool) -> TextVariable {
    TextVariable::new(
        id,
        format!("{id} & é"),
        VariableKind::RunningHeader(RunningHeader {
            before: "« ".into(),
            after: " »".into(),
            style,
            last,
            case,
            delete_end_punctuation: delete,
        }),
    )
}

fn document() -> LayoutDocument {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Heading & 見出し".into(),
        ..Default::default()
    });
    doc.styles.characters.push(CharacterStyle {
        name: "Keyword".into(),
        ..Default::default()
    });
    doc.text_variables = vec![
        header(
            "first",
            MatchStyle::Paragraph("Heading & 見出し".into()),
            false,
            ChangeCase::Title,
            true,
        ),
        header(
            "keyword",
            MatchStyle::Character("Keyword".into()),
            true,
            ChangeCase::Sentence,
            false,
        ),
    ];
    doc
}

/// Definitions save with InDesign's spellings (the paged-media `variables`
/// sample's) and read back typed, with style names, through repeated saves.
#[test]
fn running_header_definitions_save_natively_and_read_back_typed() {
    let mut doc = document();
    let expected = doc.text_variables.clone();
    for _ in 0..3 {
        let bytes = export::write(&doc).bytes;
        let package = container::read(&bytes).unwrap();
        let designmap = package.text("designmap.xml").unwrap();
        assert!(designmap.contains(r#"VariableType="MatchParagraphStyleType""#));
        assert!(designmap
            .contains(r#"AppliedParagraphStyle="ParagraphStyle/$ID/Heading &amp; 見出し""#));
        assert!(designmap.contains(
            r#"SearchStrategy="FirstOnPage" ChangeCase="Titlecase" DeleteEndPunctuation="true""#
        ));
        assert!(designmap.contains(r#"VariableType="MatchCharacterStyleType""#));
        assert!(designmap.contains(r#"AppliedCharacterStyle="CharacterStyle/$ID/Keyword""#));
        assert!(designmap.contains(r#"SearchStrategy="LastOnPage" ChangeCase="Sentencecase""#));
        doc = import::read(&bytes).unwrap().document;
        assert_eq!(doc.text_variables, expected);
    }
}

/// A preference without an explicit setting stays recovery data: its
/// defaults are not published.
#[test]
fn incomplete_preferences_stay_retained_untyped() {
    let doc = document();
    let bytes = export::write(&doc).bytes;
    let mut package = container::read(&bytes).unwrap();
    let designmap = package
        .text("designmap.xml")
        .unwrap()
        .replace(r#" SearchStrategy="FirstOnPage""#, "");
    package.insert("designmap.xml", designmap.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    let ids: Vec<_> = imported
        .document
        .text_variables
        .iter()
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(ids, ["keyword & é"]);
    assert!(!imported.document.retained_text_variables.is_empty());
}
