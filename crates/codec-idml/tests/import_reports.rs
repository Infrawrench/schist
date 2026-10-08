//! The import report names what Schist does not set, and only that. Real
//! InDesign exports carry settings at their defaults on every document (a
//! layout name on each section, default object styles enabling categories
//! at default values, group wrap and export options); those change nothing
//! and are not reported.
use schist_codec_idml::{container, import};

const PSU: &[u8] = include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml");
const TEXT: &[u8] = include_bytes!("../../../fixtures/idml/text.idml");

fn skipped(bytes: &[u8]) -> Vec<String> {
    import::read(bytes).unwrap().report.skipped
}

fn says(lines: &[String], text: &str) -> bool {
    lines.iter().any(|line| line.contains(text))
}

/// `bytes` with `part` rewritten by `edit`.
fn edited(bytes: &[u8], part: &str, edit: impl Fn(&str) -> String) -> Vec<u8> {
    let mut package = container::read(bytes).unwrap();
    let text = edit(package.text(part).unwrap());
    package.insert(part, text.into_bytes());
    container::write(&package.into_parts())
}

#[test]
fn a_real_templates_defaults_are_not_reported() {
    let lines = skipped(PSU);
    for quiet in [
        "Unsupported frame",
        "Alternate layout",
        "unsupported categories",
        "Structured story content",
    ] {
        assert!(!says(&lines, quiet), "{quiet}: {lines:#?}");
    }
    // Groups are still flattened, and say so.
    assert!(says(&lines, "Group imported as individual objects"));
}

#[test]
fn a_second_layout_is_reported_once() {
    let second = edited(TEXT, "designmap.xml", |xml| {
        let start = xml.find("<Section ").unwrap();
        let open = start + xml[start..].find('>').unwrap() + 1;
        let end = if xml[..open].ends_with("/>") {
            open
        } else {
            open + xml[open..].find("</Section>").unwrap() + "</Section>".len()
        };
        let section = &xml[start..end];
        // A second section on the same page, in another layout, is enough
        // to name a second layout; it is then refused as a duplicate.
        let other = section
            .replace(r#"Self=""#, r#"Self="second"#)
            .replace(r#"AlternateLayout=""#, r#"AlternateLayout="Phone "#);
        format!("{}{other}{}", &xml[..end], &xml[end..])
    });
    let lines = skipped(&second);
    let reported = lines
        .iter()
        .filter(|l| l.contains("Alternate layout"))
        .count();
    assert_eq!(reported, 1, "{lines:#?}");
    assert!(!says(&skipped(TEXT), "Alternate layout"));
}

#[test]
fn object_style_categories_are_reported_only_off_their_defaults() {
    let style = r#"ObjectStyle Self="ObjectStyle/$ID/[Normal Text Frame]""#;
    let columns = edited(PSU, "Resources/Styles.xml", |xml| {
        let at = xml.find(style).unwrap();
        let tail = &xml[at..];
        let at = at + tail.find(r#"TextColumnCount="1""#).unwrap();
        format!(
            "{}{}{}",
            &xml[..at],
            r#"TextColumnCount="2""#,
            &xml[at + r#"TextColumnCount="1""#.len()..]
        )
    });
    let lines = skipped(&columns);
    assert!(
        says(&lines, "[Normal Text Frame]: unsupported categories"),
        "{lines:#?}"
    );
    assert!(!says(&lines, "[Normal Grid]: unsupported categories"));
    // A drop shadow, which Schist does not draw, in an enabled category.
    let shadow = edited(PSU, "Resources/Styles.xml", |xml| {
        let style = r#"ObjectStyle Self="ObjectStyle/$ID/[Normal Graphics Frame]""#;
        let at = xml.find(style).unwrap();
        let at = at + xml[at..].find('>').unwrap() + 1;
        format!(
            "{}{}{}",
            &xml[..at],
            r#"<TransparencySetting><DropShadowSetting Mode="Drop"/></TransparencySetting>"#,
            &xml[at..]
        )
    });
    assert!(says(
        &skipped(&shadow),
        "[Normal Graphics Frame]: unsupported categories"
    ));
}

#[test]
fn an_active_group_wrap_and_untyped_story_content_are_reported() {
    // Turn on the first group's text wrap in the template's masters.
    let package = container::read(PSU).unwrap();
    let part = package
        .names()
        .into_iter()
        .find(|name| {
            name.starts_with("MasterSpreads/")
                && package.text(name).is_some_and(|t| t.contains("<Group "))
        })
        .unwrap()
        .to_owned();
    let wrapped = edited(PSU, &part, |xml| {
        let group = xml.find("<Group ").unwrap();
        let at = group + xml[group..].find(r#"TextWrapMode="None""#).unwrap();
        format!(
            "{}{}{}",
            &xml[..at],
            r#"TextWrapMode="BoundingBoxTextWrap""#,
            &xml[at + r#"TextWrapMode="None""#.len()..]
        )
    });
    let lines = skipped(&wrapped);
    assert!(says(&lines, "text wrap not applied"), "{lines:#?}");
    // A table whose counts disagree stays retained and unset.
    let package = container::read(PSU).unwrap();
    let story = package
        .names()
        .into_iter()
        .find(|name| {
            name.starts_with("Stories/")
                && package.text(name).is_some_and(|t| t.contains("<Table "))
        })
        .unwrap()
        .to_owned();
    let broken = edited(PSU, &story, |xml| {
        xml.replacen(r#"BodyRowCount="11""#, r#"BodyRowCount="12""#, 1)
    });
    assert!(says(&skipped(&broken), "Structured story content"));
}
