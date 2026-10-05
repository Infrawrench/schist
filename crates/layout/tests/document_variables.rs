//! Date and file-name text variables. The date formats are the public
//! paged-media `variables` sample's, whose InDesign PDF prints "Created
//! 2026-10-01 | modified October 1, 2026 | output Thursday 01.10.26".
use schist_layout::dates::DateTime;
use schist_layout::story::InlineControl;
use schist_layout::text_variables::{DateKind, DateVariable, FileName, TextVariable, VariableKind};
use schist_layout::{
    authoring, blank_a4, compose, History, LayoutDocument, Rect, Story, StoryStructure,
};

fn october_first() -> DateTime {
    DateTime::new(2026, 10, 1, 14, 5, 9).unwrap()
}

#[test]
fn the_native_samples_date_formats() {
    let date = october_first();
    assert_eq!(date.format("yyyy-MM-dd"), "2026-10-01");
    assert_eq!(date.format("MMMM d, yyyy"), "October 1, 2026");
    assert_eq!(date.format("EEEE dd.MM.yy"), "Thursday 01.10.26");
}

/// The formats of InDesign's default definitions, as the public templates
/// carry them.
#[test]
fn indesigns_default_date_formats() {
    let date = october_first();
    assert_eq!(date.format("MM/dd/yy"), "10/01/26");
    assert_eq!(
        date.format("MMMM d, yyyy h:mm aa"),
        "October 1, 2026 2:05 PM"
    );
    assert_eq!(date.format("YYYY-MM-dd @ hh:mma"), "2026-10-01 @ 02:05PM");
}

#[test]
fn other_codes_quotes_and_literals() {
    let date = october_first();
    assert_eq!(date.format("EEE, MMM d"), "Thu, Oct 1");
    assert_eq!(date.format("h:mm a"), "2:05 PM");
    assert_eq!(date.format("HH:mm:ss"), "14:05:09");
    assert_eq!(date.format("'Week of' d/M"), "Week of 1/10");
    assert_eq!(date.format("d 'o''clock' ''"), "1 o'clock '");
    assert_eq!(date.format("G yyyy"), "AD 2026");
    let midnight = DateTime::new(2026, 1, 5, 0, 0, 0).unwrap();
    assert_eq!(midnight.format("h a, EEEE"), "12 AM, Monday");
}

#[test]
fn dates_parse_from_xmp_and_convert_from_unix_time() {
    let parsed = |text| DateTime::parse(text);
    assert_eq!(parsed("2026-10-01T14:05:09+02:00"), Some(october_first()));
    assert_eq!(parsed("2026-10-01T14:05:09.250Z"), Some(october_first()));
    assert_eq!(parsed("2026-10-01T14:05:09-05:00"), Some(october_first()));
    assert_eq!(parsed("2026-10-01"), DateTime::new(2026, 10, 1, 0, 0, 0));
    assert_eq!(parsed("2026-10"), DateTime::new(2026, 10, 1, 0, 0, 0));
    for bad in ["2026-13-01", "2026-02-30", "2026-10-01T25:00", "soon", ""] {
        assert_eq!(parsed(bad), None, "{bad}");
    }
    assert_eq!(DateTime::from_unix(1_790_863_509), october_first());
    assert_eq!(
        DateTime::from_unix(0),
        DateTime::new(1970, 1, 1, 0, 0, 0).unwrap()
    );
    assert_eq!(
        DateTime::from_unix(951_868_799),
        DateTime::new(2000, 2, 29, 23, 59, 59).unwrap()
    );
    assert_eq!(october_first().iso(), "2026-10-01T14:05:09");
}

fn document(definitions: Vec<TextVariable>) -> LayoutDocument {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 40.0, 500.0, 300.0),
    )
    .unwrap();
    let text = "| ".repeat(definitions.len());
    let mut story = Story::from_text(text.clone(), "Body");
    story.structures = definitions
        .iter()
        .zip(text.match_indices('|').map(|(at, _)| at))
        .map(|(definition, at)| StoryStructure {
            at: Some(at),
            kind: "TextVariableInstance".into(),
            payload: String::new(),
            footnote: None,
            control: Some(InlineControl::TextVariable {
                variable: definition.id.clone(),
                character_style: String::new(),
                name: definition.name.clone(),
            }),
            anchored: None,
        })
        .collect();
    doc.stories[frame.story.0 as usize] = story;
    doc.text_variables = definitions;
    doc
}

fn shown(doc: &LayoutDocument) -> (Vec<String>, usize) {
    let thread = compose::compose_story(doc, schist_layout::StoryId(0));
    let frame = &thread.frames[0];
    let text: String = frame
        .lines
        .iter()
        .filter_map(|l| l.projected.as_ref())
        .map(|p| p.spec.text.as_str())
        .collect();
    let values = text
        .split('\u{2068}')
        .skip(1)
        .map(|rest| rest.split('\u{2069}').next().unwrap().to_owned())
        .collect();
    (values, frame.unrendered_structures)
}

#[test]
fn date_variables_show_the_documents_dates_and_unknown_ones_stay_diagnosed() {
    let date = |id: &str, kind, format: &str| {
        TextVariable::new(
            id,
            id,
            VariableKind::Date(DateVariable {
                kind,
                before: "(".into(),
                format: format.into(),
                after: ")".into(),
            }),
        )
    };
    let mut doc = document(vec![
        date("created", DateKind::Created, "yyyy-MM-dd"),
        date("modified", DateKind::Modified, "MMMM d, yyyy"),
        date("output", DateKind::Output, "EEEE dd.MM.yy"),
    ]);
    assert_eq!(shown(&doc), (Vec::new(), 3));
    doc.dates.created = Some(october_first());
    doc.dates.modified = Some(october_first());
    assert_eq!(
        shown(&doc),
        (vec!["(2026-10-01)".into(), "(October 1, 2026)".into()], 1)
    );
    doc.dates.output = Some(october_first());
    assert_eq!(shown(&doc).0[2], "(Thursday 01.10.26)");
    assert_eq!(shown(&doc).1, 0);
}

#[test]
fn file_name_variables_follow_the_documents_path() {
    let file = |id: &str, path, extension| {
        TextVariable::new(
            id,
            id,
            VariableKind::FileName(FileName {
                before: String::new(),
                path,
                extension,
                after: String::new(),
            }),
        )
    };
    let mut doc = document(vec![
        file("name", false, false),
        file("name.ext", false, true),
        file("path", true, false),
        file("path.ext", true, true),
    ]);
    doc.name = "Untitled".into();
    // Before a first save there is a name but no folder.
    assert_eq!(shown(&doc), (vec!["Untitled".into(), "Untitled".into()], 2));
    for (path, expected) in [
        (
            "C:\\Work\\Reports\\Annual.report.idml",
            [
                "Annual.report",
                "Annual.report.idml",
                "C:\\Work\\Reports\\Annual.report",
                "C:\\Work\\Reports\\Annual.report.idml",
            ],
        ),
        (
            "/home/me/.hidden",
            // A leading dot starts a name, not an extension.
            [".hidden", ".hidden", "/home/me/.hidden", "/home/me/.hidden"],
        ),
    ] {
        doc.file_path = Some(path.into());
        let (values, unrendered) = shown(&doc);
        assert_eq!(values, expected.map(String::from), "{path}");
        assert_eq!(unrendered, 0);
    }
}
