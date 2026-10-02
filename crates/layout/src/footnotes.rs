//! Document footnote preferences, independent of frame and raster documents.
//!
//! Absent values retain native inheritance/default intent. These are stored
//! settings, not a claim that footnote bodies or markers are composed yet.
use crate::{decorations::DecorationStroke, History, Ink, LayoutDocument, LayoutEdit};
use serde::{Deserialize, Serialize};

/// A resolved resource travels with its definition (ink/stroke) or Schist style
/// name. Unresolved native identities remain explicit, never guessed from names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FootnoteReference<T> {
    None,
    Resolved(T),
    Unresolved(String),
}
impl<T> FootnoteReference<T> {
    pub fn resolved(&self) -> Option<&T> {
        match self {
            Self::Resolved(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FootnoteNumbering {
    Arabic,
    RomanUpper,
    RomanLower,
    LettersUpper,
    LettersLower,
    Symbols,
    Kanji,
    FullWidthArabic,
    SingleLeadingZeros,
    DoubleLeadingZeros,
    Asterisks,
    ArabicAlifBaTah,
    ArabicAbjad,
    HebrewBiblical,
    HebrewNonStandard,
    Other(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FootnoteRestart {
    Continuous,
    Page,
    Spread,
    Section,
    Other(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FootnoteAffixes {
    None,
    Reference,
    Note,
    Both,
    Other(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FootnoteMarkerPosition {
    Normal,
    Superscript,
    Subscript,
    Ruby,
    Other(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FootnoteFirstBaseline {
    Ascent,
    CapHeight,
    Leading,
    EmBox,
    XHeight,
    Fixed,
    Other(String),
}

/// The initial or continuing footnote separator. Distances are points; tints
/// are fractions, independently retained even when the rule is disabled.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FootnoteRule {
    pub on: Option<bool>,
    pub stroke: Option<FootnoteReference<DecorationStroke>>,
    pub paint: Option<FootnoteReference<Ink>>,
    pub gap_paint: Option<FootnoteReference<Ink>>,
    pub weight: Option<f32>,
    pub tint: Option<f32>,
    pub gap_tint: Option<f32>,
    pub overprint: Option<bool>,
    pub gap_overprint: Option<bool>,
    pub left_indent: Option<f32>,
    pub width: Option<f32>,
    pub offset: Option<f32>,
}
impl FootnoteRule {
    pub fn valid(&self) -> bool {
        finite_range(self.weight, 0.0, 1000.0)
            && finite_range(self.tint, 0.0, 1.0)
            && finite_range(self.gap_tint, 0.0, 1.0)
            && finite_range(self.left_indent, -103680.0, 103680.0)
            && finite_range(self.width, 0.0, 103680.0)
            && finite_range(self.offset, -15552.0, 15552.0)
            && self
                .stroke
                .as_ref()
                .and_then(FootnoteReference::resolved)
                .is_none_or(DecorationStroke::valid)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FootnoteOptions {
    pub start_at: Option<u32>,
    pub numbering: Option<FootnoteNumbering>,
    pub restart: Option<FootnoteRestart>,
    pub affixes: Option<FootnoteAffixes>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub text_style: Option<FootnoteReference<String>>,
    pub marker_style: Option<FootnoteReference<String>>,
    pub marker_position: Option<FootnoteMarkerPosition>,
    pub separator: Option<String>,
    /// Space between consecutive notes; paragraph spacing does not replace it.
    pub space_between: Option<f32>,
    /// Minimum gap between body text and the first note.
    pub spacer: Option<f32>,
    pub first_baseline: Option<FootnoteFirstBaseline>,
    pub minimum_first_baseline: Option<f32>,
    /// At story end, place notes below body text instead of at column bottom.
    pub end_of_story: Option<bool>,
    pub no_splitting: Option<bool>,
    /// Span the frame's columns, as opposed to one area per column.
    pub straddle: Option<bool>,
    pub rule: FootnoteRule,
    pub continuing_rule: FootnoteRule,
}
impl FootnoteOptions {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    /// Public preference bounds. Unknown resource/enumeration values remain
    /// representable for retention; composers must diagnose unsupported intent.
    pub fn valid(&self) -> bool {
        self.start_at.is_none_or(|v| (1..=100000).contains(&v))
            && [&self.prefix, &self.suffix, &self.separator]
                .into_iter()
                .all(|s| s.as_ref().is_none_or(|s| s.chars().count() <= 100))
            && finite_range(self.space_between, 0.0, 864.0)
            && finite_range(self.spacer, 0.0, 864.0)
            && finite_range(self.minimum_first_baseline, 0.0, 103680.0)
            && self.rule.valid()
            && self.continuing_rule.valid()
    }
}

fn finite_range(value: Option<f32>, min: f32, max: f32) -> bool {
    value.is_none_or(|v| v.is_finite() && (min..=max).contains(&v))
}

/// A committed settings gesture is one undo step; invalid and unchanged drafts
/// do not modify either the document or its history.
pub fn set_options(
    document: &mut LayoutDocument,
    history: &mut History,
    options: FootnoteOptions,
) -> bool {
    if !options.valid() || document.footnotes == options {
        return false;
    }
    let before = crate::snapshot_settings(document);
    let mut after = before.clone();
    after.footnotes = options;
    history.apply(
        document,
        LayoutEdit::DocumentChanged {
            before: Box::new(before),
            after: Box::new(after),
        },
    )
}
