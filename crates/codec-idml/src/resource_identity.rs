//! Opaque resource IDs share IDML's package-wide namespace with generated items.
//! Remap collisions on an export-only copy; guard restoration of authored IDs
//! against the emitted native resource. Recovery XML is never rewritten.
use schist_layout::{
    language::{LanguageResource, TextLanguage},
    lists::NumberingList,
    LayoutDocument,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const LABEL: &str = "Schist.ResourceIdentity.v1";

#[derive(Serialize, Deserialize, PartialEq)]
enum Kind {
    Language,
    NumberingList,
}

#[derive(Serialize, Deserialize)]
struct Saved<T> {
    kind: Kind,
    authored_id: String,
    native: T,
}

#[derive(Default)]
struct Remap {
    languages: BTreeMap<String, String>,
    lists: BTreeMap<String, String>,
}

impl Remap {
    fn apply(&self, doc: &mut LayoutDocument) {
        for language in doc
            .styles
            .paragraphs
            .iter_mut()
            .filter_map(|s| s.language.as_mut())
            .chain(
                doc.styles
                    .characters
                    .iter_mut()
                    .filter_map(|s| s.language.as_mut()),
            )
        {
            if let TextLanguage::Reference(value) = language {
                if let Some(replacement) = self.languages.get(value) {
                    *value = replacement.clone();
                }
            }
        }
        for list in doc
            .styles
            .paragraphs
            .iter_mut()
            .filter_map(|s| s.list.list.as_mut())
        {
            if let Some(replacement) = self.lists.get(list) {
                *list = replacement.clone();
            }
        }
    }
}

fn counts(parts: &[(String, Vec<u8>)]) -> BTreeMap<String, usize> {
    use quick_xml::{events::Event, Reader};
    let mut counts = BTreeMap::new();
    for (_, bytes) in parts.iter().filter(|(name, _)| name.ends_with(".xml")) {
        let mut reader = Reader::from_reader(bytes.as_slice());
        loop {
            match reader.read_event().expect("generated IDML XML") {
                Event::Start(element) | Event::Empty(element) => {
                    for attr in element.attributes() {
                        let attr = attr.expect("generated IDML attribute");
                        if attr.key.as_ref() == b"Self" {
                            let value = attr
                                .decoded_and_normalized_value(
                                    quick_xml::XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
                                .expect("generated identity");
                            *counts.entry(value.into_owned()).or_default() += 1;
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    counts
}

pub(crate) fn prepare(doc: &LayoutDocument, parts: &[(String, Vec<u8>)]) -> Option<LayoutDocument> {
    if doc.styles.languages.is_empty() && doc.styles.numbering_lists.is_empty() {
        return None;
    }
    let counts = counts(parts);
    let collides = |id: &str| counts.get(id).is_some_and(|n| *n > 1);
    if !doc.styles.languages.iter().any(|r| collides(&r.id))
        && !doc.styles.numbering_lists.iter().any(|r| collides(&r.id))
    {
        return None;
    }
    let mut used: BTreeSet<_> = counts.keys().cloned().collect();
    // A new identity must not intercept an existing name alias or unresolved
    // reference, including a language reference whose spelling resembles a tag.
    used.extend(doc.styles.languages.iter().map(|r| r.name.clone()));
    used.extend(doc.styles.numbering_lists.iter().map(|r| r.name.clone()));
    used.extend(
        doc.styles
            .paragraphs
            .iter()
            .filter_map(|s| s.language.as_ref())
            .chain(
                doc.styles
                    .characters
                    .iter()
                    .filter_map(|s| s.language.as_ref()),
            )
            .map(|v| v.as_str().to_owned()),
    );
    used.extend(
        doc.styles
            .paragraphs
            .iter()
            .filter_map(|s| s.list.list.clone()),
    );
    let mut next = 0u64;
    let mut allocate = || loop {
        let id = format!("SchistResourceIdentity{next}");
        next += 1;
        if used.insert(id.clone()) {
            break id;
        }
    };
    let mut out = doc.clone();
    let mut remap = Remap::default();
    for resource in &mut out.styles.languages {
        if counts.get(&resource.id).copied().unwrap_or(0) <= 1 {
            continue;
        }
        let old = resource.id.clone();
        resource.id = allocate();
        remap
            .languages
            .entry(old.clone())
            .or_insert_with(|| resource.id.clone());
        let saved = Saved {
            kind: Kind::Language,
            authored_id: old,
            native: resource.clone(),
        };
        resource.labels.push((
            LABEL.into(),
            serde_json::to_string(&saved).expect("language identity metadata"),
        ));
    }
    for resource in &mut out.styles.numbering_lists {
        if counts.get(&resource.id).copied().unwrap_or(0) <= 1 {
            continue;
        }
        let old = resource.id.clone();
        resource.id = allocate();
        remap
            .lists
            .entry(old.clone())
            .or_insert_with(|| resource.id.clone());
        let saved = Saved {
            kind: Kind::NumberingList,
            authored_id: old,
            native: resource.clone(),
        };
        resource.labels.push((
            LABEL.into(),
            serde_json::to_string(&saved).expect("list identity metadata"),
        ));
    }
    remap.apply(&mut out);
    Some(out)
}

fn saved<T: serde::de::DeserializeOwned>(
    labels: &[(String, String)],
    kind: Kind,
) -> Option<Saved<T>> {
    let mut entries = labels.iter().filter(|(key, _)| key == LABEL);
    let value = &entries.next()?.1;
    if entries.next().is_some() {
        return None;
    }
    // Resource structs default omitted fields and ignore unknown ones. Their
    // shared ID/name/labels alone cannot prove that the native class still agrees.
    serde_json::from_str::<Saved<T>>(value)
        .ok()
        .filter(|saved| saved.kind == kind)
}

pub(crate) fn restore(doc: &mut LayoutDocument) {
    let mut remap = Remap::default();
    // Restoring an ID must not make an externally added, currently unresolved
    // reference start targeting this resource. Export remapped every original
    // reference, so such an old spelling is not part of our emitted graph.
    let language_refs: BTreeSet<_> = doc
        .styles
        .paragraphs
        .iter()
        .filter_map(|s| s.language.as_ref())
        .chain(
            doc.styles
                .characters
                .iter()
                .filter_map(|s| s.language.as_ref()),
        )
        .filter_map(|value| match value {
            TextLanguage::Reference(value) => Some(value.clone()),
            _ => None,
        })
        .collect();
    let list_refs: BTreeSet<_> = doc
        .styles
        .paragraphs
        .iter()
        .filter_map(|s| s.list.list.clone())
        .collect();
    let mut languages: BTreeSet<_> = doc.styles.languages.iter().map(|r| r.id.clone()).collect();
    for resource in &mut doc.styles.languages {
        let Some(saved) = saved::<LanguageResource>(&resource.labels, Kind::Language) else {
            continue;
        };
        let mut native = resource.clone();
        native.labels.retain(|(key, _)| key != LABEL);
        if native != saved.native
            || saved.authored_id.is_empty()
            || languages.contains(&saved.authored_id)
            || language_refs.contains(&saved.authored_id)
        {
            continue;
        }
        languages.remove(&resource.id);
        languages.insert(saved.authored_id.clone());
        remap
            .languages
            .insert(resource.id.clone(), saved.authored_id.clone());
        native.id = saved.authored_id;
        *resource = native;
    }
    let mut lists: BTreeSet<_> = doc
        .styles
        .numbering_lists
        .iter()
        .map(|r| r.id.clone())
        .collect();
    for resource in &mut doc.styles.numbering_lists {
        let Some(saved) = saved::<NumberingList>(&resource.labels, Kind::NumberingList) else {
            continue;
        };
        let mut native = resource.clone();
        native.labels.retain(|(key, _)| key != LABEL);
        if native != saved.native
            || saved.authored_id.is_empty()
            || lists.contains(&saved.authored_id)
            || list_refs.contains(&saved.authored_id)
        {
            continue;
        }
        lists.remove(&resource.id);
        lists.insert(saved.authored_id.clone());
        remap
            .lists
            .insert(resource.id.clone(), saved.authored_id.clone());
        native.id = saved.authored_id;
        *resource = native;
    }
    remap.apply(doc);
}
