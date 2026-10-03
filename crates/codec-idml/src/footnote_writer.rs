//! Native text-only Footnote containers and supported zero-width controls.
//! Original XML remains in the guarded
//! retention record; it is never spliced into a new package with stale IDs.
use crate::export::{self, character_reference};
use schist_layout::{footnotes::FootnoteMarker, Story, StyleSet};

pub(crate) struct InlineEvent {
    at: usize,
    style: String,
    xml: String,
    emitted: bool,
    variable: bool,
}

pub(crate) fn events(
    story: &Story,
    markers: &[FootnoteMarker],
    styles: &StyleSet,
    variables: &std::collections::BTreeMap<String, String>,
    owner: &str,
    warnings: &mut Vec<String>,
) -> Vec<InlineEvent> {
    let text = story.text();
    let mut out: Vec<_> = markers
        .iter()
        .filter(|m| text.is_char_boundary(m.at))
        .map(|marker| InlineEvent {
            at: marker.at,
            style: marker.character_style.clone(),
            xml: "<Content><?ACE 4?></Content>".into(),
            emitted: false,
            variable: false,
        })
        .collect();
    for (index, structure) in story.structures.iter().enumerate() {
        if let Some((
            at,
            schist_layout::story::InlineControl::TextVariable {
                variable,
                character_style,
                name,
            },
        )) = structure.at.zip(structure.control.as_ref())
        {
            if structure.kind == "TextVariableInstance"
                && structure.footnote.is_none()
                && text.is_char_boundary(at)
            {
                if let Some(id) = variables.get(variable) {
                    out.push(InlineEvent {
                        at,
                        style: character_style.clone(),
                        xml: format!(r#"<TextVariableInstance Self="SchistVariableInstance{}_{}" Name="{}" AssociatedTextVariable="{}" ResultText=""/>"#, export::escape(owner), index, export::escape(name), export::escape(id)),
                        emitted: false,
                        variable: true,
                    });
                }
            }
            continue;
        }

        if let Some((at, schist_layout::story::InlineControl::EndNestedStyle { character_style })) =
            structure.at.zip(structure.control.as_ref())
        {
            if structure.kind == "ProcessingInstruction"
                && structure.footnote.is_none()
                && text.is_char_boundary(at)
            {
                out.push(InlineEvent {
                    at,
                    style: character_style.clone(),
                    xml: "<Content><?ACE 3?></Content>".into(),
                    emitted: false,
                    variable: false,
                });
            }
            continue;
        }
        let Some((at, note)) = structure.at.zip(structure.footnote.as_ref()) else {
            continue;
        };
        if structure.kind != "Footnote"
            || !text.is_char_boundary(at)
            || !note.valid()
            || note.story.prefs != Default::default()
        {
            continue;
        }
        let automatic = crate::auto_direction::lower(&note.story, styles);
        let xml = format!(
            "<Footnote>{}{}</Footnote>",
            crate::auto_direction::properties(&automatic),
            export::story_native_body(
                &note.story,
                &note.markers,
                styles,
                variables,
                &format!("{owner}_note{index}"),
                warnings
            )
        );
        out.push(InlineEvent {
            at,
            style: note.reference_character_style.clone(),
            xml,
            emitted: false,
            variable: false,
        });
    }
    // Stable order matters when several notes or markers share one byte anchor.
    out.sort_by_key(|event| event.at);
    out
}

/// Events split formatting runs without adding main-story bytes. A boundary
/// belongs to the next run, except at paragraph end; every event is emitted once.
pub(crate) fn paragraph_runs(
    out: &mut String,
    runs: &[(usize, usize, String)],
    text: &str,
    offset: usize,
    events: &mut [InlineEvent],
    paragraph_end: bool,
) {
    for (index, (start, end, style)) in runs.iter().enumerate() {
        let last = index + 1 == runs.len();
        let mut cursor = *start;
        for event in events.iter_mut().filter(|event| {
            !event.emitted
                && event.at >= offset + start
                && (event.at < offset + end || (last && event.at == offset + end))
        }) {
            let at = event.at - offset;
            if cursor < at {
                content(out, &text[cursor..at], style, false);
            }
            out.push_str(&format!(
                r#"<CharacterStyleRange AppliedCharacterStyle="{}"{}>{}</CharacterStyleRange>"#,
                character_reference(&event.style),
                if event.variable {
                    r#" PageNumberType="TextVariable""#
                } else {
                    ""
                },
                event.xml
            ));
            event.emitted = true;
            cursor = at;
        }
        // Preserve a final empty range too: its Br belongs to the paragraph,
        // after any zero-width note or marker at the end of the source text.
        if cursor < *end || last {
            content(out, &text[cursor..*end], style, paragraph_end && last);
        }
    }
}

fn content(out: &mut String, text: &str, style: &str, paragraph_end: bool) {
    out.push_str(&format!(
        r#"<CharacterStyleRange AppliedCharacterStyle="{}">"#,
        character_reference(style)
    ));
    if !text.is_empty() {
        // LF inside Content is a soft break; Br outside it ends a paragraph.
        out.push_str(&format!(
            "<Content>{}</Content>",
            export::escape(text).replace('\n', "&#10;")
        ));
    }
    if paragraph_end {
        out.push_str("<Br/>");
    }
    out.push_str("</CharacterStyleRange>");
}
