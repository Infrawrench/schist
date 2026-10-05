//! Native custom-text and last-page-number definitions and guarded model
//! identities. Native XML is authoritative; document Labels preserve only
//! identities whose definition still agrees. Variable resources have no
//! per-resource Label in the schema.
use crate::{export::escape, xml::Element};
use schist_layout::{
    story::InlineControl,
    text_variables::{
        ChangeCase, ChapterNumber, LastPageNumber, MatchStyle, PageNumberFormat, RunningHeader,
        TextVariable, VariableKind, VariableScope,
    },
    Story, StoryStructure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const LABEL: &str = "Schist.CustomTextVariables.v1";

#[derive(Serialize, Deserialize)]
struct Saved {
    native: String,
    definition: TextVariable,
}

/// Only the published literal-string, last-page-number and chapter-number
/// subsets are lowered. Unknown preference properties, non-string Contents and
/// unrendered numbering formats stay in the existing recovery archive.
pub(crate) fn definition(element: &Element) -> Option<TextVariable> {
    if element.name != "TextVariable"
        || !plain(element, &["Self", "Name", "VariableType"])
        || element.children.len() != 1
    {
        return None;
    }
    match element.attr("VariableType") {
        Some("LastPageNumberType") => return last_page(element),
        Some("ChapterNumberType") => return chapter(element),
        Some("MatchParagraphStyleType") => return running_header(element, false),
        Some("MatchCharacterStyleType") => return running_header(element, true),
        _ => {}
    }
    if element.attr("VariableType") != Some("CustomTextType") {
        return None;
    }
    let preference = &element.children[0];
    if preference.name != "CustomTextVariablePreference"
        || !plain(preference, &[])
        || preference.children.len() != 1
    {
        return None;
    }
    let properties = &preference.children[0];
    if properties.name != "Properties" || !plain(properties, &[]) || properties.children.len() != 1
    {
        return None;
    }
    let contents = &properties.children[0];
    if contents.name != "Contents"
        || contents.attributes != [("type".into(), "string".into())]
        || !contents.children.is_empty()
        || !contents.instructions.is_empty()
    {
        return None;
    }
    let id = element.attr("Self")?.to_owned();
    if id.is_empty() {
        return None;
    }
    Some(TextVariable::custom(
        id,
        element.attr("Name")?,
        &contents.text,
    ))
}

const FORMATS: [(&str, PageNumberFormat); 6] = [
    ("Current", PageNumberFormat::Current),
    ("Arabic", PageNumberFormat::Arabic),
    ("UpperRoman", PageNumberFormat::UpperRoman),
    ("LowerRoman", PageNumberFormat::LowerRoman),
    ("UpperLetters", PageNumberFormat::UpperLetters),
    ("LowerLetters", PageNumberFormat::LowerLetters),
];
const SCOPES: [(&str, VariableScope); 2] = [
    ("DocumentScope", VariableScope::Document),
    ("SectionScope", VariableScope::Section),
];

/// PageNumberVariablePreference with an explicit Format and Scope. Their
/// omitted defaults are not published, so absent values remain recovery data.
/// Absent literal text is empty; the schema types it as an optional string.
fn last_page(element: &Element) -> Option<TextVariable> {
    let preference = &element.children[0];
    if preference.name != "PageNumberVariablePreference"
        || !plain(preference, &["TextBefore", "Format", "TextAfter", "Scope"])
        || !preference.children.is_empty()
    {
        return None;
    }
    let id = element.attr("Self").filter(|id| !id.is_empty())?;
    let definition = TextVariable::new(
        id,
        element.attr("Name")?,
        VariableKind::LastPage(LastPageNumber {
            before: preference.attr("TextBefore").unwrap_or_default().into(),
            format: lookup(preference.attr("Format"), &FORMATS)?,
            after: preference.attr("TextAfter").unwrap_or_default().into(),
            scope: lookup(preference.attr("Scope"), &SCOPES)?,
        }),
    );
    definition.valid().then_some(definition)
}

/// ChapterNumberVariablePreference with an explicit Format, as for page numbers.
fn chapter(element: &Element) -> Option<TextVariable> {
    let preference = &element.children[0];
    if preference.name != "ChapterNumberVariablePreference"
        || !plain(preference, &["TextBefore", "Format", "TextAfter"])
        || !preference.children.is_empty()
    {
        return None;
    }
    let id = element.attr("Self").filter(|id| !id.is_empty())?;
    let definition = TextVariable::new(
        id,
        element.attr("Name")?,
        VariableKind::Chapter(ChapterNumber {
            before: preference.attr("TextBefore").unwrap_or_default().into(),
            format: lookup(preference.attr("Format"), &FORMATS)?,
            after: preference.attr("TextAfter").unwrap_or_default().into(),
        }),
    );
    definition.valid().then_some(definition)
}

const CASES: [(&str, ChangeCase); 5] = [
    ("None", ChangeCase::None),
    ("Uppercase", ChangeCase::Upper),
    ("Lowercase", ChangeCase::Lower),
    ("Titlecase", ChangeCase::Title),
    ("Sentencecase", ChangeCase::Sentence),
];
const STRATEGIES: [(&str, bool); 2] = [("FirstOnPage", false), ("LastOnPage", true)];

/// Match…StylePreference with an explicit style, SearchStrategy, ChangeCase
/// and DeleteEndPunctuation, as InDesign writes them; their defaults are not
/// published. The style stays a native reference here and is named once the
/// package's styles are read ([`name_styles`]).
fn running_header(element: &Element, character: bool) -> Option<TextVariable> {
    let preference = &element.children[0];
    let (kind, attribute) = if character {
        ("MatchCharacterStylePreference", "AppliedCharacterStyle")
    } else {
        ("MatchParagraphStylePreference", "AppliedParagraphStyle")
    };
    if preference.name != kind
        || !plain(
            preference,
            &[
                "TextBefore",
                "TextAfter",
                attribute,
                "SearchStrategy",
                "ChangeCase",
                "DeleteEndPunctuation",
            ],
        )
        || !preference.children.is_empty()
    {
        return None;
    }
    let style = preference
        .attr(attribute)
        .filter(|s| !s.is_empty())?
        .to_owned();
    let id = element.attr("Self").filter(|id| !id.is_empty())?;
    let definition = TextVariable::new(
        id,
        element.attr("Name")?,
        VariableKind::RunningHeader(RunningHeader {
            before: preference.attr("TextBefore").unwrap_or_default().into(),
            after: preference.attr("TextAfter").unwrap_or_default().into(),
            style: if character {
                MatchStyle::Character(style)
            } else {
                MatchStyle::Paragraph(style)
            },
            last: lookup(preference.attr("SearchStrategy"), &STRATEGIES)?,
            case: lookup(preference.attr("ChangeCase"), &CASES)?,
            delete_end_punctuation: crate::xml::parse_boolean(
                preference.attr("DeleteEndPunctuation")?,
            )?,
        }),
    );
    definition.valid().then_some(definition)
}

/// Running headers name their style by native reference until the package's
/// styles are read; then by document name.
pub(crate) fn name_styles(definitions: &mut [TextVariable], refs: &crate::style_codec::References) {
    for header in definitions
        .iter_mut()
        .filter_map(|d| d.running_header.as_mut())
    {
        match &mut header.style {
            MatchStyle::Paragraph(style) => *style = refs.paragraph(style),
            MatchStyle::Character(style) => *style = refs.character(style),
        }
    }
}

/// The native references a saved definition's style names are written as.
fn style_references(definition: &mut TextVariable) {
    if let Some(header) = definition.running_header.as_mut() {
        match &mut header.style {
            MatchStyle::Paragraph(style) => {
                *style = crate::export::paragraph_reference_raw(style);
            }
            MatchStyle::Character(style) => {
                *style = crate::export::character_reference_raw(style);
            }
        }
    }
}

fn lookup<T: Copy>(value: Option<&str>, table: &[(&str, T)]) -> Option<T> {
    table
        .iter()
        .find(|(native, _)| Some(*native) == value)
        .map(|(_, typed)| *typed)
}

fn native<T: PartialEq + Copy>(table: &[(&'static str, T)], value: T) -> &'static str {
    table
        .iter()
        .find(|(_, typed)| *typed == value)
        .map(|(native, _)| *native)
        .expect("every typed value has a native spelling")
}

fn plain(element: &Element, attributes: &[&str]) -> bool {
    element.text.trim().is_empty()
        && element.instructions.is_empty()
        && element
            .attributes
            .iter()
            .all(|(key, _)| attributes.contains(&key.as_str()))
}

pub(crate) fn instance(element: &Element, character_style: &str) -> Option<InlineControl> {
    if element.name != "TextVariableInstance"
        || !plain(
            element,
            &["Self", "Name", "AssociatedTextVariable", "ResultText"],
        )
        || !element.children.is_empty()
    {
        return None;
    }
    let variable = element.attr("AssociatedTextVariable")?;
    if variable.is_empty() {
        return None;
    }
    Some(InlineControl::TextVariable {
        variable: variable.into(),
        character_style: character_style.into(),
        name: element.attr("Name").unwrap_or_default().into(),
    })
}

pub(crate) struct Imported {
    pub definitions: Vec<TextVariable>,
    /// Actual native identities to current model identities, independent of order.
    pub references: BTreeMap<String, String>,
    /// Invalid/duplicate metadata remains inert recovery data.
    pub recovery: Vec<String>,
}

pub(crate) fn read_package(
    opened: &crate::designmap::DesignPackage<'_>,
) -> Result<Imported, crate::error::Error> {
    let root = crate::xml::parse(opened.text_of(&opened.root)?).map_err(|message| {
        crate::error::Error::Xml {
            part: opened.root.clone(),
            message,
        }
    })?;
    // An authored identity must not capture a new unresolved native reference.
    let mut references = BTreeSet::new();
    let mut archived = crate::structured_story::VariableReferences::new();
    for part in opened.listed_of(crate::designmap::PartKind::Story) {
        if let Ok(text) = opened.text_of(&part.name) {
            if let Ok(story) = crate::xml::parse(text) {
                references.extend(
                    story
                        .find_all("TextVariableInstance")
                        .into_iter()
                        .filter_map(|e| e.attr("AssociatedTextVariable").map(str::to_owned)),
                );
                for native in story.find_all("Story") {
                    crate::structured_story::variable_references(native, &mut archived);
                }
            }
        }
    }
    Ok(read(&root, &references, &archived))
}

fn read(
    root: &Element,
    references: &BTreeSet<String>,
    archived: &crate::structured_story::VariableReferences,
) -> Imported {
    let mut out = Imported {
        definitions: Vec::new(),
        references: BTreeMap::new(),
        recovery: Vec::new(),
    };
    let native: Vec<_> = root.children_named("TextVariable").collect();
    let mut counts = BTreeMap::new();
    for id in native.iter().filter_map(|e| e.attr("Self")) {
        *counts.entry(id).or_insert(0usize) += 1;
    }
    let entries: Vec<_> = root
        .children_named("Properties")
        .flat_map(|p| p.children_named("Label"))
        .flat_map(|p| p.children_named("KeyValuePair"))
        .filter(|e| e.attr("Key") == Some(LABEL))
        .collect();
    let saved = (entries.len() == 1)
        .then(|| entries[0].attr("Value"))
        .flatten()
        .and_then(|v| serde_json::from_str::<Vec<Saved>>(v).ok())
        .filter(|records| {
            let mut native = BTreeSet::new();
            let mut model = BTreeSet::new();
            records.iter().all(|r| {
                !r.native.is_empty()
                    && !r.definition.id.is_empty()
                    && native.insert(&r.native)
                    && model.insert(&r.definition.id)
            })
        });
    if saved.is_none() {
        out.recovery.extend(entries.iter().map(|e| format!(
            r#"<Properties><Label><KeyValuePair Key="{LABEL}" Value="{}"/></Label></Properties>"#,
            escape(e.attr("Value").unwrap_or_default())
        )));
    }
    let saved = saved.unwrap_or_default();
    let mut used: BTreeSet<_> = counts.keys().map(|v| (*v).to_owned()).collect();
    let mut reserved = used.clone();
    reserved.extend(references.iter().cloned());
    reserved.extend(archived.keys().cloned());
    reserved.extend(saved.iter().map(|s| s.definition.id.clone()));
    let protected = |model: &str, native: &str| {
        archived
            .get(model)
            .is_some_and(|owners| owners.iter().any(|owner| owner.as_deref() != Some(native)))
    };
    for element in native {
        let Some(mut definition) = definition(element) else {
            continue;
        };
        if counts.get(definition.id.as_str()) != Some(&1) {
            continue;
        }
        let native_id = definition.id.clone();
        let matching: Vec<_> = saved.iter().filter(|s| s.native == native_id).collect();
        if let [record] = matching.as_slice() {
            let mut expected = record.definition.clone();
            expected.id.clone_from(&native_id);
            style_references(&mut expected);
            let original = &record.definition.id;
            if expected == definition
                && !original.is_empty()
                && !protected(original, &native_id)
                && (*original == native_id
                    || (!used.contains(original) && !references.contains(original)))
            {
                used.insert(original.clone());
                definition.id.clone_from(original);
            }
        }
        // Native edits can invalidate the saved identity. Its fallback spelling
        // may itself belong to an unresolved archived instance, so it needs the
        // same protection as an authored identity. Only actual native bindings
        // are remapped to this fresh identity during story restoration.
        if protected(&definition.id, &native_id) {
            let mut index = 0usize;
            loop {
                let candidate = format!("SchistVariableIdentity{index}");
                if reserved.insert(candidate.clone()) {
                    definition.id = candidate;
                    break;
                }
                index += 1;
            }
        }
        used.insert(definition.id.clone());
        reserved.insert(definition.id.clone());
        out.references.insert(native_id, definition.id.clone());
        out.definitions.push(definition);
    }
    out
}

pub(crate) struct Exported {
    pub bindings: BTreeMap<String, String>,
    definitions: Vec<TextVariable>,
}

impl Exported {
    pub fn new(definitions: &[TextVariable], warnings: &mut Vec<String>) -> Self {
        let mut out = Self {
            bindings: BTreeMap::new(),
            definitions: Vec::new(),
        };
        let mut counts = BTreeMap::new();
        for definition in definitions {
            *counts.entry(&definition.id).or_insert(0usize) += 1;
        }
        for definition in definitions {
            // An inconsistent kind is never written as some other native kind.
            if definition.id.is_empty()
                || counts[&definition.id] != 1
                || definition.kind().is_none()
            {
                super::text_variable_codec::notice(warnings);
                continue;
            }
            let id = format!("SchistTextVariable{}", out.definitions.len());
            out.bindings.insert(definition.id.clone(), id);
            out.definitions.push(definition.clone());
        }
        out
    }

    pub fn label(&self) -> String {
        if self.definitions.is_empty() {
            return String::new();
        }
        let saved: Vec<_> = self
            .definitions
            .iter()
            .map(|definition| Saved {
                native: self.bindings[&definition.id].clone(),
                definition: definition.clone(),
            })
            .collect();
        format!(
            r#"<KeyValuePair Key="{LABEL}" Value="{}"/>"#,
            escape(&serde_json::to_string(&saved).expect("custom variable identities"))
        )
    }

    pub fn resources(&self) -> String {
        self.definitions.iter().map(|definition| {
            let id = escape(&self.bindings[&definition.id]);
            let name = escape(&definition.name);
            match definition.kind() {
                Some(VariableKind::LastPage(page)) => format!(
                    r#"<TextVariable Self="{id}" Name="{name}" VariableType="LastPageNumberType"><PageNumberVariablePreference TextBefore="{}" Format="{}" TextAfter="{}" Scope="{}"/></TextVariable>"#,
                    escape(&page.before), native(&FORMATS, page.format), escape(&page.after), native(&SCOPES, page.scope)
                ),
                Some(VariableKind::Chapter(chapter)) => format!(
                    r#"<TextVariable Self="{id}" Name="{name}" VariableType="ChapterNumberType"><ChapterNumberVariablePreference TextBefore="{}" Format="{}" TextAfter="{}"/></TextVariable>"#,
                    escape(&chapter.before), native(&FORMATS, chapter.format), escape(&chapter.after)
                ),
                Some(VariableKind::RunningHeader(header)) => {
                    let (kind, preference, attribute, style) = match &header.style {
                        MatchStyle::Paragraph(style) => (
                            "MatchParagraphStyleType",
                            "MatchParagraphStylePreference",
                            "AppliedParagraphStyle",
                            crate::export::paragraph_reference_raw(style),
                        ),
                        MatchStyle::Character(style) => (
                            "MatchCharacterStyleType",
                            "MatchCharacterStylePreference",
                            "AppliedCharacterStyle",
                            crate::export::character_reference_raw(style),
                        ),
                    };
                    format!(
                        r#"<TextVariable Self="{id}" Name="{name}" VariableType="{kind}"><{preference} TextBefore="{}" TextAfter="{}" {attribute}="{}" SearchStrategy="{}" ChangeCase="{}" DeleteEndPunctuation="{}"/></TextVariable>"#,
                        escape(&header.before),
                        escape(&header.after),
                        escape(&style),
                        native(&STRATEGIES, header.last),
                        native(&CASES, header.case),
                        header.delete_end_punctuation
                    )
                }
                _ => format!(
                    r#"<TextVariable Self="{id}" Name="{name}" VariableType="CustomTextType"><CustomTextVariablePreference><Properties><Contents type="string">{}</Contents></Properties></CustomTextVariablePreference></TextVariable>"#,
                    escape(&definition.contents)
                ),
            }
        }).collect()
    }
}

pub(crate) fn remap(story: &mut Story, references: &BTreeMap<String, String>) {
    for structure in &mut story.structures {
        if let Some(InlineControl::TextVariable { variable, .. }) = &mut structure.control {
            if let Some(id) = references.get(variable) {
                variable.clone_from(id);
            }
        }
    }
}

/// Recovery-only instances from older saves are upgraded only after their native
/// story guard agrees. Their XML belongs to the codec, not the layout kernel.
pub(crate) fn upgrade(story: &mut Story, refs: &crate::style_codec::References) {
    for structure in &mut story.structures {
        let Some(mut control) = legacy_instance(structure) else {
            continue;
        };
        if let InlineControl::TextVariable {
            character_style, ..
        } = &mut control
        {
            *character_style = refs.character(character_style);
        }
        // These are original recovery IDs, not references in the current native
        // body. Never resolve their spelling through newly generated native IDs.
        structure.control = Some(control);
    }
}

pub(crate) fn legacy_instance(structure: &StoryStructure) -> Option<InlineControl> {
    if structure.kind != "TextVariableInstance"
        || structure.control.is_some()
        || structure.footnote.is_some()
    {
        return None;
    }
    let root = crate::xml::parse(&structure.payload).ok()?;
    if root.name != "ParagraphStyleRange" || root.children.len() != 1 {
        return None;
    }
    let character = &root.children[0];
    if character.name != "CharacterStyleRange" || character.children.len() != 1 {
        return None;
    }
    instance(
        &character.children[0],
        character.attr("AppliedCharacterStyle").unwrap_or_default(),
    )
}
