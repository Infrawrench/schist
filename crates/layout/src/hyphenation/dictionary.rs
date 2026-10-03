//! Pattern selection is deliberately stricter than shaping-language selection.
//! A shaping tag may discard orthography; a dictionary must not do so.
use super::HyphenationOptions;
use crate::{language::TextLanguage, ResolvedCharacter, ResolvedParagraph, Story, StyleSet};
use std::ops::Range;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

/// Bundled, independently licensed pattern sets. This is not an alias for a
/// native vendor dictionary: native vendor identities remain in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dictionary {
    EnglishUs,
    French,
    GermanReformed,
}

impl Dictionary {
    /// Resolve opaque resource IDs before names. Explicit tags bypass resource
    /// IDs, just as they do for shaping. Unknown orthographies stay unsupported.
    pub fn resolve(styles: &StyleSet, language: &TextLanguage) -> Option<Self> {
        match language {
            TextLanguage::Tag { tag } => Self::tag(tag),
            TextLanguage::Reference(value) => {
                let resource = styles
                    .languages
                    .iter()
                    .find(|r| r.id == *value)
                    .or_else(|| styles.languages.iter().find(|r| r.name == *value));
                Self::name(resource.map_or(value, |r| r.name.as_str()))
            }
        }
    }

    fn name(name: &str) -> Option<Self> {
        match name.strip_prefix("$ID/").unwrap_or(name) {
            "English: USA" => Some(Self::EnglishUs),
            "French" => Some(Self::French),
            "German: Reformed" => Some(Self::GermanReformed),
            _ if name.starts_with("$ID/") => None,
            _ => Self::tag(name),
        }
    }

    fn tag(tag: &str) -> Option<Self> {
        // No region, variant or private-use fallback. In particular en-GB,
        // de-1901 and an explicit non-Latin script cannot select these tables.
        let tag = schist_text_engine::normalize_language(tag)?;
        match tag.as_str() {
            "en-us" | "en-latn-us" => Some(Self::EnglishUs),
            "fr" | "fr-fr" | "fr-latn" | "fr-latn-fr" => Some(Self::French),
            "de-1996" | "de-de-1996" | "de-latn-1996" | "de-latn-de-1996" => {
                Some(Self::GermanReformed)
            }
            _ => None,
        }
    }

    /// Optional UTF-8 boundaries in the original word, never in normalized
    /// lookup text. Words containing manual SHY, mixed scripts, numbers or
    /// punctuation are left to their explicit/source break rules.
    pub fn word_breaks(self, word: &str, policy: &HyphenationOptions) -> Vec<usize> {
        if policy.capitalized_words == Some(false)
            && word.chars().next().is_some_and(char::is_uppercase)
        {
            return Vec::new();
        }
        let mut normalized = String::new();
        let mut boundaries = Vec::new();
        for (at, grapheme) in word.grapheme_indices(true) {
            // One Latin letter with optional marks. This also refuses control
            // characters, joiners and mixed-script words.
            let mut chars = grapheme.chars();
            if chars
                .next()
                .is_none_or(|c| !c.is_alphabetic() || c.script() != Script::Latin)
                || !chars.all(is_combining_mark)
            {
                return Vec::new();
            }
            normalized.extend(grapheme.chars().flat_map(char::to_lowercase).nfc());
            boundaries.push((normalized.len(), at + grapheme.len()));
        }
        let count = boundaries.len();
        if count < usize::from(policy.words_longer_than.unwrap_or(5)) {
            return Vec::new();
        }
        let language = match self {
            Self::EnglishUs => hypher::Lang::English,
            Self::French => hypher::Lang::French,
            Self::GermanReformed => hypher::Lang::German,
        };
        let left = usize::from(policy.after_first.unwrap_or(2)).max(1);
        let right = usize::from(policy.before_last.unwrap_or(2)).max(1);
        let mut offset = 0;
        hypher::hyphenate_bounded(&normalized, language, 1, 1)
            .filter_map(|part| {
                offset += part.len();
                let index = boundaries.binary_search_by_key(&offset, |b| b.0).ok()?;
                let before = index + 1;
                (before >= left && count - before >= right).then_some(boundaries[index].1)
            })
            .collect()
    }
}

/// Word-level opportunities for a paragraph slice, relative to `range.start`.
/// `character` contains the paragraph defaults over the document fallback, as
/// produced by `ResolvedParagraph::character`. Local ranges override it.
/// Always inspect the complete source word, even if a previous frame consumed
/// its beginning. Normalization and dictionary lookup never edit the Story.
///
/// This selects opportunities only. Line/column policies (ladder, zone, weight,
/// across-columns) still belong to composition and must be applied there before
/// enabling automatic breaks. Manual U+00AD is independent of this function.
pub fn opportunities(
    story: &Story,
    range: Range<usize>,
    styles: &StyleSet,
    paragraph: &ResolvedParagraph,
    character: &ResolvedCharacter,
) -> Vec<usize> {
    if paragraph.hyphenate == Some(false) || range.start >= range.end {
        return Vec::new();
    }
    // Inspect the complete owning paragraph, even for a continuation slice.
    let context = story
        .points
        .iter()
        .zip(story.point_offsets())
        .find_map(|(point, start)| {
            let crate::StoryPoint::Paragraph { text, .. } = point else {
                return None;
            };
            (start <= range.start && range.start <= start + text.len())
                .then_some(start..start + text.len())
        })
        .unwrap_or_else(|| range.clone());
    let runs = crate::nested_styles::runs(story, styles, context, paragraph);
    let language_at = |byte| {
        runs.iter()
            .find(|r| r.start <= byte && byte < r.end)
            .and_then(|r| r.character.language.as_ref())
            .or(character.language.as_ref())
            .and_then(|language| Dictionary::resolve(styles, language))
    };
    let no_break_at = |byte| {
        runs.iter()
            .find(|r| r.start <= byte && byte < r.end)
            .and_then(|r| r.character.no_break)
            .or(character.no_break)
            .unwrap_or(false)
    };
    let mut result = Vec::new();
    for (point, from) in story.points.iter().zip(story.point_offsets()) {
        let crate::StoryPoint::Paragraph { text, .. } = point else {
            continue;
        };
        if from >= range.end || from + text.len() <= range.start {
            continue;
        }
        let words = text.unicode_word_indices().collect::<Vec<_>>();
        for (index, &(at, word)) in words.iter().enumerate() {
            let start = from + at;
            if start >= range.end
                || start + word.len() <= range.start
                // UAX word segmentation excludes a leading SHY from the word.
                // Native typography uses that prefix to prohibit automatic
                // hyphenation of the whole word, including continuation slices.
                || text[..at].ends_with('\u{ad}')
                || (paragraph.hyphenation.last_word == Some(false) && index + 1 == words.len())
            {
                continue;
            }
            let Some(dictionary) = language_at(start) else {
                continue;
            };
            if word
                .char_indices()
                .any(|(at, _)| language_at(start + at) != Some(dictionary))
            {
                continue;
            }
            for at in dictionary.word_breaks(word, &paragraph.hyphenation) {
                let byte = start + at;
                if byte > range.start
                    && byte < range.end
                    && !(no_break_at(byte - 1) && no_break_at(byte))
                {
                    result.push(byte - range.start);
                }
            }
        }
    }
    result
}
