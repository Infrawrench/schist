use schist_codec_idml::{container, export, import};
use schist_layout::dates::DateTime;
use schist_layout::text_variables::{DateKind, DateVariable, FileName, TextVariable, VariableKind};
use schist_layout::{blank_a4, LayoutDocument};

fn date(id: &str, kind: DateKind, format: &str) -> TextVariable {
    TextVariable::new(
        id,
        format!("{id} é"),
        VariableKind::Date(DateVariable {
            kind,
            before: "« ".into(),
            format: format.into(),
            after: " »".into(),
        }),
    )
}

fn document() -> LayoutDocument {
    let mut doc = blank_a4();
    doc.text_variables = vec![
        date("created", DateKind::Created, "yyyy-MM-dd"),
        date("modified", DateKind::Modified, "MMMM d, yyyy"),
        date("output", DateKind::Output, "EEEE dd.MM.yy 'at' h:mm a"),
        TextVariable::new(
            "file",
            "File",
            VariableKind::FileName(FileName {
                before: String::new(),
                path: true,
                extension: false,
                after: " & co".into(),
            }),
        ),
    ];
    doc.dates.created = DateTime::new(2026, 10, 1, 8, 26, 47);
    doc.dates.modified = DateTime::new(2026, 10, 2, 17, 0, 5);
    doc.dates.output = DateTime::new(2026, 10, 3, 9, 0, 0);
    doc
}

/// Definitions save with the spellings the public paged-media `variables`
/// sample writes and read back typed; creation and modification dates travel
/// in the package's XMP. The output date is not document data.
#[test]
fn date_and_file_name_definitions_and_dates_survive_saves() {
    let mut doc = document();
    let expected = doc.text_variables.clone();
    for _ in 0..3 {
        let bytes = export::write(&doc).bytes;
        let package = container::read(&bytes).unwrap();
        let designmap = package.text("designmap.xml").unwrap();
        for spelling in [
            r#"VariableType="CreationDateType"><DateVariablePreference TextBefore="« " Format="yyyy-MM-dd" TextAfter=" »"/>"#,
            r#"VariableType="ModificationDateType""#,
            r#"VariableType="OutputDateType"><DateVariablePreference TextBefore="« " Format="EEEE dd.MM.yy &apos;at&apos; h:mm a" TextAfter=" »"/>"#,
            r#"VariableType="FileNameType"><FileNameVariablePreference TextBefore="" IncludePath="true" IncludeExtension="false" TextAfter=" &amp; co"/>"#,
        ] {
            assert!(designmap.contains(spelling), "{spelling}\n{designmap}");
        }
        let metadata = package.text("META-INF/metadata.xml").unwrap();
        assert!(metadata.contains("<xmp:CreateDate>2026-10-01T08:26:47</xmp:CreateDate>"));
        assert!(metadata.contains("<xmp:ModifyDate>2026-10-02T17:00:05</xmp:ModifyDate>"));
        doc = import::read(&bytes).unwrap().document;
        assert_eq!(doc.text_variables, expected);
        assert_eq!(doc.dates.created, DateTime::new(2026, 10, 1, 8, 26, 47));
        assert_eq!(doc.dates.modified, DateTime::new(2026, 10, 2, 17, 0, 5));
        assert_eq!(doc.dates.output, None);
    }
}

/// XMP writes dates as elements or as Description attributes, with any
/// prefix bound to the XMP namespace and with a time zone, which is ignored.
#[test]
fn native_xmp_dates_are_read_in_either_form() {
    let bytes = export::write(&blank_a4()).bytes;
    for metadata in [
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:CreateDate="2026-10-01T08:26:47+02:00" xmp:ModifyDate="2026-10-02T17:00:05+02:00"/></rdf:RDF></x:xmpmeta>"#,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:xap="http://ns.adobe.com/xap/1.0/"><xap:CreateDate>2026-10-01T08:26:47Z</xap:CreateDate><xap:ModifyDate>2026-10-02T17:00:05.000-04:00</xap:ModifyDate></rdf:Description></rdf:RDF></x:xmpmeta>"#,
    ] {
        let mut package = container::read(&bytes).unwrap();
        package.insert("META-INF/metadata.xml", metadata.as_bytes().to_vec());
        let doc = import::read(&container::write(&package.into_parts()))
            .unwrap()
            .document;
        assert_eq!(doc.dates.created, DateTime::new(2026, 10, 1, 8, 26, 47));
        assert_eq!(doc.dates.modified, DateTime::new(2026, 10, 2, 17, 0, 5));
    }
}

/// A date preference without a Format, or a file-name preference without
/// both switches, stays retained recovery data: their defaults are not
/// published.
#[test]
fn incomplete_preferences_stay_retained_untyped() {
    let bytes = export::write(&document()).bytes;
    let mut package = container::read(&bytes).unwrap();
    let designmap = package
        .text("designmap.xml")
        .unwrap()
        .replace(r#" Format="yyyy-MM-dd""#, "")
        .replace(r#" IncludePath="true""#, "");
    package.insert("designmap.xml", designmap.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    let typed: Vec<_> = imported
        .document
        .text_variables
        .iter()
        .map(|v| v.id.as_str())
        .collect();
    assert_eq!(typed, ["modified", "output"]);
    assert!(imported.document.retained_text_variables.len() >= 2);
}
