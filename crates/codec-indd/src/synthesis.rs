//! The recovered objects written as IDML parts.
//!
//! The IDML importer already knows how a spread places its pages and
//! items, how a parent page applies, how frames thread and how inks are
//! named, and it is tested against InDesign's own exports. Writing the
//! subset as the parts InDesign would have exported for it, under the
//! same `u…` ids, sends an INDD document down that same path instead of
//! a second one that could disagree with it.

use std::fmt::Write;

use crate::model::{
    ColorModel, ColorValue, Document, Item, ItemKind, Matrix, PageObject, Path, Spread, Story,
};

const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const PACKAGING: &str = "http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";
const DOM_VERSION: &str = "20.0";

/// The parts of an IDML package holding `document`, and what could not
/// be written into them.
pub(crate) fn parts(document: &Document) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let mut writer = Writer {
        document,
        skipped: Vec::new(),
    };
    let mut parts = vec![
        (
            "mimetype".to_owned(),
            schist_codec_idml::container::MIMETYPE.as_bytes().to_vec(),
        ),
        (
            "META-INF/container.xml".to_owned(),
            container().into_bytes(),
        ),
        ("designmap.xml".to_owned(), writer.designmap().into_bytes()),
        (
            "Resources/Graphic.xml".to_owned(),
            writer.graphic().into_bytes(),
        ),
        (
            "Resources/Preferences.xml".to_owned(),
            writer.preferences().into_bytes(),
        ),
    ];
    for master in &document.masters {
        parts.push((
            format!("MasterSpreads/MasterSpread_u{:x}.xml", master.uid),
            writer.spread(master, "MasterSpread").into_bytes(),
        ));
    }
    for spread in &document.spreads {
        parts.push((
            format!("Spreads/Spread_u{:x}.xml", spread.uid),
            writer.spread(spread, "Spread").into_bytes(),
        ));
    }
    for story in &document.stories {
        parts.push((
            format!("Stories/Story_u{:x}.xml", story.uid),
            writer.story(story).into_bytes(),
        ));
    }
    (parts, writer.skipped)
}

struct Writer<'a> {
    document: &'a Document,
    skipped: Vec<String>,
}

fn container() -> String {
    format!(
        r#"{DECLARATION}<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="designmap.xml" media-type="text/xml" /></rootfiles></container>"#
    )
}

/// Text for an attribute or element: escaped, and without the control
/// characters XML 1.0 cannot carry.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push('\t'),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

fn numbers(values: &[f64]) -> String {
    values
        .iter()
        .map(|v| format!("{v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn matrix(m: &Matrix) -> String {
    numbers(m)
}

impl Writer<'_> {
    fn designmap(&self) -> String {
        let mut out = format!(
            r#"{DECLARATION}<Document xmlns:idPkg="{PACKAGING}" DOMVersion="{DOM_VERSION}" Self="d"><idPkg:Graphic src="Resources/Graphic.xml" /><idPkg:Preferences src="Resources/Preferences.xml" />"#
        );
        for layer in &self.document.layers {
            let _ = write!(
                out,
                r#"<Layer Self="u{:x}" Name="{}" Visible="{}" Locked="false" />"#,
                layer.uid,
                escape(&layer.name),
                layer.visible
            );
        }
        for master in &self.document.masters {
            let _ = write!(
                out,
                r#"<idPkg:MasterSpread src="MasterSpreads/MasterSpread_u{:x}.xml" />"#,
                master.uid
            );
        }
        for spread in &self.document.spreads {
            let _ = write!(
                out,
                r#"<idPkg:Spread src="Spreads/Spread_u{:x}.xml" />"#,
                spread.uid
            );
        }
        for story in &self.document.stories {
            let _ = write!(
                out,
                r#"<idPkg:Story src="Stories/Story_u{:x}.xml" />"#,
                story.uid
            );
        }
        let first = self
            .document
            .spreads
            .iter()
            .flat_map(|s| &s.pages)
            .map(|p| p.uid)
            .next();
        for section in &self.document.sections {
            let Some(start) = section.start.or(first) else {
                continue;
            };
            let _ = write!(
                out,
                r#"<Section Self="u{:x}" PageStart="u{start:x}" ContinueNumbering="{}""#,
                section.uid, section.continues
            );
            if !section.continues {
                let _ = write!(out, r#" PageNumberStart="{}""#, section.number);
            }
            out.push_str(" />");
        }
        out.push_str("</Document>");
        out
    }

    fn graphic(&self) -> String {
        let mut out = format!(
            r#"{DECLARATION}<idPkg:Graphic xmlns:idPkg="{PACKAGING}" DOMVersion="{DOM_VERSION}">"#
        );
        let used = used_fills(self.document);
        for swatch in &self.document.swatches {
            if swatch.name.is_empty() && !used.contains(&swatch.uid) {
                continue;
            }
            let model = match swatch.model {
                ColorModel::Process => "Process",
                ColorModel::Spot => "Spot",
                ColorModel::Registration => "Registration",
            };
            let (space, value) = match &swatch.value {
                ColorValue::Cmyk(v) => ("CMYK", numbers(v)),
                ColorValue::Rgb(v) => ("RGB", numbers(v)),
            };
            let _ = write!(
                out,
                r#"<Color Self="Color/u{:x}" Model="{model}" Space="{space}" ColorValue="{value}" Name="{}" />"#,
                swatch.uid,
                escape(&swatch.name)
            );
        }
        out.push_str("</idPkg:Graphic>");
        out
    }

    fn preferences(&self) -> String {
        let mut out = format!(
            r#"{DECLARATION}<idPkg:Preferences xmlns:idPkg="{PACKAGING}" DOMVersion="{DOM_VERSION}"><DocumentPreference"#
        );
        if let Some(facing) = self.document.facing {
            let _ = write!(out, r#" FacingPages="{facing}""#);
        }
        if let Some(bleed) = self.document.bleed {
            let _ = write!(
                out,
                r#" DocumentBleedUniformSize="true" DocumentBleedTopOffset="{bleed}" DocumentBleedBottomOffset="{bleed}" DocumentBleedInsideOrLeftOffset="{bleed}" DocumentBleedOutsideOrRightOffset="{bleed}""#
            );
        }
        out.push_str(" /></idPkg:Preferences>");
        out
    }

    fn spread(&mut self, spread: &Spread, element: &str) -> String {
        let mut out = format!(
            r#"{DECLARATION}<idPkg:{element} xmlns:idPkg="{PACKAGING}" DOMVersion="{DOM_VERSION}"><{element} Self="u{:x}" ItemTransform="{}" PageCount="{}""#,
            spread.uid,
            matrix(&spread.transform),
            spread.pages.len()
        );
        if let Some(name) = &spread.master {
            let _ = write!(
                out,
                r#" NamePrefix="{}" BaseName="{}" Name="{}-{}""#,
                escape(&name.prefix),
                escape(&name.base),
                escape(&name.prefix),
                escape(&name.base)
            );
        }
        out.push('>');
        for page in &spread.pages {
            out.push_str(&self.page(page));
        }
        for item in &spread.items {
            self.item(&mut out, item);
        }
        let _ = write!(out, "</{element}></idPkg:{element}>");
        out
    }

    fn page(&self, page: &PageObject) -> String {
        let [left, top, right, bottom] = page.bounds;
        let mut out = format!(
            r#"<Page Self="u{:x}" Name="{}" GeometricBounds="{}" ItemTransform="{}""#,
            page.uid,
            escape(&page.name),
            numbers(&[top, left, bottom, right]),
            matrix(&page.transform)
        );
        match page.master {
            Some((master, transform)) if self.document.masters.iter().any(|m| m.uid == master) => {
                let _ = write!(
                    out,
                    r#" AppliedMaster="u{master:x}" MasterPageTransform="{}""#,
                    matrix(&transform)
                );
            }
            _ => out.push_str(r#" AppliedMaster="n""#),
        }
        out.push('>');
        if let Some([left, top, right, bottom]) = page.margins {
            let _ = write!(
                out,
                r#"<MarginPreference Top="{top}" Bottom="{bottom}" Left="{left}" Right="{right}" />"#
            );
        }
        out.push_str("</Page>");
        out
    }

    fn item(&mut self, out: &mut String, item: &Item) {
        let mut layer = item
            .layer
            .filter(|uid| self.document.layers.iter().any(|l| l.uid == *uid))
            .map(|uid| format!(r#" ItemLayer="u{uid:x}""#))
            .unwrap_or_default();
        if !item.visible {
            layer.push_str(r#" Visible="false""#);
        }
        match &item.kind {
            ItemKind::Group(children) => {
                let _ = write!(
                    out,
                    r#"<Group Self="u{:x}"{layer} ItemTransform="{}">"#,
                    item.uid,
                    matrix(&item.transform)
                );
                for child in children {
                    self.item(out, child);
                }
                out.push_str("</Group>");
            }
            ItemKind::Spline { paths, fill, frame } => {
                let story =
                    frame.filter(|f| self.document.stories.iter().any(|s| s.uid == f.story));
                let element = match (story, paths.as_slice()) {
                    (Some(_), _) => "TextFrame",
                    (None, [path]) if path.open && path.points.len() == 2 => "GraphicLine",
                    (None, [path]) if is_rectangle(path) => "Rectangle",
                    _ => "Polygon",
                };
                let _ = write!(
                    out,
                    r#"<{element} Self="u{:x}"{layer} ItemTransform="{}""#,
                    item.uid,
                    matrix(&item.transform)
                );
                if let Some(frame) = story {
                    let reference =
                        |uid: Option<u32>| uid.map(|u| format!("u{u:x}")).unwrap_or("n".into());
                    let _ = write!(
                        out,
                        r#" ParentStory="u{:x}" PreviousTextFrame="{}" NextTextFrame="{}" ContentType="TextType""#,
                        frame.story,
                        reference(frame.previous),
                        reference(frame.next)
                    );
                }
                match fill {
                    Some(uid) if self.document.swatches.iter().any(|s| s.uid == *uid) => {
                        let _ = write!(out, r#" FillColor="Color/u{uid:x}""#);
                    }
                    Some(uid) => {
                        self.skipped.push(schist_i18n::tf!(
                            "design.indd_fill_unread",
                            name = format!("u{:x}", item.uid),
                            swatch = format!("u{uid:x}")
                        ));
                    }
                    None => {}
                }
                out.push_str("><Properties><PathGeometry>");
                for path in paths {
                    let _ = write!(
                        out,
                        r#"<GeometryPathType PathOpen="{}"><PathPointArray>"#,
                        path.open
                    );
                    for [x, y] in &path.points {
                        let point = numbers(&[*x, *y]);
                        let _ = write!(
                            out,
                            r#"<PathPointType Anchor="{point}" LeftDirection="{point}" RightDirection="{point}" />"#
                        );
                    }
                    out.push_str("</PathPointArray></GeometryPathType>");
                }
                let _ = write!(out, "</PathGeometry></Properties></{element}>");
            }
        }
    }

    fn story(&mut self, story: &Story) -> String {
        let mut out = format!(
            r#"{DECLARATION}<idPkg:Story xmlns:idPkg="{PACKAGING}" DOMVersion="{DOM_VERSION}"><Story Self="u{:x}"><ParagraphStyleRange><CharacterStyleRange>"#,
            story.uid
        );
        // InDesign's stories end in a paragraph return that IDML leaves
        // implicit.
        let text = story.text.strip_suffix('\r').unwrap_or(&story.text);
        let mut dropped = std::collections::BTreeSet::new();
        let mut content = String::new();
        let flush = |out: &mut String, content: &mut String| {
            if !content.is_empty() {
                let _ = write!(out, "<Content>{}</Content>", escape(content));
                content.clear();
            }
        };
        for c in text.chars() {
            let instruction = match c {
                '\r' => {
                    flush(&mut out, &mut content);
                    out.push_str("<Br />");
                    continue;
                }
                '\n' | '\u{2028}' => {
                    content.push('\u{2028}');
                    continue;
                }
                '\t' => {
                    content.push('\t');
                    continue;
                }
                // Page number, section marker and end nested style, which
                // IDML writes as processing instructions.
                '\u{18}' => "ACE 18",
                '\u{19}' => "ACE 19",
                '\u{3}' => "ACE 3",
                // Zero-width markers anchoring hyperlinks and index
                // entries, which hold no text of their own.
                '\u{feff}' => continue,
                c if (c as u32) < 0x20 || c == '\u{fffc}' => {
                    dropped.insert(c as u32);
                    continue;
                }
                c => {
                    content.push(c);
                    continue;
                }
            };
            flush(&mut out, &mut content);
            let _ = write!(out, "<Content><?{instruction}?></Content>");
        }
        flush(&mut out, &mut content);
        out.push_str("</CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>");
        if !dropped.is_empty() {
            self.skipped.push(schist_i18n::tf!(
                "design.indd_special_characters",
                name = format!("u{:x}", story.uid),
                characters = dropped
                    .iter()
                    .map(|c| format!("U+{c:04X}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out
    }
}

/// Every swatch an item fills with.
fn used_fills(document: &Document) -> std::collections::HashSet<u32> {
    fn walk(items: &[Item], out: &mut std::collections::HashSet<u32>) {
        for item in items {
            match &item.kind {
                ItemKind::Group(children) => walk(children, out),
                ItemKind::Spline { fill, .. } => out.extend(*fill),
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    for spread in document.spreads.iter().chain(&document.masters) {
        walk(&spread.items, &mut out);
    }
    out
}

/// Four corners at right angles, in the order a rectangle is drawn.
fn is_rectangle(path: &Path) -> bool {
    if path.open || path.points.len() != 4 {
        return false;
    }
    let p = &path.points;
    (0..4).all(|i| {
        let (a, b) = (p[i], p[(i + 1) % 4]);
        (a[0] - b[0]).abs() < 1e-9 || (a[1] - b[1]).abs() < 1e-9
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Story, TextFrame};

    fn document() -> Document {
        Document {
            stories: vec![Story {
                uid: 0xd7,
                text: "One & two\r\u{18}\tthree\r".into(),
            }],
            ..Document::default()
        }
    }

    #[test]
    fn story_text_becomes_contents_breaks_and_instructions() {
        let document = document();
        let (parts, skipped) = parts(&document);
        let story = parts
            .iter()
            .find(|(name, _)| name == "Stories/Story_ud7.xml")
            .map(|(_, bytes)| String::from_utf8(bytes.clone()).unwrap())
            .unwrap();
        assert!(story.contains(
            "<Content>One &amp; two</Content><Br /><Content><?ACE 18?></Content><Content>\tthree</Content></CharacterStyleRange>"
        ));
        assert!(skipped.is_empty());
    }

    #[test]
    fn unknown_control_characters_are_reported_not_written() {
        let mut document = document();
        document.stories[0].text = "a\u{4}b\u{fffc}c".into();
        let (parts, skipped) = parts(&document);
        let story = String::from_utf8(parts.last().unwrap().1.clone()).unwrap();
        assert!(story.contains("<Content>abc</Content>"));
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("U+0004") && skipped[0].contains("U+FFFC"));
    }

    #[test]
    fn a_frame_whose_story_is_missing_is_written_as_a_shape() {
        let mut document = Document::default();
        document.spreads.push(Spread {
            uid: 0xc7,
            transform: crate::model::IDENTITY,
            master: None,
            pages: Vec::new(),
            items: vec![Item {
                uid: 0xe9,
                layer: None,
                transform: crate::model::IDENTITY,
                visible: true,
                kind: ItemKind::Spline {
                    paths: vec![Path {
                        points: vec![[0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0]],
                        open: false,
                    }],
                    fill: None,
                    frame: Some(TextFrame {
                        story: 0x99,
                        previous: None,
                        next: None,
                    }),
                },
            }],
        });
        let (parts, _) = parts(&document);
        let spread = String::from_utf8(parts.last().unwrap().1.clone()).unwrap();
        assert!(spread.contains(r#"<Rectangle Self="ue9""#));
        assert!(!spread.contains("ParentStory"));
    }
}
