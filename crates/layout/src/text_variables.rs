//! Shared text-variable definitions. Instances refer to opaque identities rather
//! than names or vector positions. The kernel never interprets retained XML.
use crate::{
    story::InlineControl, History, LayoutDocument, LayoutEdit, Story, StoryId, StoryStructure,
};
use serde::{Deserialize, Serialize};

/// A text definition shared by every referring story instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextVariable {
    pub id: String,
    pub name: String,
    /// Literal custom contents, not an instance's cached ResultText. Empty for
    /// a computed definition.
    pub contents: String,
    /// A last-page-number definition, evaluated from page placement. Older
    /// snapshots have no field and remain literal custom text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_page: Option<LastPageNumber>,
    /// A chapter-number definition, evaluated from document chapter numbering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapter: Option<ChapterNumber>,
}

/// What a definition displays. The stored fields keep older snapshots readable;
/// authoring goes through this single choice so kinds cannot be combined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableKind {
    Custom(String),
    LastPage(LastPageNumber),
    Chapter(ChapterNumber),
}

impl TextVariable {
    pub fn custom(id: impl Into<String>, name: impl Into<String>, contents: &str) -> Self {
        Self::new(id, name, VariableKind::Custom(contents.into()))
    }

    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: VariableKind) -> Self {
        let mut out = Self {
            id: id.into(),
            name: name.into(),
            contents: String::new(),
            last_page: None,
            chapter: None,
        };
        match kind {
            VariableKind::Custom(contents) => out.contents = contents,
            VariableKind::LastPage(spec) => out.last_page = Some(spec),
            VariableKind::Chapter(spec) => out.chapter = Some(spec),
        }
        out
    }

    /// The single kind this definition stores, or None for an inconsistent
    /// record combining computed kinds or literal contents with one.
    pub fn kind(&self) -> Option<VariableKind> {
        match (&self.last_page, &self.chapter) {
            (None, None) => Some(VariableKind::Custom(self.contents.clone())),
            (Some(page), None) if self.contents.is_empty() => {
                Some(VariableKind::LastPage(page.clone()))
            }
            (None, Some(chapter)) if self.contents.is_empty() => {
                Some(VariableKind::Chapter(chapter.clone()))
            }
            _ => None,
        }
    }

    /// Literal parts must be displayable on one line. A computed definition
    /// has no literal contents of its own.
    pub fn valid(&self) -> bool {
        match self.kind() {
            Some(VariableKind::Custom(contents)) => valid_contents(&contents),
            Some(VariableKind::LastPage(page)) => {
                valid_contents(&page.before) && valid_contents(&page.after)
            }
            Some(VariableKind::Chapter(chapter)) => {
                valid_contents(&chapter.before) && valid_contents(&chapter.after)
            }
            None => false,
        }
    }
}

/// ChapterNumberVariablePreference: literal text around a formatted number.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChapterNumber {
    pub before: String,
    pub format: PageNumberFormat,
    pub after: String,
}

/// Where a document's chapter number comes from. Book-relative sources are
/// resolved by a book, which a standalone document does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChapterSource {
    UserDefined,
    ContinueFromPreviousDocument,
    SameAsPreviousDocument,
}

/// The native ChapterNumberPreference, retained exactly for interchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChapterNumbering {
    pub number: u32,
    pub source: ChapterSource,
    /// The native ChapterNumberFormat display string, such as "1, 2, 3, 4...".
    pub format: String,
}

/// The only ChapterNumberFormat spelling established by public files.
pub const ARABIC_CHAPTER_FORMAT: &str = "1, 2, 3, 4...";

/// A standalone document's chapter value. An absent preference is the
/// application default, chapter 1 in Arabic. A user-defined number is used as
/// is; book-relative sources agree on chapter 1 whether a standalone document
/// uses the stored number or restarts, and are not guessed for other numbers.
/// Current format needs a recognized document format spelling.
pub fn chapter_value(doc: &LayoutDocument, spec: &ChapterNumber) -> Option<String> {
    let (number, current) = match &doc.chapter_numbering {
        None => (1, Some(crate::NumberStyle::Arabic)),
        Some(numbering) => {
            let number = match numbering.source {
                ChapterSource::UserDefined => numbering.number,
                _ if numbering.number == 1 => 1,
                _ => return None,
            };
            (
                number,
                (numbering.format == ARABIC_CHAPTER_FORMAT).then_some(crate::NumberStyle::Arabic),
            )
        }
    };
    let style = match spec.format {
        PageNumberFormat::Current => current?,
        format => format.style(crate::NumberStyle::Arabic),
    };
    Some(format!(
        "{}{}{}",
        spec.before,
        style.format(number),
        spec.after
    ))
}

/// The rendered subset of the public VariableNumberingStyles enumeration.
/// Kanji, full-width and leading-zero formats remain codec recovery data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PageNumberFormat {
    /// The numbering style of the page that supplies the value.
    #[default]
    Current,
    Arabic,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
}

impl PageNumberFormat {
    pub const ALL: [Self; 6] = [
        Self::Current,
        Self::Arabic,
        Self::UpperRoman,
        Self::LowerRoman,
        Self::UpperLetters,
        Self::LowerLetters,
    ];

    fn style(self, current: crate::NumberStyle) -> crate::NumberStyle {
        use crate::NumberStyle as S;
        match self {
            Self::Current => current,
            Self::Arabic => S::Arabic,
            Self::UpperRoman => S::RomanUpper,
            Self::LowerRoman => S::RomanLower,
            Self::UpperLetters => S::AlphaUpper,
            Self::LowerLetters => S::AlphaLower,
        }
    }
}

/// Whether the last page is the document's or the instance's own section's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VariableScope {
    Document,
    /// The native default for a new last-page-number definition.
    #[default]
    Section,
}

/// PageNumberVariablePreference: literal text around a formatted page number.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LastPageNumber {
    pub before: String,
    pub format: PageNumberFormat,
    pub after: String,
    pub scope: VariableScope,
}

/// The value of a last-page-number definition for an instance that may occupy
/// any of `pages`. A section-scoped value is known only when every candidate
/// page belongs to one section; it is never guessed from the first frame.
/// A visible section prefix has no established native meaning here, so such
/// values stay unrendered rather than choosing whether to include it.
pub fn last_page_value(
    doc: &LayoutDocument,
    spec: &LastPageNumber,
    pages: &[usize],
) -> Option<String> {
    let count = doc.pages.len();
    let last = match spec.scope {
        VariableScope::Document => count.checked_sub(1)?,
        VariableScope::Section => {
            let first = single_section(doc, pages)?;
            (first + 1..count)
                .find(|&page| doc.pages[page].section.is_some())
                .map_or(count - 1, |next| next - 1)
        }
    };
    let (_, section) = doc.section_at(last);
    if section.include_prefix && !section.prefix.is_empty() {
        return None;
    }
    let number = spec
        .format
        .style(section.style)
        .format(doc.page_number_value(last));
    Some(format!("{}{number}{}", spec.before, spec.after))
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
    create_definition(doc, history, name, VariableKind::Custom(contents.into()))
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && !name.chars().any(char::is_control)
}

/// Create a definition of any kind in one undo step.
pub fn create_definition(
    doc: &mut LayoutDocument,
    history: &mut History,
    name: &str,
    kind: VariableKind,
) -> Option<String> {
    let mut definition = TextVariable::new(String::new(), name, kind);
    if !valid_name(name) || !definition.valid() {
        return None;
    }
    let mut reserved: std::collections::BTreeSet<String> =
        doc.text_variables.iter().map(|d| d.id.clone()).collect();
    for story in &doc.stories {
        references(story, &mut |id| {
            reserved.insert(id.to_owned());
        });
    }
    definition.id = (0usize..)
        .map(|i| format!("SchistCustom{i}"))
        .find(|id| !reserved.contains(id))?;
    let id = definition.id.clone();
    let mut after = doc.text_variables.clone();
    after.push(definition);
    change(doc, history, after).then_some(id)
}

/// Commit a custom name and literal value once against the captured
/// definition. Editing one shared resource changes every instance without
/// copying or rewriting its stories.
pub fn update(
    doc: &mut LayoutDocument,
    history: &mut History,
    expected: &TextVariable,
    name: &str,
    contents: &str,
) -> bool {
    update_definition(
        doc,
        history,
        expected,
        name,
        VariableKind::Custom(contents.into()),
    )
}

/// Replace a captured definition's name and kind together, keeping its identity
/// so every instance follows the shared resource.
pub fn update_definition(
    doc: &mut LayoutDocument,
    history: &mut History,
    expected: &TextVariable,
    name: &str,
    kind: VariableKind,
) -> bool {
    let replacement = TextVariable::new(expected.id.clone(), name, kind);
    if unique(doc, &expected.id) != Some(expected) || !valid_name(name) || !replacement.valid() {
        return false;
    }
    let mut after = doc.text_variables.clone();
    *after
        .iter_mut()
        .find(|d| d.id == expected.id)
        .expect("unique definition") = replacement;
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

    /// The surrounding character range supplies inserted formatting, as typed
    /// text would take it: the following character's range, or at the end of
    /// a run inside a paragraph the preceding character's. Paragraph defaults
    /// remain inherited when no named range applies.
    fn character_style(&self) -> String {
        let ranges = &self.expected.ranges;
        let following = ranges
            .iter()
            .rev()
            .find(|r| r.start <= self.at && self.at < r.end);
        let inside =
            paragraph(&self.expected, self.at).is_some_and(|(_, start, _)| self.at > start);
        following
            .or_else(|| {
                inside
                    .then(|| {
                        ranges
                            .iter()
                            .rev()
                            .find(|r| r.start < self.at && r.end == self.at)
                    })
                    .flatten()
            })
            .map(|r| r.style.clone())
            .unwrap_or_default()
    }

    /// Insert after existing coincident structures, preserving their order.
    fn insert_structure(
        &self,
        doc: &mut LayoutDocument,
        history: &mut History,
        kind: &str,
        control: InlineControl,
    ) -> bool {
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
                kind: kind.into(),
                payload: String::new(),
                footnote: None,
                control: Some(control),
            },
        );
        self.commit(doc, history, after)
    }

    /// Insert one instance of a shared definition.
    pub fn insert(
        &self,
        doc: &mut LayoutDocument,
        history: &mut History,
        expected: &TextVariable,
    ) -> bool {
        if !self.valid(doc) || unique(doc, &expected.id) != Some(expected) || !expected.valid() {
            return false;
        }
        let control = InlineControl::TextVariable {
            variable: expected.id.clone(),
            character_style: self.character_style(),
            name: expected.name.clone(),
        };
        self.insert_structure(doc, history, "TextVariableInstance", control)
    }

    /// Insert a current page number or a section marker. These are native
    /// zero-width instructions, not shared definitions.
    pub fn insert_marker(
        &self,
        doc: &mut LayoutDocument,
        history: &mut History,
        section: bool,
    ) -> bool {
        if !self.valid(doc) {
            return false;
        }
        let character_style = self.character_style();
        let control = if section {
            InlineControl::SectionMarker { character_style }
        } else {
            InlineControl::PageNumber {
                kind: crate::story::PageNumberKind::Current,
                character_style,
            }
        };
        self.insert_structure(doc, history, "ProcessingInstruction", control)
    }

    /// Remove exactly the chosen main-story instance or marker at this cursor,
    /// including unresolved references. Never guess between coincident
    /// zero-width objects.
    pub fn remove_instance(
        &self,
        doc: &mut LayoutDocument,
        history: &mut History,
        index: usize,
    ) -> bool {
        if !self
            .expected
            .structures
            .get(index)
            .is_some_and(|s| s.at == Some(self.at) && removable(s))
        {
            return false;
        }
        let mut after = self.expected.clone();
        after.structures.remove(index);
        self.commit(doc, history, after)
    }
}

/// Main-story variables and page/section markers the authoring window lists.
pub fn removable(structure: &StoryStructure) -> bool {
    structure.footnote.is_none()
        && matches!(
            (&structure.kind[..], &structure.control),
            (
                "TextVariableInstance",
                Some(InlineControl::TextVariable { .. })
            ) | (
                "ProcessingInstruction",
                Some(InlineControl::PageNumber { .. } | InlineControl::SectionMarker { .. })
            )
        )
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

/// The one page every candidate shares. Several pages are never narrowed to a
/// guess such as the first frame's page.
fn single_page(doc: &LayoutDocument, pages: &[usize]) -> Option<usize> {
    let (&first, rest) = pages.split_first()?;
    (first < doc.pages.len() && rest.iter().all(|&page| page == first)).then_some(first)
}

/// The boundary page of the one section every candidate shares.
fn single_section(doc: &LayoutDocument, pages: &[usize]) -> Option<usize> {
    let mut sections = pages
        .iter()
        .map(|&page| (page < doc.pages.len()).then(|| doc.section_at(page).0));
    let first = sections.next()??;
    sections
        .all(|section| section == Some(first))
        .then_some(first)
}

/// A current page-number marker shows its page's label, including a section
/// prefix only when the section asks for it, as the Pages panel does.
pub fn current_page_value(doc: &LayoutDocument, pages: &[usize]) -> Option<String> {
    let label = doc.page_number(single_page(doc, pages)?);
    valid_contents(&label).then_some(label)
}

/// A section marker shows its section's marker text, which may be empty.
pub fn section_marker_value(doc: &LayoutDocument, pages: &[usize]) -> Option<String> {
    let marker = doc.section_at(single_section(doc, pages)?).1.marker;
    valid_contents(&marker).then_some(marker)
}

/// Only supported, unambiguous references become display objects. Others stay
/// on the existing unrendered-structure diagnostic path, with their XML intact.
/// `pages` are every page this composition pass can place the story on.
pub(crate) fn instances(
    doc: &crate::LayoutDocument,
    story: &crate::Story,
    pages: &[usize],
) -> Vec<Instance> {
    use crate::story::{InlineControl, PageNumberKind};
    story
        .structures
        .iter()
        .enumerate()
        .filter_map(|(index, structure)| {
            if structure.footnote.is_some() {
                return None;
            }
            let (value, character_style) = match (structure.kind.as_str(), &structure.control) {
                (
                    "TextVariableInstance",
                    Some(InlineControl::TextVariable {
                        variable,
                        character_style,
                        ..
                    }),
                ) => {
                    let mut definitions = doc
                        .text_variables
                        .iter()
                        .filter(|d| !d.id.is_empty() && d.id == *variable);
                    let definition = definitions.next()?;
                    if definitions.next().is_some() || !definition.valid() {
                        return None;
                    }
                    let value = match definition.kind()? {
                        VariableKind::Custom(contents) => contents,
                        VariableKind::LastPage(spec) => last_page_value(doc, &spec, pages)?,
                        VariableKind::Chapter(spec) => chapter_value(doc, &spec)?,
                    };
                    (value, character_style)
                }
                (
                    "ProcessingInstruction",
                    Some(InlineControl::PageNumber {
                        kind: PageNumberKind::Current,
                        character_style,
                    }),
                ) => (current_page_value(doc, pages)?, character_style),
                (
                    "ProcessingInstruction",
                    Some(InlineControl::SectionMarker { character_style }),
                ) => (section_marker_value(doc, pages)?, character_style),
                _ => return None,
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
            // Initial and nested rules count an inline object as one logical
            // object. Their source-only rule projection does not implement that.
            if (paragraph.drop_caps_lines.unwrap_or(0) > 0
                && paragraph.drop_caps_characters.unwrap_or(1) > 0)
                || paragraph
                    .nested_styles
                    .as_ref()
                    .is_some_and(|rules| !rules.is_empty())
            {
                return None;
            }
            Some(Instance {
                structure: index,
                at,
                text: format!("\u{2068}{value}\u{2069}"),
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
