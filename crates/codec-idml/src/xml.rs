//! A small XML tree, and the IDML conveniences built on it.
//!
//! IDML is XML with three habits that shape this parser:
//!
//! - **Almost everything is an attribute.** A page is nine attributes and
//!   no text, so an element has to keep its attributes and not just its
//!   content.
//! - **Nesting is shallow but real.** `ParagraphStyleRange` contains
//!   `CharacterStyleRange` contains `Content`, and that is three levels
//!   that matter. A streaming reader is not enough; a tree is.
//! - **The same name is used for two things.** `<Link>` is both an
//!   attribute (`ParentStory`) and an element. Nothing here assumes a name
//!   is unique.
//!
//! So: a tree, with attributes kept and text kept, and lookups written in
//! terms of what a caller actually wants ("the first child called X",
//! "the attribute called Y") rather than a general-purpose API.

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

/// One element in the tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    /// The element's name, namespace prefix included: `idPkg:Spread`.
    ///
    /// Prefixes are kept because IDML uses them meaningfully — the
    /// `idPkg:` namespace is how a part is listed, and a `Spread` element
    /// is a different thing from an `idPkg:Spread` reference to one.
    pub name: String,
    pub attributes: Vec<(String, String)>,
    /// Character data directly inside this element, concatenated.
    pub text: String,
    pub children: Vec<Element>,
    /// Processing instructions and their byte offsets in this element's direct
    /// decoded text. In particular, ACE 4 is a footnote marker, not a character.
    pub instructions: Vec<(usize, String)>,
    /// Exact XML for an outer unsupported story structure. The tree does not
    /// retain arbitrary mixed-content order, so it cannot reconstruct every
    /// native structure. Nested structures share this original payload.
    pub raw: Option<std::sync::Arc<str>>,
}

impl Element {
    /// An attribute's value, if present.
    ///
    /// A namespace prefix is ignored on lookup, because IDML writes
    /// `xlink:href` where a caller thinks of it as `href`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name || local(key) == name)
            .map(|(_, value)| value.as_str())
    }

    /// An attribute parsed as a number.
    pub fn number(&self, name: &str) -> Option<f32> {
        self.attr(name).and_then(parse_number)
    }

    /// An xsd:boolean attribute, retaining absent/invalid values as None.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        self.attr(name).and_then(parse_boolean)
    }

    /// An attribute's value, or the empty string.
    pub fn attr_or_empty(&self, name: &str) -> String {
        self.attr(name).unwrap_or_default().to_owned()
    }

    /// The first direct child with this name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.name == name)
    }

    /// Every direct child with this name.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// Every descendant with this name, at any depth.
    pub fn find_all(&self, name: &str) -> Vec<&Element> {
        let mut out = Vec::new();
        self.collect(name, &mut out);
        out
    }

    fn collect<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        for child in &self.children {
            if child.name == name {
                out.push(child);
            }
            child.collect(name, out);
        }
    }

    /// The first descendant with this name, at any depth.
    pub fn find(&self, name: &str) -> Option<&Element> {
        self.children
            .iter()
            .find(|child| child.name == name)
            .or_else(|| self.children.iter().find_map(|child| child.find(name)))
    }

    /// The whitespace of this element's text, trimmed.
    pub fn trimmed(&self) -> &str {
        self.text.trim()
    }
}

/// An element's local name, dropping any namespace prefix.
pub fn local(name: &str) -> &str {
    match name.rfind(':') {
        Some(at) => &name[at + 1..],
        None => name,
    }
}

/// Parse a whole document into its root element.
///
/// A document with more than one root is a parse error rather than a
/// silently-truncated one, because a part is one root by definition and
/// two roots means the file is not the file it claims to be.
pub fn parse(text: &str) -> Result<Element, String> {
    let mut roots = parse_all(text)?;
    match roots.len() {
        1 => Ok(roots.remove(0)),
        0 => Ok(Element::default()),
        _ => Err(format!("{} root elements, expected one", roots.len())),
    }
}

/// Native inline containers which this version retains without composing.
pub(crate) fn story_structure(name: &str) -> bool {
    matches!(
        name,
        "Table"
            | "Footnote"
            | "TextFrame"
            | "Rectangle"
            | "Polygon"
            | "Oval"
            | "GraphicLine"
            | "Group"
    )
}

/// Every element in a well-formed fragment, including exact opaque story data.
pub fn parse_all(text: &str) -> Result<Vec<Element>, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut stack: Vec<Element> = Vec::new();
    let mut starts: Vec<Option<usize>> = Vec::new();
    let mut roots = Vec::new();
    loop {
        let from = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buf);
        let empty = matches!(event, Ok(Event::Empty(_)));
        match event {
            Ok(Event::Start(start) | Event::Empty(start)) => {
                let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
                let to = reader.buffer_position() as usize;
                let capture = story_structure(&name)
                    && starts.iter().all(Option::is_none)
                    && (stack.is_empty() || stack.iter().any(|e| e.name == "Story"));
                let mut element = Element {
                    name,
                    attributes: attributes(&start),
                    ..Default::default()
                };
                if empty {
                    if capture {
                        element.raw = Some(text[from..to].into());
                    }
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(element),
                        None => roots.push(element),
                    }
                } else {
                    stack.push(element);
                    starts.push(capture.then_some(from));
                }
            }
            Ok(Event::End(_)) => {
                if let Some(mut element) = stack.pop() {
                    if let Some(from) = starts.pop().flatten() {
                        element.raw = Some(text[from..reader.buffer_position() as usize].into());
                    }
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(element),
                        None => roots.push(element),
                    }
                }
            }
            Ok(Event::Text(value)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&decode(&value));
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&resolve_reference(&reference));
                }
            }
            Ok(Event::CData(value)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(value.as_ref()));
                }
            }
            Ok(Event::PI(value)) => {
                if let Some(top) = stack.last_mut() {
                    top.instructions.push((
                        top.text.len(),
                        String::from_utf8_lossy(value.as_ref()).into_owned(),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(error.to_string()),
        }
        buf.clear();
    }
    if !stack.is_empty() {
        return Err(format!(
            "{} element(s) left unclosed, the document is truncated",
            stack.len()
        ));
    }
    Ok(roots)
}

/// A start tag's attributes.
fn attributes(start: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    start
        .attributes()
        .flatten()
        .map(|attribute| {
            let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
            // The value is unescaped here so a caller never sees `&amp;`
            // in a path, which is the one place it really matters. IDML
            // parts declare XML 1.0, and every specimen does.
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|value| value.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(&attribute.value).into_owned());
            (key, value)
        })
        .collect()
}

/// An entity reference, as the character it stands for.
///
/// quick-xml hands the reference over unresolved and separately from the
/// text either side of it, so this is where `&amp;` becomes `&`. Both
/// forms matter: the five predefined names, and a numeric character
/// reference, which IDML uses for a control character in a name.
fn resolve_reference(reference: &quick_xml::events::BytesRef<'_>) -> String {
    let name = String::from_utf8_lossy(reference.as_ref()).into_owned();
    match name.strip_prefix('#') {
        // `&#65;` and `&#x42;` are a code point in decimal or hex.
        Some(digits) => {
            let (digits, radix) = match digits
                .strip_prefix('x')
                .or_else(|| digits.strip_prefix('X'))
            {
                Some(hex) => (hex, 16),
                None => (digits, 10),
            };
            u32::from_str_radix(digits, radix)
                .ok()
                .and_then(char::from_u32)
                .map(String::from)
                // A reference to a character that does not exist is kept
                // literally rather than dropped, so nothing vanishes
                // silently.
                .unwrap_or_else(|| format!("&{name};"))
        }
        None => quick_xml::escape::resolve_predefined_entity(&name)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("&{name};")),
    }
}

/// A text node, with its entities resolved.
fn decode(value: &quick_xml::events::BytesText<'_>) -> String {
    value
        .xml10_content()
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| String::from_utf8_lossy(value.as_ref()).into_owned())
}

/// The public IDML schema uses xsd:boolean. Its whitespace facet collapses
/// XML whitespace and accepts exactly four case-sensitive lexical forms.
/// Do not use Unicode trim here: NBSP is not XML whitespace.
pub fn parse_boolean(value: &str) -> Option<bool> {
    match value.trim_matches([' ', '\t', '\r', '\n']) {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Parse a number the way IDML writes them.
///
/// Values are bare decimal numbers, sometimes with a unit suffix, and
/// occasionally an empty attribute. An empty or unparseable value is
/// `None` rather than zero, so a caller can tell "not stated" from
/// "stated as nothing" — a distinction that matters for indents, where
/// zero is a real and common value.
pub fn parse_number(value: &str) -> Option<f32> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    // Trailing units ("12pt") are not converted; the numeric prefix is
    // taken, which is right for a length and wrong for a colour. Colours
    // are read through `numbers` instead.
    let numeric: String = value
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        .collect();
    if numeric.is_empty() {
        return None;
    }
    numeric.parse().ok()
}

/// Every number in a whitespace-separated list, as `GeometricBounds` and
/// `ItemTransform` carry.
pub fn numbers(value: &str) -> Vec<f32> {
    value.split_whitespace().filter_map(parse_number).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_and_nesting_survive() {
        let root = parse(
            r#"<Story Self="u373">
                 <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Body">
                   <CharacterStyleRange FontStyle="Bold">
                     <Content>hello</Content>
                   </CharacterStyleRange>
                 </ParagraphStyleRange>
               </Story>"#,
        )
        .expect("parses");
        assert_eq!(root.name, "Story");
        assert_eq!(root.attr("Self"), Some("u373"));
        // Three levels, which is what a streaming reader cannot give a
        // caller and what the story reader needs.
        let paragraph = root.child("ParagraphStyleRange").expect("paragraph");
        assert_eq!(
            paragraph.attr("AppliedParagraphStyle"),
            Some("ParagraphStyle/$ID/Body")
        );
        let run = paragraph
            .child("CharacterStyleRange")
            .expect("character range");
        assert_eq!(run.attr("FontStyle"), Some("Bold"));
        assert_eq!(run.child("Content").map(|c| c.text.as_str()), Some("hello"));
    }

    #[test]
    fn an_empty_element_keeps_its_attributes() {
        // Most of IDML is empty elements carrying attributes, and an
        // empty element with no attributes would be indistinguishable from
        // nothing at all.
        let root = parse(r#"<Page Self="uf0" GeometricBounds="0 0 600 800" />"#).expect("parses");
        assert_eq!(root.attr("Self"), Some("uf0"));
        assert_eq!(root.attr("GeometricBounds"), Some("0 0 600 800"));
        assert!(root.children.is_empty());
    }

    #[test]
    fn text_keeps_its_entities_decoded() {
        // An entity arrives as its own event, so a reader that handles
        // only text loses every one of them. A frame named "Line & count"
        // would come back as "Line  count" with no error anywhere.
        let root = parse(
            r#"<a><Content>Limited line &amp; character count &lt;RT_f&gt; &apos;x&apos;</Content></a>"#,
        )
        .expect("parses");
        let content = root.child("Content").expect("content");
        assert_eq!(content.text, "Limited line & character count <RT_f> 'x'");
    }

    #[test]
    fn a_numeric_character_reference_resolves() {
        let root = parse("<a><Content>&#65;&#x42;</Content></a>").expect("parses");
        assert_eq!(root.child("Content").map(|c| c.text.as_str()), Some("AB"));
    }

    #[test]
    fn an_unresolvable_reference_is_kept_rather_than_dropped() {
        // Losing the character entirely would be worse than keeping
        // something that is visibly not resolved.
        let root = parse("<a><Content>&nosuch;</Content></a>").expect("parses");
        assert_eq!(
            root.child("Content").map(|c| c.text.as_str()),
            Some("&nosuch;")
        );
    }

    #[test]
    fn an_attribute_lookup_ignores_the_namespace_prefix() {
        let root = parse(r#"<a xlink:href="target" />"#).expect("parses");
        assert_eq!(root.attr("href"), Some("target"));
        assert_eq!(local("idPkg:Spread"), "Spread");
    }

    #[test]
    fn a_descendant_can_be_found_at_any_depth() {
        let root = parse(r#"<a><b><c><d Found="yes" /></c></b></a>"#).expect("parses");
        assert!(root.find("d").is_some());
        assert_eq!(root.find("d").and_then(|d| d.attr("Found")), Some("yes"));
        assert_eq!(root.find_all("d").len(), 1);
        assert!(root.find("missing").is_none());
    }

    #[test]
    fn a_mismatched_tag_is_an_error_rather_than_a_truncation() {
        assert!(parse("<a></b>").is_err());
        assert!(parse("<a>").is_err());
    }

    #[test]
    fn an_empty_document_is_not_an_error() {
        // A caller has to be able to ask about a part and get an answer
        // rather than an exception.
        assert!(parse("")
            .expect("an empty document parses")
            .children
            .is_empty());
        assert!(parse("<?xml version=\"1.0\"?>").is_ok());
    }

    #[test]
    fn numbers_parse_and_unparseable_values_do_not() {
        assert_eq!(numbers("0 0 600 800"), vec![0.0, 0.0, 600.0, 800.0]);
        assert_eq!(
            numbers("1 0 0 1 -400 -300"),
            vec![1.0, 0.0, 0.0, 1.0, -400.0, -300.0]
        );
        // A unit suffix takes the numeric prefix.
        assert_eq!(parse_number("12pt"), Some(12.0));
        assert_eq!(parse_number("-4.5"), Some(-4.5));
        // And an empty or non-numeric value is absent, not zero.
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("   "), None);
        assert_eq!(parse_number("none"), None);
        assert_eq!(parse_number("0"), Some(0.0), "zero is a real value");
    }

    #[test]
    fn a_declaration_is_not_a_root_element() {
        // Every part starts with `<?xml ...?>` and some with a processing
        // instruction, and neither is a document.
        let root =
            parse("<?xml version=\"1.0\" encoding=\"UTF-8\"?><?aid style=\"50\"?><Document/>")
                .expect("parses");
        assert_eq!(root.name, "Document");
    }
}
