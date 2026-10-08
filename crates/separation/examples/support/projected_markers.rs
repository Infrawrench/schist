//! Source-context list markers beside generated note references, compared with
//! explicitly styled markers that cannot inherit the reference's superscript.
#[path = "named_initials.rs"]
mod initials;
use schist_layout::{lists::ListKind, styles::TextPosition, LayoutDocument};
pub const CASES: usize = 4;
pub fn register_font() {
    initials::register_font();
}
pub fn document(reference: bool, case: usize) -> LayoutDocument {
    assert!(case < CASES);
    let mut doc = initials::document(false, if case >= 2 { 6 } else { 7 });
    let mut control = doc.styles.character("Initial").unwrap().clone();
    control.name = "Marker control".into();
    control.position = Some(TextPosition::Normal);
    doc.styles.add_character(control);
    let paragraph = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Source")
        .unwrap();
    paragraph.drop_caps_lines = Some(1);
    paragraph.list.kind = Some(if case.is_multiple_of(2) {
        ListKind::Bullet
    } else {
        ListKind::Numbered
    });
    paragraph.list.bullet_character_style = reference.then(|| "Marker control".into());
    paragraph.list.numbering_character_style = reference.then(|| "Marker control".into());
    paragraph.left_indent = Some(26.0);
    paragraph.first_line_indent = Some(-26.0);
    if case >= 2 {
        let body = &mut doc.stories[0].structures[0]
            .footnote
            .as_mut()
            .unwrap()
            .story;
        let schist_layout::StoryPoint::Paragraph { text, .. } = &mut body.points[0] else {
            unreachable!()
        };
        text.push_str("\u{2028}ninth row\u{2028}tenth row");
    }
    if let schist_layout::StoryPoint::Paragraph { text, .. } =
        &mut doc.stories.last_mut().unwrap().points[0]
    {
        *text = format!(
            "Source list markers / case {} / {} notes",
            case + 1,
            if case >= 2 { "split" } else { "whole" }
        );
    }
    doc
}
