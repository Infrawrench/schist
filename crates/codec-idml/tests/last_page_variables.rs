use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    authoring, blank_a4, compose,
    text_variables::{
        self as variables, Cursor, LastPageNumber, PageNumberFormat, VariableKind, VariableScope,
    },
    History, LayoutDocument, NumberStyle, ParentObject, ParentPage, Rect, Section, Story, StoryId,
};

fn native(content: &str, definitions: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 300.0, 200.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("seed", "Body");
    let package = container::read(&export::write(&doc).bytes).unwrap();
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = package.get(name).unwrap();
            let bytes = if name.starts_with("Stories/") {
                let root = xml::parse(std::str::from_utf8(bytes).unwrap()).unwrap();
                let id = root.find("Story").unwrap().attr("Self").unwrap();
                format!(r#"<idPkg:Story><Story Self="{id}"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" PageNumberType="TextVariable">{content}</CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes()
            } else if name == "designmap.xml" {
                std::str::from_utf8(bytes).unwrap().replace("</Document>", &format!("{definitions}</Document>")).into_bytes()
            } else {
                bytes.to_vec()
            };
            (name.to_owned(), bytes)
        })
        .collect();
    container::write(&parts)
}

fn instance(id: &str, definition: &str) -> String {
    format!(
        r#"<TextVariableInstance Self="{id}" Name="Last" AssociatedTextVariable="{definition}" ResultText="99"/>"#
    )
}

fn last_page(id: &str, attributes: &str) -> String {
    format!(
        r#"<TextVariable Self="{id}" Name="Last Page" VariableType="LastPageNumberType"><PageNumberVariablePreference {attributes}/></TextVariable>"#
    )
}

const FORMATS: [(&str, PageNumberFormat, &str); 6] = [
    ("Current", PageNumberFormat::Current, "1"),
    ("Arabic", PageNumberFormat::Arabic, "1"),
    ("UpperRoman", PageNumberFormat::UpperRoman, "I"),
    ("LowerRoman", PageNumberFormat::LowerRoman, "i"),
    ("UpperLetters", PageNumberFormat::UpperLetters, "A"),
    ("LowerLetters", PageNumberFormat::LowerLetters, "a"),
];
const SCOPES: [(&str, VariableScope); 2] = [
    ("DocumentScope", VariableScope::Document),
    ("SectionScope", VariableScope::Section),
];

fn values(doc: &LayoutDocument, story: StoryId) -> (Vec<String>, usize) {
    let composed = compose::compose_story(doc, story);
    let text: String = composed.frames[0]
        .lines
        .iter()
        .filter_map(|l| l.projected.as_ref())
        .map(|p| p.spec.text.as_str())
        .collect();
    (
        text.split('\u{2068}')
            .skip(1)
            .map(|rest| rest.split('\u{2069}').next().unwrap().to_owned())
            .collect(),
        composed.frames[0].unrendered_structures,
    )
}

fn fonts() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
}

#[test]
fn native_last_page_preferences_lower_render_and_save_with_their_published_spellings() {
    fonts();
    for (format_name, format, number) in FORMATS {
        for (scope_name, scope) in SCOPES {
            for (before, after, before_xml, after_xml, omit) in [
                ("", "", "", "", false),
                ("of ", " pp. & é", "of ", " pp. &amp; é", false),
                ("", "", "", "", true),
            ] {
                let text = if omit {
                    format!(r#"Format="{format_name}" Scope="{scope_name}""#)
                } else {
                    format!(
                        r#"TextBefore="{before_xml}" Format="{format_name}" TextAfter="{after_xml}" Scope="{scope_name}""#
                    )
                };
                let bytes = native(
                    &(instance("i", "last") + "<Content>x</Content>" + &instance("j", "last")),
                    &last_page("last", &text),
                );
                let mut doc = import::read(&bytes).unwrap().document;
                let expected = schist_layout::text_variables::TextVariable::new(
                    "last",
                    "Last Page",
                    VariableKind::LastPage(LastPageNumber {
                        before: before.into(),
                        format,
                        after: after.into(),
                        scope,
                    }),
                );
                assert_eq!(doc.text_variables, std::slice::from_ref(&expected));
                // Exact native XML is archived too, as for custom text.
                let archive = doc.retained_text_variables.clone();
                assert_eq!(archive.len(), 1);
                assert_eq!(doc.stories[0].text(), "x");
                for _ in 0..3 {
                    let value = format!("{before}{number}{after}");
                    assert_eq!(values(&doc, StoryId(0)), (vec![value.clone(), value], 0));
                    let written = export::write(&doc);
                    let package = container::read(&written.bytes).unwrap();
                    let root = xml::parse(package.text("designmap.xml").unwrap()).unwrap();
                    let natives: Vec<_> = root.children_named("TextVariable").collect();
                    assert_eq!(natives.len(), 1);
                    assert_eq!(natives[0].attr("VariableType"), Some("LastPageNumberType"));
                    let preference = natives[0].child("PageNumberVariablePreference").unwrap();
                    assert_eq!(preference.attr("Format"), Some(format_name));
                    assert_eq!(preference.attr("Scope"), Some(scope_name));
                    assert_eq!(preference.attr("TextBefore"), Some(before));
                    assert_eq!(preference.attr("TextAfter"), Some(after));
                    doc = import::read(&written.bytes).unwrap().document;
                    assert_eq!(doc.text_variables.len(), 1);
                    assert_eq!(doc.text_variables[0].last_page, expected.last_page);
                    assert_eq!(doc.text_variables[0].name, expected.name);
                    assert_eq!(doc.retained_text_variables, archive);
                    assert_eq!(doc.stories[0].text(), "x");
                }
            }
        }
    }
}

#[test]
fn unrendered_or_incomplete_last_page_preferences_stay_exact_recovery_data() {
    let full = |format: &str| {
        format!(r#"TextBefore="" Format="{format}" TextAfter="" Scope="SectionScope""#)
    };
    let mut cases: Vec<String> = [
        "Kanji",
        "FullWidthArabic",
        "SingleLeadingZeros",
        "DoubleLeadingZeros",
        "Future",
        "",
    ]
    .into_iter()
    .map(|format| last_page("last", &full(format)))
    .collect();
    cases.extend([
        last_page("last", r#"TextBefore="" TextAfter="" Scope="SectionScope""#),
        last_page("last", r#"Format="Current" TextBefore="""#),
        last_page("last", r#"Format="Current" Scope="BookScope""#),
        last_page("last", &(full("Current") + r#" Future="true""#)),
        last_page(
            "last",
            r#"Format="Current" Scope="SectionScope" TextBefore="a&#9;b""#,
        ),
        last_page(
            "last",
            r#"Format="Current" Scope="SectionScope" TextAfter="&#x2067;""#,
        ),
        last_page("", &full("Current")),
        last_page("last", &full("Current")).replace(
            "/></TextVariable>",
            "><Child/></PageNumberVariablePreference></TextVariable>",
        ),
        last_page("last", &full("Current")).replace(
            "/></TextVariable>",
            ">text</PageNumberVariablePreference></TextVariable>",
        ),
        last_page("last", &full("Current")) + &last_page("last", &full("Arabic")),
        last_page("last", &full("Current")).replace(
            "PageNumberVariablePreference",
            "ChapterNumberVariablePreference",
        ),
    ]);
    for raw in cases {
        let mut doc = import::read(&native(&instance("i", "last"), &raw))
            .unwrap()
            .document;
        let archive = doc.retained_text_variables.clone();
        assert!(!archive.is_empty(), "{raw}");
        for _ in 0..3 {
            assert!(doc.text_variables.is_empty(), "{raw}");
            assert_eq!(doc.retained_text_variables, archive);
            assert_eq!(values(&doc, StoryId(0)).1, 1, "{raw}");
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
        }
    }
}

#[test]
fn authored_parent_footers_keep_section_values_through_native_saves() {
    fonts();
    let mut doc = blank_a4();
    for _ in 1..5 {
        doc.add_page(doc.pages[0].clone());
    }
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap()
        .family = Some("IBM Plex Sans".into());
    doc.pages[2].section = Some(Section {
        start: 1,
        continue_numbering: false,
        style: NumberStyle::RomanLower,
        ..Default::default()
    });
    let mut history = History::default();
    let frame = authoring::text_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(30.0, 760.0, 520.0, 40.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("Page of , ends ", "Body");
    let story = frame.story;
    for (name, format, scope, at) in [
        (
            "Document",
            PageNumberFormat::Current,
            VariableScope::Document,
            8,
        ),
        (
            "Section",
            PageNumberFormat::UpperLetters,
            VariableScope::Section,
            15,
        ),
    ] {
        let id = variables::create_definition(
            &mut doc,
            &mut history,
            name,
            VariableKind::LastPage(LastPageNumber {
                before: "#".into(),
                format,
                after: String::new(),
                scope,
            }),
        )
        .unwrap();
        let definition = doc
            .text_variables
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .clone();
        assert!(Cursor::capture(&doc, story, at).unwrap().insert(
            &mut doc,
            &mut history,
            &definition
        ));
    }
    let object = doc.objects.remove(0);
    doc.parents.push(ParentPage {
        name: "A".into(),
        applied_to: Vec::new(),
        based_on: None,
        hidden: false,
        sheets: Vec::new(),
        placements: Vec::new(),
        objects: vec![ParentObject {
            object,
            overridden_on: Vec::new(),
        }],
    });
    // Saved pages name their parent; assignment keeps both sides in agreement.
    for page in 0..5 {
        assert!(schist_layout::structure::set_parent(
            &mut doc,
            &mut history,
            page,
            Some(0)
        ));
    }
    let definitions = doc.text_variables.clone();
    for _ in 0..3 {
        assert_eq!(doc.text_variables, definitions);
        // Each page's only text frame is its parent footer instance.
        for page in 0..5 {
            let object = doc
                .page_objects(page)
                .into_iter()
                .find(|o| matches!(o.object, schist_layout::LayoutObject::TextFrame { .. }))
                .expect("footer instance");
            let frame = compose::compose_object(&doc, &object).unwrap();
            assert_eq!(frame.unrendered_structures, 0);
            let text: String = frame
                .lines
                .iter()
                .filter_map(|l| l.projected.as_ref())
                .map(|p| p.spec.text.as_str())
                .collect();
            let found: Vec<_> = text
                .split('\u{2068}')
                .skip(1)
                .map(|rest| rest.split('\u{2069}').next().unwrap())
                .collect();
            // Labels are 1 2 i ii iii: the document ends at iii; the first
            // section ends at 2, rendered as B in the explicit letter format.
            assert_eq!(
                found,
                ["#iii", if page < 2 { "#B" } else { "#C" }],
                "page {page}"
            );
        }
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}

#[test]
fn external_preference_edits_supersede_saved_identity_data() {
    let mut doc = import::read(&native("<Content>x</Content>", ""))
        .unwrap()
        .document;
    let mut history = History::default();
    let id = variables::create_definition(
        &mut doc,
        &mut history,
        "Last",
        VariableKind::LastPage(LastPageNumber::default()),
    )
    .unwrap();
    let definition = doc.text_variables[0].clone();
    assert!(Cursor::capture(&doc, StoryId(0), 1).unwrap().insert(
        &mut doc,
        &mut history,
        &definition
    ));
    let saved = export::write(&doc).bytes;
    let reopened = import::read(&saved).unwrap().document;
    assert_eq!(reopened.text_variables[0].id, id);
    let package = container::read(&saved).unwrap();
    let root = package.text("designmap.xml").unwrap().to_owned();
    assert!(root.contains(r#"Scope="SectionScope""#));
    let edited = root.replace(r#"Scope="SectionScope""#, r#"Scope="DocumentScope""#);
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                if name == "designmap.xml" {
                    edited.as_bytes().to_vec()
                } else {
                    package.get(name).unwrap().to_vec()
                },
            )
        })
        .collect();
    let changed = import::read(&container::write(&parts)).unwrap().document;
    assert_eq!(changed.text_variables.len(), 1);
    assert_eq!(
        changed.text_variables[0].last_page.as_ref().unwrap().scope,
        VariableScope::Document
    );
    // The stale saved identity is not restored over a native edit, but the
    // native binding still resolves the story's instance.
    let reference = changed.stories[0]
        .structures
        .iter()
        .find_map(|s| match &s.control {
            Some(schist_layout::story::InlineControl::TextVariable { variable, .. }) => {
                Some(variable.clone())
            }
            _ => None,
        });
    assert_eq!(
        reference.as_deref(),
        Some(changed.text_variables[0].id.as_str())
    );
}
