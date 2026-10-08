//! Public IDML schema 152 and example 92: typed, ordered nested-style records.
use crate::{export::escape, import::Report, style_codec::References, xml::Element};
use schist_layout::nested_styles::{CharacterStyle, Delimiter, NestedStyle};

fn invalid(report: &mut Report, key: &str, value: &str) {
    report.skip(schist_i18n::tf!(
        "design.idml_text_preference_invalid",
        property = key,
        value = value
    ));
}

fn record(element: &Element, refs: &References) -> Option<NestedStyle> {
    if element.name != "ListItem"
        || element.attr("type") != Some("record")
        || element.children.len() != 4
        || !element.text.trim().is_empty()
        || element
            .children
            .iter()
            .any(|child| !child.children.is_empty())
    {
        return None;
    }
    let field = |name, kind| {
        element
            .child(name)
            .filter(|child| child.attr("type") == Some(kind))
    };
    let reference = field("AppliedCharacterStyle", "object")?
        .text
        .trim_matches([' ', '\t', '\r', '\n']);
    let character_style = match reference {
        "CharacterStyle/$ID/[No character style]" | "$ID/[No character style]" => {
            CharacterStyle::None
        }
        _ => match refs.known_character(reference) {
            Some("[No character style]") => CharacterStyle::None,
            Some(name) => CharacterStyle::Named(name.to_owned()),
            None => CharacterStyle::Unresolved(reference.to_owned()),
        },
    };
    let delimiter = element.child("Delimiter")?;
    let delimiter = match delimiter.attr("type")? {
        "string" => Delimiter::Text(delimiter.text.clone()),
        "enumeration" => Delimiter::Enumeration(
            delimiter
                .text
                .trim_matches([' ', '\t', '\r', '\n'])
                .to_owned(),
        ),
        _ => return None,
    };
    let repetition = field("Repetition", "long")?
        .text
        .trim_matches([' ', '\t', '\r', '\n'])
        .parse()
        .ok()?;
    let inclusive = crate::xml::parse_boolean(&field("Inclusive", "boolean")?.text)?;
    Some(NestedStyle {
        character_style,
        delimiter,
        repetition,
        inclusive,
    })
}

pub(crate) fn read(
    element: &Element,
    refs: &References,
    report: &mut Report,
) -> Option<Vec<NestedStyle>> {
    // Native exports can clear a based-on list using only this attribute.
    // An omitted list alone inherits, so these two representations differ.
    let mut invalid_empty = false;
    let empty = element.attr("EmptyNestedStyles").and_then(|value| {
        let parsed = crate::xml::parse_boolean(value);
        if parsed.is_none() {
            invalid(report, "EmptyNestedStyles", value);
            invalid_empty = true;
        }
        parsed
    });
    let Some(list) = element
        .child("Properties")
        .and_then(|properties| properties.child("AllNestedStyles"))
    else {
        // A malformed explicit reset must not silently enable parent rules.
        return (empty == Some(true) || invalid_empty).then(Vec::new);
    };
    let parsed = (list.attr("type") == Some("list") && list.text.trim().is_empty())
        .then(|| {
            list.children
                .iter()
                .map(|child| record(child, refs))
                .collect::<Option<Vec<_>>>()
        })
        .flatten();
    match parsed {
        Some(rules) => {
            if empty.is_some_and(|empty| empty != rules.is_empty()) {
                invalid(
                    report,
                    "EmptyNestedStyles",
                    element.attr("EmptyNestedStyles").unwrap(),
                );
                // A native clear takes precedence over a conflicting list.
                // Report the contradiction instead of applying dormant rules.
                if empty == Some(true) {
                    return Some(Vec::new());
                }
            }
            for rule in &rules {
                if let CharacterStyle::Unresolved(reference) = &rule.character_style {
                    invalid(report, "AllNestedStyles.AppliedCharacterStyle", reference);
                }
            }
            Some(rules)
        }
        None => {
            // Never apply a truncated ordered list or silently inherit a parent
            // rule after rejecting a malformed local list.
            invalid(report, "AllNestedStyles", &list.text);
            Some(Vec::new())
        }
    }
}

pub(crate) fn attributes(out: &mut String, rules: &Option<Vec<NestedStyle>>) {
    if let Some(rules) = rules {
        out.push_str(if rules.is_empty() {
            " EmptyNestedStyles=\"true\""
        } else {
            " EmptyNestedStyles=\"false\""
        });
    }
}

pub(crate) fn properties(out: &mut String, rules: &Option<Vec<NestedStyle>>) {
    let Some(rules) = rules else {
        return;
    };
    // Match public native exports: an empty list uses the reset attribute,
    // not an empty AllNestedStyles element.
    if rules.is_empty() {
        return;
    }
    let mut body = String::from("<AllNestedStyles type=\"list\">");
    for rule in rules {
        let reference = match &rule.character_style {
            CharacterStyle::None => "CharacterStyle/$ID/[No character style]".into(),
            CharacterStyle::Named(name) => format!("CharacterStyle/$ID/{name}"),
            CharacterStyle::Unresolved(reference) => reference.clone(),
        };
        let (kind, value) = match &rule.delimiter {
            Delimiter::Text(value) => ("string", value),
            Delimiter::Enumeration(value) => ("enumeration", value),
        };
        body.push_str(&format!("<ListItem type=\"record\"><AppliedCharacterStyle type=\"object\">{}</AppliedCharacterStyle><Delimiter type=\"{kind}\">{}</Delimiter><Repetition type=\"long\">{}</Repetition><Inclusive type=\"boolean\">{}</Inclusive></ListItem>", escape(&reference), escape(value), rule.repetition, rule.inclusive));
    }
    body.push_str("</AllNestedStyles>");
    if let Some(at) = out.find("</Properties>") {
        out.insert_str(at, &body);
    }
}

pub(crate) fn warn(paragraph: &schist_layout::ResolvedParagraph, warnings: &mut Vec<String>) {
    if let Some(value) = schist_layout::nested_styles::unsupported(paragraph) {
        let message = schist_i18n::tf!("design.idml_nested_style_unsupported", value = value);
        if !warnings.contains(&message) {
            warnings.push(message);
        }
    }
}
