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
    /// A literal character repeated at the start of each paragraph.
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

/// Where a paragraph may begin in its text thread. IDML StartParagraph values
/// are independent of line keeps and do not insert characters into the story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParagraphStart {
    Anywhere,
    NextColumn,
    NextFrame,
    NextPage,
    NextOddPage,
    NextEvenPage,
}

impl ParagraphStart {
    pub fn native_name(self) -> &'static str {
        match self {
            Self::Anywhere => "Anywhere",
            Self::NextColumn => "NextColumn",
            Self::NextFrame => "NextFrame",
            Self::NextPage => "NextPage",
            Self::NextOddPage => "NextOddPage",
            Self::NextEvenPage => "NextEvenPage",
        }
    }

    pub fn from_native(value: &str) -> Option<Self> {
        Some(match value {
            "Anywhere" => Self::Anywhere,
            "NextColumn" => Self::NextColumn,
            "NextFrame" => Self::NextFrame,
            "NextPage" => Self::NextPage,
            "NextOddPage" => Self::NextOddPage,
            "NextEvenPage" => Self::NextEvenPage,
            _ => return None,
        })
    }
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
    /// Synthetic small capitals as a percentage of nominal glyph size.
    pub small_cap_size: f32,
}

impl Default for TextPreferences {
    fn default() -> Self {
        Self {
            superscript_size: 58.3,
            superscript_position: 33.3,
            subscript_size: 58.3,
            subscript_position: 33.3,
            small_cap_size: 70.0,
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

/// No-ink is a real override, distinct from an unspecified/inherited color.
pub fn inherited_text_paint(
    paint: &Option<Ink>,
    disabled: bool,
    tint: Option<f32>,
    fallback: Option<&Ink>,
    fallback_disabled: bool,
) -> (Option<Ink>, bool) {
    if disabled {
        (None, true)
    } else if paint.is_some() {
        (paint.clone(), false)
    } else if fallback_disabled {
        (None, true)
    } else {
        (inherited_paint(paint, tint, fallback), false)
    }
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
    /// Exact typographic subfamily. Overrides bold/italic at this level;
    /// a nearer explicit bold/italic choice resets an inherited name.
    pub font_style: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    #[serde(default)]
    pub underline_style: crate::decorations::DecorationStyle,
    #[serde(default)]
    pub strike_style: crate::decorations::DecorationStyle,
    pub baseline_shift: Option<BaselineShift>,
    pub position: Option<TextPosition>,
    /// Independent legacy flags; both true selects OpenType all-small-caps.
    pub all_caps: Option<bool>,
    pub small_caps: Option<bool>,
    /// Fraction of full-strength ink; None inherits independently of colour.
    pub fill_tint: Option<f32>,
    pub stroke_tint: Option<f32>,
    /// The fill ink; unset inherits the paragraph or document text colour.
    pub fill: Option<Ink>,
    pub stroke: Option<Ink>,
    pub stroke_weight: Option<f32>,
    /// True paints wholly outside the outline; false centers the stroke.
    pub stroke_outside: Option<bool>,
    /// Glyph outline corner; None inherits, with Miter as the document default.
    #[serde(default)]
    pub stroke_join: Option<schist_text_engine::TextStrokeJoin>,
    /// Nonnegative ratio; zero bevels every nonstraight miter. None inherits four.
    #[serde(default)]
    pub stroke_miter_limit: Option<f32>,
    /// Explicit native no-ink values. False with no ink means inherit; true
    /// suppresses even an ancestor's ink. A disabled paint takes precedence.
    #[serde(default)]
    pub fill_disabled: bool,
    #[serde(default)]
    pub stroke_disabled: bool,
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
    /// Legacy toggle: keep the final line with the next paragraph's first line.
    pub keep_with_next: Option<bool>,
    /// Minimum lines left at the bottom of a column before the
    /// paragraph moves to the next. Orphans and widows in one setting.
    pub keep_lines: Option<usize>,
    /// Native keep options override the legacy toggle and symmetric count.
    #[serde(default)]
    pub keeps: crate::paragraph_keeps::ParagraphKeeps,
    /// Required container boundary for this paragraph's first line. None
    /// inherits, while Anywhere explicitly removes a base style's constraint.
    pub start_paragraph: Option<ParagraphStart>,
    /// Number of body lines spanned by opening characters; zero or one disables it.
    pub drop_caps_lines: Option<usize>,
    /// Opening graphemes to enlarge. Unset means one; zero disables drop caps.
    pub drop_caps_characters: Option<usize>,

    pub bullet: Option<Bullet>,
    #[serde(default)]
    pub list: crate::lists::ListStyle,
    /// Whether hyphenation is allowed in this paragraph.
    pub hyphenate: Option<bool>,
    /// A BCP 47 tag or a declared native language resource. Used by shaping and
    /// display casing; hyphenation dictionaries and proofing remain separate work.
    pub language: Option<crate::language::TextLanguage>,
    /// Base paragraph direction. `None` means auto, where the first
    /// strong character decides.
    pub direction: Option<ParagraphDirection>,
    /// Whether this paragraph runs horizontally or vertically.
    pub writing_mode: Option<WritingMode>,
    /// Per-tag OpenType overrides. Unspecified tags inherit independently.
    #[serde(default)]
    pub features: Vec<(String, bool)>,
    #[serde(default)]
    pub directional_features: crate::directional_features::DirectionalFeatures,
}

/// A named set of character properties, applied over a paragraph style.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterStyle {
    pub name: String,
    pub based_on: Option<String>,

    /// The font family name, matched against the installed font database
    /// rather than being a path.
    pub family: Option<String>,
    /// Exact typographic subfamily. Overrides bold/italic at this level;
    /// a nearer explicit bold/italic choice resets an inherited name.
    pub font_style: Option<String>,
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
    #[serde(default)]
    pub underline_style: crate::decorations::DecorationStyle,
    #[serde(default)]
    pub strike_style: crate::decorations::DecorationStyle,
    /// Explicit shift; legacy script variants resolve through position.
    pub baseline_shift: Option<BaselineShift>,
    pub position: Option<TextPosition>,
    /// Independent flags: all only = uppercase, small only = small caps,
    /// both true = OpenType all-small-caps; both false explicitly resets.
    pub all_caps: Option<bool>,
    pub small_caps: Option<bool>,

    /// Fraction of full-strength ink; None inherits independently of colour.
    pub fill_tint: Option<f32>,
    pub stroke_tint: Option<f32>,
    /// The fill ink; unset inherits the paragraph or document text colour.
    pub fill: Option<Ink>,
    pub stroke: Option<Ink>,
    /// Explicit native no-ink values. False with no ink means inherit; true
    /// suppresses even an ancestor's ink. A disabled paint takes precedence.
    #[serde(default)]
    pub fill_disabled: bool,
    #[serde(default)]
    pub stroke_disabled: bool,
    /// Outline thickness in points, independent of font size.
    pub stroke_weight: Option<f32>,
    /// True paints wholly outside the outline; false centers the stroke.
    pub stroke_outside: Option<bool>,
    /// Glyph outline corner; None inherits, with Miter as the document default.
    #[serde(default)]
    pub stroke_join: Option<schist_text_engine::TextStrokeJoin>,
    /// Nonnegative ratio; zero bevels every nonstraight miter. None inherits four.
    #[serde(default)]
    pub stroke_miter_limit: Option<f32>,
    /// Text paint opacity, 0..=1. Transparency and overprint are separate.
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
    #[serde(default)]
    pub directional_features: crate::directional_features::DirectionalFeatures,
    pub language: Option<crate::language::TextLanguage>,
}

/// The document's style tables.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleSet {
    #[serde(default)]
    pub numbering_lists: Vec<crate::lists::NumberingList>,
    #[serde(default)]
    pub languages: Vec<crate::language::LanguageResource>,
    #[serde(default)]
    pub strokes: Vec<crate::decorations::DecorationStroke>,
    #[serde(default)]
    pub objects: Vec<crate::object_styles::ObjectStyle>,
    #[serde(default)]
    pub text_preferences: TextPreferences,
    pub paragraphs: Vec<ParagraphStyle>,
    pub characters: Vec<CharacterStyle>,
}

impl StyleSet {
    /// A set with the handful of styles a new document cannot do without.
    ///
    /// Every document gets a "Default" for both text kinds and a "Body" based
    /// on it. A blank style table would force every consumer to special
    /// case the empty state, and a `None` resolved against nothing still
    /// has to produce a value.
    pub fn with_defaults() -> StyleSet {
        StyleSet {
            numbering_lists: Vec::new(),
            languages: Vec::new(),
            objects: Vec::new(),
            strokes: Vec::new(),
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
    pub directional_features: crate::directional_features::DirectionalFeatures,
    pub family: Option<String>,
    /// Exact typographic subfamily. Overrides bold/italic at this level;
    /// a nearer explicit bold/italic choice resets an inherited name.
    pub font_style: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub underline_style: crate::decorations::DecorationStyle,
    pub strike_style: crate::decorations::DecorationStyle,
    pub baseline_shift: Option<BaselineShift>,
    pub position: Option<TextPosition>,
    /// Independent legacy flags; both true selects OpenType all-small-caps.
    pub all_caps: Option<bool>,
    pub small_caps: Option<bool>,
    /// Fraction of full-strength ink; None inherits independently of colour.
    pub fill_tint: Option<f32>,
    pub stroke_tint: Option<f32>,
    /// The fill ink; unset inherits the paragraph or document text colour.
    pub fill: Option<Ink>,
    pub stroke: Option<Ink>,
    pub stroke_weight: Option<f32>,
    /// True paints wholly outside the outline; false centers the stroke.
    pub stroke_outside: Option<bool>,
    /// Glyph outline corner; None inherits, with Miter as the document default.
    pub stroke_join: Option<schist_text_engine::TextStrokeJoin>,
    /// Nonnegative ratio; zero bevels every nonstraight miter. None inherits four.
    pub stroke_miter_limit: Option<f32>,
    /// Explicit native no-ink values. False with no ink means inherit; true
    /// suppresses even an ancestor's ink. A disabled paint takes precedence.
    pub fill_disabled: bool,
    pub stroke_disabled: bool,
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
    pub keeps: crate::paragraph_keeps::ParagraphKeeps,
    pub start_paragraph: Option<ParagraphStart>,
    pub drop_caps_lines: Option<usize>,
    pub drop_caps_characters: Option<usize>,
    pub bullet: Option<Bullet>,
    pub list: crate::lists::ListStyle,
    pub hyphenate: Option<bool>,
    pub language: Option<crate::language::TextLanguage>,
    pub direction: Option<ParagraphDirection>,
    pub writing_mode: Option<WritingMode>,
}

impl ResolvedParagraph {
    /// Character defaults for an unstyled run, with document fallback.
    pub fn character(&self, mut fallback: ResolvedCharacter) -> ResolvedCharacter {
        fallback.features = self
            .directional_features
            .inherit_features(&self.features, &fallback.features);
        fallback.directional_features = self
            .directional_features
            .over(fallback.directional_features);
        fallback.language = self.language.clone().or(fallback.language);
        fallback.family = self.family.clone().or(fallback.family);
        if self.font_style.is_some() || self.bold.is_some() || self.italic.is_some() {
            fallback.font_style = self.font_style.clone();
        }
        fallback.bold = self.bold.or(fallback.bold);
        fallback.italic = self.italic.or(fallback.italic);
        fallback.underline = self.underline.or(fallback.underline);
        fallback.strikethrough = self.strikethrough.or(fallback.strikethrough);
        fallback.underline_style = self.underline_style.over(&fallback.underline_style);
        fallback.strike_style = self.strike_style.over(&fallback.strike_style);
        fallback.position = self.position.or(fallback.position);
        fallback.all_caps = self.all_caps.or(fallback.all_caps);
        fallback.small_caps = self.small_caps.or(fallback.small_caps);
        fallback.baseline_shift = self.baseline_shift.or(fallback.baseline_shift);
        (fallback.fill, fallback.fill_disabled) = inherited_text_paint(
            &self.fill,
            self.fill_disabled,
            self.fill_tint,
            fallback.fill.as_ref(),
            fallback.fill_disabled,
        );
        (fallback.stroke, fallback.stroke_disabled) = inherited_text_paint(
            &self.stroke,
            self.stroke_disabled,
            self.stroke_tint,
            fallback.stroke.as_ref(),
            fallback.stroke_disabled,
        );
        fallback.stroke_weight = self.stroke_weight.or(fallback.stroke_weight);
        fallback.stroke_outside = self.stroke_outside.or(fallback.stroke_outside);
        fallback.stroke_join = self.stroke_join.or(fallback.stroke_join);
        fallback.stroke_miter_limit = self.stroke_miter_limit.or(fallback.stroke_miter_limit);
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
        let mut face_selected = false;
        for style in set.paragraph_chain(name) {
            if !face_selected
                && (style.font_style.is_some() || style.bold.is_some() || style.italic.is_some())
            {
                out.font_style = style.font_style.clone();
                face_selected = true;
            }
            out.features = out
                .directional_features
                .inherit_features(&out.features, &style.features);
            out.directional_features = out.directional_features.over(style.directional_features);
            out.family = out.family.clone().or_else(|| style.family.clone());
            let hints = style
                .font_style
                .as_deref()
                .map(schist_text_engine::font_style_hints);
            out.bold = out.bold.or(hints.map(|v| v.0).or(style.bold));
            out.italic = out.italic.or(hints.map(|v| v.1).or(style.italic));
            out.underline = out.underline.or(style.underline);
            out.strikethrough = out.strikethrough.or(style.strikethrough);
            out.underline_style = out.underline_style.over(&style.underline_style);
            out.strike_style = out.strike_style.over(&style.strike_style);
            out.position = out.position.or(style.position);
            out.all_caps = out.all_caps.or(style.all_caps);
            out.small_caps = out.small_caps.or(style.small_caps);
            out.baseline_shift = out.baseline_shift.or(style.baseline_shift);
            (out.fill, out.fill_disabled) = inherited_text_paint(
                &out.fill,
                out.fill_disabled,
                out.fill_tint,
                style.fill.as_ref(),
                style.fill_disabled,
            );
            (out.stroke, out.stroke_disabled) = inherited_text_paint(
                &out.stroke,
                out.stroke_disabled,
                out.stroke_tint,
                style.stroke.as_ref(),
                style.stroke_disabled,
            );
            out.fill_tint = out.fill_tint.or(style.fill_tint);
            out.stroke_tint = out.stroke_tint.or(style.stroke_tint);
            out.stroke_weight = out.stroke_weight.or(style.stroke_weight);
            out.stroke_outside = out.stroke_outside.or(style.stroke_outside);
            out.stroke_join = out.stroke_join.or(style.stroke_join);
            out.stroke_miter_limit = out.stroke_miter_limit.or(style.stroke_miter_limit);
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
            out.keeps = out.keeps.over(&style.keeps.over(
                &crate::paragraph_keeps::ParagraphKeeps::from_legacy(
                    style.keep_with_next,
                    style.keep_lines,
                ),
            ));
            out.start_paragraph = out.start_paragraph.or(style.start_paragraph);
            out.drop_caps_lines = out.drop_caps_lines.or(style.drop_caps_lines);
            out.drop_caps_characters = out.drop_caps_characters.or(style.drop_caps_characters);
            out.bullet = out.bullet.or(style.bullet);
            out.list = out.list.over(
                &style
                    .list
                    .over(&crate::lists::ListStyle::from_legacy(style.bullet)),
            );
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
    /// Exact typographic subfamily. Overrides bold/italic at this level;
    /// a nearer explicit bold/italic choice resets an inherited name.
    pub font_style: Option<String>,
    pub point_size: Option<f32>,
    pub leading: Option<Leading>,
    pub tracking: Option<f32>,
    pub kerning: Option<bool>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub underline_style: crate::decorations::DecorationStyle,
    pub strike_style: crate::decorations::DecorationStyle,
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
    /// Explicit native no-ink values. False with no ink means inherit; true
    /// suppresses even an ancestor's ink. A disabled paint takes precedence.
    pub fill_disabled: bool,
    pub stroke_disabled: bool,
    pub stroke_weight: Option<f32>,
    /// True paints wholly outside the outline; false centers the stroke.
    pub stroke_outside: Option<bool>,
    /// Glyph outline corner; None inherits, with Miter as the document default.
    pub stroke_join: Option<schist_text_engine::TextStrokeJoin>,
    /// Nonnegative ratio; zero bevels every nonstraight miter. None inherits four.
    pub stroke_miter_limit: Option<f32>,
    pub opacity: Option<f32>,
    pub overprint_fill: Option<bool>,
    pub overprint_stroke: Option<bool>,
    pub optical_margin: Option<bool>,
    pub features: Vec<(String, bool)>,
    pub directional_features: crate::directional_features::DirectionalFeatures,
    pub language: Option<crate::language::TextLanguage>,
}

impl ResolvedCharacter {
    /// Materialize a resolved composition style in a temporary style set.
    /// All paint and font properties remain typed, including spot ink identity.
    pub fn into_style(self, name: impl Into<String>) -> CharacterStyle {
        CharacterStyle {
            name: name.into(),
            based_on: None,
            family: self.family,
            font_style: self.font_style,
            point_size: self.point_size,
            leading: self.leading,
            tracking: self.tracking,
            kerning: self.kerning,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strikethrough: self.strikethrough,
            underline_style: self.underline_style,
            strike_style: self.strike_style,
            baseline_shift: self.baseline_shift,
            position: self.position,
            all_caps: self.all_caps,
            small_caps: self.small_caps,
            fill_tint: self.fill_tint,
            stroke_tint: self.stroke_tint,
            fill: self.fill,
            stroke: self.stroke,
            fill_disabled: self.fill_disabled,
            stroke_disabled: self.stroke_disabled,
            stroke_weight: self.stroke_weight,
            stroke_outside: self.stroke_outside,
            stroke_join: self.stroke_join,
            stroke_miter_limit: self.stroke_miter_limit,
            opacity: self.opacity,
            overprint_fill: self.overprint_fill,
            overprint_stroke: self.overprint_stroke,
            optical_margin: self.optical_margin,
            features: self.features,
            directional_features: self.directional_features,
            language: self.language,
        }
    }

    /// Merge all character properties with a lower-precedence resolved style.
    /// Used by generated markers: an explicit marker style overrides the first
    /// character, which in turn overrides the paragraph and document defaults.
    pub fn over(mut self, base: &Self) -> Self {
        let face_selected =
            self.font_style.is_some() || self.bold.is_some() || self.italic.is_some();
        self = self.with_paint_defaults(base);
        if !face_selected {
            self.font_style = base.font_style.clone();
        }
        fn fallback<T: Clone>(value: &mut Option<T>, base: &Option<T>) {
            if value.is_none() {
                *value = base.as_ref().cloned();
            }
        }
        macro_rules! inherit {
            ($($field:ident),+ $(,)?) => { $(fallback(&mut self.$field, &base.$field);)+ };
        }
        inherit!(
            family,
            point_size,
            leading,
            tracking,
            kerning,
            bold,
            italic,
            underline,
            strikethrough,
            baseline_shift,
            position,
            all_caps,
            small_caps,
            optical_margin,
            language
        );
        self.features = self
            .directional_features
            .inherit_features(&self.features, &base.features);
        self.directional_features = self.directional_features.over(base.directional_features);
        self
    }

    /// Paint defaults layered under this character style. Geometry and font
    /// properties are resolved separately by composition.
    pub fn with_paint_defaults(mut self, base: &Self) -> Self {
        self.underline_style = self.underline_style.over(&base.underline_style);
        self.strike_style = self.strike_style.over(&base.strike_style);
        (self.fill, self.fill_disabled) = inherited_text_paint(
            &self.fill,
            self.fill_disabled,
            self.fill_tint,
            base.fill.as_ref(),
            base.fill_disabled,
        );
        (self.stroke, self.stroke_disabled) = inherited_text_paint(
            &self.stroke,
            self.stroke_disabled,
            self.stroke_tint,
            base.stroke.as_ref(),
            base.stroke_disabled,
        );
        self.stroke_weight = self.stroke_weight.or(base.stroke_weight);
        self.stroke_outside = self.stroke_outside.or(base.stroke_outside);
        self.stroke_join = self.stroke_join.or(base.stroke_join);
        self.stroke_miter_limit = self.stroke_miter_limit.or(base.stroke_miter_limit);
        self.fill_tint = self.fill_tint.or(base.fill_tint);
        self.stroke_tint = self.stroke_tint.or(base.stroke_tint);
        self.opacity = self.opacity.or(base.opacity);
        self.overprint_fill = self.overprint_fill.or(base.overprint_fill);
        self.overprint_stroke = self.overprint_stroke.or(base.overprint_stroke);
        self
    }

    pub fn preview_stroke(&self) -> Option<schist_text_engine::TextStroke> {
        let ink = self.stroke.as_ref().filter(|_| !self.stroke_disabled)?;
        let width = self.stroke_weight.unwrap_or(1.0);
        if !width.is_finite() || width <= 0.0 {
            return None;
        }
        let rgb = ink.preview_at_tint(self.stroke_tint.unwrap_or(1.0));
        Some(schist_text_engine::TextStroke {
            width,
            outside: self.stroke_outside.unwrap_or(false),
            join: self.stroke_join.unwrap_or_default(),
            miter_limit: self
                .stroke_miter_limit
                .filter(|v| v.is_finite() && *v >= 0.0)
                .unwrap_or(4.0),
            color: Some([
                (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                (self.opacity.unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8,
            ]),
        })
    }

    fn new(set: &StyleSet, name: &str) -> ResolvedCharacter {
        let mut out = ResolvedCharacter::default();
        let chain = set.character_chain(name);
        let mut face_selected = false;
        for style in &chain {
            if !face_selected
                && (style.font_style.is_some() || style.bold.is_some() || style.italic.is_some())
            {
                out.font_style = style.font_style.clone();
                face_selected = true;
            }
            out.family = out.family.clone().or_else(|| style.family.clone());
            out.point_size = out.point_size.or(style.point_size);
            out.leading = out.leading.or(style.leading);
            out.tracking = out.tracking.or(style.tracking);
            out.kerning = out.kerning.or(style.kerning);
            let hints = style
                .font_style
                .as_deref()
                .map(schist_text_engine::font_style_hints);
            out.bold = out.bold.or(hints.map(|v| v.0).or(style.bold));
            out.italic = out.italic.or(hints.map(|v| v.1).or(style.italic));
            out.underline = out.underline.or(style.underline);
            out.strikethrough = out.strikethrough.or(style.strikethrough);
            out.underline_style = out.underline_style.over(&style.underline_style);
            out.strike_style = out.strike_style.over(&style.strike_style);
            out.position = out.position.or(style.position);
            out.baseline_shift = out.baseline_shift.or(style.baseline_shift);
            out.all_caps = out.all_caps.or(style.all_caps);
            out.small_caps = out.small_caps.or(style.small_caps);
            (out.fill, out.fill_disabled) = inherited_text_paint(
                &out.fill,
                out.fill_disabled,
                out.fill_tint,
                style.fill.as_ref(),
                style.fill_disabled,
            );
            (out.stroke, out.stroke_disabled) = inherited_text_paint(
                &out.stroke,
                out.stroke_disabled,
                out.stroke_tint,
                style.stroke.as_ref(),
                style.stroke_disabled,
            );
            out.fill_tint = out.fill_tint.or(style.fill_tint);
            out.stroke_tint = out.stroke_tint.or(style.stroke_tint);
            out.stroke_weight = out.stroke_weight.or(style.stroke_weight);
            out.stroke_outside = out.stroke_outside.or(style.stroke_outside);
            out.stroke_join = out.stroke_join.or(style.stroke_join);
            out.stroke_miter_limit = out.stroke_miter_limit.or(style.stroke_miter_limit);
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
            out.features = out
                .directional_features
                .inherit_features(&out.features, &style.features);
            out.directional_features = out.directional_features.over(style.directional_features);
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
            numbering_lists: Vec::new(),
            languages: Vec::new(),
            objects: Vec::new(),
            strokes: Vec::new(),
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
            numbering_lists: Vec::new(),
            languages: Vec::new(),
            objects: Vec::new(),
            strokes: Vec::new(),
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
            numbering_lists: Vec::new(),
            languages: Vec::new(),
            objects: Vec::new(),
            strokes: Vec::new(),
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
