//! Temporary footnote flows. Original source text, styles and marker anchors
//! remain untouched; native interchange uses those originals, never these runs.
use crate::{
    footnotes::{
        FootnoteAffixes, FootnoteMarkerPosition, FootnoteNumbering, FootnoteReference,
        FootnoteRestart,
    },
    inline_text::{Insertion, Projection},
    list_numbering::CounterFormat,
    styles::TextPosition,
    LayoutDocument, ParagraphStyle, StoryId, StoryPoint, StyleSet,
};

#[derive(Debug, Clone, PartialEq)]
pub struct NoteArea {
    pub structure: usize,
    pub anchor: usize,
    pub bounds: crate::Rect,
    pub lines: Vec<crate::ComposedLine>,
    pub rule: Option<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub bounds: crate::Rect,
    pub ink: crate::Ink,
    pub tint: f32,
    pub overprint: bool,
}

impl Rule {
    pub fn path(&self) -> crate::ShapePath {
        crate::ShapePath {
            subpaths: vec![crate::SubPath {
                points: vec![
                    crate::Point::new(0.0, 0.0),
                    crate::Point::new(self.bounds.width, 0.0),
                    crate::Point::new(self.bounds.width, self.bounds.height),
                    crate::Point::new(0.0, self.bounds.height),
                ],
                closed: true,
                handles: Vec::new(),
            }],
            even_odd: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedNote {
    pub structure: usize,
    pub anchor: usize,
    /// Projected main-story range occupied by the reference number.
    pub reference: std::ops::Range<usize>,
    pub body: Projection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedStory {
    pub main: Projection,
    pub styles: StyleSet,
    pub notes: Vec<PreparedNote>,
}

/// Prepare continuous, text-only notes in source order. Layout-dependent
/// restarts and unknown anchors need a placement pass, not an assumed order.
/// This prepares real inline advances; it does not yet reserve a note area.
pub fn prepare(doc: &LayoutDocument, id: StoryId) -> Option<PreparedStory> {
    let source = doc.story(id)?;
    let options = &doc.footnotes;
    if !options.valid()
        || !matches!(options.restart, None | Some(FootnoteRestart::Continuous))
        || matches!(options.affixes, Some(FootnoteAffixes::Other(_)))
        || matches!(
            options.marker_position,
            Some(FootnoteMarkerPosition::Ruby | FootnoteMarkerPosition::Other(_))
        )
        || matches!(options.marker_style, Some(FootnoteReference::Unresolved(_)))
    {
        return None;
    }
    let text = source.text();
    let mut entries = Vec::new();
    for (index, structure) in source.structures.iter().enumerate() {
        if structure.kind != "Footnote" {
            continue;
        }
        let at = structure.at?;
        let body = structure.footnote.as_ref()?;
        if !text.is_char_boundary(at) || !body.valid() {
            return None;
        }
        entries.push((at, index, body));
    }
    if entries.is_empty() {
        return None;
    }
    entries.sort_by_key(|(at, _, _)| *at);
    let mut styles = doc.styles.clone();
    let mut insertions = Vec::new();
    let mut notes = Vec::new();
    for (offset, (at, structure, note)) in entries.into_iter().enumerate() {
        let number = u64::from(options.start_at.unwrap_or(1)).checked_add(offset as u64)?;
        let number = number_text(options.numbering.as_ref(), number)?;
        let affixed = format!(
            "{}{}{}",
            options.prefix.as_deref().unwrap_or(""),
            number,
            options.suffix.as_deref().unwrap_or("")
        );
        let reference = if matches!(
            options.affixes,
            Some(FootnoteAffixes::Reference | FootnoteAffixes::Both)
        ) {
            &affixed
        } else {
            &number
        };
        let marker = if matches!(
            options.affixes,
            Some(FootnoteAffixes::Note | FootnoteAffixes::Both)
        ) {
            &affixed
        } else {
            &number
        };
        let character = reference_character(doc, source, at, &note.reference_character_style)?;
        let name = unique_character(&styles, offset);
        styles.characters.push(character.into_style(&name));
        insertions.push(Insertion {
            at,
            text: reference.clone(),
            style: name,
        });
        let mut body = Projection::new(
            &note.story,
            note.markers
                .iter()
                .map(|m| Insertion {
                    at: m.at,
                    text: marker.clone(),
                    style: m.character_style.clone(),
                })
                .collect(),
        )?;
        // Existing note source already owns its separator characters. Only
        // ACE 4's zero-width instruction becomes a number, never literal digits.
        // Area spacing replaces the first paragraph's space-before and the
        // last paragraph's space-after. Internal paragraph spacing still applies.
        let mut aliases = std::collections::BTreeMap::new();
        let count = body.story.points.len();
        for (index, point) in body.story.points.iter_mut().enumerate() {
            let StoryPoint::Paragraph { style, .. } = point else {
                continue;
            };
            let first = index == 0;
            let last = index + 1 == count;
            if !first && !last {
                continue;
            }
            let alias = aliases
                .entry((style.clone(), first, last))
                .or_insert_with(|| {
                    let name = unique_paragraph(&styles, styles.paragraphs.len());
                    styles.paragraphs.push(ParagraphStyle {
                        name: name.clone(),
                        based_on: Some(style.clone()),
                        space_before: first.then_some(0.0),
                        space_after: last.then_some(0.0),
                        ..Default::default()
                    });
                    name
                });
            *style = alias.clone();
        }
        notes.push(PreparedNote {
            structure,
            anchor: at,
            reference: 0..0,
            body,
        });
    }
    let main = Projection::new(source, insertions)?;
    for (note, span) in notes.iter_mut().zip(&main.positions.generated) {
        note.reference = span.start..span.end;
    }
    Some(PreparedStory {
        main,
        styles,
        notes,
    })
}

/// The native reference's actual character context, also used for resource
/// inventories. Explicit character styles override the document position choice.
pub fn reference_character(
    doc: &LayoutDocument,
    source: &crate::Story,
    at: usize,
    name: &str,
) -> Option<crate::ResolvedCharacter> {
    if !source.text().is_char_boundary(at) {
        return None;
    }
    let paragraph_style =
        source
            .points
            .iter()
            .zip(source.point_offsets())
            .find_map(|(point, start)| {
                if let StoryPoint::Paragraph { text, style } = point {
                    (start <= at && at <= start + text.len()).then_some(style)
                } else {
                    None
                }
            })?;
    let paragraph = doc.styles.resolve_paragraph(paragraph_style);
    let mut base = paragraph.character(doc.styles.resolve_character(&doc.default_character_style));
    base.point_size = paragraph.point_size.or(base.point_size);
    base.leading = paragraph.leading.or(base.leading);
    base.tracking = paragraph.tracking.or(base.tracking);
    base.position = Some(match doc.footnotes.marker_position {
        Some(FootnoteMarkerPosition::Normal) => TextPosition::Normal,
        Some(FootnoteMarkerPosition::Subscript) => TextPosition::Subscript,
        _ => TextPosition::Superscript,
    });
    let explicit = |name: &str| {
        let mut style = doc.styles.resolve_character(name);
        if style.position.is_none()
            && matches!(
                style.baseline_shift,
                Some(
                    crate::styles::BaselineShift::Superscript
                        | crate::styles::BaselineShift::Subscript
                )
            )
        {
            style.position = Some(TextPosition::resolved(None, style.baseline_shift));
        }
        style
    };
    let mut character = explicit(name).over(&base);
    if let Some(FootnoteReference::Resolved(name)) = &doc.footnotes.marker_style {
        character = explicit(name).over(&character);
    }
    Some(character)
}

fn unique_character(styles: &StyleSet, index: usize) -> String {
    let mut name = format!("Schist generated footnote reference {index}");
    while styles.characters.iter().any(|s| s.name == name) {
        name.push('_');
    }
    name
}
fn unique_paragraph(styles: &StyleSet, index: usize) -> String {
    let mut name = format!("Schist generated footnote paragraph {index}");
    while styles.paragraphs.iter().any(|s| s.name == name) {
        name.push('_');
    }
    name
}

fn number_text(format: Option<&FootnoteNumbering>, number: u64) -> Option<String> {
    use FootnoteNumbering::*;
    let counter = match format.unwrap_or(&Arabic) {
        Arabic => CounterFormat::Decimal,
        RomanUpper => CounterFormat::UpperRoman,
        RomanLower => CounterFormat::LowerRoman,
        LettersUpper => CounterFormat::UpperLetters,
        LettersLower => CounterFormat::LowerLetters,
        SingleLeadingZeros => CounterFormat::SingleLeadingZeros,
        DoubleLeadingZeros => CounterFormat::DoubleLeadingZeros,
        FullWidthArabic => {
            return Some(
                number
                    .to_string()
                    .chars()
                    .map(|c| char::from_u32('０' as u32 + c as u32 - '0' as u32).unwrap())
                    .collect(),
            )
        }
        _ => return None,
    };
    counter.render(number).ok()
}
