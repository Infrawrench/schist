//! Shared text-variable definitions. Instances refer to opaque identities rather
//! than names or vector positions. The kernel never interprets retained XML.
use crate::{
    story::InlineControl, History, LayoutDocument, LayoutEdit, Story, StoryId, StoryStructure,
};
use serde::{Deserialize, Serialize};

/// A custom text definition shared by every referring story instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextVariable {
    pub id: String,
    pub name: String,
    /// Literal custom contents, not an instance's cached ResultText.
    pub contents: String,
}

/// Literal display values cannot contain line breaks or directional controls.
/// Empty values and ordinary Unicode, including joiners, are valid.
pub fn valid_contents(contents: &str) -> bool {
    !contents.chars().any(|c| {
        c.is_control() || matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    })
}

fn unique<'a>(doc: &'a LayoutDocument, id: &str) -> Option<&'a TextVariable> {
    let mut definitions = doc
        .text_variables
        .iter()
        .filter(|d| !id.is_empty() && d.id == id);
    let definition = definitions.next()?;
    definitions.next().is_none().then_some(definition)
}

fn references(story: &Story, visit: &mut impl FnMut(&str)) {
    for structure in &story.structures {
        if let Some(InlineControl::TextVariable { variable, .. }) = &structure.control {
            visit(variable);
        }
        if let Some(note) = &structure.footnote {
            references(&note.story, visit);
        }
    }
}

/// Count typed references, including unplaced stories and note bodies. Opaque
/// recovery XML stays untouched; it is interpreted only at the codec boundary.
pub fn usage_count(doc: &LayoutDocument, id: &str) -> usize {
    let mut count = 0;
    for story in &doc.stories {
        references(story, &mut |variable| count += usize::from(variable == id));
    }
    count
}

fn change(doc: &mut LayoutDocument, history: &mut History, after: Vec<TextVariable>) -> bool {
    if doc.text_variables == after {
        return false;
    }
    let before = doc.text_variables.clone();
    history.apply(doc, LayoutEdit::TextVariablesChanged { before, after })
}

/// Create a distinct identity even when names repeat. Unresolved typed references
/// reserve their IDs so authoring cannot accidentally activate them. Opaque legacy
/// identities are separately protected by the IDML codec when restored.
pub fn create(
    doc: &mut LayoutDocument,
    history: &mut History,
    name: &str,
    contents: &str,
) -> Option<String> {
    if name.trim().is_empty() || name.chars().any(char::is_control) || !valid_contents(contents) {
        return None;
    }
    let mut reserved: std::collections::BTreeSet<String> =
        doc.text_variables.iter().map(|d| d.id.clone()).collect();
    for story in &doc.stories {
        references(story, &mut |id| {
            reserved.insert(id.to_owned());
        });
    }
    let id = (0usize..)
        .map(|i| format!("SchistCustom{i}"))
        .find(|id| !reserved.contains(id))?;
    let mut after = doc.text_variables.clone();
    after.push(TextVariable {
        id: id.clone(),
        name: name.to_owned(),
        contents: contents.to_owned(),
    });
    change(doc, history, after).then_some(id)
}

/// Commit both fields once against the captured definition. Editing one shared
/// resource changes every instance without copying or rewriting its stories.
pub fn update(
    doc: &mut LayoutDocument,
    history: &mut History,
    expected: &TextVariable,
    name: &str,
    contents: &str,
) -> bool {
    if unique(doc, &expected.id) != Some(expected)
        || name.trim().is_empty()
        || name.chars().any(char::is_control)
        || !valid_contents(contents)
    {
        return false;
    }
    let mut after = doc.text_variables.clone();
    let value = after
        .iter_mut()
        .find(|d| d.id == expected.id)
        .expect("unique definition");
    value.name = name.to_owned();
    value.contents = contents.to_owned();
    change(doc, history, after)
}

/// Remove an unused definition. Referenced definitions require explicit instance
/// removal first, so deleting a resource never silently deletes story content.
pub fn remove(doc: &mut LayoutDocument, history: &mut History, expected: &TextVariable) -> bool {
    if unique(doc, &expected.id) != Some(expected) || usage_count(doc, &expected.id) != 0 {
        return false;
    }
    let after = doc
        .text_variables
        .iter()
        .filter(|d| d.id != expected.id)
        .cloned()
        .collect();
    change(doc, history, after)
}

/// A source cursor captured by authoring UI. Equality guards indices as well as
/// byte positions: an external story edit cannot retarget a pending action.
#[derive(Clone, Debug, PartialEq)]
pub struct Cursor {
    pub story: StoryId,
    pub at: usize,
    pub expected: Story,
}

impl Cursor {
    pub fn capture(doc: &LayoutDocument, story: StoryId, at: usize) -> Option<Self> {
        let expected = doc.story(story)?;
        valid_anchor(expected, at).then(|| Self {
            story,
            at,
            expected: expected.clone(),
        })
    }

    pub fn valid(&self, doc: &LayoutDocument) -> bool {
        doc.story(self.story) == Some(&self.expected) && valid_anchor(&self.expected, self.at)
    }

    fn commit(&self, doc: &mut LayoutDocument, history: &mut History, after: Story) -> bool {
        if !self.valid(doc) || after == self.expected {
            return false;
        }
        history.apply(
            doc,
            LayoutEdit::StoryChanged {
                id: self.story.0,
                before: crate::edit::snapshot_story(&self.expected),
                after: crate::edit::snapshot_story(&after),
            },
        )
    }

    /// Insert after existing coincident structures, preserving their relative
    /// order. The surrounding character range supplies instance formatting;
    /// paragraph defaults remain inherited when no named range covers the caret.
    pub fn insert(
        &self,
        doc: &mut LayoutDocument,
        history: &mut History,
        expected: &TextVariable,
    ) -> bool {
        if !self.valid(doc)
            || unique(doc, &expected.id) != Some(expected)
            || !valid_contents(&expected.contents)
        {
            return false;
        }
        let character_style = self
            .expected
            .ranges
            .iter()
            .rev()
            .find(|r| r.start <= self.at && self.at < r.end)
            .map(|r| r.style.clone())
            .unwrap_or_default();
        let mut after = self.expected.clone();
        let index = after
            .structures
            .iter()
            .rposition(|s| s.at == Some(self.at))
            .map_or(after.structures.len(), |i| i + 1);
        after.structures.insert(
            index,
            StoryStructure {
                at: Some(self.at),
                kind: "TextVariableInstance".into(),
                payload: String::new(),
                footnote: None,
                control: Some(InlineControl::TextVariable {
                    variable: expected.id.clone(),
                    character_style,
                    name: expected.name.clone(),
                }),
            },
        );
        self.commit(doc, history, after)
    }

    /// Remove exactly the chosen main-story instance at this cursor, including
    /// unresolved references. Never guess between coincident zero-width objects.
    pub fn remove_instance(
        &self,
        doc: &mut LayoutDocument,
        history: &mut History,
        index: usize,
    ) -> bool {
        if !self.expected.structures.get(index).is_some_and(|s| {
            s.at == Some(self.at)
                && s.kind == "TextVariableInstance"
                && s.footnote.is_none()
                && matches!(s.control, Some(InlineControl::TextVariable { .. }))
        }) {
            return false;
        }
        let mut after = self.expected.clone();
        after.structures.remove(index);
        self.commit(doc, history, after)
    }
}

fn valid_anchor(story: &Story, at: usize) -> bool {
    paragraph(story, at).is_some_and(|(text, start, _)| {
        schist_text_engine::grapheme_boundaries(text).any(|offset| start + offset == at)
    })
}

/// Effective instance formatting, also used by package font inventories. An
/// instance inherits its paragraph, without footnote superscript preferences.
pub fn instance_character(
    doc: &crate::LayoutDocument,
    story: &crate::Story,
    at: usize,
    name: &str,
) -> Option<crate::ResolvedCharacter> {
    let (_, _, style) = paragraph(story, at)?;
    let paragraph = doc.styles.resolve_paragraph(if style.is_empty() {
        &doc.default_paragraph_style
    } else {
        style
    });
    let mut base = paragraph.character(doc.styles.resolve_character(&doc.default_character_style));
    base.point_size = paragraph.point_size.or(base.point_size);
    base.leading = paragraph.leading.or(base.leading);
    base.tracking = paragraph.tracking.or(base.tracking);
    Some(doc.styles.resolve_character(name).over(&base))
}

fn paragraph(story: &crate::Story, at: usize) -> Option<(&str, usize, &str)> {
    story
        .points
        .iter()
        .zip(story.point_offsets())
        .find_map(|(point, start)| {
            let crate::StoryPoint::Paragraph { text, style } = point else {
                return None;
            };
            (start <= at && at <= start + text.len() && text.is_char_boundary(at - start))
                .then_some((text.as_str(), start, style.as_str()))
        })
}

pub(crate) struct Instance {
    pub structure: usize,
    pub at: usize,
    pub text: String,
    pub character: crate::ResolvedCharacter,
}

/// Only supported, unambiguous references become display objects. Others stay
/// on the existing unrendered-structure diagnostic path, with their XML intact.
pub(crate) fn instances(doc: &crate::LayoutDocument, story: &crate::Story) -> Vec<Instance> {
    story
        .structures
        .iter()
        .enumerate()
        .filter_map(|(index, structure)| {
            if structure.kind != "TextVariableInstance" || structure.footnote.is_some() {
                return None;
            }
            let Some(crate::story::InlineControl::TextVariable {
                variable,
                character_style,
                ..
            }) = &structure.control
            else {
                return None;
            };
            let at = structure.at?;
            let (text, start, style) = paragraph(story, at)?;
            if !schist_text_engine::grapheme_boundaries(text).any(|offset| start + offset == at) {
                return None;
            }
            let paragraph = doc.styles.resolve_paragraph(if style.is_empty() {
                &doc.default_paragraph_style
            } else {
                style
            });
            // Initial and nested rules count a variable as one logical object.
            // Their source-only rule projection does not implement that yet.
            if (paragraph.drop_caps_lines.unwrap_or(0) > 0
                && paragraph.drop_caps_characters.unwrap_or(1) > 0)
                || paragraph
                    .nested_styles
                    .as_ref()
                    .is_some_and(|rules| !rules.is_empty())
            {
                return None;
            }
            let mut definitions = doc
                .text_variables
                .iter()
                .filter(|d| !d.id.is_empty() && d.id == *variable);
            let definition = definitions.next()?;
            if definitions.next().is_some() || !valid_contents(&definition.contents) {
                return None;
            }
            Some(Instance {
                structure: index,
                at,
                text: format!("\u{2068}{}\u{2069}", definition.contents),
                character: instance_character(doc, story, at, character_style)?,
            })
        })
        .collect()
}

pub(crate) fn slice_objects(
    objects: &[std::ops::Range<usize>],
    range: std::ops::Range<usize>,
) -> Vec<std::ops::Range<usize>> {
    objects
        .iter()
        .filter(|span| span.start >= range.start && span.end <= range.end)
        .map(|span| span.start - range.start..span.end - range.start)
        .collect()
}
