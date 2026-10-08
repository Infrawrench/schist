//! Schist chronology in standard object Labels. Native paint order is not
//! creation order; unlabelled imports must keep that distinction explicit.
use std::collections::BTreeSet;

use crate::{
    import::Report,
    xml::{self, Element},
};
use schist_layout::ObjectId;

const KEY: &str = "Schist.ObjectCreationOrder.v1";

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    native: String,
    kind: String,
    index: usize,
}

pub(crate) fn label(out: &mut String, index: Option<usize>) {
    let Some(index) = index else {
        return;
    };
    let Ok(element) = xml::parse(out) else {
        return;
    };
    let Some(native) = element.attr("Self") else {
        return;
    };
    let entry = Entry {
        native: native.into(),
        kind: element.name.clone(),
        index,
    };
    let value =
        serde_json::to_string(&entry).expect("creation metadata contains strings and an index");
    let pair = format!(
        r#"<KeyValuePair Key="{KEY}" Value="{}"/>"#,
        crate::style_codec::escape(&value)
    );
    // Our page-item encoders put their own Properties before nested content.
    // Inspect only those direct properties before inserting the metadata.
    if let Some(properties) = element.child("Properties") {
        if properties.child("Label").is_some() {
            let at = out
                .find("</Label>")
                .expect("encoded object label has a closing tag");
            out.insert_str(at, &pair);
        } else {
            let at = out
                .find("</Properties>")
                .expect("encoded object properties have a closing tag");
            out.insert_str(at, &format!("<Label>{pair}</Label>"));
        }
    } else if let Some(at) = out.rfind(&format!("</{}>", element.name)) {
        out.insert_str(
            at,
            &format!("<Properties><Label>{pair}</Label></Properties>"),
        );
    } else if out.ends_with("/>") {
        out.truncate(out.len() - 2);
        out.push_str(&format!(
            "><Properties><Label>{pair}</Label></Properties></{}>",
            element.name
        ));
    }
}

#[derive(Default)]
pub(crate) struct Reader {
    known: Vec<(usize, String, ObjectId)>,
    seen: BTreeSet<String>,
    duplicate: BTreeSet<String>,
    invalid: bool,
}

impl Reader {
    pub(crate) fn collect(&mut self, element: &Element, object: ObjectId) {
        let native = element.attr("Self").unwrap_or_default();
        if !self.seen.insert(native.into()) {
            self.duplicate.insert(native.into());
        }
        let Some(label) = element.child("Properties").and_then(|p| p.child("Label")) else {
            return;
        };
        let entries: Vec<_> = label
            .children
            .iter()
            .filter(|entry| entry.attr("Key") == Some(KEY))
            .collect();
        if entries.is_empty() {
            return;
        }
        let entry = (entries.len() == 1)
            .then(|| entries[0].attr("Value"))
            .flatten()
            .and_then(|value| serde_json::from_str::<Entry>(value).ok())
            .filter(|entry| {
                entry.native == native && !native.is_empty() && entry.kind == element.name
            });
        if let Some(entry) = entry {
            self.known.push((entry.index, entry.native, object));
        } else {
            self.invalid = true;
        }
    }

    pub(crate) fn finish(mut self, report: &mut Report) -> Vec<ObjectId> {
        self.known.sort_by_key(|entry| entry.0);
        let duplicate_indices: BTreeSet<_> = self
            .known
            .windows(2)
            .filter_map(|pair| (pair[0].0 == pair[1].0).then_some(pair[0].0))
            .collect();
        let mut order = Vec::new();
        for (index, native, object) in self.known {
            if duplicate_indices.contains(&index) || self.duplicate.contains(&native) {
                self.invalid = true;
            } else {
                order.push(object);
            }
        }
        if self.invalid {
            report.skip(schist_i18n::t("design.idml_creation_order"));
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(native: &str, kind: &str, index: usize) -> Element {
        let value = serde_json::to_string(&Entry {
            native: native.into(),
            kind: kind.into(),
            index,
        })
        .unwrap();
        xml::parse(&format!(r#"<TextFrame Self="{native}"><Properties><Label><KeyValuePair Key="{KEY}" Value="{}"/></Label></Properties></TextFrame>"#, crate::style_codec::escape(&value))).unwrap()
    }

    #[test]
    fn chronology_rejects_ambiguous_or_changed_identity_without_allocating_by_ordinal() {
        for case in 0..6 {
            let first = item("first", "TextFrame", 0);
            let mut second = item("second", "TextFrame", usize::MAX);
            let mut reader = Reader::default();
            let expected = match case {
                0 => vec![ObjectId(1), ObjectId(2)],
                1 => {
                    second
                        .attributes
                        .iter_mut()
                        .find(|(key, _)| key == "Self")
                        .unwrap()
                        .1 = "changed".into();
                    vec![ObjectId(1)]
                }
                2 => {
                    second.name = "Rectangle".into();
                    vec![ObjectId(1)]
                }
                3 => {
                    second = item("second", "TextFrame", 0);
                    vec![]
                }
                4 => {
                    reader.collect(
                        &xml::parse(r#"<TextFrame Self="second"/>"#).unwrap(),
                        ObjectId(3),
                    );
                    vec![ObjectId(1)]
                }
                5 => {
                    let label = second.children[0].children[0].children[0].clone();
                    second.children[0].children[0].children.push(label);
                    vec![ObjectId(1)]
                }
                _ => unreachable!(),
            };
            reader.collect(&first, ObjectId(1));
            reader.collect(&second, ObjectId(2));
            let mut report = Report::default();
            assert_eq!(reader.finish(&mut report), expected, "case {case}");
            assert_eq!(report.skipped.is_empty(), case == 0, "case {case}");
        }
    }
}
