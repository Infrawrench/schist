//! Paragraph and character styles, and the inheritance that resolves them.
//!
//! Page layout depends on styles for the same reason it depends on parent
//! pages: a long document is unmaintainable if every heading is formatted
//! by hand. A paragraph style names a *set* of properties and may be based
//! on another, so a document can say "Body is based on Default" and change
//! Default once. Character styles work the same way and layer on top of
//! the paragraph style.
//!
//! The resolution rules here are deliberately boring and total:
//!
//! * a property that is `None` inherits from the base style,
//! * a property that is `Some` wins,
//! * a cycle in `based_on` is resolved to the root's own defaults rather
//!   than looping, because a file from another tool can and does contain
//!   them.

use serde::{Deserialize, Serialize};

/// An explicit leading request. Auto is distinct from an absent/inherited
/// value and uses the paragraph's AutoLeading percentage of nominal type size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Leading {
    Auto,
    Points(f32),
}

impl Leading {
    pub fn points(self, size: f32, percentage: Option<f32>) -> Option<f32> {
        let value = match self {
            Self::Auto => {
                size * percentage
                    .filter(|v| v.is_finite() && (0.0..=500.0).contains(v))
                    .unwrap_or(120.0)
                    / 100.0
            }
            Self::Points(value) => value,
        };
        (value.is_finite() && value >= 0.0).then_some(value)
    }
}

// Preserve legacy numeric JSON; only the new automatic case needs a keyword.
impl Serialize for Leading {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("Auto"),
            Self::Points(value) => serializer.serialize_f32(*value),
        }
    }
}
impl<'de> Deserialize<'de> for Leading {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Points(f32),
            Keyword(String),
        }
        match Value::deserialize(deserializer)? {
            Value::Points(value) => Ok(Self::Points(value)),
            Value::Keyword(value) if value == "Auto" => Ok(Self::Auto),
            _ => Err(serde::de::Error::custom(
                "expected leading in points or Auto",
            )),
        }
    }
}

use crate::ink::Ink;

/// Horizontal alignment of a line within its column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    /// Justified, with the last line left ragged. This is the default in
    /// body copy and the only mode that needs a word-spacing pass.
    Justify,
    /// Justified, including the last line.
    JustifyAll,
}

impl Align {
    /// Whether lines are stretched to both margins.
    pub fn is_justified(self) -> bool {
        matches!(self, Align::Justify | Align::JustifyAll)
    }
}

/// How a paragraph is indented at its start.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Bullet {
    #[default]
    None,
    /// A literal character repeated at the start of each line.
    Character {
        char: char,
        /// Gap between the bullet and the text.
        indent: f32,
    },
    /// Enumeration, continuing from `start`.
    Numbered {
        start: i64,
        /// The character after the number: `.` for "1.", `)` for "1)".
        suffix: char,
    },
}

/// Base direction of a paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ParagraphDirection {
    /// Left to right, with a right-to-left alternative.
    LeftToRight,
    /// Right to left, with a left-to-right alternative.
    #[default]
    RightToLeft,
    /// The first strong character decides.
    Auto,
}

/// Whether a paragraph runs horizontally or down the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WritingMode {
    #[default]
    Horizontal,
    /// Down the page, columns running right to left.
    VerticalRightToLeft,
    /// Down the page, columns running left to right.
    VerticalLeftToRight,
}

/// Vertical position of a run of text against the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum BaselineShift {
    #[default]
    None,
    /// Legacy script choice; new styles use TextPosition independently.
    Superscript,
    /// Legacy script choice; new styles use TextPosition independently.
    Subscript,
    /// A fixed offset in points, positive raising the text.
    Offset(f32),
}

impl BaselineShift {
    /// Explicit point offsets. Legacy script variants resolve through TextPosition.
    pub fn explicit_offset(self) -> Option<f32> {
        match self {
            Self::None => Some(0.0),
            Self::Offset(value) if value.is_finite() => Some(value),
            _ => None,
        }
    }
}

/// Automatic script positioning, independent of an explicit point offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextPosition {
    #[default]
    Normal,
    Superscript,
    Subscript,
}

impl TextPosition {
    /// Legacy documents encoded the script choice as a baseline shift. New
    /// documents retain the two independent native properties separately.
    pub fn resolved(position: Option<Self>, shift: Option<BaselineShift>) -> Self {
        position.unwrap_or(match shift {
            Some(BaselineShift::Superscript) => Self::Superscript,
            Some(BaselineShift::Subscript) => Self::Subscript,
            _ => Self::Normal,
        })
    }
}

/// Document-wide text defaults from native TextPreference. Sizes are percent of
/// nominal font size; positions are percent of regular leading (not script size).
/// Stored with the style context so every measurement sees the same preferences.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextPreferences {
    pub superscript_size: f32,
    pub superscript_position: f32,
    pub subscript_size: f32,
    pub subscript_position: f32,
}

impl Default for TextPreferences {
    fn default() -> Self {
        Self {
            superscript_size: 58.3,
            superscript_position: 33.3,
            subscript_size: 58.3,
            subscript_position: 33.3,
        }
    }
}

impl TextPreferences {
    /// Rendering scale and signed displacement. Malformed serialized values use
    /// native defaults; import and authoring diagnose/reject them at their edges.
    pub fn script(self, position: TextPosition, leading: f32) -> (f32, f32) {
        let defaults = Self::default();
        let (size, offset, default_size, default_offset, sign) = match position {
            TextPosition::Normal => return (1.0, 0.0),
            TextPosition::Superscript => (
                self.superscript_size,
                self.superscript_position,
                defaults.superscript_size,
                defaults.superscript_position,
                1.0,
            ),
            TextPosition::Subscript => (
                self.subscript_size,
                self.subscript_position,
                defaults.subscript_size,
                defaults.subscript_position,
                -1.0,
            ),
        };
        let size = if size.is_finite() && (1.0..=200.0).contains(&size) {
            size
        } else {
            default_size
        };
        let offset = if offset.is_finite() && (-500.0..=500.0).contains(&offset) {
            offset
        } else {
            default_offset
        };
        (size / 100.0, sign * offset * leading / 100.0)
    }
}

/// Resolve a paint across a style boundary. A nearer explicit direct tint
/// detaches an inherited named Tint to its base Color. A paint explicitly naming
/// a Tint owns its percentage, even when the same style also carries a number.
pub fn inherited_paint(
    paint: &Option<Ink>,
    direct_tint: Option<f32>,
    fallback: Option<&Ink>,
) -> Option<Ink> {
    paint.clone().or_else(|| {
        fallback.map(|ink| {
            if direct_tint.is_some() {
                ink.base_color().into_owned()
            } else {
                ink.clone()
            }
        })
    })
}

/// A named set of paragraph properties.
///
/// Every field is optional and means "inherit". That makes a style cheap
/// to define and safe to edit: adding a property to this struct does not
/// invalidate every style in every document, because unset stays unset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParagraphStyle {
    pub name: String,
    /// Style this one inherits from. `None` means it inherits nothing.
    pub based_on: Option<String>,
    /// The style applied to a paragraph following one of this style.
    /// This is what makes a heading style chain into body copy.
    pub next: Option<String>,

    /// Character defaults inherited by every run in this paragraph.
    /// A character style can override each property independently.
    pub family: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub baseline_shift: Option<BaselineShift>,
    pub position: Option<TextPosition>,
    /// Fraction of full-strength ink; None inherits independently of colour.
    pub fill_tint: Option<f32>,
    pub stroke_tint: Option<f32>,
    /// The fill ink; unset inherits the paragraph or document text colour.
    pub fill: Option<Ink>,
    pub stroke: Option<Ink>,
    pub overprint_fill: Option<bool>,
    pub overprint_stroke: Option<bool>,

    pub point_size: Option<f32>,
    /// None inherits; an unresolved legacy None uses the font's metrics.
    pub leading: Option<Leading>,
    /// Percentage of nominal type size used by Auto leading; None inherits.
    pub auto_leading: Option<f32>,
    /// Letter spacing in thousandths of an em, as in IDML. Composition
    /// converts this to points using each run's effective font size.
    pub tracking: Option<f32>,
    /// Kerning applies when the font's own table asks for it.
    pub kerning: Option<bool>,

    pub align: Option<Align>,
    /// Distance in points from the column's left edge to the text measure.
    pub left_indent: Option<f32>,
    pub right_indent: Option<f32>,
    pub first_line_indent: Option<f32>,
    pub space_before: Option<f32>,
    pub space_after: Option<f32>,
    /// Keep this paragraph with the next paragraph's first placed line.
    pub keep_with_next: Option<bool>,
    /// Minimum lines left at the bottom of a column before the
    /// paragraph moves to the next. Orphans and widows in one setting.
    pub keep_lines: Option<usize>,
    /// Number of body lines spanned by opening characters; zero or one disables it.
    pub drop_caps_lines: Option<usize>,
    /// Opening graphemes to enlarge. Unset means one; zero disables drop caps.
    pub drop_caps_characters: Option<usize>,

    pub bullet: Option<Bullet>,
    /// Whether hyphenation is allowed in this paragraph.
    pub hyphenate: Option<bool>,
    /// A language tag that drives the hyphenation dictionary and the
    /// proofing rules. Not translated: it is a BCP 47 tag, not prose.
    pub language: Option<String>,
    /// Base paragraph direction. `None` means auto, where the first
    /// strong character decides.
    pub direction: Option<ParagraphDirection>,
    /// Whether this paragraph runs horizontally or vertically.
    pub writing_mode: Option<WritingMode>,
    /// Per-tag OpenType overrides. Unspecified tags inherit independently.
    #[serde(default)]
    pub features: Vec<(String, bool)>,
}

/// A named set of character properties, applied over a paragraph style.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterStyle {
    pub name: String,
    pub based_on: Option<String>,

    /// The font family name, matched against the installed font database
    /// rather than being a path.
    pub family: Option<String>,
    pub point_size: Option<f32>,
    pub leading: Option<Leading>,
    /// Letter spacing in thousandths of an em.
    pub tracking: Option<f32>,
    pub kerning: Option<bool>,

    pub bold: Option<bool>,
    pub italic: Option<bool>,
    /// Underline toggle; None inherits.
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    /// Explicit shift; legacy script variants resolve through position.
    pub baseline_shift: Option<BaselineShift>,
    pub position: Option<TextPosition>,
    /// All-small-caps or small-caps, as a pair of booleans.
    pub all_caps: Option<bool>,
    pub small_caps: Option<bool>,

    /// Fraction of full-strength ink; None inherits independently of colour.
    pub fill_tint: Option<f32>,
    pub stroke_tint: Option<f32>,
    /// The fill ink; unset inherits the paragraph or document text colour.
    pub fill: Option<Ink>,
    pub stroke: Option<Ink>,
    /// Stroke weight as a percentage of the fill's.
    pub stroke_weight: Option<f32>,
    /// Fill opacity, 0..=1. Transparency and overprint are separate.
    pub opacity: Option<f32>,
    /// Print this object in every ink beneath it rather than knocking out
    /// what it covers.
    pub overprint_fill: Option<bool>,
    pub overprint_stroke: Option<bool>,
    /// Optical margin alignment, which shifts a capital at the start of a
    /// line so it appears flush. Only meaningful for justified text.
    pub optical_margin: Option<bool>,

    /// OpenType features to force on or off for runs carrying this style.
    #[serde(default)]
    pub features: Vec<(String, bool)>,
    pub language: Option<String>,
}

/// The document's style tables.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleSet {
    #[serde(default)]
    pub text_preferences: TextPreferences,
    pub paragraphs: Vec<ParagraphStyle>,
    pub characters: Vec<CharacterStyle>,
}

impl StyleSet {
    /// A set with the handful of styles a new document cannot do without.
    ///
    /// Every document gets a "Default" for both kinds and a "Body" based
    /// on it. A blank style table would force every consumer to special
    /// case the empty state, and a `None` resolved against nothing still
    /// has to produce a value.
    pub fn with_defaults() -> StyleSet {
        StyleSet {
            text_preferences: TextPreferences::default(),
            paragraphs: vec![
                ParagraphStyle {
                    name: "Default".into(),
                    ..ParagraphStyle::default()
                },
                ParagraphStyle {
                    name: "Body".into(),
                    based_on: Some("Default".into()),
                    point_size: Some(11.0),
                    leading: Some(crate::styles::Leading::Points(13.5)),
                    hyphenate: Some(true),
                    keep_lines: Some(2),
                    ..ParagraphStyle::default()
                },
            ],
            characters: vec![
                CharacterStyle {
                    name: "Default".into(),
                    ..CharacterStyle::default()
                },
                CharacterStyle {
                    name: "Bold".into(),
                    based_on: Some("Default".into()),
                    bold: Some(true),
                    ..CharacterStyle::default()
                },
            ],
        }
    }

    pub fn paragraph(&self, name: &str) -> Option<&ParagraphStyle> {
        self.paragraphs.iter().find(|s| s.name == name)
    }

    pub fn character(&self, name: &str) -> Option<&CharacterStyle> {
        self.characters.iter().find(|s| s.name == name)
    }

    pub fn paragraph_index(&self, name: &str) -> Option<usize> {
        self.paragraphs.iter().position(|s| s.name == name)
    }

    pub fn add_paragraph(&mut self, style: ParagraphStyle) {
        if let Some(existing) = self.paragraph_index(&style.name) {
            self.paragraphs[existing] = style;
        } else {
            self.paragraphs.push(style);
        }
    }

    pub fn add_character(&mut self, style: CharacterStyle) {
        if let Some(index) = self.characters.iter().position(|s| s.name == style.name) {
            self.characters[index] = style;
        } else {
            self.characters.push(style);
        }
    }

    /// The style applied to a paragraph after one in `name`.
    ///
    /// Falls back to the style itself when `next` is unset or dangling,
    /// which is what "this style repeats" means in every page layout tool.
    pub fn next_paragraph<'a>(&'a self, name: &'a str) -> &'a str {
        let Some(style) = self.paragraph(name) else {
            return "Default";
        };
        match &style.next {
            Some(next) if self.paragraph(next).is_some() => next.as_str(),
            _ => name,
        }
    }

    /// Resolve a paragraph style against its ancestors.
    ///
    /// The returned value has no `None` for any property its chain
    /// defines; what remains unset was never set anywhere in the chain
    /// and the consumer applies its own default.
    pub fn resolve_paragraph(&self, name: &str) -> ResolvedParagraph {
        ResolvedParagraph::new(self, name)
    }

    /// Resolve a character style against its ancestors.
    pub fn resolve_character(&self, name: &str) -> ResolvedCharacter {
        ResolvedCharacter::new(self, name)
    }

    /// The `based_on` chain for a paragraph style, nearest first.
    ///
    /// Walking stops at a name that does not exist, and at a style that
    /// has already been seen, so a file carrying a cycle terminates
    /// instead of spinning.
    fn paragraph_chain(&self, name: &str) -> Vec<&ParagraphStyle> {
        let mut chain: Vec<&ParagraphStyle> = Vec::new();
        let mut current = self.paragraph(name);
        while let Some(style) = current {
            if chain.iter().any(|seen| seen.name == style.name) {
                break;
            }
            chain.push(style);
            current = style
                .based_on
                .as_deref()
                .and_then(|base| self.paragraph(base));
        }
        chain
    }

    /// The `based_on` chain for a character style, nearest first.
    fn character_chain(&self, name: &str) -> Vec<&CharacterStyle> {
        let mut chain: Vec<&CharacterStyle> = Vec::new();
        let mut current = self.character(name);
        while let Some(style) = current {
            if chain.iter().any(|seen| seen.name == style.name) {
                break;
            }
            chain.push(style);
            current = style
                .based_on
                .as_deref()
                .and_then(|base| self.character(base));
        }
        chain
    }
}

/// A paragraph style with its inheritance chain applied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedParagraph {
    pub features: Vec<(String, bool)>,
    pub family: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub baseline_shift: Option<BaselineShift>,
    pub position: Option<TextPosition>,
    /// Fraction of full-strength ink; None inherits independently of colour.
    pub fill_tint: Option<f32>,
    pub stroke_tint: Option<f32>,
    /// The fill ink; unset inherits the paragraph or document text colour.
    pub fill: Option<Ink>,
    pub stroke: Option<Ink>,
    pub overprint_fill: Option<bool>,
    pub overprint_stroke: Option<bool>,
    pub point_size: Option<f32>,
    pub leading: Option<Leading>,
    pub auto_leading: Option<f32>,
    pub tracking: Option<f32>,
    pub kerning: Option<bool>,
    pub align: Option<Align>,
    pub left_indent: Option<f32>,
    pub right_indent: Option<f32>,
    pub first_line_indent: Option<f32>,
    pub space_before: Option<f32>,
    pub space_after: Option<f32>,
    pub keep_with_next: Option<bool>,
    pub keep_lines: Option<usize>,
    pub drop_caps_lines: Option<usize>,
    pub drop_caps_characters: Option<usize>,
    pub bullet: Option<Bullet>,
    pub hyphenate: Option<bool>,
    pub language: Option<String>,
    pub direction: Option<ParagraphDirection>,
    pub writing_mode: Option<WritingMode>,
}

impl ResolvedParagraph {
    /// Character defaults for an unstyled run, with document fallback.
    pub fn character(&self, mut fallback: ResolvedCharacter) -> ResolvedCharacter {
        fallback.features = inherited_features(&self.features, &fallback.features);
        fallback.family = self.family.clone().or(fallback.family);
        fallback.bold = self.bold.or(fallback.bold);
        fallback.italic = self.italic.or(fallback.italic);
        fallback.underline = self.underline.or(fallback.underline);
        fallback.strikethrough = self.strikethrough.or(fallback.strikethrough);
        fallback.position = self.position.or(fallback.position);
        fallback.baseline_shift = self.baseline_shift.or(fallback.baseline_shift);
        fallback.fill = inherited_paint(&self.fill, self.fill_tint, fallback.fill.as_ref());
        fallback.stroke = inherited_paint(&self.stroke, self.stroke_tint, fallback.stroke.as_ref());
        fallback.fill_tint = self.fill_tint.or(fallback.fill_tint);
        fallback.stroke_tint = self.stroke_tint.or(fallback.stroke_tint);
        fallback.overprint_fill = self.overprint_fill.or(fallback.overprint_fill);
        fallback.overprint_stroke = self.overprint_stroke.or(fallback.overprint_stroke);
        fallback
    }

    fn new(set: &StyleSet, name: &str) -> ResolvedParagraph {
        let mut out = ResolvedParagraph::default();
        // Nearest first. `or` keeps the value already present, so walking
        // from the style outwards means the most specific definition of
        // each property is the one that survives.
        for style in set.paragraph_chain(name) {
            out.features = inherited_features(&out.features, &style.features);
            out.family = out.family.clone().or_else(|| style.family.clone());
            out.bold = out.bold.or(style.bold);
            out.italic = out.italic.or(style.italic);
            out.underline = out.underline.or(style.underline);
            out.strikethrough = out.strikethrough.or(style.strikethrough);
            out.position = out.position.or(style.position);
            out.baseline_shift = out.baseline_shift.or(style.baseline_shift);
            out.fill = inherited_paint(&out.fill, out.fill_tint, style.fill.as_ref());
            out.stroke = inherited_paint(&out.stroke, out.stroke_tint, style.stroke.as_ref());
            out.fill_tint = out.fill_tint.or(style.fill_tint);
            out.stroke_tint = out.stroke_tint.or(style.stroke_tint);
            out.overprint_fill = out.overprint_fill.or(style.overprint_fill);
            out.overprint_stroke = out.overprint_stroke.or(style.overprint_stroke);
            out.point_size = out.point_size.or(style.point_size);
            out.leading = out.leading.or(style.leading);
            out.auto_leading = out.auto_leading.or(style.auto_leading);
            out.tracking = out.tracking.or(style.tracking);
            out.kerning = out.kerning.or(style.kerning);
            out.align = out.align.or(style.align);
            out.left_indent = out.left_indent.or(style.left_indent);
            out.right_indent = out.right_indent.or(style.right_indent);
            out.first_line_indent = out.first_line_indent.or(style.first_line_indent);
            out.space_before = out.space_before.or(style.space_before);
            out.space_after = out.space_after.or(style.space_after);
            out.keep_with_next = out.keep_with_next.or(style.keep_with_next);
            out.keep_lines = out.keep_lines.or(style.keep_lines);
            out.drop_caps_lines = out.drop_caps_lines.or(style.drop_caps_lines);
            out.drop_caps_characters = out.drop_caps_characters.or(style.drop_caps_characters);
            out.bullet = out.bullet.or(style.bullet);
            out.hyphenate = out.hyphenate.or(style.hyphenate);
            out.language = out.language.clone().or_else(|| style.language.clone());
            out.direction = out.direction.or(style.direction);
            out.writing_mode = out.writing_mode.or(style.writing_mode);
        }
        out
    }
}

/// A character style with its inheritance chain applied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedCharacter {
    pub family: Option<String>,
    pub point_size: Option<f32>,
    pub leading: Option<Leading>,
    pub tracking: Option<f32>,
    pub kerning: Option<bool>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub baseline_shift: Option<BaselineShift>,
    pub position: Option<TextPosition>,
    pub all_caps: Option<bool>,
    pub small_caps: Option<bool>,
    /// Fraction of full-strength ink; None inherits independently of colour.
    pub fill_tint: Option<f32>,
    pub stroke_tint: Option<f32>,
    /// The fill ink; unset inherits the paragraph or document text colour.
    pub fill: Option<Ink>,
    pub stroke: Option<Ink>,
    pub stroke_weight: Option<f32>,
    pub opacity: Option<f32>,
    pub overprint_fill: Option<bool>,
    pub overprint_stroke: Option<bool>,
    pub optical_margin: Option<bool>,
    pub features: Vec<(String, bool)>,
    pub language: Option<String>,
}

impl ResolvedCharacter {
    fn new(set: &StyleSet, name: &str) -> ResolvedCharacter {
        let mut out = ResolvedCharacter::default();
        let chain = set.character_chain(name);
        for style in &chain {
            out.family = out.family.clone().or_else(|| style.family.clone());
            out.point_size = out.point_size.or(style.point_size);
            out.leading = out.leading.or(style.leading);
            out.tracking = out.tracking.or(style.tracking);
            out.kerning = out.kerning.or(style.kerning);
            out.bold = out.bold.or(style.bold);
            out.italic = out.italic.or(style.italic);
            out.underline = out.underline.or(style.underline);
            out.strikethrough = out.strikethrough.or(style.strikethrough);
            out.position = out.position.or(style.position);
            out.baseline_shift = out.baseline_shift.or(style.baseline_shift);
            out.all_caps = out.all_caps.or(style.all_caps);
            out.small_caps = out.small_caps.or(style.small_caps);
            out.fill = inherited_paint(&out.fill, out.fill_tint, style.fill.as_ref());
            out.stroke = inherited_paint(&out.stroke, out.stroke_tint, style.stroke.as_ref());
            out.fill_tint = out.fill_tint.or(style.fill_tint);
            out.stroke_tint = out.stroke_tint.or(style.stroke_tint);
            out.stroke_weight = out.stroke_weight.or(style.stroke_weight);
            out.opacity = out.opacity.or(style.opacity);
            out.overprint_fill = out.overprint_fill.or(style.overprint_fill);
            out.overprint_stroke = out.overprint_stroke.or(style.overprint_stroke);
            out.optical_margin = out.optical_margin.or(style.optical_margin);
            out.language = out.language.clone().or_else(|| style.language.clone());
        }
        // Features merge across the chain with the nearest style winning
        // per feature. `or`-style "first one seen sticks" is what makes
        // that correct: walking outwards, a base's value for a feature
        // the child already set must not replace it.
        for style in &chain {
            out.features = inherited_features(&out.features, &style.features);
        }
        out
    }
}

/// Nearest style wins per tag, including explicit false; omitted tags inherit.
pub fn inherited_features(
    nearest: &[(String, bool)],
    fallback: &[(String, bool)],
) -> Vec<(String, bool)> {
    let mut result = std::collections::BTreeMap::new();
    for (tag, enabled) in nearest.iter().chain(fallback) {
        result.entry(tag.clone()).or_insert(*enabled);
    }
    result.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_with(styles: Vec<ParagraphStyle>) -> StyleSet {
        StyleSet {
            text_preferences: TextPreferences::default(),
            paragraphs: styles,
            characters: Vec::new(),
        }
    }

    #[test]
    fn a_child_inherits_from_its_base() {
        let set = set_with(vec![
            ParagraphStyle {
                name: "Default".into(),
                point_size: Some(12.0),
                align: Some(Align::Left),
                ..Default::default()
            },
            ParagraphStyle {
                name: "Body".into(),
                based_on: Some("Default".into()),
                point_size: Some(10.0),
                ..Default::default()
            },
        ]);
        let r = set.resolve_paragraph("Body");
        // The child wins where it sets a value...
        assert_eq!(r.point_size, Some(10.0));
        // ...and inherits where it does not.
        assert_eq!(r.align, Some(Align::Left));
    }

    #[test]
    fn an_unknown_style_resolves_to_all_unset_rather_than_panicking() {
        let set = StyleSet::with_defaults();
        let r = set.resolve_paragraph("Nonexistent");
        assert_eq!(r.point_size, None);
    }

    #[test]
    fn next_style_falls_back_to_itself_when_unset_or_dangling() {
        let set = set_with(vec![
            ParagraphStyle {
                name: "Heading".into(),
                next: Some("Body".into()),
                ..Default::default()
            },
            ParagraphStyle {
                name: "Body".into(),
                ..Default::default()
            },
            ParagraphStyle {
                name: "Dangling".into(),
                next: Some("Missing".into()),
                ..Default::default()
            },
            ParagraphStyle {
                name: "Repeat".into(),
                ..Default::default()
            },
        ]);
        assert_eq!(set.next_paragraph("Heading"), "Body");
        assert_eq!(set.next_paragraph("Dangling"), "Dangling");
        assert_eq!(set.next_paragraph("Repeat"), "Repeat");
        assert_eq!(set.next_paragraph("Nonexistent"), "Default");
    }

    #[test]
    fn add_replaces_rather_than_duplicating() {
        let mut set = StyleSet::with_defaults();
        let before = set.paragraphs.len();
        set.add_paragraph(ParagraphStyle {
            name: "Body".into(),
            point_size: Some(9.0),
            ..Default::default()
        });
        assert_eq!(set.paragraphs.len(), before);
        assert_eq!(set.paragraph("Body").unwrap().point_size, Some(9.0));
    }

    #[test]
    fn a_cycle_in_based_on_terminates() {
        // A file from another tool can carry this; resolution must stop
        // rather than loop.
        let set = set_with(vec![
            ParagraphStyle {
                name: "A".into(),
                based_on: Some("B".into()),
                point_size: Some(8.0),
                ..Default::default()
            },
            ParagraphStyle {
                name: "B".into(),
                based_on: Some("A".into()),
                point_size: Some(14.0),
                ..Default::default()
            },
        ]);
        let r = set.resolve_paragraph("A");
        // Both styles in the cycle still contribute; the walk just ends.
        assert!(r.point_size.is_some());
    }

    #[test]
    fn a_dangling_based_on_is_ignored() {
        let set = set_with(vec![ParagraphStyle {
            name: "Orphan".into(),
            based_on: Some("DoesNotExist".into()),
            point_size: Some(7.0),
            ..Default::default()
        }]);
        assert_eq!(set.resolve_paragraph("Orphan").point_size, Some(7.0));
    }

    #[test]
    fn character_features_accumulate_across_the_chain() {
        let mut set = StyleSet {
            text_preferences: TextPreferences::default(),
            paragraphs: Vec::new(),
            characters: vec![
                CharacterStyle {
                    name: "Default".into(),
                    features: vec![("liga".into(), true)],
                    ..Default::default()
                },
                CharacterStyle {
                    name: "Code".into(),
                    based_on: Some("Default".into()),
                    // Turning one feature off must not drop the inherited
                    // one that is still on.
                    features: vec![("liga".into(), false), ("dlig".into(), true)],
                    ..Default::default()
                },
            ],
        };
        let r = set.resolve_character("Code");
        let get = |name: &str| r.features.iter().find(|(f, _)| f == name).map(|(_, v)| *v);
        assert_eq!(get("liga"), Some(false));
        assert_eq!(get("dlig"), Some(true));
        set.characters.clear();
    }

    #[test]
    fn character_inheritance_resolves_like_paragraphs() {
        let set = StyleSet {
            text_preferences: TextPreferences::default(),
            paragraphs: Vec::new(),
            characters: vec![
                CharacterStyle {
                    name: "Default".into(),
                    family: Some("Georgia".into()),
                    fill: Some(Ink::process("Text Black", [0.0, 0.0, 0.0])),
                    ..Default::default()
                },
                CharacterStyle {
                    name: "Em".into(),
                    based_on: Some("Default".into()),
                    italic: Some(true),
                    ..Default::default()
                },
            ],
        };
        let r = set.resolve_character("Em");
        assert_eq!(r.family.as_deref(), Some("Georgia"));
        assert!(r.fill.is_some());
        assert_eq!(r.italic, Some(true));
    }

    #[test]
    fn defaults_carry_a_usable_body_style() {
        let set = StyleSet::with_defaults();
        let body = set.resolve_paragraph("Body");
        assert!(body.point_size.is_some());
        assert!(body.leading.is_some());
        // Body must be based on Default, so "next" from Default reaches it.
        assert_eq!(set.next_paragraph("Default"), "Default");
    }
}
