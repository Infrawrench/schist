//! Text layout and rasterization for text layers.
//!
//! Scope: system font discovery, with fill/stroke paints, family,
//! style and size free to change from character to character (see
//! [`StyleRun`]), Unicode bidirectional and vertical layout with shaping,
//! word wrapping and alignment, rasterized to an 8-bit coverage mask.
//! Stored paths position and rotate horizontal text glyphs.

use schist_core::IntRect;
use std::path::PathBuf;
#[cfg(not(schist_library))]
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};
use unicode_segmentation::UnicodeSegmentation;

mod capitalization;
mod language;
pub use language::normalize_language;
mod decoration_dashes;
mod decoration_fitting;
pub use decoration_fitting::DecorationFit;
mod decoration_pattern;
mod path_decoration;
pub use decoration_pattern::{DecorationCap, DecorationDashes, TextDecorationPattern};
#[cfg(test)]
mod directions_tests;
pub use capitalization::Capitalization;
mod shaping;
mod tab_leaders;
mod tabs;
mod text_path;
pub use tabs::{valid_tab_leader, InlineMeasure, TabAlignment, TabStops};
mod text_stroke;
pub use text_path::TextPath;

/// An OpenType feature override. Tags are four ASCII bytes,
/// e.g. `liga`, `kern`, `smcp`, or `ss01`; zero disables a feature.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OpenTypeFeature {
    pub tag: String,
    pub value: u32,
}

/// Overlay features by tag. A run inherits unspecified layer features; zero is
/// an explicit disable. Stable ordering avoids splitting equivalent shaping runs.
fn merged_features(
    base: &[OpenTypeFeature],
    overrides: &[OpenTypeFeature],
) -> Vec<OpenTypeFeature> {
    let mut result = std::collections::BTreeMap::new();
    for feature in overrides.iter().chain(base) {
        result.entry(feature.tag.clone()).or_insert(feature.value);
    }
    result
        .into_iter()
        .map(|(tag, value)| OpenTypeFeature { tag, value })
        .collect()
}

/// Alignment along the inline axis (horizontal x, or vertical y).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub fn display_name(self) -> &'static str {
        match self {
            Align::Left => "Left",
            Align::Center => "Center",
            Align::Right => "Right",
        }
    }
}

/// Base paragraph direction. Auto uses the first strong Unicode character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ParagraphDirection {
    #[default]
    Auto,
    LeftToRight,
    RightToLeft,
}

/// Inline text flows downwards in vertical modes; columns advance left or right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum WritingMode {
    #[default]
    Horizontal,
    VerticalRl,
    VerticalLr,
}

impl WritingMode {
    pub fn is_vertical(self) -> bool {
        self != Self::Horizontal
    }
}

/// Everything needed to lay a text layer out. Kept serializable-simple so a
/// text layer can be re-rendered whenever its content changes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextSpec {
    /// BCP 47 language for shaping and display casing; empty uses Unicode defaults.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    pub text: String,
    pub family: String,
    /// Exact typographic subfamily, such as Light or Bold Condensed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    /// Ask the font database for the bold face of `family`. Defaulted so
    /// that text layers written before this existed still load.
    #[serde(default)]
    pub bold: bool,
    /// Ask for the italic face.
    #[serde(default)]
    pub italic: bool,
    /// Size in pixels (em size).
    pub size: f32,
    pub align: Align,
    #[serde(default)]
    pub direction: ParagraphDirection,
    #[serde(default)]
    pub writing_mode: WritingMode,
    /// Extra spacing between lines, as a multiple of the font's default.
    pub line_height: f32,
    /// Absolute leading in pixels. None uses font metrics times line_height.
    /// Zero is a deliberate overlap; an empty line uses this value too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading: Option<f32>,
    /// Extra spacing between characters, in pixels.
    pub tracking: f32,
    /// Extra advance for ordinary word spaces, independent of tracking. With
    /// tab stops, only spaces in the final field of each line expand.
    #[serde(default)]
    pub word_spacing: f32,
    /// Inline wrap length in pixels (column length in vertical writing); `None` means never wrap.
    pub wrap_width: Option<f32>,
    /// Optional aligned tab stops and leaders. None retains legacy behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<TabStops>,
    /// Stretches of `text` set differently from the rest, by byte range.
    /// The first matching run wins; callers may append a whole-paragraph
    /// fallback after local overrides. Editing normalizes ranges. Uncovered
    /// text uses the layer's own `family`/`bold`/`italic`/`size`. Empty for
    /// text in one font, including files written before runs existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<StyleRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<OpenTypeFeature>,
    /// A copy of the baseline path in layout coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<TextPath>,
}

impl Default for TextSpec {
    fn default() -> Self {
        TextSpec {
            language: String::new(),
            text: String::new(),
            family: default_family(),
            font_style: None,
            bold: false,
            italic: false,
            size: 48.0,
            align: Align::Left,
            direction: ParagraphDirection::Auto,
            writing_mode: WritingMode::Horizontal,
            line_height: 1.0,
            leading: None,
            tracking: 0.0,
            word_spacing: 0.0,
            wrap_width: None,
            tabs: None,
            runs: Vec::new(),
            features: Vec::new(),
            path: None,
        }
    }
}

/// Corner geometry for an outlined glyph; independent of font shaping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TextStrokeJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

fn default_miter_limit() -> f32 {
    4.0
}

/// Glyph-outline stroke, in pixels. A zero width explicitly disables
/// a previous stroke during editing. Color is opaque to the coverage renderer.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextStroke {
    pub width: f32,
    #[serde(default)]
    pub outside: bool,
    #[serde(default)]
    pub join: TextStrokeJoin,
    #[serde(default = "default_miter_limit")]
    pub miter_limit: f32,
    pub color: Option<[u8; 4]>,
}
impl Default for TextStroke {
    fn default() -> Self {
        Self {
            width: 0.0,
            outside: false,
            join: TextStrokeJoin::Miter,
            miter_limit: default_miter_limit(),
            color: None,
        }
    }
}
impl TextStroke {
    /// Conservative distance from the contour, also used across page gutters.
    pub fn extent(self) -> f32 {
        self.width
            * if self.join == TextStrokeJoin::Miter {
                self.miter_limit.max(1.0)
            } else {
                1.0
            }
    }
}

/// Resolved decoration settings. Missing dimensions use font metrics;
/// color None follows glyph fill. Horizontal offsets measure pixels from the
/// baseline: underline positive below, strikethrough positive above. Vertical
/// offsets measure from the column center, positive toward its outside edge.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TextDecoration {
    pub fitting: DecorationFit,
    pub pattern: TextDecorationPattern,
    /// No color means transparent gaps, independently of the main line paint.
    pub gap_color: Option<[u8; 4]>,
    pub weight: Option<f32>,
    pub offset: Option<f32>,
    pub color: Option<[u8; 4]>,
    pub disabled: bool,
}
impl TextDecoration {
    pub fn scaled(&mut self, scale: f32) {
        self.weight = self.weight.map(|v| v * scale);
        self.offset = self.offset.map(|v| v * scale);
        self.pattern.scaled(scale);
    }
}

/// A range of characters set in something other than the layer's own
/// font: each field that is `Some` overrides the layer's, the rest
/// inherit. Byte offsets into `TextSpec::text`.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct StyleRun {
    /// None inherits; an empty string explicitly restores default language behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Display casing; source text and byte ranges are never rewritten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capitalization: Option<Capitalization>,
    /// Synthetic small-cap scale, default 0.7. Only used without native glyphs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub small_cap_scale: Option<f32>,
    pub start: usize,
    pub end: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Exact face name; an explicit bold/italic override resets inheritance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// Nominal size for line metrics when glyphs use a different size (scripts).
    /// None uses the rendered size. Does not change shaping or glyph coverage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_size: Option<f32>,
    /// Per-tag overrides of the layer's OpenType features. Empty inherits all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<OpenTypeFeature>,
    /// Extra advance in pixels, overriding the layer tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking: Option<f32>,
    /// Absolute line advance in pixels for this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline_style: Option<TextDecoration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike_style: Option<TextDecoration>,
    /// Cross-axis offset in pixels: positive raises horizontal text and moves
    /// vertical text right. It does not change line advance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_shift: Option<f32>,
    /// Explicitly suppress glyph fill, independently from the outline stroke.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<TextStroke>,
    /// Character fill; None inherits the text layer fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
}

impl StyleRun {
    /// True when this run changes nothing, so it can be dropped.
    pub fn is_plain(&self) -> bool {
        self.language.is_none()
            && self.capitalization.is_none()
            && self.small_cap_scale.is_none()
            && self.font_style.is_none()
            && self.family.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.size.is_none()
            && self.metric_size.is_none()
            && self.features.is_empty()
            && self.color.is_none()
            && self.fill_disabled.is_none()
            && self.stroke.is_none()
            && self.tracking.is_none()
            && self.leading.is_none()
            && self.underline.is_none()
            && self.strikethrough.is_none()
            && self.underline_style.is_none()
            && self.strike_style.is_none()
            && self.baseline_shift.is_none()
    }

    /// The overrides alone, without the range: what an edit applies.
    pub fn overrides(&self) -> StyleRun {
        StyleRun {
            start: 0,
            end: 0,
            ..self.clone()
        }
    }

    /// Lay `over`'s overrides on top of this run's.
    pub fn merge(&mut self, over: &StyleRun) {
        if over.language.is_some() {
            self.language = over.language.clone();
        }
        if over.capitalization.is_some() {
            self.capitalization = over.capitalization;
        }
        if over.small_cap_scale.is_some() {
            self.small_cap_scale = over.small_cap_scale;
        }
        if over.family.is_some() {
            self.family = over.family.clone();
        }
        if over.font_style.is_some() {
            self.font_style = over.font_style.clone();
            self.bold = None;
            self.italic = None;
        } else if over.bold.is_some() || over.italic.is_some() {
            if let Some(name) = self.font_style.take() {
                let (bold, italic) = font_style_hints(&name);
                self.bold = Some(bold);
                self.italic = Some(italic);
            }
        }
        if over.bold.is_some() {
            self.bold = over.bold;
        }
        if over.italic.is_some() {
            self.italic = over.italic;
        }
        if over.size.is_some() {
            self.size = over.size;
        }
        if over.metric_size.is_some() {
            self.metric_size = over.metric_size;
        }
        self.features = merged_features(&self.features, &over.features);
        if over.tracking.is_some() {
            self.tracking = over.tracking;
        }
        if over.leading.is_some() {
            self.leading = over.leading;
        }
        if over.baseline_shift.is_some() {
            self.baseline_shift = over.baseline_shift;
        }
        if over.underline_style.is_some() {
            self.underline_style = over.underline_style.clone();
        }
        if over.strike_style.is_some() {
            self.strike_style = over.strike_style.clone();
        }
        if over.strikethrough.is_some() {
            self.strikethrough = over.strikethrough;
        }
        if over.underline.is_some() {
            self.underline = over.underline;
        }
        if over.fill_disabled.is_some() {
            self.fill_disabled = over.fill_disabled;
        }
        if over.stroke.is_some() {
            self.stroke = over.stroke;
        }
        if over.color.is_some() {
            self.color = over.color;
        }
    }

    /// Whether the two runs would set a character the same way.
    fn same_style(&self, other: &StyleRun) -> bool {
        self.language == other.language
            && self.capitalization == other.capitalization
            && self.small_cap_scale == other.small_cap_scale
            && self.font_style == other.font_style
            && self.family == other.family
            && self.bold == other.bold
            && self.italic == other.italic
            && self.size == other.size
            && self.metric_size == other.metric_size
            && self.features == other.features
            && self.color == other.color
            && self.fill_disabled == other.fill_disabled
            && self.stroke == other.stroke
            && self.tracking == other.tracking
            && self.leading == other.leading
            && self.underline == other.underline
            && self.strikethrough == other.strikethrough
            && self.underline_style == other.underline_style
            && self.strike_style == other.strike_style
            && self.baseline_shift == other.baseline_shift
    }
}

/// The font one character is set in, once the layer's own settings and
/// any run covering it have been reconciled.
#[derive(Debug, Clone, PartialEq)]
// Language is independent of face selection, but splits shaping runs.
pub struct CharStyle {
    pub language: String,
    pub capitalization: Capitalization,
    pub small_cap_scale: f32,
    pub fill_disabled: bool,
    pub stroke: Option<TextStroke>,
    pub tracking: f32,
    pub leading: Option<f32>,
    pub underline: bool,
    pub strikethrough: bool,
    pub underline_style: TextDecoration,
    pub strike_style: TextDecoration,
    pub baseline_shift: f32,
    pub color: Option<[u8; 4]>,
    pub family: String,
    pub font_style: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub size: f32,
    pub metric_size: Option<f32>,
    pub features: Vec<OpenTypeFeature>,
}

impl CharStyle {
    fn same_font_request(&self, other: &Self) -> bool {
        let same_name = match (&self.font_style, &other.font_style) {
            (Some(a), Some(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
            (None, None) => true,
            _ => false,
        };
        same_name
            && self.family == other.family
            && self.bold == other.bold
            && self.italic == other.italic
            && self.size == other.size
    }

    /// Decorations do not split shaping runs: toggling a line through a word
    /// must not change its ligatures, kerning, wrapping or caret positions.
    fn shapes_like(&self, other: &Self) -> bool {
        self.same_font_request(other)
            && self.language == other.language
            && self.capitalization == other.capitalization
            && self.small_cap_scale == other.small_cap_scale
            && self.features == other.features
            && self.tracking == other.tracking
            && self.baseline_shift == other.baseline_shift
            && (self.color == other.color || (self.fill_disabled && other.fill_disabled))
            && self.fill_disabled == other.fill_disabled
            && self.stroke == other.stroke
    }

    /// The style as an override that would reproduce it in full.
    pub fn as_run(&self) -> StyleRun {
        StyleRun {
            language: Some(self.language.clone()),
            capitalization: Some(self.capitalization),
            small_cap_scale: Some(self.small_cap_scale),
            start: 0,
            end: 0,
            family: Some(self.family.clone()),
            font_style: self.font_style.clone(),
            bold: Some(self.bold),
            italic: Some(self.italic),
            size: Some(self.size),
            metric_size: self.metric_size,
            features: self.features.clone(),
            color: self.color,
            fill_disabled: Some(self.fill_disabled),
            stroke: Some(self.stroke.unwrap_or_default()),
            tracking: Some(self.tracking),
            leading: self.leading,
            underline: Some(self.underline),
            strikethrough: Some(self.strikethrough),
            underline_style: Some(self.underline_style.clone()),
            strike_style: Some(self.strike_style.clone()),
            baseline_shift: Some(self.baseline_shift),
        }
    }
}

impl TextSpec {
    /// Explicit leading measures baseline/column-center distances, independent
    /// of font ascent. Legacy relative line-height keeps its original box flow.
    pub fn has_absolute_leading(&self) -> bool {
        self.leading
            .into_iter()
            .chain(self.runs.iter().filter_map(|run| run.leading))
            .any(|v| v.is_finite() && v >= 0.0)
    }
    pub fn feature(&self, tag: &str, default: bool) -> bool {
        self.features
            .iter()
            .rev()
            .find(|f| f.tag == tag)
            .map_or(default, |f| f.value != 0)
    }

    pub fn set_feature(&mut self, tag: &str, enabled: bool) {
        self.features.retain(|f| f.tag != tag);
        self.features.push(OpenTypeFeature {
            tag: tag.into(),
            value: u32::from(enabled),
        });
    }

    /// The layer's own font, which uncovered text is set in.
    pub fn base_style(&self) -> CharStyle {
        CharStyle {
            language: language::effective(&self.language),
            capitalization: Capitalization::Normal,
            small_cap_scale: 0.7,
            tracking: self.tracking,
            leading: self.leading.filter(|v| v.is_finite() && *v >= 0.0),
            underline: false,
            strikethrough: false,
            underline_style: TextDecoration::default(),
            strike_style: TextDecoration::default(),
            baseline_shift: 0.0,
            color: None,
            fill_disabled: false,
            stroke: None,
            family: self.family.clone(),
            font_style: self.font_style.clone(),
            bold: self
                .font_style
                .as_deref()
                .map_or(self.bold, |n| font_style_hints(n).0),
            italic: self
                .font_style
                .as_deref()
                .map_or(self.italic, |n| font_style_hints(n).1),
            size: self.size,
            metric_size: None,
            features: merged_features(&[], &self.features),
        }
    }

    /// The font the character at `byte` is set in.
    pub fn style_at(&self, byte: usize) -> CharStyle {
        let mut style = self.base_style();
        if let Some(run) = self.runs.iter().find(|r| r.start <= byte && byte < r.end) {
            if let Some(language) = &run.language {
                style.language = language::effective(language);
            }
            style.capitalization = run.capitalization.unwrap_or_default();
            style.small_cap_scale = run
                .small_cap_scale
                .filter(|v| v.is_finite() && (0.01..=2.0).contains(v))
                .unwrap_or(0.7);
            if let Some(f) = &run.family {
                style.family = f.clone();
            }
            if run.font_style.is_some() || run.bold.is_some() || run.italic.is_some() {
                style.font_style = run.font_style.clone();
            }
            if let Some(b) = run.bold {
                style.bold = b;
            }
            if let Some(i) = run.italic {
                style.italic = i;
            }
            if let Some(name) = &style.font_style {
                (style.bold, style.italic) = font_style_hints(name);
            }
            if let Some(s) = run.size {
                style.size = s;
            }
            style.metric_size = run.metric_size.filter(|v| v.is_finite() && *v > 0.0);
            style.features = merged_features(&style.features, &run.features);
            style.color = run.color;
            style.fill_disabled = run.fill_disabled.unwrap_or(false);
            style.stroke = run
                .stroke
                .filter(|s| s.width.is_finite() && s.width > 0.0)
                .map(|mut stroke| {
                    if !stroke.miter_limit.is_finite() || stroke.miter_limit < 0.0 {
                        stroke.miter_limit = default_miter_limit();
                    }
                    stroke
                });
            style.tracking = run.tracking.unwrap_or(style.tracking);
            style.leading = run
                .leading
                .filter(|v| v.is_finite() && *v >= 0.0)
                .or(style.leading);
            style.underline = run.underline.unwrap_or(false);
            style.strikethrough = run.strikethrough.unwrap_or(false);
            style.underline_style = run.underline_style.clone().unwrap_or_default();
            style.strike_style = run.strike_style.clone().unwrap_or_default();
            style.baseline_shift = run.baseline_shift.filter(|v| v.is_finite()).unwrap_or(0.0);
        }
        style
    }

    /// Every family the text is set in: the layer's own first, then the
    /// runs', without repeats.
    pub fn families(&self) -> Vec<&str> {
        let mut out = vec![self.family.as_str()];
        for run in &self.runs {
            if let Some(f) = &run.family {
                if !out.contains(&f.as_str()) {
                    out.push(f);
                }
            }
        }
        out
    }

    /// Set `range` in `over`'s overrides, splitting whatever runs it
    /// cuts through. A whole-text edit containing only font properties moves
    /// those properties onto the layer and lifts them from every run. Paint
    /// and other per-run settings stay in runs, including whole-text edits.
    pub fn apply_style(&mut self, range: std::ops::Range<usize>, over: &StyleRun) {
        let len = self.text.len();
        let range = range.start.min(len)..range.end.min(len);
        if over.is_plain() {
            return;
        }
        if range.start == 0
            && range.end == len
            && over.capitalization.is_none()
            && over.small_cap_scale.is_none()
            && over.color.is_none()
            && over.stroke.is_none()
            && over.fill_disabled.is_none()
            && over.metric_size.is_none()
            && over.language.is_none()
            && over.features.is_empty()
            && over.tracking.is_none()
            && over.leading.is_none()
            && over.underline.is_none()
            && over.strikethrough.is_none()
            && over.underline_style.is_none()
            && over.strike_style.is_none()
            && over.baseline_shift.is_none()
        {
            if let Some(f) = &over.family {
                self.family = f.clone();
                self.runs.iter_mut().for_each(|r| r.family = None);
            }
            if over.font_style.is_some() || over.bold.is_some() || over.italic.is_some() {
                if let Some(name) = &self.font_style {
                    (self.bold, self.italic) = font_style_hints(name);
                }
                self.font_style = over.font_style.clone();
                self.runs.iter_mut().for_each(|r| {
                    if over.font_style.is_some() {
                        r.bold = None;
                        r.italic = None;
                    } else if let Some(name) = &r.font_style {
                        let (bold, italic) = font_style_hints(name);
                        r.bold = Some(bold);
                        r.italic = Some(italic);
                    }
                    r.font_style = None;
                });
            }
            if let Some(b) = over.bold {
                self.bold = b;
                self.runs.iter_mut().for_each(|r| r.bold = None);
            }
            if let Some(i) = over.italic {
                self.italic = i;
                self.runs.iter_mut().for_each(|r| r.italic = None);
            }
            if let Some(s) = over.size {
                self.size = s;
                self.runs.iter_mut().for_each(|r| r.size = None);
            }
            self.normalize_runs();
            return;
        }
        if range.is_empty() {
            return;
        }
        // Cut the existing runs at the range's edges, so the ones inside
        // can take the override while the parts outside keep theirs.
        let mut runs = Vec::with_capacity(self.runs.len() + 2);
        for run in self.runs.drain(..) {
            if run.end <= range.start || run.start >= range.end {
                runs.push(run);
                continue;
            }
            if run.start < range.start {
                runs.push(StyleRun {
                    end: range.start,
                    ..run.clone()
                });
            }
            let mut inside = StyleRun {
                start: run.start.max(range.start),
                end: run.end.min(range.end),
                ..run.clone()
            };
            inside.merge(over);
            runs.push(inside);
            if run.end > range.end {
                runs.push(StyleRun {
                    start: range.end,
                    ..run
                });
            }
        }
        // Whatever the range covers that no run did gets a fresh run.
        let mut at = range.start;
        let mut covered: Vec<(usize, usize)> = runs
            .iter()
            .filter(|r| r.start >= range.start && r.end <= range.end)
            .map(|r| (r.start, r.end))
            .collect();
        covered.sort_unstable();
        for (s, e) in covered {
            if s > at {
                runs.push(StyleRun {
                    start: at,
                    end: s,
                    ..over.overrides()
                });
            }
            at = at.max(e);
        }
        if at < range.end {
            runs.push(StyleRun {
                start: at,
                end: range.end,
                ..over.overrides()
            });
        }
        self.runs = runs;
        self.normalize_runs();
    }

    /// Keep the runs in step with `text` after `range` was replaced by
    /// `inserted` bytes.
    ///
    /// An edge before the edit stays, one after it shifts, one inside
    /// the replaced span collapses to its start. Text put down at the
    /// end of a run joins it, the way typing after a bold word stays
    /// bold; text put down at a run's start goes before it; text
    /// replacing a selection takes the style of what it replaced.
    pub fn splice_runs(&mut self, range: std::ops::Range<usize>, inserted: usize) {
        let removed = range.end.saturating_sub(range.start);
        let map = |at: usize| -> usize {
            if at < range.start {
                at
            } else if at >= range.end {
                at - removed + inserted
            } else {
                range.start
            }
        };
        for run in &mut self.runs {
            let (s, e) = (run.start, run.end);
            run.start = map(s);
            run.end = map(e);
            // The run holding the first replaced character takes the
            // replacement, however much of the run the selection took.
            if s <= range.start && range.start < e {
                run.end = run.end.max(range.start + inserted);
            }
        }
        self.normalize_runs();
    }

    /// Partition using first-match precedence, drop empty/plain intervals and
    /// join equal neighbours. Normalization must preserve overlapping fallbacks.
    pub fn normalize_runs(&mut self) {
        let len = self.text.len();
        let mut input = std::mem::take(&mut self.runs);
        let mut boundaries = Vec::with_capacity(input.len() * 2);
        for run in &mut input {
            run.start = run.start.min(len);
            run.end = run.end.min(len);
            if run.start < run.end {
                boundaries.extend([run.start, run.end]);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        for interval in boundaries.windows(2) {
            let (start, end) = (interval[0], interval[1]);
            let Some(mut run) = input
                .iter()
                .find(|r| r.start <= start && start < r.end)
                .cloned()
            else {
                continue;
            };
            // Choose before dropping plain runs: an explicit plain interval can
            // mask a later fallback and must leave a plain gap in the result.
            if run.is_plain() {
                continue;
            }
            run.start = start;
            run.end = end;
            if let Some(previous) = self.runs.last_mut() {
                if previous.end == start && previous.same_style(&run) {
                    previous.end = end;
                    continue;
                }
            }
            self.runs.push(run);
        }
    }
}

/// One consecutive paint in glyph draw order. Coverage uses the parent
/// raster's bounds. Separate paints preserve overlapping differently colored
/// glyphs; the merged preview mask alone cannot recover them.
#[derive(Debug, Clone)]
pub struct TextPaint {
    pub color: Option<[u8; 4]>,
    pub coverage: Vec<u8>,
}

/// A rasterized text run: an 8-bit coverage mask and where it sits relative
/// to the text origin.
#[derive(Debug, Clone)]
pub struct TextRaster {
    /// Bounds relative to the layout origin (may start negative: glyphs sit
    /// above the baseline).
    pub bounds: IntRect,
    /// `bounds.width() * bounds.height()` coverage bytes.
    pub coverage: Vec<u8>,
    /// Legacy merged paint colors, empty when glyphs inherit the layer fill.
    /// Use `rgba` to preserve overlapping stroke/fill paints on screen.
    pub colors: Vec<Option<[u8; 4]>>,
    /// Populated by `rasterize_with_paints`, and for overlapping stroke/fill.
    /// Use `rgba` for screen output; colors in separation rasters are opaque IDs.
    pub paints: Vec<TextPaint>,
    /// Baseline of the first line, in the same space as `bounds`. With
    /// `bounds.top` this gives the block's cap height, which is what
    /// page geometry recorded by other apps tends to be measured from.
    pub first_baseline: f32,
    /// Baseline-to-baseline distance actually used, so a caller that
    /// must hit a recorded block height can solve for `line_height`.
    pub line_advance: f32,
    /// The widest line's advance (pen) width — sum of advances rather
    /// than ink extent, so it includes both side bearings. Layout boxes
    /// recorded by other apps measure this, not the ink. The pen box
    /// spans `0..layout_width` in the same space as `bounds`.
    pub layout_width: f32,
    /// The face's capital height at the requested size, when the face
    /// declares one — the distance a flat-topped capital rises above the
    /// baseline, which is less than the ink top of an ascender.
    pub cap_height: Option<f32>,
}

impl TextRaster {
    /// Composite paint coverage to straight RGBA. A merged maximum mask cannot
    /// represent differently colored translucent outline/fill intersections.
    pub fn rgba(&self, fallback: [u8; 4]) -> Vec<u8> {
        if self.paints.is_empty() {
            return self
                .coverage
                .iter()
                .enumerate()
                .flat_map(|(i, &coverage)| {
                    let [r, g, b, a] = self.colors.get(i).copied().flatten().unwrap_or(fallback);
                    [r, g, b, ((coverage as u16 * a as u16 + 127) / 255) as u8]
                })
                .collect();
        }
        let mut out = vec![[0.0f32; 4]; self.coverage.len()];
        for paint in &self.paints {
            let color = paint.color.unwrap_or(fallback);
            for (pixel, &coverage) in out.iter_mut().zip(&paint.coverage) {
                let alpha = coverage as f32 * color[3] as f32 / (255.0 * 255.0);
                for channel in 0..3 {
                    pixel[channel] = color[channel] as f32 * alpha + pixel[channel] * (1.0 - alpha);
                }
                pixel[3] = alpha + pixel[3] * (1.0 - alpha);
            }
        }
        out.into_iter()
            .flat_map(|p| {
                if p[3] <= 0.0 {
                    [0; 4]
                } else {
                    [
                        (p[0] / p[3]).round() as u8,
                        (p[1] / p[3]).round() as u8,
                        (p[2] / p[3]).round() as u8,
                        (p[3] * 255.0).round() as u8,
                    ]
                }
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.bounds.is_empty() || self.coverage.iter().all(|&c| c == 0)
    }
}

/// The process-wide font database, behind a lock rather than a
/// `OnceLock` because installing a font has to take effect at once: a
/// document that asked for a family we just fetched should set in it
/// now, not after a restart.
#[cfg(not(schist_library))]
fn font_db() -> &'static RwLock<Arc<fontdb::Database>> {
    static DB: OnceLock<RwLock<Arc<fontdb::Database>>> = OnceLock::new();
    DB.get_or_init(|| RwLock::new(Arc::new(scan_fonts())))
}

/// A snapshot of the database. Callers hold an `Arc` so a concurrent
/// [`refresh`] swapping in a new scan cannot pull it out from under them.
fn db() -> Arc<fontdb::Database> {
    let cell = font_db();
    let snapshot = match cell.read() {
        Ok(g) => Arc::clone(&g),
        Err(poisoned) => Arc::clone(&poisoned.into_inner()),
    };
    snapshot
}

/// Re-scan the font directories and drop every cached face.
///
/// Call after installing a font. Names previously returned by
/// [`family_names`] stay valid: the list is rebuilt and re-leaked rather
/// than mutated, so a caller still holding the old slice keeps reading
/// good memory.
#[cfg(not(schist_library))]
pub fn refresh() {
    let scanned = Arc::new(scan_fonts());
    match font_db().write() {
        Ok(mut g) => *g = scanned,
        Err(poisoned) => *poisoned.into_inner() = scanned,
    }
    if let Ok(mut cache) = font_cache().lock() {
        cache.clear();
    }
    if let Ok(mut names) = family_name_cache().write() {
        *names = leak_family_names();
    }
}

/// Fonts supplied in memory, without installing files in the user's font
/// directory. The browser also uses this for its startup font catalog.
#[cfg(not(schist_library))]
fn registered_faces() -> &'static std::sync::Mutex<Vec<Vec<u8>>> {
    static FACES: OnceLock<std::sync::Mutex<Vec<Vec<u8>>>> = OnceLock::new();
    FACES.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Register a font from raw bytes for this process without installing it.
#[cfg(not(schist_library))]
pub fn add_font_data(bytes: Vec<u8>) {
    if let Ok(mut faces) = registered_faces().lock() {
        faces.push(bytes);
    }
    refresh();
}

/// Where fonts fetched by the app are installed, alongside whatever the
/// platform already provides.
pub fn font_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Fonts"));
    #[cfg(target_os = "windows")]
    let base =
        std::env::var_os("LOCALAPPDATA").map(|a| PathBuf::from(a).join("Microsoft/Windows/Fonts"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("fonts"));
    base
}

/// Write one font face into [`font_dir`] and make it usable at once.
///
/// `file_name` is trusted only as far as its last component; anything
/// that looks like a path is rejected rather than escaped, since these
/// names come from a remote catalogue.
pub fn install_face(file_name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let stem = std::path::Path::new(file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        .ok_or_else(|| format!("unusable font file name {file_name:?}"))?;
    if !stem
        .rsplit('.')
        .next()
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
    {
        return Err(format!("{stem:?} is not a font file"));
    }
    // Parse it before it lands in a directory the whole system scans:
    // a catalogue that hands us an HTML error page should fail here, not
    // pollute every font list on the machine.
    let mut probe = fontdb::Database::new();
    probe.load_font_data(bytes.to_vec());
    if probe.is_empty() {
        return Err("not a usable font file".into());
    }
    let dir = font_dir().ok_or_else(|| "no user font directory on this platform".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(stem);
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Parsed faces, keyed by the whole request: the bold face of a family
/// is a different file from its regular one.
type FaceKey = (String, Option<String>, bool, bool);

/// A loaded face: the parsed font plus its raw file bytes, kept because
/// fontdue reads only the legacy `kern` table and modern faces store
/// their kerning as GPOS pair adjustments, which layout reads itself.
#[derive(Clone)]
struct LoadedFace {
    font: Arc<fontdue::Font>,
    data: Arc<Vec<u8>>,
    index: u32,
    /// OS/2 `sCapHeight` as a fraction of the em, when declared.
    cap_ratio: Option<f32>,
}

#[cfg(not(schist_library))]
fn font_cache() -> &'static std::sync::Mutex<std::collections::HashMap<FaceKey, Option<LoadedFace>>>
{
    static CACHE: OnceLock<
        std::sync::Mutex<std::collections::HashMap<FaceKey, Option<LoadedFace>>>,
    > = OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn scan_fonts() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    #[cfg(schist_library)]
    db.load_font_data(include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec());
    db.load_system_fonts();
    if let Some(dir) = font_dir() {
        db.load_fonts_dir(dir);
    }
    // Font Book is sandboxed on modern macOS, so a font "installed" by
    // double-clicking lands in its container rather than ~/Library/Fonts.
    // Every CoreText app sees it; a directory scan does not, which is why
    // user-installed fonts were missing from the font menu.
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        db.load_fonts_dir(
            PathBuf::from(home).join("Library/Containers/com.apple.FontBook/Data/Library/Fonts"),
        );
    }
    #[cfg(not(schist_library))]
    if let Ok(faces) = registered_faces().lock() {
        for bytes in faces.iter() {
            db.load_font_data(bytes.clone());
        }
    }
    // Point the generic families at something that actually exists, so
    // an unknown family name resolves through `Family::SansSerif`
    // instead of failing the query outright.
    let pick = |db: &fontdb::Database, candidates: &[&str]| -> Option<String> {
        candidates
            .iter()
            .find(|c| db.faces().any(|f| f.families.iter().any(|(n, _)| n == *c)))
            .map(|c| c.to_string())
            .or_else(|| {
                db.faces()
                    .next()
                    .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
            })
    };
    if let Some(name) = pick(
        &db,
        &[
            "DejaVu Sans",
            "Noto Sans",
            "Liberation Sans",
            "Arial",
            "Helvetica",
        ],
    ) {
        db.set_sans_serif_family(name);
    }
    if let Some(name) = pick(
        &db,
        &[
            "DejaVu Serif",
            "Noto Serif",
            "Liberation Serif",
            "Times New Roman",
        ],
    ) {
        db.set_serif_family(name);
    }
    if let Some(name) = pick(
        &db,
        &[
            "DejaVu Sans Mono",
            "Noto Sans Mono",
            "Liberation Mono",
            "Courier New",
        ],
    ) {
        db.set_monospace_family(name);
    }
    log::debug!("text-engine: {} font faces", db.len());
    db
}

/// Families available on this system, sorted and de-duplicated.
pub fn families() -> Vec<String> {
    let mut names: Vec<String> = db()
        .faces()
        .filter_map(|f| f.families.first().map(|(name, _)| name.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// True when this exact family is installed — not a substitute for it.
///
/// [`rasterize`] never fails on an unknown family (the query falls
/// through to the generic sans), so this is the only way to tell that a
/// document asked for something we do not have.
pub fn has_family(name: &str) -> bool {
    let name = name.trim();
    db().faces()
        .any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
}

#[cfg(not(schist_library))]
fn leak_family_names() -> &'static [&'static str] {
    let names: Vec<&'static str> = families()
        .into_iter()
        .map(|n| &*Box::leak(n.into_boxed_str()))
        .collect();
    Box::leak(names.into_boxed_slice())
}

#[cfg(not(schist_library))]
fn family_name_cache() -> &'static RwLock<&'static [&'static str]> {
    static NAMES: OnceLock<RwLock<&'static [&'static str]>> = OnceLock::new();
    NAMES.get_or_init(|| RwLock::new(leak_family_names()))
}

/// The installed families as a fixed list, for controls that need one.
///
/// The options bar asks for this on every frame it draws, so the list is
/// built once and leaked rather than re-collected; [`refresh`] rebuilds
/// it after an install.
#[cfg(not(schist_library))]
pub fn family_names() -> &'static [&'static str] {
    match family_name_cache().read() {
        Ok(g) => *g,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

/// A reasonable default family: whatever the database resolved as its
/// sans-serif alias.
pub fn default_family() -> String {
    db().family_name(&fontdb::Family::SansSerif).to_string()
}

/// The installed face's PostScript identifier for editable file interchange.
/// Do not silently substitute a different family when exporting a font name.
pub fn postscript_name(family: &str, bold: bool, italic: bool) -> Option<String> {
    let database = db();
    let id = database.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        },
        style: if italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
        ..Default::default()
    })?;
    let face = database.face(id)?;
    if (face.weight >= fontdb::Weight::BOLD) != bold
        || (face.style != fontdb::Style::Normal) != italic
    {
        return None;
    }
    Some(face.post_script_name.clone())
}

/// Resolve an imported PostScript identifier to this system's family and face.
pub fn family_from_postscript(name: &str) -> Option<(String, bool, bool)> {
    let database = db();
    let face = database
        .faces()
        .find(|face| face.post_script_name == name)?;
    Some((
        face.families.first()?.0.clone(),
        face.weight >= fontdb::Weight::BOLD,
        face.style != fontdb::Style::Normal,
    ))
}

/// Metric-compatible stand-ins for a family this system lacks, best
/// match first.
///
/// A document was laid out against real advance widths, so substituting
/// Arial with a Helvetica clone reproduces its line breaks and its
/// measured text extents; falling straight through to the generic sans
/// (often DejaVu, which is appreciably wider) does not. Only families
/// designed to share metrics belong here — this is not a lookalike
/// table.
pub fn substitutes(family: &str) -> &'static [&'static str] {
    match family.trim().to_ascii_lowercase().as_str() {
        "arial" | "arial mt" | "helvetica" | "helvetica neue" | "swiss 721" => &[
            "Liberation Sans",
            "Arimo",
            "Nimbus Sans",
            "Helvetica",
            "Arial",
        ],
        "times" | "times new roman" | "timesnewromanpsmt" => &[
            "Liberation Serif",
            "Tinos",
            "Nimbus Roman",
            "Times New Roman",
        ],
        "courier" | "courier new" => &[
            "Liberation Mono",
            "Cousine",
            "Nimbus Mono PS",
            "Courier New",
        ],
        "georgia" => &["Gelasio", "Tinos"],
        "verdana" | "tahoma" => &["DejaVu Sans", "Bitstream Vera Sans"],
        "calibri" => &["Carlito", "Liberation Sans"],
        "cambria" => &["Caladea", "Liberation Serif"],
        _ => &[],
    }
}

/// The best metric-compatible stand-in for a family, whether or not it
/// is installed yet — what to offer someone whose document names a font
/// they cannot legally be given.
///
/// Returns `None` for a family with no such twin; that one has to be
/// found in a font catalogue or not at all.
pub fn nearest_substitute(family: &str) -> Option<&'static str> {
    substitutes(family).first().copied()
}

fn font_family(family: &str) -> fontdb::Family<'_> {
    match family {
        "sans-serif" | "system-ui" => fontdb::Family::SansSerif,
        "serif" => fontdb::Family::Serif,
        "monospace" => fontdb::Family::Monospace,
        _ => fontdb::Family::Name(family),
    }
}

fn font_name(name: ttf_parser::name::Name<'_>) -> Option<String> {
    name.to_string().or_else(|| {
        (name.platform_id == ttf_parser::PlatformId::Macintosh && name.encoding_id == 0).then(
            || {
                encoding_rs::MACINTOSH
                    .decode_without_bom_handling(name.name)
                    .0
                    .into_owned()
            },
        )
    })
}

/// Match a static face's typographic subfamily, falling back to the legacy
/// subfamily only when no typographic subfamily exists. A Light face can have
/// legacy name "Regular"; matching both would select the wrong font.
fn named_face(database: &fontdb::Database, family: &str, name: &str) -> Option<fontdb::ID> {
    let requested = font_family(family.trim());
    let family = database.family_name(&requested);
    database.faces().find_map(|info| {
        if !info
            .families
            .iter()
            .any(|(f, _)| f.eq_ignore_ascii_case(family.trim()))
        {
            return None;
        }
        database
            .with_face_data(info.id, |data, index| {
                let face = ttf_parser::Face::parse(data, index).ok()?;
                let names = face.names();
                let id = if names
                    .into_iter()
                    .any(|n| n.name_id == ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY)
                {
                    ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY
                } else {
                    ttf_parser::name_id::SUBFAMILY
                };
                names
                    .into_iter()
                    .any(|n| {
                        n.name_id == id
                            && font_name(n).is_some_and(|s| s.eq_ignore_ascii_case(name.trim()))
                    })
                    .then_some(info.id)
            })
            .flatten()
    })
}

/// Conventional weight/slant hints when an exact named face is unavailable.
/// These are fallback hints, never a replacement for named face matching.
pub fn font_style_hints(name: &str) -> (bool, bool) {
    let name = name.to_ascii_lowercase();
    (
        name.contains("bold") || name.contains("black") || name.contains("heavy"),
        name.contains("italic") || name.contains("oblique"),
    )
}

/// Whether this exact family and named static face are available. Rendering
/// substitutes a font when absent; preflight must disclose that substitution.
pub fn has_font_style(family: &str, name: &str) -> bool {
    named_face(&db(), family, name).is_some()
}

/// Load and cache a parsed font by family name.
fn load_font(
    family: &str,
    font_style: Option<&str>,
    bold: bool,
    italic: bool,
) -> Option<LoadedFace> {
    let (bold, italic) = font_style.map_or((bold, italic), font_style_hints);
    let cache = font_cache();
    let key = (
        family.to_string(),
        font_style.map(str::to_owned),
        bold,
        italic,
    );
    if let Some(hit) = cache.lock().ok()?.get(&key) {
        return hit.clone();
    }

    // Asked-for family first, then its metric equivalents, then the
    // generic sans as a last resort.
    let mut families = vec![font_family(family)];
    families.extend(substitutes(family).iter().map(|n| fontdb::Family::Name(n)));
    families.push(fontdb::Family::SansSerif);
    let query = fontdb::Query {
        families: &families,
        weight: if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        },
        style: if italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
        ..Default::default()
    };
    let database = db();
    let font = font_style
        .and_then(|name| named_face(&database, family, name))
        .or_else(|| database.query(&query))
        .and_then(|id| {
            database
                .with_face_data(id, |data, index| {
                    fontdue::Font::from_bytes(
                        data,
                        fontdue::FontSettings {
                            collection_index: index,
                            ..Default::default()
                        },
                    )
                    .ok()
                    .map(|font| LoadedFace {
                        font: Arc::new(font),
                        data: Arc::new(data.to_vec()),
                        index,
                        cap_ratio: ttf_parser::Face::parse(data, index).ok().and_then(|f| {
                            let cap = f.capital_height().filter(|&c| c > 0)? as f32;
                            Some(cap / f.units_per_em() as f32)
                        }),
                    })
                })
                .flatten()
        });
    if font.is_none() {
        log::warn!("text-engine: no usable font for {family:?}");
    }
    if let Ok(mut c) = cache.lock() {
        c.insert(key, font.clone());
    }
    font
}

/// The GPOS `kern`-feature pair adjustments of a face, resolved once per
/// layout. fontdue reads only the legacy `kern` table; most modern faces
/// keep their kerning here instead, and Affinity applies it, so matching
/// its line widths requires it.
struct GposKern<'a> {
    face: ttf_parser::Face<'a>,
    subtables: Vec<ttf_parser::gpos::PairAdjustment<'a>>,
    /// Pixels per font unit at the requested size.
    scale: f32,
}

impl<'a> GposKern<'a> {
    fn new(data: &'a [u8], index: u32, size: f32) -> Option<Self> {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let gpos = face.tables().gpos?;
        let mut lookup_indices: Vec<u16> = Vec::new();
        for feature in gpos.features {
            if feature.tag == ttf_parser::Tag::from_bytes(b"kern") {
                for i in feature.lookup_indices {
                    if !lookup_indices.contains(&i) {
                        lookup_indices.push(i);
                    }
                }
            }
        }
        let mut subtables = Vec::new();
        for i in lookup_indices {
            let Some(lookup) = gpos.lookups.get(i) else {
                continue;
            };
            for j in 0..lookup.subtables.len() {
                if let Some(ttf_parser::gpos::PositioningSubtable::Pair(pair)) =
                    lookup
                        .subtables
                        .get::<ttf_parser::gpos::PositioningSubtable>(j)
                {
                    subtables.push(pair);
                }
            }
        }
        if subtables.is_empty() {
            return None;
        }
        let upem = face.units_per_em();
        Some(Self {
            scale: size / upem as f32,
            face,
            subtables,
        })
    }

    /// Advance adjustment for the adjacent pair `(prev, next)`, in px.
    /// The first subtable that covers the pair speaks for the face.
    fn kern(&self, prev: char, next: char) -> Option<f32> {
        use ttf_parser::gpos::PairAdjustment;
        let a = self.face.glyph_index(prev)?;
        let b = self.face.glyph_index(next)?;
        for st in &self.subtables {
            match st {
                PairAdjustment::Format1 { coverage, sets } => {
                    if let Some(idx) = coverage.get(a) {
                        if let Some((first, _)) = sets.get(idx).and_then(|s| s.get(b)) {
                            return Some(first.x_advance as f32 * self.scale);
                        }
                    }
                }
                PairAdjustment::Format2 {
                    coverage,
                    classes,
                    matrix,
                } => {
                    if coverage.contains(a) {
                        let pair = (classes.0.get(a), classes.1.get(b));
                        if let Some((first, _)) = matrix.get(pair) {
                            return Some(first.x_advance as f32 * self.scale);
                        }
                    }
                }
            }
        }
        None
    }
}

/// One laid-out glyph, positioned relative to the layout origin.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlacedGlyph {
    glyph: u16,
    byte: usize,
    x: f32,
    baseline: f32,
    /// Index into the layout's faces: which font, at which size.
    face: usize,
    /// Sideways Latin runs in vertical writing.
    sideways: bool,
}

/// Where a character's pen starts, for caret placement.
#[derive(Debug, Clone, Copy)]
struct CharPos {
    byte: usize,
    x: f32,
    /// The other edge of this logical character, decreasing for RTL.
    end_x: f32,
}

/// Laid-out glyphs, the widest line, the first baseline and the
/// baseline-to-baseline step.
#[derive(Default)]
struct Layout {
    glyphs: Vec<PlacedGlyph>,
    first_baseline: f32,
    line_advance: f32,
    layout_width: f32,
    lines: Vec<LineSpan>,
    chars: Vec<CharPos>,
    /// Selected explicit stop for each source tab. Paint is generated later,
    /// so line measurement and wrapping do not enumerate repeated glyphs.
    tab_stops: Vec<(usize, usize)>,
}

/// The faces a spec sets its text in, one per distinct family, style
/// and size, and which of them each byte of the text uses. Index 0 is
/// the layer's own font.
struct Faces {
    faces: Vec<(LoadedFace, f32)>,
    by_byte: Vec<usize>,
    uppercase: Vec<bool>,
    synthetic_caps: Vec<bool>,
    leaders: Vec<tab_leaders::Pattern>,
}

impl Faces {
    fn resolve(spec: &TextSpec, base: &LoadedFace) -> Faces {
        let base_style = spec.base_style();
        let mut styles = vec![base_style.clone()];
        let mut faces = vec![(base.clone(), spec.size)];
        let mut by_byte = vec![0usize; spec.text.len()];
        // Runs may overlap (a paragraph fallback follows more specific runs).
        // Resolve disjoint intervals with style_at's first-match precedence;
        // assigning every byte of the last run would erase local font overrides.
        let mut boundaries = vec![0, spec.text.len()];
        for run in &spec.runs {
            boundaries.extend([run.start.min(spec.text.len()), run.end.min(spec.text.len())]);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        for interval in boundaries.windows(2) {
            let (s, e) = (interval[0], interval[1]);
            let style = spec.style_at(s);
            // Only face selection and glyph size choose a rasterizer. Equivalent
            // case-insensitive names must not create a false shaping boundary.
            if style.same_font_request(&base_style) {
                continue;
            }
            let ix = match styles.iter().position(|k| k.same_font_request(&style)) {
                Some(ix) => ix,
                None => {
                    // A family that cannot be loaded falls back to the
                    // layer's own face, at the run's size.
                    let face = load_font(
                        &style.family,
                        style.font_style.as_deref(),
                        style.bold,
                        style.italic,
                    )
                    .unwrap_or_else(|| base.clone());
                    styles.push(style.clone());
                    faces.push((face, style.size));
                    faces.len() - 1
                }
            };
            for b in &mut by_byte[s..e] {
                *b = ix;
            }
        }
        let display_case = spec.runs.iter().any(|run| {
            matches!(
                run.capitalization,
                Some(Capitalization::AllCaps | Capitalization::SmallCaps)
            )
        });
        let case_bytes = if display_case { spec.text.len() } else { 0 };
        let mut uppercase = vec![false; case_bytes];
        let mut synthetic_caps = vec![false; case_bytes];
        let mut probes = std::collections::HashMap::new();
        let mut scaled = Vec::<(usize, f32, usize)>::new();
        if display_case {
            for (start, grapheme) in spec.text.grapheme_indices(true) {
                let style = spec.style_at(start);
                let end = start + grapheme.len();
                if style.capitalization == Capitalization::AllCaps {
                    uppercase[start..end].fill(true);
                } else if style.capitalization == Capitalization::SmallCaps
                    && grapheme.chars().any(char::is_lowercase)
                    && !style
                        .features
                        .iter()
                        .any(|f| f.tag == "smcp" && f.value == 0)
                {
                    let ix = by_byte[start];
                    let native = *probes
                        .entry((ix, grapheme, style.language.clone()))
                        .or_insert_with(|| {
                            capitalization::has_small_caps(&faces[ix].0, grapheme, &style.language)
                        });
                    if !native {
                        let size = faces[ix].1 * style.small_cap_scale;
                        let scaled_ix = scaled
                            .iter()
                            .find(|(base, value, _)| *base == ix && *value == size)
                            .map(|(_, _, index)| *index)
                            .unwrap_or_else(|| {
                                let index = faces.len();
                                faces.push((faces[ix].0.clone(), size));
                                scaled.push((ix, size, index));
                                index
                            });
                        by_byte[start..end].fill(scaled_ix);
                        uppercase[start..end].fill(true);
                        synthetic_caps[start..end].fill(true);
                    }
                }
            }
        }
        let mut resolved = Faces {
            faces,
            by_byte,
            uppercase,
            synthetic_caps,
            leaders: Vec::new(),
        };
        tab_leaders::resolve(spec, base, &mut resolved);
        resolved
    }

    fn at(&self, byte: usize) -> usize {
        self.by_byte.get(byte).copied().unwrap_or(0)
    }

    fn line_metrics_at(&self, spec: &TextSpec, byte: usize) -> (f32, f32) {
        let ix = self.at(byte);
        let style = spec.style_at(byte);
        let Some(size) = style.metric_size.or_else(|| {
            self.synthetic_caps
                .get(byte)
                .copied()
                .unwrap_or(false)
                .then_some(style.size)
        }) else {
            return self.line_metrics(ix);
        };
        self.faces[ix]
            .0
            .font
            .horizontal_line_metrics(size)
            .map(|m| (m.ascent, m.new_line_size))
            .unwrap_or((size * 0.8, size * 1.2))
    }

    /// Ascent and natural line step of face `ix`.
    fn line_metrics(&self, ix: usize) -> (f32, f32) {
        let (face, size) = &self.faces[ix];
        face.font
            .horizontal_line_metrics(*size)
            .map(|m| (m.ascent, m.new_line_size))
            .unwrap_or((size * 0.8, size * 1.2))
    }
}

/// The largest requested leading on a line controls its distance from the
/// preceding baseline. Unset runs use font metrics; explicit tight values may
/// deliberately overlap nominal cells.
fn run_line_advance(
    spec: &TextSpec,
    faces: &Faces,
    start: usize,
    end: usize,
    fallback: f32,
) -> f32 {
    spec.text[start..end]
        .char_indices()
        .map(|(i, _)| {
            spec.style_at(start + i)
                .leading
                .filter(|v| v.is_finite() && *v >= 0.0)
                .unwrap_or_else(|| {
                    faces.line_metrics_at(spec, start + i).1 * spec.line_height.max(0.1)
                })
        })
        .reduce(f32::max)
        .unwrap_or_else(|| {
            spec.leading
                .filter(|v| v.is_finite() && *v >= 0.0)
                .unwrap_or(fallback * spec.line_height.max(0.1))
        })
}

/// One laid-out line, and the byte range of `TextSpec::text` it covers.
///
/// Wrapping means a line does not always correspond to a source line, so
/// the range is what lets a caret offset be mapped onto the page. In vertical
/// writing these remain inline/block coordinates: x/width measure down the
/// column, and top/height measure the logical column position and spacing.
/// Use `caret_at`, `hit_test` and `selection_rects` for canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineSpan {
    /// Byte offset of the line's first character in `TextSpec::text`.
    pub start: usize,
    /// Byte offset one past the line's last character.
    pub end: usize,
    /// x of the line's first glyph, after alignment.
    pub x: f32,
    /// Advance width of the line.
    pub width: f32,
    /// y of the line's top, relative to the raster origin.
    pub top: f32,
    /// Baseline in logical block coordinates, relative to the raster origin.
    pub baseline: f32,
    /// Nominal line cell height. Explicit leading can overlap adjacent cells;
    /// glyphs and carets retain their font metrics.
    pub height: f32,
    /// Requested advance from the preceding baseline/vertical column center.
    /// Legacy relative spacing advances line boxes, using the same cell height.
    pub advance: f32,
}

fn next_line_top(
    previous: Option<&LineSpan>,
    ascent: f32,
    height: f32,
    advance: f32,
    mode: WritingMode,
    absolute: bool,
) -> f32 {
    previous.map_or(0.0, |previous| {
        if !absolute {
            previous.top + previous.height
        } else if mode.is_vertical() {
            previous.top + previous.height / 2.0 + advance - height / 2.0
        } else {
            previous.baseline + advance - ascent
        }
    })
}
fn block_extent(lines: &[LineSpan]) -> f32 {
    lines
        .iter()
        .map(|line| line.top + line.height)
        .fold(0.0, f32::max)
}

/// Where a caret sits, relative to the text raster's origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub x: f32,
    pub top: f32,
    pub height: f32,
    /// Clockwise rotation in radians; zero for ordinary horizontal text.
    pub angle: f32,
}

impl Caret {
    /// Distance to this caret's line segment: cross-axis first, then inline.
    /// Comparing these pairs keeps drags beyond a short line on that line.
    pub fn hit_distance(self, x: f32, y: f32) -> (f32, f32) {
        let (sin, cos) = self.angle.sin_cos();
        let (dx, dy) = (x - self.x, y - self.top);
        let along = -dx * sin + dy * cos;
        let cross = if along < 0.0 {
            -along
        } else {
            (along - self.height).max(0.0)
        };
        (cross, (dx * cos + dy * sin).abs())
    }
}

/// A logical boundary can have two visual positions where directional runs
/// meet. Downstream follows the next character; upstream follows the previous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaretAffinity {
    Upstream,
    #[default]
    Downstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaretPosition {
    pub byte: usize,
    pub affinity: CaretAffinity,
}

impl From<usize> for CaretPosition {
    fn from(byte: usize) -> Self {
        Self {
            byte,
            affinity: CaretAffinity::Downstream,
        }
    }
}

/// Physical keyboard movement, independent of the paragraph's reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretMovement {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

fn layout(spec: &TextSpec, base: &LoadedFace) -> Layout {
    layout_with_widths(spec, base, &[])
}

fn wrap_width_at(spec: &TextSpec, widths: &[f32], index: usize) -> Option<f32> {
    widths
        .get(index)
        .or_else(|| widths.last())
        .copied()
        .or(spec.wrap_width)
}

fn layout_with_widths(spec: &TextSpec, base: &LoadedFace, widths: &[f32]) -> Layout {
    let measures = widths
        .iter()
        .map(|width| InlineMeasure {
            width: *width,
            start: 0.0,
        })
        .collect::<Vec<_>>();
    layout_with_measures(spec, base, &measures)
}

fn layout_with_measures(spec: &TextSpec, base: &LoadedFace, measures: &[InlineMeasure]) -> Layout {
    if spec.tabs.as_ref().is_some_and(|tabs| !tabs.valid()) {
        return Layout::default();
    }
    if shaping::required(spec) {
        return shaping::layout(spec, base, measures);
    }
    let widths = measures.iter().map(|m| m.width).collect::<Vec<_>>();
    let faces = Faces::resolve(spec, base);
    // A face with GPOS kerning speaks through it alone; the legacy
    // `kern` table is only consulted when there is no GPOS to read.
    // Kerning is looked up per face, and only between neighbours set in
    // the same face at the same size: a pair that straddles a font
    // change has no table to consult.
    let kerns: Vec<Option<GposKern>> = faces
        .faces
        .iter()
        .map(|(face, size)| GposKern::new(&face.data, face.index, *size))
        .collect();
    let advance = |ch: char, prev: Option<(char, usize)>, ix: usize, byte: usize| -> f32 {
        let (face, size) = &faces.faces[ix];
        let m = face.font.metrics(ch, *size);
        let kern = prev
            .filter(|(_, pix)| *pix == ix)
            .and_then(|(p, _)| match &kerns[ix] {
                Some(g) => g.kern(p, ch),
                None => face.font.horizontal_kern(p, ch, *size),
            })
            .unwrap_or(0.0);
        m.advance_width
            + kern
            + spec.style_at(byte).tracking
            + if ch == ' ' { spec.word_spacing } else { 0.0 }
    };
    // Advance of `word` starting at byte `from`, after `prev`.
    let measure_word = |word: &str, from: usize, mut prev: Option<(char, usize)>| -> f32 {
        let mut width = 0.0;
        for (i, ch) in word.char_indices() {
            let ix = faces.at(from + i);
            width += advance(ch, prev, ix, from + i);
            prev = Some((ch, ix));
        }
        width
    };

    // Split into wrapped lines, carrying each line's byte range in
    // `spec.text` so a caret offset can be mapped onto the page. The
    // caret and the glyphs must come from the same pass: measuring them
    // separately is what let the overlay drift away from the ink.
    struct Line {
        text: String,
        width: f32,
        start: usize,
        end: usize,
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut line_start = 0usize;
    for raw_line in spec.text.split('\n') {
        let mut current = String::new();
        let mut width = 0.0f32;
        let mut prev: Option<(char, usize)> = None;
        let mut start = line_start;
        let mut at = line_start;
        // Wrap on word boundaries; a single over-long word is left to
        // overflow rather than being broken mid-word.
        for word in raw_line.split_inclusive(' ') {
            let mut word_width = measure_word(word, at, prev);
            let wraps = wrap_width_at(spec, &widths, lines.len()).is_some_and(|w| {
                spec.path.is_none() && !current.is_empty() && width + word_width > w
            });
            if wraps {
                lines.push(Line {
                    text: std::mem::take(&mut current),
                    width,
                    start,
                    end: at,
                });
                start = at;
                width = 0.0;
                // Re-measure the word with no kerning context: it now
                // starts a line, so there is no preceding glyph.
                word_width = measure_word(word, at, None);
            }
            current.push_str(word);
            width += word_width;
            prev = word
                .char_indices()
                .last()
                .map(|(i, ch)| (ch, faces.at(at + i)));
            at += word.len();
        }
        lines.push(Line {
            text: current,
            width,
            start,
            end: at,
        });
        // Step past this source line and the newline that ended it.
        line_start = at + 1;
    }

    let max_width = lines.iter().map(|l| l.width).fold(0.0f32, f32::max);
    let mut placed = Vec::new();
    let mut chars = Vec::new();
    let mut spans = Vec::with_capacity(lines.len());
    let mut first_baseline = 0.0;
    let mut first_advance = 0.0;
    let absolute = spec.has_absolute_leading();
    for (i, line) in lines.iter().enumerate() {
        // Nominal script sizes keep line geometry independent of glyph scaling.
        let (ascent, line_gap) = line
            .text
            .char_indices()
            .map(|(byte, _)| faces.line_metrics_at(spec, line.start + byte))
            .reduce(|(a, g), (fa, fg)| (a.max(fa), g.max(fg)))
            .unwrap_or_else(|| faces.line_metrics(0));
        let line_advance = run_line_advance(spec, &faces, line.start, line.end, line_gap);
        let height = if absolute { line_gap } else { line_advance };
        let top = next_line_top(
            spans.last(),
            ascent,
            height,
            line_advance,
            spec.writing_mode,
            absolute,
        );
        let baseline = top + ascent;
        if i == 0 {
            first_baseline = ascent;
            first_advance = line_advance;
        }
        let start_x = match spec.align {
            Align::Left => 0.0,
            Align::Center => (max_width - line.width) / 2.0,
            Align::Right => max_width - line.width,
        };
        spans.push(LineSpan {
            start: line.start,
            end: line.end,
            x: start_x,
            width: line.width,
            top,
            baseline,
            height,
            advance: line_advance,
        });
        let mut x = start_x;
        let mut prev: Option<(char, usize)> = None;
        for (k, ch) in line.text.char_indices() {
            let byte = line.start + k;
            let ix = faces.at(byte);
            chars.push(CharPos {
                byte,
                x,
                end_x: x + advance(ch, prev, ix, byte),
            });
            if !ch.is_whitespace() {
                placed.push(PlacedGlyph {
                    glyph: faces.faces[ix].0.font.lookup_glyph_index(ch),
                    byte,
                    x,
                    baseline: baseline - spec.style_at(byte).baseline_shift,
                    face: ix,
                    sideways: false,
                });
            }
            x += advance(ch, prev, ix, byte);
            prev = Some((ch, ix));
        }
    }
    Layout {
        glyphs: placed,
        first_baseline,
        line_advance: first_advance,
        layout_width: max_width,
        lines: spans,
        chars,
        tab_stops: Vec::new(),
    }
}

/// The laid-out lines of `spec`, with the byte range of `spec.text` each
/// one covers.
///
/// Returns an empty vec when no font can be loaded.
pub fn line_spans(spec: &TextSpec) -> Vec<LineSpan> {
    line_spans_with_widths(spec, &[])
}

/// Resolve an automatic paragraph direction before splitting it into frames
/// or rendering individual lines. Continuations retain this original context.
pub fn base_direction(text: &str) -> ParagraphDirection {
    if unicode_bidi::BidiInfo::new(text, None)
        .paragraphs
        .first()
        .is_some_and(|p| p.level.is_rtl())
    {
        ParagraphDirection::RightToLeft
    } else {
        ParagraphDirection::LeftToRight
    }
}

/// Compose a paragraph against successive inline measures, keeping full
/// shaping context. The final width repeats for remaining lines. An empty
/// slice uses `spec.wrap_width`. Callers supply finite, positive widths.
pub fn line_spans_with_widths(spec: &TextSpec, widths: &[f32]) -> Vec<LineSpan> {
    let measures = widths
        .iter()
        .map(|width| InlineMeasure {
            width: *width,
            start: 0.0,
        })
        .collect::<Vec<_>>();
    line_spans_with_measures(spec, &measures)
}

/// Compose using paired widths and inline starts. A tab's column origin stays
/// fixed when indents, list markers or initials move an individual line's start.
pub fn line_spans_with_measures(spec: &TextSpec, measures: &[InlineMeasure]) -> Vec<LineSpan> {
    if measures
        .iter()
        .any(|m| !m.width.is_finite() || m.width <= 0.0 || !m.start.is_finite())
    {
        return Vec::new();
    }
    let Some(face) = load_font(
        &spec.family,
        spec.font_style.as_deref(),
        spec.bold,
        spec.italic,
    ) else {
        return Vec::new();
    };
    layout_with_measures(spec, &face, measures).lines
}

/// Where a caret sitting at `byte` in `spec.text` lands, relative to the
/// raster's top-left origin.
///
/// `byte` is clamped into range and snapped to a char boundary, so a
/// caller that has lost track of the text cannot panic the layout.
pub fn caret_at(spec: &TextSpec, byte: usize) -> Option<Caret> {
    caret_at_position(spec, byte.into())
}

pub fn caret_at_position(spec: &TextSpec, position: CaretPosition) -> Option<Caret> {
    let face = load_font(
        &spec.family,
        spec.font_style.as_deref(),
        spec.bold,
        spec.italic,
    )?;
    let laid = layout(spec, &face);
    let caret = caret_in_layout_affinity(spec, &laid, position);
    Some(match path_guide(spec, &laid) {
        Some(guide) => guide.caret(caret, laid.first_baseline),
        None => caret,
    })
}

fn path_guide(spec: &TextSpec, laid: &Layout) -> Option<text_path::Guide> {
    if spec.writing_mode.is_vertical() {
        return None;
    }
    text_path::Guide::new(spec.path.as_ref()?, spec.align, laid.layout_width)
}

/// All insertion points in a single layout pass, including the final one.
pub fn carets(spec: &TextSpec) -> Vec<(usize, Caret)> {
    let Some(face) = load_font(
        &spec.family,
        spec.font_style.as_deref(),
        spec.bold,
        spec.italic,
    ) else {
        return Vec::new();
    };
    let laid = layout(spec, &face);
    let guide = path_guide(spec, &laid);
    spec.text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain(std::iter::once(spec.text.len()))
        .map(|i| {
            let caret = caret_in_layout(spec, &laid, i);
            (
                i,
                guide
                    .as_ref()
                    .map_or(caret, |g| g.caret(caret, laid.first_baseline)),
            )
        })
        .collect()
}

fn caret_in_layout(spec: &TextSpec, laid: &Layout, byte: usize) -> Caret {
    caret_in_layout_affinity(spec, laid, byte.into())
}

fn caret_in_layout_affinity(spec: &TextSpec, laid: &Layout, position: CaretPosition) -> Caret {
    let byte = clamp_to_boundary(&spec.text, position.byte);

    // The last line whose range starts at or before `byte`: with an
    // explicit newline the offset sits in two ranges (the end of one and
    // the start of the next), and a caret after a newline belongs on the
    // new line.
    let span = laid
        .lines
        .iter()
        .rev()
        .find(|l| {
            l.start <= byte
                && !(position.affinity == CaretAffinity::Upstream
                    && byte == l.start
                    && byte > 0
                    && !spec.text[..byte].chars().next_back().is_some_and(|c| {
                        unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::B || c == '\u{2028}'
                    }))
        })
        .copied()
        .or_else(|| laid.lines.first().copied())
        .unwrap_or(LineSpan {
            start: 0,
            end: 0,
            x: 0.0,
            width: 0.0,
            top: 0.0,
            baseline: laid.first_baseline,
            height: laid.line_advance,
            advance: laid.line_advance,
        });

    let upto = byte.clamp(span.start, span.end);
    // The pen position of the character the caret sits before, or the
    // line's end after its last one. Same pass as the glyphs, so a
    // caret between two fonts lands exactly where the ink changes.
    let x = if upto >= span.end
        || (position.affinity == CaretAffinity::Upstream && upto > span.start)
    {
        laid.chars
            .iter()
            .filter(|c| span.start <= c.byte && c.byte < upto)
            .max_by_key(|c| c.byte)
            .map_or(span.x, |c| c.end_x)
    } else {
        laid.chars
            .iter()
            .find(|c| c.byte == upto)
            .map(|c| c.x)
            .unwrap_or(span.x + span.width)
    };
    let style_byte = if upto >= span.end
        || (position.affinity == CaretAffinity::Upstream && upto > span.start)
    {
        spec.text[..upto]
            .grapheme_indices(true)
            .next_back()
            .map_or(upto, |(at, _)| at)
    } else {
        upto
    };
    styled_caret(spec, laid, span, style_byte, x)
}

/// The same styled insertion segment bounds the caret and each selection cell.
fn styled_caret(spec: &TextSpec, laid: &Layout, span: LineSpan, byte: usize, x: f32) -> Caret {
    let style = spec.style_at(byte);
    let ratio = style
        .metric_size
        .map(|nominal| style.size / nominal)
        .unwrap_or(1.0);
    let height = if span.height > 0.0 {
        span.height
    } else {
        spec.size
    };
    let mut caret = orient_caret(
        spec,
        laid,
        Caret {
            x,
            top: span.top,
            height,
            angle: 0.0,
        },
    );
    if spec.writing_mode.is_vertical() {
        caret.x -= height * (1.0 - ratio) / 2.0;
    } else {
        caret.top += (span.baseline - span.top) * (1.0 - ratio);
    }
    caret.height *= ratio;
    shift_caret(spec, byte, caret)
}

fn shift_caret(spec: &TextSpec, byte: usize, mut caret: Caret) -> Caret {
    let shift = spec.style_at(byte).baseline_shift;
    if spec.writing_mode.is_vertical() {
        caret.x += shift;
    } else {
        caret.top -= shift;
    }
    caret
}

fn orient_caret(spec: &TextSpec, laid: &Layout, caret: Caret) -> Caret {
    if !spec.writing_mode.is_vertical() {
        return caret;
    }
    let total = block_extent(&laid.lines);
    let right = match spec.writing_mode {
        WritingMode::VerticalRl => total - caret.top,
        _ => caret.top + caret.height,
    };
    Caret {
        x: right,
        top: caret.x,
        angle: std::f32::consts::FRAC_PI_2,
        ..caret
    }
}

/// The text position nearest a point in layout coordinates.
///
/// `x` and `y` are relative to the same origin as [`Caret`]. The closest
/// line is used above or below the text, and the closest pen position on
/// that line is used to its left or right, so a drag can continue beyond
/// the ink and still select predictably.
pub fn hit_test(spec: &TextSpec, x: f32, y: f32) -> Option<usize> {
    hit_test_position(spec, x, y).map(|p| p.byte)
}

/// Both affinities at directional boundaries; coincident positions are merged.
pub fn insertion_points(spec: &TextSpec) -> Vec<(CaretPosition, Caret)> {
    let Some(face) = load_font(
        &spec.family,
        spec.font_style.as_deref(),
        spec.bold,
        spec.italic,
    ) else {
        return Vec::new();
    };
    let laid = layout(spec, &face);
    let guide = path_guide(spec, &laid);
    let mut out = Vec::new();
    for byte in spec
        .text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain(std::iter::once(spec.text.len()))
    {
        let position = CaretPosition::from(byte);
        let map = |c| {
            guide
                .as_ref()
                .map_or(c, |g| g.caret(c, laid.first_baseline))
        };
        let downstream = map(caret_in_layout_affinity(spec, &laid, position));
        out.push((position, downstream));
        let position = CaretPosition {
            byte,
            affinity: CaretAffinity::Upstream,
        };
        let upstream = map(caret_in_layout_affinity(spec, &laid, position));
        if (upstream.x - downstream.x).abs() > 0.01 || (upstream.top - downstream.top).abs() > 0.01
        {
            out.push((position, upstream));
        }
    }
    out
}

pub fn hit_test_position(spec: &TextSpec, x: f32, y: f32) -> Option<CaretPosition> {
    insertion_points(spec)
        .into_iter()
        .min_by(|(_, a), (_, b)| {
            let distance = |c: &Caret| {
                let (cross_distance, inline_distance) = c.hit_distance(x, y);
                if spec.path.is_some() && !spec.writing_mode.is_vertical() {
                    (cross_distance.hypot(inline_distance), 0.0)
                } else {
                    // Resolve the line/column first. A drag beyond a short line
                    // must not jump to a wider line merely because its end is closer.
                    (cross_distance, inline_distance)
                }
            };
            let a = distance(a);
            let b = distance(b);
            a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
        })
        .map(|(position, _)| position)
}

pub fn move_caret(spec: &TextSpec, from: CaretPosition, movement: CaretMovement) -> CaretPosition {
    if spec.path.is_some() && !spec.writing_mode.is_vertical() {
        // Keyboard inline movement follows the path's baseline, including
        // curved paths whose screen-space caret tops are all different.
        let mut straight = spec.clone();
        straight.path = None;
        straight.wrap_width = None;
        return move_caret(&straight, from, movement);
    }
    let points = insertion_points(spec);
    let Some(current) = caret_at_position(spec, from) else {
        return from;
    };
    let vertical = spec.writing_mode.is_vertical();
    let coordinates = |c: Caret| {
        if vertical {
            (c.top, c.x - c.height / 2.0)
        } else {
            (c.x, c.top + c.height / 2.0)
        }
    };
    let (inline, cross) = coordinates(current);
    let inline_move = matches!(movement, CaretMovement::Home | CaretMovement::End)
        || if vertical {
            matches!(movement, CaretMovement::Up | CaretMovement::Down)
        } else {
            matches!(movement, CaretMovement::Left | CaretMovement::Right)
        };
    let forward = matches!(
        movement,
        CaretMovement::Right | CaretMovement::Down | CaretMovement::End
    );
    let spans = line_spans(spec);
    let total = block_extent(&spans);
    let line_index = spans
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let distance = |l: &LineSpan| {
                let center = if spec.writing_mode == WritingMode::VerticalRl {
                    total - l.top - l.height / 2.0
                } else {
                    l.top + l.height / 2.0
                };
                (center - cross).abs()
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(index, _)| index);
    if matches!(movement, CaretMovement::Home | CaretMovement::End) {
        return line_index.map_or(from, |i| {
            if forward {
                CaretPosition {
                    byte: spans[i].end,
                    affinity: CaretAffinity::Upstream,
                }
            } else {
                spans[i].start.into()
            }
        });
    }
    let candidates = points.iter().filter_map(|(p, c)| {
        let (i, b) = coordinates(*c);
        let delta = if inline_move { i - inline } else { b - cross };
        if (forward && delta <= 0.01)
            || (!forward && delta >= -0.01)
            || (inline_move && (b - cross).abs() > 0.1)
        {
            return None;
        }
        Some((*p, delta.abs(), (i - inline).abs()))
    });
    if let Some((p, _, _)) = candidates.min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
    {
        return p;
    }
    // At an inline edge, continue on the adjacent line/column in reading
    // order. Up/Down in a vertical column similarly traverse hard newlines.
    if inline_move {
        let forward = forward ^ (!vertical && shaping::paragraph_is_rtl(spec, from.byte));
        let at = line_index.unwrap_or(0);
        if forward && at + 1 < spans.len() {
            return spans[at + 1].start.into();
        }
        if !forward && at > 0 {
            return CaretPosition {
                byte: spans[at - 1].end,
                affinity: CaretAffinity::Upstream,
            };
        }
    }
    from
}

/// Highlight each selected character's own visual cell. A logical bidi
/// selection can occupy disjoint rectangles on the same line.
pub fn selection_rects(spec: &TextSpec, range: std::ops::Range<usize>) -> Vec<IntRect> {
    let Some(face) = load_font(
        &spec.family,
        spec.font_style.as_deref(),
        spec.bold,
        spec.italic,
    ) else {
        return Vec::new();
    };
    let laid = layout(spec, &face);
    let guide = path_guide(spec, &laid);
    let mut out: Vec<IntRect> = Vec::new();
    for span in &laid.lines {
        for ch in laid
            .chars
            .iter()
            .filter(|c| span.start <= c.byte && c.byte < span.end && range.contains(&c.byte))
        {
            let mut points = Vec::new();
            for x in [ch.x, ch.end_x] {
                let c = styled_caret(spec, &laid, *span, ch.byte, x);
                let c = guide
                    .as_ref()
                    .map_or(c, |g| g.caret(c, laid.first_baseline));
                points.push((c.x, c.top));
                points.push((
                    c.x - c.angle.sin() * c.height,
                    c.top + c.angle.cos() * c.height,
                ));
            }
            let rect = IntRect::new(
                points
                    .iter()
                    .map(|p| p.0)
                    .fold(f32::INFINITY, f32::min)
                    .floor() as i32,
                points
                    .iter()
                    .map(|p| p.1)
                    .fold(f32::INFINITY, f32::min)
                    .floor() as i32,
                points
                    .iter()
                    .map(|p| p.0)
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil() as i32,
                points
                    .iter()
                    .map(|p| p.1)
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil() as i32,
            );
            if !rect.is_empty() {
                if spec.path.is_none() {
                    if let Some(previous) = out.last_mut() {
                        let adjacent = if spec.writing_mode.is_vertical() {
                            previous.left == rect.left
                                && previous.right == rect.right
                                && rect.top <= previous.bottom
                                && rect.bottom >= previous.top
                        } else {
                            previous.top == rect.top
                                && previous.bottom == rect.bottom
                                && rect.left <= previous.right
                                && rect.right >= previous.left
                        };
                        if adjacent {
                            *previous = previous.union(&rect);
                            continue;
                        }
                    }
                }
                out.push(rect);
            }
        }
    }
    out
}

/// Extended Unicode grapheme boundaries used by editing and caret placement.
pub fn grapheme_boundaries(text: &str) -> impl DoubleEndedIterator<Item = usize> + '_ {
    text.grapheme_indices(true)
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
}

/// Nearest char boundary at or below `byte`, clamped to the string.
fn clamp_to_boundary(text: &str, byte: usize) -> usize {
    let mut at = byte.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Geometry for file interchange without allocating a glyph coverage bitmap.
#[derive(Debug, Clone, Copy)]
pub struct TextMetrics {
    pub first_baseline: f32,
    pub line_advance: f32,
    pub width: f32,
    pub height: f32,
    /// Union of horizontal glyph outlines [left, top, right, bottom], in
    /// layout coordinates. Excludes decorations; absent for empty text,
    /// vertical writing. Path glyphs use their individual baseline rotations.
    /// No coverage bitmap is allocated.
    pub ink_bounds: Option<[f32; 4]>,
}

pub fn measure(spec: &TextSpec) -> Option<TextMetrics> {
    let face = load_font(
        &spec.family,
        spec.font_style.as_deref(),
        spec.bold,
        spec.italic,
    )?;
    let laid = layout(spec, &face);
    let ink_bounds = if spec.writing_mode == WritingMode::Horizontal {
        let faces = Faces::resolve(spec, &face);
        let guide = path_guide(spec, &laid);
        laid.glyphs
            .iter()
            .filter_map(|glyph| {
                let (face, size) = &faces.faces[glyph.face];
                let metrics = face.font.metrics_indexed(glyph.glyph, *size);
                let b = metrics.bounds;
                (b.width > 0.0 && b.height > 0.0).then(|| {
                    if let Some(guide) = &guide {
                        let center = metrics.advance_width / 2.0;
                        let (x, y, angle) =
                            guide.at_glyph(glyph.x, center, glyph.baseline - laid.first_baseline);
                        let (sin, cos) = angle.sin_cos();
                        [
                            (b.xmin - center, -b.ymin - b.height),
                            (b.xmin - center + b.width, -b.ymin - b.height),
                            (b.xmin - center, -b.ymin),
                            (b.xmin - center + b.width, -b.ymin),
                        ]
                        .into_iter()
                        .map(|(dx, dy)| {
                            let (x, y) = (x + dx * cos - dy * sin, y + dx * sin + dy * cos);
                            [x, y, x, y]
                        })
                        .reduce(|a, b| {
                            [
                                a[0].min(b[0]),
                                a[1].min(b[1]),
                                a[2].max(b[2]),
                                a[3].max(b[3]),
                            ]
                        })
                        .unwrap()
                    } else {
                        [
                            glyph.x + b.xmin,
                            glyph.baseline - b.ymin - b.height,
                            glyph.x + b.xmin + b.width,
                            glyph.baseline - b.ymin,
                        ]
                    }
                })
            })
            .reduce(|a, b| {
                [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ]
            })
    } else {
        None
    };
    Some(TextMetrics {
        ink_bounds,
        first_baseline: laid.first_baseline,
        line_advance: laid.line_advance,
        width: laid.layout_width,
        height: laid
            .lines
            .iter()
            .map(|line| line.top + line.height)
            .fold(0.0, f32::max),
    })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PaintKind {
    UnderlineGap,
    Underline,
    Fill,
    Stroke,
    StrikeGap,
    Strike,
}
impl PaintKind {
    fn color(self, style: &CharStyle) -> Option<[u8; 4]> {
        match self {
            Self::UnderlineGap => style.underline_style.gap_color,
            Self::StrikeGap => style.strike_style.gap_color,
            Self::Underline => style.underline_style.color.or(style.color),
            Self::Strike => style.strike_style.color.or(style.color),
            Self::Fill => style.color,
            Self::Stroke => style.stroke.and_then(|s| s.color),
        }
    }
}

/// A glyph or decoration fragment, retaining the owning source cluster.
struct ColoredRaster {
    kind: PaintKind,
    rect: IntRect,
    bitmap: Vec<u8>,
    byte: usize,
}

/// Glyph masks use integer placement. A few f32 arithmetic operations (for
/// example stop - field width + glyph advance) can land just below an integer;
/// flooring that round-off would move the whole mask by a pixel. Snap only
/// within the arithmetic precision before flooring, without changing layout
/// metrics, source coordinates or genuinely fractional placements.
fn glyph_pixel_start(value: f32) -> i32 {
    let integer = value.round();
    if (value - integer).abs() <= 4.0 * f32::EPSILON * value.abs().max(1.0) {
        integer as i32
    } else {
        value.floor() as i32
    }
}

/// Font-derived line weight at a source position, without composing the text.
/// Used by layout contribution bounds when automatic capped lines can extend
/// along the inline axis beyond their otherwise nonintersecting frame.
pub fn automatic_decoration_weight(spec: &TextSpec, byte: usize, strike: bool) -> f32 {
    let style = spec.style_at(byte);
    let weight = load_font(
        &style.family,
        style.font_style.as_deref(),
        style.bold,
        style.italic,
    )
    .and_then(|loaded| {
        let face = ttf_parser::Face::parse(&loaded.data, loaded.index).ok()?;
        let metric = if strike {
            face.strikeout_metrics()
        } else {
            face.underline_metrics()
        }?;
        (metric.thickness > 0)
            .then_some(f32::from(metric.thickness) * style.size / f32::from(face.units_per_em()))
    })
    .unwrap_or(style.size / 16.0);
    weight.round().max(1.0)
}

struct DecorationPhase {
    origin: f32,
    end: f32,
    cross: f32,
    extent: f32,
    pattern: TextDecorationPattern,
    fitting: DecorationFit,
}

/// Decorations follow character advances, including spaces and RTL.
/// Horizontal offsets/thickness use OpenType post/OS/2 metrics when available.
/// Vertical underlines follow the column's outside edge; strikes cross its
/// center. Explicit dimensions override those defaults. Stripes and straight
/// capped dashes/dots with endpoint fitting use the same inline masks. Path
/// decorations bend those masks after consecutive paint fragments are joined.
fn decoration_rasters(spec: &TextSpec, faces: &Faces, laid: &Layout) -> Vec<ColoredRaster> {
    let mut out = Vec::new();
    if !spec
        .runs
        .iter()
        .any(|run| run.underline == Some(true) || run.strikethrough == Some(true))
    {
        return out;
    }
    let metrics: Vec<_> = faces
        .faces
        .iter()
        .map(|(loaded, size)| {
            let face = ttf_parser::Face::parse(&loaded.data, loaded.index).ok();
            let scale = face
                .as_ref()
                .map_or(1.0, |face| size / face.units_per_em() as f32);
            [false, true].map(|strike| {
                let metric = face.as_ref().and_then(|face| {
                    if strike {
                        face.strikeout_metrics()
                    } else {
                        face.underline_metrics()
                    }
                });
                metric.filter(|m| m.thickness > 0).map_or(
                    (if strike { size * 0.3 } else { -size * 0.1 }, size / 16.0),
                    |m| (m.position as f32 * scale, m.thickness as f32 * scale),
                )
            })
        })
        .collect();
    let total_height = block_extent(&laid.lines);
    for line in &laid.lines {
        // Walk in visual order so a dash phase survives character boundaries,
        // bidi reordering and paint-only style changes along a continuous line.
        let mut chars: Vec<_> = laid
            .chars
            .iter()
            .filter(|ch| line.start <= ch.byte && ch.byte < line.end)
            .collect();
        chars.sort_by(|a, b| a.x.min(a.end_x).total_cmp(&b.x.min(b.end_x)));
        let mut phases: Vec<DecorationPhase> = Vec::new();
        let mut active: [Option<usize>; 2] = [None, None];
        let mut pieces = Vec::new();
        for ch in chars {
            let style = spec.style_at(ch.byte);
            let start = ch.x.min(ch.end_x).floor() as i32;
            let length = (ch.x.max(ch.end_x).ceil() as i32 - start).max(0) as u32;
            for (strike, enabled) in [(false, style.underline), (true, style.strikethrough)] {
                let decoration = if strike {
                    style.strike_style.clone()
                } else {
                    style.underline_style.clone()
                };
                let main_paint =
                    !decoration.disabled && !(style.fill_disabled && decoration.color.is_none());
                if !enabled {
                    active[usize::from(strike)] = None;
                    continue;
                }
                if decoration
                    .weight
                    .is_some_and(|v| !v.is_finite() || v <= 0.0)
                {
                    active[usize::from(strike)] = None;
                    continue;
                }
                let face = faces.at(ch.byte);
                let (mut position, mut thickness) = metrics[face][usize::from(strike)];
                if faces.synthetic_caps.get(ch.byte).copied().unwrap_or(false) {
                    // Synthetic caps keep the nominal character's line paint;
                    // a lowercase letter must not kink or thin its underline.
                    let nominal = style.size / faces.faces[face].1;
                    position *= nominal;
                    thickness *= nominal;
                }
                let explicit = decoration.weight.is_some() || decoration.offset.is_some();
                let weight = decoration.weight.unwrap_or(thickness.round().max(1.0));
                let mut x = start as f32;
                let mut y;
                let mut width = length as f32;
                let mut height = weight;
                if spec.writing_mode.is_vertical() {
                    let rl = spec.writing_mode == WritingMode::VerticalRl;
                    let center = if rl {
                        total_height - line.top - line.height / 2.0
                    } else {
                        line.top + line.height / 2.0
                    };
                    x = if let Some(offset) = decoration.offset.filter(|v| v.is_finite()) {
                        center + (if rl { offset } else { -offset }) - weight / 2.0
                    } else if strike {
                        center - weight / 2.0
                    } else if rl {
                        center + style.size * 0.55
                    } else {
                        center - style.size * 0.55 - weight
                    };
                    x += style.baseline_shift;
                    y = start as f32;
                    width = weight;
                    height = length as f32;
                } else {
                    y = line.baseline - style.baseline_shift
                        + decoration
                            .offset
                            .filter(|v| v.is_finite())
                            .map_or(-position, |v| {
                                if strike {
                                    -v - weight / 2.0
                                } else {
                                    v - weight / 2.0
                                }
                            });
                }
                if !explicit {
                    x = x.round();
                    y = y.round();
                }
                let (cross, extent, inline, length) = if spec.writing_mode.is_vertical() {
                    (x, width, y, height)
                } else {
                    (y, height, x, width)
                };
                let previous = active[usize::from(strike)].filter(|&index| {
                    let p = &phases[index];
                    inline <= p.end
                        && cross == p.cross
                        && extent == p.extent
                        && p.pattern == decoration.pattern
                        && p.fitting == decoration.fitting
                });
                let index = if let Some(index) = previous {
                    phases[index].end = phases[index].end.max(inline + length);
                    index
                } else {
                    phases.push(DecorationPhase {
                        origin: inline,
                        end: inline + length,
                        cross,
                        extent,
                        pattern: decoration.pattern.clone(),
                        fitting: decoration.fitting,
                    });
                    phases.len() - 1
                };
                active[usize::from(strike)] = Some(index);
                if main_paint || decoration.gap_color.is_some() {
                    pieces.push((
                        index,
                        [x, y, width, height],
                        ch.byte,
                        strike,
                        main_paint,
                        style.color,
                        decoration,
                    ));
                }
            }
        }
        // The whole geometric segment is known before drawing: an end cap belongs
        // at its actual endpoint, never at an internal character/paint boundary.
        for (index, geometry, byte, strike, main_paint, color, decoration) in pieces {
            let phase = &phases[index];
            let [x, y, width, height] = geometry;
            let vertical = spec.writing_mode.is_vertical();
            let (inline, length, weight) = if vertical {
                (y, height, width)
            } else {
                (x, width, height)
            };
            let extension = decoration.pattern.cap_extension(weight);
            let start_extra = if inline == phase.origin {
                extension
            } else {
                0.0
            };
            let end_extra = if inline + length == phase.end {
                extension
            } else {
                0.0
            };
            let rect = if vertical {
                IntRect::new(
                    x.floor() as i32,
                    (y - start_extra).floor() as i32,
                    (x + width).ceil() as i32,
                    (y + height + end_extra).ceil() as i32,
                )
            } else {
                IntRect::new(
                    (x - start_extra).floor() as i32,
                    y.floor() as i32,
                    (x + width + end_extra).ceil() as i32,
                    (y + height).ceil() as i32,
                )
            };
            if rect.is_empty() {
                continue;
            }
            // Exact rectangle coverage also retains fractional line weights.
            let mut bitmap = Vec::with_capacity(rect.width() as usize * rect.height() as usize);
            for row in rect.top..rect.bottom {
                let dy = ((row as f32 + 1.0).min(y + height) - (row as f32).max(y)).max(0.0);
                for col in rect.left..rect.right {
                    let dx = ((col as f32 + 1.0).min(x + width) - (col as f32).max(x)).max(0.0);
                    bitmap.push((dx * dy * 255.0).round() as u8);
                }
            }
            if let Some((mask, gap, combined)) = decoration.pattern.fitted_masks(
                rect,
                geometry,
                vertical,
                [phase.origin, phase.end],
                decoration.fitting,
            ) {
                if main_paint
                    && decoration.gap_color.is_some()
                    && decoration.gap_color == decoration.color.or(color)
                {
                    bitmap = combined;
                } else {
                    if decoration.gap_color.is_some() && gap.iter().any(|v| *v != 0) {
                        out.push(ColoredRaster {
                            kind: if strike {
                                PaintKind::StrikeGap
                            } else {
                                PaintKind::UnderlineGap
                            },
                            rect,
                            bitmap: gap,
                            byte,
                        });
                    }
                    bitmap = mask;
                }
            }
            if main_paint {
                out.push(ColoredRaster {
                    kind: if strike {
                        PaintKind::Strike
                    } else {
                        PaintKind::Underline
                    },
                    rect,
                    bitmap,
                    byte,
                });
            }
        }
    }
    out
}

/// Lay out and rasterize `spec` into a coverage mask.
///
/// Returns `None` when no font could be loaded; an empty string yields an
/// empty raster rather than an error.
pub fn rasterize(spec: &TextSpec) -> Option<TextRaster> {
    rasterize_impl(spec, false)
}

/// Shape once, retaining separate coverage for each consecutive paint.
/// Color values are opaque to rasterization, so a separation client may use
/// them as ink IDs without converting a spot ink through screen RGB.
pub fn rasterize_with_paints(spec: &TextSpec) -> Option<TextRaster> {
    rasterize_impl(spec, true)
}

fn rasterize_impl(spec: &TextSpec, retain_paints: bool) -> Option<TextRaster> {
    let face = load_font(
        &spec.family,
        spec.font_style.as_deref(),
        spec.bold,
        spec.italic,
    )?;
    if spec.text.is_empty() || spec.size <= 0.0 {
        return Some(TextRaster {
            bounds: IntRect::EMPTY,
            coverage: Vec::new(),
            colors: Vec::new(),
            paints: Vec::new(),
            first_baseline: 0.0,
            line_advance: 0.0,
            layout_width: 0.0,
            cap_height: face.cap_ratio.map(|r| r * spec.size),
        });
    }
    let faces = Faces::resolve(spec, &face);
    let mut laid = layout(spec, &face);
    laid.glyphs
        .extend(tab_leaders::glyphs(spec, &faces, &laid)?);
    // Coverage rectangles use signed 32-bit coordinates. Reject distant inline
    // geometry before saturated float casts overflow rectangle arithmetic.
    let coordinate_limit = (i32::MAX / 2) as f32;
    if !laid.layout_width.is_finite()
        || laid.layout_width > coordinate_limit
        || laid.glyphs.iter().any(|g| {
            !g.x.is_finite()
                || !g.baseline.is_finite()
                || g.x.abs() > coordinate_limit
                || g.baseline.abs() > coordinate_limit
        })
    {
        return None;
    }
    let guide = path_guide(spec, &laid);
    let mut decorations = decoration_rasters(spec, &faces, &laid);
    // Associate decorations with the same consecutive visual paint as their
    // glyphs. Appending all decoration masks after the glyphs repaints earlier
    // translucent runs and lets their lines cross later differently colored ink.
    let mut signatures = Vec::new();
    let mut paint_groups = std::collections::HashMap::new();
    for ch in &laid.chars {
        let style = spec.style_at(ch.byte);
        // An invisible glyph paint cannot split a visible decoration into
        // separate opacity groups at an otherwise unchanged style boundary.
        let signature = (
            if style.fill_disabled {
                None
            } else {
                style.color
            },
            style.fill_disabled,
            style.stroke,
        );
        if signatures.last() != Some(&signature) {
            signatures.push(signature);
        }
        paint_groups.insert(ch.byte, signatures.len() - 1);
    }
    if let Some(guide) = &guide {
        decorations =
            path_decoration::bend(spec, guide, laid.first_baseline, decorations, &paint_groups);
    }
    // Screen clients need both paints to composite translucent overlaps.
    let retain_paints = retain_paints
        || signatures.iter().any(|s| s.2.is_some())
        || spec.runs.iter().any(|r| {
            r.underline_style
                .as_ref()
                .is_some_and(|d| d.color.is_some() || d.gap_color.is_some())
                || r.strike_style
                    .as_ref()
                    .is_some_and(|d| d.color.is_some() || d.gap_color.is_some())
        });
    let Layout {
        glyphs: placed,
        first_baseline,
        line_advance,
        layout_width,
        ..
    } = laid;
    if placed.is_empty() && decorations.is_empty() {
        return Some(TextRaster {
            bounds: IntRect::EMPTY,
            coverage: Vec::new(),
            colors: Vec::new(),
            paints: Vec::new(),
            first_baseline,
            line_advance,
            layout_width,
            cap_height: face.cap_ratio.map(|r| r * spec.size),
        });
    }

    // Rasterize once to find the union of glyph boxes...
    let mut rasterized = Vec::with_capacity(placed.len());
    let mut bounds = IntRect::EMPTY;
    for g in &placed {
        let (font, size) = &faces.faces[g.face];
        let style = spec.style_at(g.byte);
        let mut fill_covered = false;
        if let Some(stroke) = style.stroke {
            // Rasterize equal fill/outline ink as one silhouette. Unioning two
            // antialiased masks at a shared outside edge leaves a pale seam.
            let combined = !style.fill_disabled && stroke.color == style.color;
            if let Some(fragment) = text_stroke::raster(
                font,
                *size,
                g,
                stroke,
                combined,
                guide.as_ref(),
                first_baseline,
            ) {
                bounds = bounds.union(&fragment.rect);
                rasterized.push(fragment);
                fill_covered = combined;
            }
        }
        if style.fill_disabled || fill_covered {
            continue;
        }
        let (metrics, bitmap) = font.font.rasterize_indexed(g.glyph, *size);
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        let (rect, bitmap) = if let Some(guide) = &guide {
            text_path::glyph_bitmap(guide, g, first_baseline, &metrics, bitmap)
        } else if g.sideways {
            let left = glyph_pixel_start(g.x + metrics.ymin as f32);
            let top = glyph_pixel_start(g.baseline + metrics.xmin as f32);
            let mut rotated = vec![0; bitmap.len()];
            for y in 0..metrics.height {
                for x in 0..metrics.width {
                    rotated[x * metrics.height + metrics.height - 1 - y] =
                        bitmap[y * metrics.width + x];
                }
            }
            (
                IntRect::from_xywh(left, top, metrics.height as u32, metrics.width as u32),
                rotated,
            )
        } else {
            let left = glyph_pixel_start(g.x + metrics.xmin as f32);
            let top = glyph_pixel_start(g.baseline - metrics.height as f32 - metrics.ymin as f32);
            (
                IntRect::from_xywh(left, top, metrics.width as u32, metrics.height as u32),
                bitmap,
            )
        };
        bounds = bounds.union(&rect);
        rasterized.push(ColoredRaster {
            kind: PaintKind::Fill,
            rect,
            bitmap,
            byte: g.byte,
        });
    }
    for decoration in decorations {
        bounds = bounds.union(&decoration.rect);
        rasterized.push(decoration);
    }
    // Stable within a paint: glyph overlap and decoration masks are unioned
    // before opacity/knockout are applied once by the separation client.
    // In each consecutive visual run: underline, glyph fill, outline, then strike.
    // Centered strokes straddle the contour; outside strokes exclude its fill.
    // Equal inks union once, retaining one opacity application.
    rasterized.sort_by_key(|fragment| {
        (
            paint_groups.get(&fragment.byte).copied().unwrap_or(0),
            fragment.kind,
        )
    });
    if bounds.is_empty() {
        return Some(TextRaster {
            bounds: IntRect::EMPTY,
            coverage: Vec::new(),
            colors: Vec::new(),
            paints: Vec::new(),
            first_baseline,
            line_advance,
            layout_width,
            cap_height: face.cap_ratio.map(|r| r * spec.size),
        });
    }

    // ...then blit them into one mask, taking the max where glyphs overlap.
    let w = usize::try_from(i64::from(bounds.right) - i64::from(bounds.left)).ok()?;
    let h = usize::try_from(i64::from(bounds.bottom) - i64::from(bounds.top)).ok()?;
    // A distant finite tab must not request an effectively unbounded bitmap.
    // Separation exposes raster failure as an explicit preflight error.
    if w > i32::MAX as usize || h > i32::MAX as usize || w.checked_mul(h)? > 256_000_000 {
        return None;
    }
    let mut coverage = vec![0u8; w * h];
    let mut colors = if spec.runs.iter().any(|r| r.color.is_some()) {
        vec![None; w * h]
    } else {
        Vec::new()
    };
    let mut paints: Vec<TextPaint> = Vec::new();
    for ColoredRaster {
        rect,
        bitmap,
        byte,
        kind,
    } in rasterized
    {
        let style = spec.style_at(byte);
        let color = kind.color(&style);
        if retain_paints && paints.last().is_none_or(|paint| paint.color != color) {
            paints.push(TextPaint {
                color,
                coverage: vec![0; w * h],
            });
        }
        for gy in 0..rect.height() {
            for gx in 0..rect.width() {
                let v = bitmap[(gy * rect.width() + gx) as usize];
                if v == 0 {
                    continue;
                }
                let x = (rect.left + gx - bounds.left) as usize;
                let y = (rect.top + gy - bounds.top) as usize;
                if let Some(paint) = paints.last_mut() {
                    let slot = &mut paint.coverage[y * w + x];
                    *slot = (*slot).max(v);
                }
                let slot = &mut coverage[y * w + x];
                if v >= *slot {
                    *slot = v;
                    if let Some(fill) = colors.get_mut(y * w + x) {
                        *fill = color;
                    }
                }
            }
        }
    }
    Some(TextRaster {
        bounds,
        coverage,
        colors,
        paints,
        first_baseline,
        line_advance,
        layout_width,
        cap_height: face.cap_ratio.map(|r| r * spec.size),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curved_decorations_keep_patterns_paints_and_carets_across_every_character_boundary() {
        use schist_core::path::{Anchor, SubPath};
        for curved in [false, true] {
            for pattern in [
                TextDecorationPattern::Solid,
                TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]),
                TextDecorationPattern::Dashes(vec![4.5, 2.25].into()),
                TextDecorationPattern::Dashes(DecorationDashes {
                    lengths: vec![4.5, 2.25],
                    cap: DecorationCap::Round,
                }),
                TextDecorationPattern::Dashes(DecorationDashes {
                    lengths: vec![4.5, 2.25],
                    cap: DecorationCap::Projecting,
                }),
                TextDecorationPattern::Dots(vec![5.5, 7.25]),
            ] {
                let (mut spec, _) = opentype_spec("office é אבג a\u{301}");
                spec.size = 18.0;
                spec.direction = ParagraphDirection::LeftToRight;
                let mut anchors = vec![Anchor::corner(10.0, 40.0), Anchor::corner(210.0, 40.0)];
                if curved {
                    anchors[0].handle_out = (60.0, -70.0);
                    anchors[1].handle_in = (-60.0, 70.0);
                }
                spec.path = Some(TextPath {
                    curve: SubPath {
                        anchors,
                        closed: false,
                    },
                    offset: 7.25,
                    span: Some(180.0),
                });
                let decoration = TextDecoration {
                    pattern: pattern.clone(),
                    fitting: DecorationFit::DashesAndGaps,
                    color: Some([40, 80, 160, 128]),
                    gap_color: Some([160, 80, 40, 128]),
                    weight: Some(3.5),
                    offset: Some(7.25),
                    ..Default::default()
                };
                let style = StyleRun {
                    start: 0,
                    end: spec.text.len(),
                    fill_disabled: Some(true),
                    underline: Some(true),
                    strikethrough: Some(true),
                    underline_style: Some(decoration.clone()),
                    strike_style: Some(decoration),
                    ..Default::default()
                };
                spec.runs = vec![style.clone()];
                let carets_before = carets(&spec);
                let lines_before = line_spans(&spec);
                let expected = rasterize_with_paints(&spec).unwrap();
                assert!(expected.coverage.iter().any(|v| *v != 0), "{pattern:?}");
                let boundaries: Vec<_> = grapheme_boundaries(&spec.text).collect();
                spec.runs = boundaries
                    .windows(2)
                    .enumerate()
                    .map(|(index, pair)| StyleRun {
                        start: pair[0],
                        end: pair[1],
                        color: Some([index as u8, 0, 0, 255]),
                        ..style.clone()
                    })
                    .collect();
                assert_eq!(carets(&spec), carets_before);
                assert_eq!(line_spans(&spec), lines_before);
                let actual = rasterize_with_paints(&spec).unwrap();
                assert_eq!(actual.bounds, expected.bounds, "{pattern:?}");
                assert!(
                    actual.rgba([0, 0, 0, 255]) == expected.rgba([0, 0, 0, 255]),
                    "{pattern:?}, curved={curved}"
                );
                for run in &mut spec.runs {
                    run.underline = Some(false);
                    run.strikethrough = Some(false);
                }
                assert_eq!(carets(&spec), carets_before);
                assert_eq!(line_spans(&spec), lines_before);
                assert!(rasterize(&spec).unwrap().coverage.iter().all(|v| *v == 0));
            }
        }
    }

    #[test]
    fn outline_joins_change_corners_and_miter_limits_without_changing_text_geometry() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for outside in [false, true] {
                let (mut spec, _) = opentype_spec("AVMW");
                spec.size = 80.0;
                spec.writing_mode = mode;
                let before = carets(&spec);
                let lines = line_spans(&spec);
                let mut rasters = Vec::new();
                for (join, miter_limit) in [
                    (TextStrokeJoin::Bevel, 4.0),
                    (TextStrokeJoin::Miter, 0.0),
                    (TextStrokeJoin::Round, 4.0),
                    (TextStrokeJoin::Miter, 12.0),
                ] {
                    spec.runs = vec![StyleRun {
                        start: 0,
                        end: spec.text.len(),
                        fill_disabled: Some(true),
                        stroke: Some(TextStroke {
                            width: 8.0,
                            outside,
                            join,
                            miter_limit,
                            color: None,
                        }),
                        ..Default::default()
                    }];
                    let raster = rasterize(&spec).unwrap();
                    assert_eq!(carets(&spec), before);
                    assert_eq!(line_spans(&spec), lines);
                    assert!(raster.coverage.iter().any(|v| *v != 0));
                    rasters.push(raster);
                }
                assert_eq!(rasters[0].bounds, rasters[1].bounds);
                assert_eq!(
                    rasters[0].coverage, rasters[1].coverage,
                    "zero limit must bevel every corner"
                );
                let mass = |r: &TextRaster| r.coverage.iter().map(|v| u64::from(*v)).sum::<u64>();
                assert!(
                    mass(&rasters[2]) > mass(&rasters[0]),
                    "round joins must fill beyond bevels"
                );
                assert!(
                    mass(&rasters[3]) > mass(&rasters[0]),
                    "miter joins must extend beyond bevels"
                );
                assert_ne!(rasters[2].coverage, rasters[3].coverage);
                let stroke = spec.style_at(0).stroke.unwrap();
                assert_eq!(stroke.extent(), 96.0);
                let mut old = serde_json::to_value(stroke).unwrap();
                for key in ["join", "miter_limit"] {
                    old.as_object_mut().unwrap().remove(key);
                }
                let old: TextStroke = serde_json::from_value(old).unwrap();
                assert_eq!(old.join, TextStrokeJoin::Miter);
                assert_eq!(old.miter_limit, 4.0);
            }
        }
    }

    #[test]
    fn outside_strokes_exclude_glyph_interiors_and_expand_by_the_full_weight() {
        let (mut spec, _) = opentype_spec("HH");
        spec.size = 80.0;
        let fill = rasterize(&spec).unwrap();
        let sample = |r: &TextRaster, x: i32, y: i32| -> u8 {
            if !r.bounds.contains(x, y) {
                return 0;
            }
            r.coverage[((y - r.bounds.top) * r.bounds.width() + x - r.bounds.left) as usize]
        };
        spec.runs = vec![StyleRun {
            start: 0,
            end: 2,
            fill_disabled: Some(true),
            stroke: Some(TextStroke {
                width: 3.0,
                outside: false,
                color: None,
                ..Default::default()
            }),
            ..Default::default()
        }];
        let center = rasterize(&spec).unwrap();
        spec.runs[0].stroke.as_mut().unwrap().outside = true;
        let outside = rasterize(&spec).unwrap();
        assert!(outside.bounds.left < center.bounds.left);
        assert!(outside.bounds.right > center.bounds.right);
        let mut interior = 0;
        for y in fill.bounds.top..fill.bounds.bottom {
            for x in fill.bounds.left..fill.bounds.right {
                if (-1..=1).all(|dy| (-1..=1).all(|dx| sample(&fill, x + dx, y + dy) == 255)) {
                    interior += 1;
                    assert_eq!(
                        sample(&outside, x, y),
                        0,
                        "outside stroke entered filled interior"
                    );
                }
            }
        }
        assert!(interior > 50);
        spec.runs[0].fill_disabled = Some(false);
        let union = rasterize(&spec).unwrap();
        assert_eq!(union.paints.len(), 1);
        for y in fill.bounds.top..fill.bounds.bottom {
            for x in fill.bounds.left..fill.bounds.right {
                assert!(
                    sample(&union, x, y) >= sample(&fill, x, y),
                    "equal-ink outside stroke must not erode its fill edge"
                );
            }
        }
    }

    #[test]
    fn stroke_changes_ink_but_preserves_metrics_carets_and_equal_shaping_boundaries() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for outside in [false, true] {
                let (mut spec, _) = opentype_spec("office AV");
                spec.writing_mode = mode;
                spec.set_feature("liga", true);
                let ordinary = rasterize(&spec).unwrap();
                let carets_before = carets(&spec);
                let spans = line_spans(&spec);
                let style = StyleRun {
                    start: 0,
                    end: spec.text.len(),
                    color: Some([20, 50, 100, 128]),
                    stroke: Some(TextStroke {
                        width: 3.0,
                        outside,
                        color: Some([200, 30, 10, 128]),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                spec.runs = vec![style.clone()];
                let painted = rasterize_with_paints(&spec).unwrap();
                assert_eq!(carets(&spec), carets_before);
                assert_eq!(line_spans(&spec), spans);
                assert_eq!(painted.layout_width, ordinary.layout_width);
                assert_eq!(painted.line_advance, ordinary.line_advance);
                assert_eq!(painted.first_baseline, ordinary.first_baseline);
                assert_eq!(painted.paints.len(), 2);
                assert_eq!(painted.paints[0].color, style.color);
                assert_eq!(painted.paints[1].color, style.stroke.unwrap().color);
                assert!(painted.bounds.width() >= ordinary.bounds.width());
                assert!(painted.bounds.height() >= ordinary.bounds.height());
                spec.runs = vec![
                    StyleRun {
                        end: 3,
                        ..style.clone()
                    },
                    StyleRun {
                        start: 3,
                        ..style.clone()
                    },
                ];
                let partitioned = rasterize_with_paints(&spec).unwrap();
                assert_eq!(partitioned.bounds, painted.bounds);
                assert_eq!(partitioned.paints.len(), painted.paints.len());
                for (a, b) in partitioned.paints.iter().zip(&painted.paints) {
                    assert_eq!(a.coverage, b.coverage);
                }
                spec.runs = vec![StyleRun {
                    fill_disabled: Some(true),
                    ..style.clone()
                }];
                let outline = rasterize_with_paints(&spec).unwrap();
                assert_eq!(outline.paints.len(), 1);
                assert_eq!(outline.bounds, painted.bounds);
                assert_eq!(outline.coverage, painted.paints[1].coverage);
                spec.runs[0].stroke = None;
                let invisible = rasterize(&spec).unwrap();
                assert!(invisible.is_empty());
                assert_eq!(invisible.layout_width, ordinary.layout_width);
                assert_eq!(carets(&spec), carets_before);
            }
        }
    }

    #[test]
    fn stroke_paints_union_equal_inks_and_composite_distinct_translucent_inks_in_order() {
        let (mut spec, _) = opentype_spec("HH");
        spec.runs = vec![StyleRun {
            start: 0,
            end: 2,
            color: Some([10, 50, 200, 128]),
            stroke: Some(TextStroke {
                width: 5.0,
                color: Some([200, 30, 10, 128]),
                ..Default::default()
            }),
            ..Default::default()
        }];
        let raster = rasterize(&spec).unwrap();
        let pixels = raster.rgba([0, 0, 0, 255]);
        let overlap = (0..raster.coverage.len())
            .find(|i| raster.paints.iter().all(|p| p.coverage[*i] == 255))
            .expect("centered outline crosses filled ink");
        let pixel = &pixels[overlap * 4..overlap * 4 + 4];
        assert_eq!(pixel[3], 192);
        assert!(pixel[0] > pixel[2], "outline is the later paint");
        spec.runs[0].stroke.as_mut().unwrap().color = spec.runs[0].color;
        let union = rasterize(&spec).unwrap();
        assert_eq!(union.paints.len(), 1);
        assert!(union
            .rgba([0; 4])
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] <= 128));
        let mut reset = spec.clone();
        reset.apply_style(0..reset.text.len(), &reset.base_style().as_run());
        assert!(reset.style_at(0).stroke.is_none());
        assert!(!reset.style_at(0).fill_disabled);
        for end in [1, 2, 9] {
            let mut edited = spec.clone();
            edited.apply_style(
                0..end,
                &StyleRun {
                    fill_disabled: Some(true),
                    stroke: Some(TextStroke::default()),
                    ..Default::default()
                },
            );
            assert!(edited.style_at(0).fill_disabled);
            assert!(edited.style_at(0).stroke.is_none());
            if end == 1 {
                assert_eq!(edited.style_at(1), spec.style_at(1));
            } else {
                assert!(edited.style_at(1).fill_disabled);
                assert!(edited.style_at(1).stroke.is_none());
            }
            assert_eq!(
                serde_json::from_str::<TextSpec>(&serde_json::to_string(&edited).unwrap()).unwrap(),
                edited
            );
        }
    }

    #[test]
    fn typographic_names_prevent_light_faces_from_claiming_regular() {
        let mac = ttf_parser::name::Name {
            platform_id: ttf_parser::PlatformId::Macintosh,
            encoding_id: 0,
            language_id: 0,
            name_id: 2,
            name: b"L\x8eger",
        };
        assert_eq!(font_name(mac).as_deref(), Some("Léger"));
        let mut database = fontdb::Database::new();
        database.load_font_data(include_bytes!("../tests/fixtures/IBMPlexSans-Light.ttf").to_vec());
        let light = named_face(&database, "IBM Plex Sans", "Light").unwrap();
        assert!(named_face(&database, "IBM Plex Sans", "Regular").is_none());
        database
            .load_font_data(include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec());
        let regular = named_face(&database, "IBM Plex Sans", "Regular").unwrap();
        assert_ne!(light, regular);
        assert_eq!(
            database.face(light).unwrap().post_script_name,
            "IBMPlexSans-Light"
        );
        assert_eq!(
            database.face(regular).unwrap().post_script_name,
            "IBMPlexSans"
        );
        assert!(named_face(&database, "Missing family", "Light").is_none());
    }

    fn opentype_spec(text: &str) -> (TextSpec, LoadedFace) {
        // The browser's OFL font is also a deterministic shaping fixture.
        let data = include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf");
        let face = LoadedFace {
            font: Arc::new(fontdue::Font::from_bytes(data.as_slice(), Default::default()).unwrap()),
            data: Arc::new(data.to_vec()),
            index: 0,
            cap_ratio: None,
        };
        let mut spec = spec(text);
        spec.family = "Schist OpenType test fixture".into();
        font_cache().lock().unwrap().insert(
            (spec.family.clone(), None, false, false),
            Some(face.clone()),
        );
        (spec, face)
    }

    #[test]
    fn opentype_ligatures_and_kerning_change_the_glyph_layout() {
        let (mut s, face) = opentype_spec("office AV");
        s.set_feature("liga", false);
        let separate = layout(&s, &face);
        s.set_feature("liga", true);
        let joined = layout(&s, &face);
        assert!(
            joined.glyphs.len() < separate.glyphs.len(),
            "liga must substitute real glyphs"
        );
        for (byte, c) in carets(&s) {
            assert_eq!(hit_test(&s, c.x, c.top + c.height / 2.0), Some(byte));
        }
        s.set_feature("kern", false);
        let unkerned = layout(&s, &face).layout_width;
        s.set_feature("kern", true);
        assert!(layout(&s, &face).layout_width < unkerned);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<TextSpec>(&json).unwrap(), s);
    }

    #[test]
    fn range_features_override_only_their_text_and_equivalent_boundaries_do_not_split_ligatures() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for enabled in [false, true] {
                let (mut spec, face) = opentype_spec("office office");
                spec.writing_mode = mode;
                spec.set_feature("liga", enabled);
                spec.set_feature("kern", true);
                let original = layout(&spec, &face);
                spec.runs.push(StyleRun {
                    start: 2,
                    end: 4,
                    features: vec![
                        OpenTypeFeature {
                            tag: "kern".into(),
                            value: 1,
                        },
                        OpenTypeFeature {
                            tag: "liga".into(),
                            value: u32::from(enabled),
                        },
                    ],
                    ..Default::default()
                });
                let equal = layout(&spec, &face);
                assert_eq!(original.glyphs.len(), equal.glyphs.len());
                assert_eq!(original.layout_width, equal.layout_width);
                spec.runs[0].start = 0;
                spec.runs[0].end = 6;
                spec.runs[0].features[1].value = u32::from(!enabled);
                let changed = layout(&spec, &face);
                let count = |laid: &Layout, from, to| {
                    laid.glyphs
                        .iter()
                        .filter(|g| g.byte >= from && g.byte < to)
                        .count()
                };
                assert_eq!(count(&original, 7, 13), count(&changed, 7, 13));
                if enabled {
                    assert!(count(&changed, 0, 6) > count(&original, 0, 6));
                } else {
                    assert!(count(&changed, 0, 6) < count(&original, 0, 6));
                }
                for (byte, caret) in carets(&spec) {
                    assert!(spec.text.is_char_boundary(byte));
                    assert!(caret.x.is_finite() && caret.top.is_finite());
                }
                // A whole-range override is equivalent to a layer setting,
                // including activation of shaping when no global flag is set.
                spec.runs[0].end = spec.text.len();
                let mut uniform = spec.clone();
                uniform.runs.clear();
                uniform.set_feature("liga", !enabled);
                spec.features.clear();
                assert_eq!(line_spans(&spec), line_spans(&uniform));
                assert_eq!(
                    rasterize(&spec).unwrap().coverage,
                    rasterize(&uniform).unwrap().coverage
                );
            }
        }
    }

    #[test]
    fn feature_edits_merge_by_tag_preserve_unset_values_and_load_legacy_runs() {
        let (mut spec, _) = opentype_spec("office affine");
        spec.set_feature("liga", true);
        spec.apply_style(
            2..4,
            &StyleRun {
                features: vec![OpenTypeFeature {
                    tag: "kern".into(),
                    value: 0,
                }],
                ..Default::default()
            },
        );
        spec.apply_style(
            1..8,
            &StyleRun {
                features: vec![OpenTypeFeature {
                    tag: "dlig".into(),
                    value: 1,
                }],
                ..Default::default()
            },
        );
        for byte in 0..spec.text.len() {
            let style = spec.style_at(byte);
            let get = |tag| {
                style
                    .features
                    .iter()
                    .find(|f| f.tag == tag)
                    .map(|f| f.value)
            };
            assert_eq!(get("liga"), Some(1));
            assert_eq!(get("kern"), (2..4).contains(&byte).then_some(0));
            assert_eq!(get("dlig"), (1..8).contains(&byte).then_some(1));
        }
        // The full-range font fast path must not discard feature-only edits.
        spec.apply_style(
            0..spec.text.len(),
            &StyleRun {
                features: vec![OpenTypeFeature {
                    tag: "liga".into(),
                    value: 0,
                }],
                ..Default::default()
            },
        );
        assert!(spec
            .style_at(0)
            .features
            .iter()
            .any(|f| f.tag == "liga" && f.value == 0));
        let mut json = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            serde_json::from_value::<TextSpec>(json.clone()).unwrap(),
            spec
        );
        for run in json["runs"].as_array_mut().unwrap() {
            run.as_object_mut().unwrap().remove("features");
        }
        let old: TextSpec = serde_json::from_value(json).unwrap();
        assert!(old.runs.iter().all(|run| run.features.is_empty()));
        assert!(old
            .style_at(0)
            .features
            .iter()
            .any(|f| f.tag == "liga" && f.value == 1));
    }

    #[test]
    fn word_spacing_changes_only_spaces_in_both_layout_paths() {
        for text in ["AV", "A V", "A  V ", "café café", "שלום עולם"] {
            for shaped in [false, true] {
                let (mut s, face) = opentype_spec(text);
                if shaped {
                    s.set_feature("liga", true);
                }
                let before = layout(&s, &face).layout_width;
                for spacing in [-1.0, 2.0, 10.0] {
                    s.word_spacing = spacing;
                    let after = layout(&s, &face).layout_width;
                    let expected = text.chars().filter(|c| *c == ' ').count() as f32 * spacing;
                    assert!(
                        (after - before - expected).abs() < 0.001,
                        "{text}: {before} -> {after}"
                    );
                }
            }
        }
    }

    #[test]
    fn run_tracking_and_leading_affect_only_their_text() {
        for shaped in [false, true] {
            let (mut s, face) = opentype_spec("AB CD\nEF GH");
            if shaped {
                s.set_feature("liga", true);
            }
            let before = layout(&s, &face);
            s.apply_style(
                0..2,
                &StyleRun {
                    tracking: Some(7.0),
                    leading: Some(120.0),
                    ..Default::default()
                },
            );
            let after = layout(&s, &face);
            assert!((after.lines[0].width - before.lines[0].width - 14.0).abs() < 0.001);
            assert_eq!(after.lines[1].width, before.lines[1].width);
            assert_eq!(after.lines[0].advance, 120.0);
            assert_eq!(after.lines[0].height, before.lines[0].height);
            assert_eq!(after.lines[1].height, before.lines[1].height);
            assert!((after.lines[1].top - before.lines[1].top).abs() < 0.001);
            // Leading belongs to the arriving line, not the preceding one.
            s.apply_style(
                6..8,
                &StyleRun {
                    leading: Some(120.0),
                    ..Default::default()
                },
            );
            let after = layout(&s, &face);
            assert!((after.lines[1].baseline - after.lines[0].baseline - 120.0).abs() < 0.001);
            let saved = serde_json::to_string(&s).unwrap();
            assert_eq!(serde_json::from_str::<TextSpec>(&saved).unwrap(), s);
        }
    }

    #[test]
    fn absolute_leading_keeps_nominal_cells_and_ligatures_in_every_writing_mode() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for shaped in [false, true] {
                let (mut spec, face) = opentype_spec("office\n\noffice");
                spec.writing_mode = mode;
                if shaped {
                    spec.set_feature("liga", true);
                }
                let original = layout(&spec, &face);
                for leading in [0.0, 3.0, 120.0] {
                    spec.leading = Some(leading);
                    spec.runs = vec![StyleRun {
                        start: 2,
                        end: 3,
                        leading: Some(leading + 1.0),
                        ..Default::default()
                    }];
                    let spaced = layout(&spec, &face);
                    assert_eq!(spaced.glyphs.len(), original.glyphs.len());
                    assert_eq!(spaced.lines.len(), 3);
                    for (a, b) in original.lines.iter().zip(&spaced.lines) {
                        assert_eq!(a.height, b.height);
                        assert_eq!(a.width, b.width);
                    }
                    assert_eq!(spaced.lines[0].top, 0.0);
                    assert_eq!(spaced.lines[0].advance, leading + 1.0);
                    for pair in spaced.lines.windows(2) {
                        assert!((pair[1].baseline - pair[0].baseline - leading).abs() < 0.001);
                    }
                    for (byte, _) in spec.text.char_indices() {
                        let caret = caret_in_layout(&spec, &spaced, byte);
                        assert!(caret.x.is_finite() && caret.top.is_finite());
                        assert!(
                            caret.height > 0.0,
                            "overlap does not collapse the insertion segment"
                        );
                    }
                }
            }
        }
        let (spec, _) = opentype_spec("legacy");
        let mut old = serde_json::to_value(&spec).unwrap();
        old.as_object_mut().unwrap().remove("leading");
        assert_eq!(serde_json::from_value::<TextSpec>(old).unwrap(), spec);
    }

    #[test]
    fn uniform_baseline_shifts_translate_ink_and_editing_geometry_without_reflow() {
        for text in ["AV office ffi\nH H", "אבגד אבגד"] {
            for mode in [
                WritingMode::Horizontal,
                WritingMode::VerticalRl,
                WritingMode::VerticalLr,
            ] {
                for decorated in [false, true] {
                    let (mut spec, _) = opentype_spec(text);
                    spec.writing_mode = mode;
                    spec.size = 32.0;
                    spec.wrap_width = Some(140.0);
                    spec.apply_style(
                        0..text.len(),
                        &StyleRun {
                            underline: Some(decorated),
                            strikethrough: Some(decorated),
                            ..Default::default()
                        },
                    );
                    let plain = rasterize_with_paints(&spec).unwrap();
                    let lines = line_spans(&spec);
                    let carets_before = carets(&spec);
                    let selection = selection_rects(&spec, 0..text.len());
                    let measured = measure(&spec).unwrap();
                    for shift in [-12.0, 0.0, 7.0, 20.0] {
                        spec.apply_style(
                            0..text.len(),
                            &StyleRun {
                                baseline_shift: Some(shift),
                                ..Default::default()
                            },
                        );
                        let shifted = rasterize_with_paints(&spec).unwrap();
                        let (dx, dy) = if mode.is_vertical() {
                            (shift as i32, 0)
                        } else {
                            (0, -shift as i32)
                        };
                        assert_eq!(shifted.bounds.left, plain.bounds.left + dx);
                        assert_eq!(shifted.bounds.top, plain.bounds.top + dy);
                        assert_eq!(shifted.bounds.width(), plain.bounds.width());
                        assert_eq!(shifted.bounds.height(), plain.bounds.height());
                        assert_eq!(shifted.coverage, plain.coverage);
                        assert_eq!(line_spans(&spec), lines);
                        let after = measure(&spec).unwrap();
                        assert_eq!(
                            (
                                after.width,
                                after.height,
                                after.first_baseline,
                                after.line_advance
                            ),
                            (
                                measured.width,
                                measured.height,
                                measured.first_baseline,
                                measured.line_advance
                            )
                        );
                        if let (Some(a), Some(b)) = (measured.ink_bounds, after.ink_bounds) {
                            for (i, delta) in [0.0, -shift, 0.0, -shift].into_iter().enumerate() {
                                assert!((b[i] - a[i] - delta).abs() < 0.001);
                            }
                        }
                        for ((byte, before), (after_byte, after)) in
                            carets_before.iter().zip(carets(&spec))
                        {
                            assert_eq!(*byte, after_byte);
                            assert!((after.x - before.x - dx as f32).abs() < 0.001);
                            assert!((after.top - before.top - dy as f32).abs() < 0.001);
                            assert_eq!(after.height, before.height);
                        }
                        for (before, after) in
                            selection.iter().zip(selection_rects(&spec, 0..text.len()))
                        {
                            assert_eq!(after.left, before.left + dx);
                            assert_eq!(after.top, before.top + dy);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn mixed_baseline_shifts_keep_affinities_and_selection_on_their_own_runs() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            let (mut spec, _) = opentype_spec("HéH H");
            spec.writing_mode = mode;
            let before = caret_at(&spec, 1).unwrap();
            spec.apply_style(
                1..3,
                &StyleRun {
                    baseline_shift: Some(9.0),
                    ..Default::default()
                },
            );
            let raised = caret_at(&spec, 1).unwrap();
            let previous = caret_at_position(
                &spec,
                CaretPosition {
                    byte: 1,
                    affinity: CaretAffinity::Upstream,
                },
            )
            .unwrap();
            assert_eq!(previous, before);
            if mode.is_vertical() {
                assert!((raised.x - before.x - 9.0).abs() < 0.001);
            } else {
                assert!((raised.top - before.top + 9.0).abs() < 0.001);
            }
            let selected = selection_rects(&spec, 1..3);
            assert_eq!(selected.len(), 1);
            let hit = hit_test_position(&spec, raised.x, raised.top).unwrap();
            assert_eq!(hit.byte, 1);
            let points = insertion_points(&spec);
            assert!(points.iter().any(|(p, c)| p.byte == 1 && *c == raised));
            assert!(points.iter().any(|(p, c)| p.byte == 1 && *c == previous));
        }
    }

    #[test]
    fn baseline_shift_edits_preserve_explicit_zero_and_legacy_serialization() {
        let (mut spec, _) = opentype_spec("Aé中Z");
        spec.apply_style(
            0..spec.text.len(),
            &StyleRun {
                baseline_shift: Some(12.0),
                ..Default::default()
            },
        );
        spec.apply_style(
            1..6,
            &StyleRun {
                baseline_shift: Some(0.0),
                ..Default::default()
            },
        );
        assert_eq!(spec.style_at(0).baseline_shift, 12.0);
        assert_eq!(spec.style_at(3).as_run().baseline_shift, Some(0.0));
        spec.text.replace_range(1..3, "ab");
        spec.splice_runs(1..3, 2);
        assert_eq!(spec.style_at(1).baseline_shift, 0.0);
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            serde_json::from_value::<TextSpec>(json.clone()).unwrap(),
            spec
        );
        let mut old = json;
        for run in old["runs"].as_array_mut().unwrap() {
            run.as_object_mut().unwrap().remove("baseline_shift");
        }
        let old: TextSpec = serde_json::from_value(old).unwrap();
        assert!(old.runs.iter().all(|r| r.baseline_shift.is_none()));
        assert_eq!(old.style_at(0).baseline_shift, 0.0);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            spec.apply_style(
                0..spec.text.len(),
                &StyleRun {
                    baseline_shift: Some(invalid),
                    ..Default::default()
                },
            );
            assert_eq!(spec.style_at(0).baseline_shift, 0.0);
        }
    }

    #[test]
    fn decorations_never_change_shaping_wrapping_or_carets() {
        for text in [
            "AV office affine office AV",
            "אבג אבג אבג",
            "é ffi café ffi",
        ] {
            for mode in [
                WritingMode::Horizontal,
                WritingMode::VerticalRl,
                WritingMode::VerticalLr,
            ] {
                let (mut spec, face) = opentype_spec(text);
                spec.writing_mode = mode;
                spec.wrap_width = Some(170.0);
                spec.set_feature("liga", true);
                spec.set_feature("kern", true);
                let before = layout(&spec, &face);
                let before_lines = line_spans(&spec);
                let before_carets = carets(&spec);
                let glyphs = |laid: &Layout| {
                    laid.glyphs
                        .iter()
                        .map(|g| (g.glyph, g.byte, g.x, g.baseline, g.sideways))
                        .collect::<Vec<_>>()
                };
                let boundaries: Vec<_> = text.char_indices().map(|(i, _)| i).collect();
                for (i, byte) in boundaries.iter().enumerate() {
                    spec.apply_style(
                        *byte..boundaries.get(i + 1).copied().unwrap_or(text.len()),
                        &StyleRun {
                            underline: Some(i % 2 == 0),
                            strikethrough: Some(i % 3 == 0),
                            underline_style: Some(TextDecoration {
                                weight: Some(0.25 + i as f32),
                                offset: Some(i as f32 - 4.0),
                                color: Some([i as u8, 50, 100, 128]),
                                ..Default::default()
                            }),
                            strike_style: Some(TextDecoration {
                                weight: Some(2.75),
                                offset: Some(6.0),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    );
                }
                let after = layout(&spec, &face);
                assert_eq!(glyphs(&before), glyphs(&after));
                assert_eq!(line_spans(&spec), before_lines);
                assert_eq!(carets(&spec), before_carets);
                assert!(!rasterize(&spec).unwrap().is_empty());
            }
        }
    }

    #[test]
    fn custom_decorations_have_point_geometry_and_ink_independent_of_glyph_fill() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for strike in [false, true] {
                for weight in [0.25, 1.5, 6.0] {
                    let (mut spec, face) = opentype_spec("HH HH");
                    spec.writing_mode = mode;
                    let settings = TextDecoration {
                        weight: Some(weight),
                        offset: Some(8.0),
                        color: Some([20, 80, 170, 128]),
                        ..Default::default()
                    };
                    spec.runs = vec![StyleRun {
                        start: 0,
                        end: spec.text.len(),
                        fill_disabled: Some(true),
                        underline: Some(!strike),
                        strikethrough: Some(strike),
                        underline_style: Some(settings.clone()),
                        strike_style: Some(settings.clone()),
                        ..Default::default()
                    }];
                    let laid = layout(&spec, &face);
                    let faces = Faces::resolve(&spec, &face);
                    let lines = decoration_rasters(&spec, &faces, &laid);
                    assert_eq!(lines.len(), spec.text.len());
                    let line = &laid.lines[0];
                    let center = match mode {
                        WritingMode::Horizontal => line.baseline + if strike { -8.0 } else { 8.0 },
                        WritingMode::VerticalRl => {
                            block_extent(&laid.lines) - line.top - line.height / 2.0 + 8.0
                        }
                        WritingMode::VerticalLr => line.top + line.height / 2.0 - 8.0,
                    };
                    for fragment in &lines {
                        let (lo, hi) = if mode.is_vertical() {
                            (fragment.rect.left, fragment.rect.right)
                        } else {
                            (fragment.rect.top, fragment.rect.bottom)
                        };
                        // Line length comes from character advances.
                        let ch = laid
                            .chars
                            .iter()
                            .find(|ch| ch.byte == fragment.byte)
                            .unwrap();
                        let length = ch.x.max(ch.end_x).ceil() - ch.x.min(ch.end_x).floor();
                        assert!(lo as f32 <= center - weight / 2.0);
                        assert!(hi as f32 >= center + weight / 2.0);
                        let area = fragment
                            .bitmap
                            .iter()
                            .map(|v| f64::from(*v) / 255.0)
                            .sum::<f64>();
                        assert!((area / f64::from(length) - f64::from(weight)).abs() < 0.03, "{mode:?}, strike={strike}, weight={weight}, area={area}, length={length}, bounds={:?}", fragment.rect);
                    }
                    let painted = rasterize(&spec).unwrap();
                    assert_eq!(painted.paints.len(), 1);
                    assert_eq!(painted.paints[0].color, settings.color);
                    assert!(painted
                        .rgba([0, 0, 0, 255])
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[3] <= 128));
                    for disabled in [
                        TextDecoration {
                            disabled: true,
                            ..settings.clone()
                        },
                        TextDecoration {
                            weight: Some(0.0),
                            ..settings.clone()
                        },
                    ] {
                        spec.runs[0].underline_style = Some(disabled.clone());
                        spec.runs[0].strike_style = Some(disabled.clone());
                        assert!(rasterize(&spec).unwrap().is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn custom_decoration_paints_preserve_order_and_union_equal_translucent_ink_once() {
        let (mut spec, _) = opentype_spec("HH HH");
        let red = Some([200, 20, 10, 128]);
        let blue = Some([20, 30, 200, 128]);
        let green = Some([10, 200, 20, 128]);
        let line = TextDecoration {
            weight: Some(30.0),
            offset: Some(0.0),
            color: red,
            ..Default::default()
        };
        spec.runs = vec![StyleRun {
            start: 0,
            end: spec.text.len(),
            color: blue,
            underline: Some(true),
            strikethrough: Some(true),
            underline_style: Some(line.clone()),
            strike_style: Some(TextDecoration {
                color: green,
                ..line
            }),
            ..Default::default()
        }];
        let painted = rasterize(&spec).unwrap();
        assert_eq!(
            painted.paints.iter().map(|p| p.color).collect::<Vec<_>>(),
            vec![red, blue, green]
        );
        let at = (0..painted.coverage.len())
            .find(|i| painted.paints.iter().all(|p| p.coverage[*i] == 255))
            .unwrap();
        let rgba = painted.rgba([0, 0, 0, 255]);
        assert!(rgba[at * 4 + 1] > rgba[at * 4 + 2]);
        spec.runs[0].underline_style.as_mut().unwrap().color = blue;
        spec.runs[0].strike_style.as_mut().unwrap().color = blue;
        let union = rasterize(&spec).unwrap();
        assert_eq!(union.paints.len(), 1);
        assert!(union
            .rgba([0, 0, 0, 255])
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] <= 128));
    }

    #[test]
    fn dashes_keep_one_visual_phase_across_characters_spaces_bidi_and_paint_boundaries() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for text in ["HH HH", "AVé漢字", "אבג xyz דהו"] {
                for strike in [false, true] {
                    let (mut spec, _) = opentype_spec(text);
                    spec.writing_mode = mode;
                    let decoration = TextDecoration {
                        weight: Some(2.0),
                        offset: Some(8.0),
                        color: Some([120, 30, 40, 128]),
                        ..Default::default()
                    };
                    spec.runs = vec![StyleRun {
                        start: 0,
                        end: text.len(),
                        fill_disabled: Some(true),
                        underline: Some(!strike),
                        strikethrough: Some(strike),
                        underline_style: Some(decoration.clone()),
                        strike_style: Some(decoration),
                        ..Default::default()
                    }];
                    let solid = rasterize(&spec).unwrap();
                    let run = &mut spec.runs[0];
                    for style in [&mut run.underline_style, &mut run.strike_style]
                        .into_iter()
                        .flatten()
                    {
                        style.pattern = TextDecorationPattern::Dashes(vec![5.0, 3.0].into());
                    }
                    let dashed = rasterize(&spec).unwrap();
                    assert_eq!(dashed.bounds, solid.bounds);
                    let rect = solid.bounds;
                    for y in rect.top..rect.bottom {
                        for x in rect.left..rect.right {
                            let i = ((y - rect.top) * rect.width() + x - rect.left) as usize;
                            let along = if mode.is_vertical() {
                                y - rect.top
                            } else {
                                x - rect.left
                            };
                            assert_eq!(
                                dashed.coverage[i],
                                if along % 8 < 5 { solid.coverage[i] } else { 0 },
                                "{mode:?}, {text}, strike={strike}, x={x}, y={y}"
                            );
                        }
                    }
                    let run = &mut spec.runs[0];
                    for style in [&mut run.underline_style, &mut run.strike_style]
                        .into_iter()
                        .flatten()
                    {
                        style.gap_color = Some([30, 80, 120, 128]);
                    }
                    let unsplit = rasterize(&spec).unwrap();
                    let start = text.char_indices().nth(1).unwrap().0;
                    spec.apply_style(
                        start..text.len(),
                        &StyleRun {
                            color: Some([30, 160, 90, 128]),
                            ..Default::default()
                        },
                    );
                    let split = rasterize(&spec).unwrap();
                    assert_eq!(split.bounds, unsplit.bounds);
                    assert!(split.rgba([0, 0, 0, 255]) == unsplit.rgba([0, 0, 0, 255]), "inactive glyph paint must not split a continuous decoration: {mode:?}, {text}, strike={strike}");
                }
            }
        }
    }

    #[test]
    fn dash_caps_extend_only_segment_ends_preserve_metrics_and_ignore_inactive_paint_boundaries() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for cap in [
                DecorationCap::Butt,
                DecorationCap::Round,
                DecorationCap::Projecting,
            ] {
                for fitting in [
                    DecorationFit::None,
                    DecorationFit::Dashes,
                    DecorationFit::Gaps,
                    DecorationFit::DashesAndGaps,
                ] {
                    for strike in [false, true] {
                        let (mut spec, _) = opentype_spec("HéH AV");
                        spec.writing_mode = mode;
                        let before_carets = carets(&spec);
                        let before_lines = line_spans(&spec);
                        let decoration = TextDecoration {
                            fitting,
                            weight: Some(8.0),
                            offset: Some(12.0),
                            color: Some([200, 40, 20, 128]),
                            gap_color: Some([20, 80, 160, 128]),
                            pattern: TextDecorationPattern::Dashes(DecorationDashes {
                                lengths: vec![5.0, 7.0],
                                cap,
                            }),
                            ..Default::default()
                        };
                        spec.runs = vec![StyleRun {
                            start: 0,
                            end: spec.text.len(),
                            fill_disabled: Some(true),
                            underline: Some(!strike),
                            strikethrough: Some(strike),
                            underline_style: Some(decoration.clone()),
                            strike_style: Some(decoration),
                            ..Default::default()
                        }];
                        let unsplit = rasterize(&spec).unwrap();
                        let along_start = if mode.is_vertical() {
                            unsplit.bounds.top
                        } else {
                            unsplit.bounds.left
                        };
                        assert_eq!(along_start, if cap == DecorationCap::Butt { 0 } else { -4 });
                        for byte in spec
                            .text
                            .char_indices()
                            .map(|(b, _)| b)
                            .skip(1)
                            .collect::<Vec<_>>()
                        {
                            let mut split = spec.clone();
                            split.apply_style(
                                byte..spec.text.len(),
                                &StyleRun {
                                    color: Some([30, 160, 90, 128]),
                                    ..Default::default()
                                },
                            );
                            let actual = rasterize(&split).unwrap();
                            assert_eq!(actual.bounds, unsplit.bounds);
                            assert_eq!(
                                actual.rgba([0, 0, 0, 255]),
                                unsplit.rgba([0, 0, 0, 255]),
                                "{mode:?},{cap:?},strike={strike},byte={byte}"
                            );
                        }
                        assert_eq!(carets(&spec), before_carets);
                        assert_eq!(line_spans(&spec), before_lines);
                    }
                }
            }
        }
    }

    #[test]
    fn striped_gap_masks_partition_solid_lines_and_equal_inks_receive_opacity_once() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for strike in [false, true] {
                for weight in [None, Some(0.75), Some(5.5)] {
                    let (mut spec, face) = opentype_spec("HH HH");
                    spec.writing_mode = mode;
                    let mut decoration = TextDecoration {
                        weight,
                        color: Some([200, 40, 20, 128]),
                        ..Default::default()
                    };
                    spec.runs = vec![StyleRun {
                        start: 0,
                        end: spec.text.len(),
                        fill_disabled: Some(true),
                        underline: Some(!strike),
                        strikethrough: Some(strike),
                        underline_style: Some(decoration.clone()),
                        strike_style: Some(decoration.clone()),
                        ..Default::default()
                    }];
                    let solid = rasterize(&spec).unwrap();
                    let before_carets = carets(&spec);
                    let before_lines = line_spans(&spec);
                    decoration.pattern =
                        TextDecorationPattern::Stripes(vec![0.0, 20.0, 70.0, 100.0]);
                    decoration.gap_color = decoration.color;
                    spec.runs[0].underline_style = Some(decoration.clone());
                    spec.runs[0].strike_style = Some(decoration.clone());
                    let same = rasterize(&spec).unwrap();
                    assert_eq!(same.rgba([0, 0, 0, 255]), solid.rgba([0, 0, 0, 255]));
                    decoration.gap_color = Some([20, 80, 200, 128]);
                    spec.runs[0].underline_style = Some(decoration.clone());
                    spec.runs[0].strike_style = Some(decoration.clone());
                    let faces = Faces::resolve(&spec, &face);
                    let laid = layout(&spec, &face);
                    let parts = decoration_rasters(&spec, &faces, &laid);
                    assert_eq!(parts.len(), spec.text.len() * 2);
                    let mut reference = spec.clone();
                    reference.runs[0].underline_style.as_mut().unwrap().pattern =
                        TextDecorationPattern::Solid;
                    reference.runs[0].strike_style.as_mut().unwrap().pattern =
                        TextDecorationPattern::Solid;
                    let full = decoration_rasters(&reference, &faces, &laid);
                    for (pair, solid) in parts.as_chunks::<2>().0.iter().zip(full) {
                        assert_eq!(pair[0].rect, solid.rect);
                        assert_eq!(pair[1].rect, solid.rect);
                        for ((gap, stripe), full) in
                            pair[0].bitmap.iter().zip(&pair[1].bitmap).zip(solid.bitmap)
                        {
                            // Each separate ink rounds once; the byte sum can
                            // differ from the full line by one rounding level.
                            assert!(
                                (i16::from(*gap) + i16::from(*stripe) - i16::from(full)).abs() <= 1
                            );
                        }
                    }
                    decoration.disabled = true;
                    spec.runs[0].underline_style = Some(decoration.clone());
                    spec.runs[0].strike_style = Some(decoration.clone());
                    let gaps_only = rasterize(&spec).unwrap();
                    assert_eq!(gaps_only.paints.len(), 1);
                    assert_eq!(gaps_only.paints[0].color, decoration.gap_color);
                    assert_eq!(carets(&spec), before_carets);
                    assert_eq!(line_spans(&spec), before_lines);
                }
            }
        }
    }

    #[test]
    fn decoration_dimensions_survive_range_edits_and_explicit_default_resets() {
        let (mut spec, _) = opentype_spec("aé中z");
        let decoration = TextDecoration {
            weight: Some(0.75),
            offset: Some(-4.0),
            color: Some([1, 2, 3, 128]),
            ..Default::default()
        };
        spec.apply_style(
            0..spec.text.len(),
            &StyleRun {
                underline: Some(true),
                underline_style: Some(decoration.clone()),
                strike_style: Some(decoration.clone()),
                ..Default::default()
            },
        );
        assert_eq!(spec.style_at(0).underline_style, decoration);
        spec.apply_style(
            1..6,
            &StyleRun {
                underline_style: Some(TextDecoration::default()),
                ..Default::default()
            },
        );
        assert_eq!(spec.style_at(1).underline_style, TextDecoration::default());
        assert_eq!(spec.style_at(1).strike_style, decoration);
        spec.text.replace_range(1..3, "éé");
        spec.splice_runs(1..3, 4);
        assert_eq!(spec.style_at(8).underline_style, decoration);
        let saved = serde_json::to_vec(&spec).unwrap();
        assert_eq!(serde_json::from_slice::<TextSpec>(&saved).unwrap(), spec);
        let mut legacy = serde_json::to_value(&spec).unwrap();
        for run in legacy["runs"].as_array_mut().unwrap() {
            for key in ["underline_style", "strike_style"] {
                run.as_object_mut().unwrap().remove(key);
            }
        }
        let legacy: TextSpec = serde_json::from_value(legacy).unwrap();
        spec.apply_style(0..spec.text.len(), &legacy.style_at(0).as_run());
        assert_eq!(spec.style_at(8).underline_style, TextDecoration::default());
        assert_eq!(spec.style_at(8).strike_style, TextDecoration::default());
    }

    #[test]
    fn decorations_cover_spaces_without_repainting_translucent_glyphs() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for underline in [false, true] {
                for strike in [false, true] {
                    let (mut spec, face) = opentype_spec("HH HH HH");
                    spec.writing_mode = mode;
                    spec.size = 40.0;
                    let red = Some([220, 0, 0, 128]);
                    let blue = Some([0, 0, 220, 128]);
                    for (start, end, color) in [(0, 3, red), (3, 6, blue), (6, 8, red)] {
                        spec.apply_style(
                            start..end,
                            &StyleRun {
                                color,
                                underline: Some(underline),
                                strikethrough: Some(strike),
                                ..Default::default()
                            },
                        );
                    }
                    let raster = rasterize_with_paints(&spec).unwrap();
                    assert_eq!(
                        raster.paints.iter().map(|p| p.color).collect::<Vec<_>>(),
                        vec![red, blue, red]
                    );
                    // Each pixel of a paint is a union, even where its glyphs
                    // and both decoration lines overlap. Alpha is applied once.
                    let laid = layout(&spec, &face);
                    let faces = Faces::resolve(&spec, &face);
                    let decorations = decoration_rasters(&spec, &faces, &laid);
                    assert_eq!(
                        decorations.len(),
                        8 * (usize::from(underline) + usize::from(strike))
                    );
                    for decoration in decorations {
                        let group = if decoration.byte < 3 {
                            0
                        } else if decoration.byte < 6 {
                            1
                        } else {
                            2
                        };
                        for y in decoration.rect.top..decoration.rect.bottom {
                            for x in decoration.rect.left..decoration.rect.right {
                                let at = ((y - raster.bounds.top) * raster.bounds.width() + x
                                    - raster.bounds.left)
                                    as usize;
                                assert_eq!(raster.paints[group].coverage[at], 255);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn decoration_fragments_preserve_visual_paint_order_in_mixed_bidi_text() {
        for text in ["ab אב cd גד ef", "אב ab גד cd הו"] {
            let (mut spec, _) = opentype_spec(text);
            spec.size = 32.0;
            let boundaries: Vec<_> = text
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(text.len()))
                .collect();
            for (i, pair) in boundaries.windows(2).enumerate() {
                spec.apply_style(
                    pair[0]..pair[1],
                    &StyleRun {
                        color: Some([(i % 3) as u8, 0, 0, 128]),
                        ..Default::default()
                    },
                );
            }
            let before = rasterize_with_paints(&spec).unwrap();
            // Spaces also get glyph entries in shaping, but have no bitmap.
            // Avoid decorating spaces here so paints have identical membership.
            for (byte, ch) in text.char_indices() {
                if !ch.is_whitespace() {
                    spec.apply_style(
                        byte..byte + ch.len_utf8(),
                        &StyleRun {
                            underline: Some(true),
                            strikethrough: Some(true),
                            ..Default::default()
                        },
                    );
                }
            }
            let after = rasterize_with_paints(&spec).unwrap();
            assert_eq!(
                after.paints.iter().map(|p| p.color).collect::<Vec<_>>(),
                before.paints.iter().map(|p| p.color).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn strike_and_underline_are_distinct_and_run_edits_preserve_their_values() {
        for size in [12.0, 28.0, 72.0] {
            let (mut spec, face) = opentype_spec("HH HH");
            spec.size = size;
            let glyphs = rasterize(&spec).unwrap().bounds;
            spec.apply_style(
                0..spec.text.len(),
                &StyleRun {
                    strikethrough: Some(true),
                    underline: Some(true),
                    ..Default::default()
                },
            );
            let faces = Faces::resolve(&spec, &face);
            let laid = layout(&spec, &face);
            let lines = decoration_rasters(&spec, &faces, &laid);
            assert!(lines.iter().any(|r| r.rect.top >= glyphs.bottom));
            assert!(lines
                .iter()
                .any(|r| r.rect.top > glyphs.top && r.rect.bottom < glyphs.bottom));
            spec.apply_style(
                1..4,
                &StyleRun {
                    strikethrough: Some(false),
                    ..Default::default()
                },
            );
            assert!(spec.style_at(0).strikethrough);
            assert!(!spec.style_at(2).strikethrough);
            assert!(spec.style_at(2).underline);
            spec.text.replace_range(2..3, "é中");
            spec.splice_runs(2..3, "é中".len());
            assert!(!spec.style_at(4).strikethrough);
            assert!(spec.style_at(4).underline);
            assert_eq!(spec.style_at(4).as_run().strikethrough, Some(false));
            let saved = serde_json::to_vec(&spec).unwrap();
            assert_eq!(serde_json::from_slice::<TextSpec>(&saved).unwrap(), spec);
            let mut old = serde_json::to_value(&spec).unwrap();
            for run in old["runs"].as_array_mut().unwrap() {
                run.as_object_mut().unwrap().remove("strikethrough");
            }
            let old: TextSpec = serde_json::from_value(old).unwrap();
            assert!(!old.style_at(0).strikethrough);
        }
    }

    #[test]
    fn underline_uses_spaces_and_survives_vertical_layout() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            let (mut s, _) = opentype_spec("   ");
            s.writing_mode = mode;
            assert!(rasterize(&s).unwrap().is_empty());
            s.apply_style(
                0..s.text.len(),
                &StyleRun {
                    underline: Some(true),
                    ..Default::default()
                },
            );
            let raster = rasterize(&s).unwrap();
            assert!(!raster.is_empty());
            assert!(raster.coverage.contains(&255));
            assert_eq!(
                raster.coverage.len(),
                raster.bounds.width() as usize * raster.bounds.height() as usize
            );
        }
    }

    #[test]
    fn shaping_keeps_utf8_carets_wrapping_and_style_runs() {
        let (mut s, _) = opentype_spec("café office second line");
        s.set_feature("liga", true);
        s.wrap_width = Some(180.0);
        s.apply_style(
            6..12,
            &StyleRun {
                size: Some(56.0),
                ..Default::default()
            },
        );
        assert!(line_spans(&s).len() > 1);
        let raster = rasterize(&s).unwrap();
        assert!(!raster.is_empty());
        for (byte, c) in carets(&s) {
            assert!(s.text.is_char_boundary(byte));
            assert!(c.x.is_finite() && c.top.is_finite());
            assert_eq!(hit_test(&s, c.x, c.top + c.height / 2.0), Some(byte));
        }
    }

    #[test]
    fn path_rotates_ink_carets_and_hit_testing_together() {
        use schist_core::path::{Anchor, SubPath};
        let (mut s, _) = opentype_spec("office");
        s.set_feature("liga", true);
        let straight = rasterize(&s).unwrap();
        s.path = Some(TextPath {
            curve: SubPath {
                anchors: vec![Anchor::corner(100.0, 0.0), Anchor::corner(100.0, 400.0)],
                closed: false,
            },
            offset: 0.0,
            span: None,
        });
        let vertical = rasterize(&s).unwrap();
        assert!((vertical.bounds.height() - straight.bounds.width()).abs() <= 2);
        assert!((vertical.bounds.width() - straight.bounds.height()).abs() <= 2);
        for (byte, c) in carets(&s) {
            assert!((c.angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
            assert_eq!(hit_test(&s, c.x - c.height / 2.0, c.top), Some(byte));
        }
        s.path.as_mut().unwrap().offset = 25.0;
        let shifted = rasterize(&s).unwrap();
        assert_eq!(shifted.bounds, vertical.bounds.translated(0, 25));
        s.align = Align::Right;
        let end = caret_at(&s, s.text.len()).unwrap();
        assert!((end.top - 425.0).abs() < 0.01);
    }

    #[test]
    fn curved_and_degenerate_paths_remain_renderable() {
        use schist_core::path::{Anchor, SubPath};
        let (mut s, _) = opentype_spec("Along the curve");
        s.path = Some(TextPath {
            curve: SubPath {
                anchors: vec![
                    Anchor::smooth(0.0, 100.0, 80.0, -100.0),
                    Anchor::smooth(300.0, 100.0, 80.0, 100.0),
                ],
                closed: false,
            },
            offset: 0.0,
            span: None,
        });
        assert!(!rasterize(&s).unwrap().is_empty());
        let cursors = carets(&s);
        assert!((cursors[0].1.angle - cursors.last().unwrap().1.angle).abs() > 0.1);
        s.path
            .as_mut()
            .unwrap()
            .curve
            .anchors
            .fill(Anchor::corner(0.0, 0.0));
        let fallback = rasterize(&s).unwrap();
        s.path = None;
        assert_eq!(fallback.coverage, rasterize(&s).unwrap().coverage);
    }

    fn spec(text: &str) -> TextSpec {
        TextSpec {
            text: text.into(),
            size: 32.0,
            ..Default::default()
        }
    }

    fn ink(r: &TextRaster) -> usize {
        r.coverage.iter().filter(|&&v| v > 0).count()
    }

    fn run(start: usize, end: usize) -> StyleRun {
        StyleRun {
            start,
            end,
            ..Default::default()
        }
    }

    #[test]
    fn a_run_sets_part_of_the_text_in_another_size() {
        let plain = spec("AB");
        let mut mixed = spec("AB");
        mixed.runs.push(StyleRun {
            size: Some(96.0),
            ..run(1, 2)
        });
        let small = rasterize(&plain).unwrap();
        let big_b = rasterize(&mixed).unwrap();
        assert!(big_b.bounds.width() > small.bounds.width() + 10);
        assert!(big_b.bounds.height() > small.bounds.height() + 10);
        // The A is untouched, so the caret between the two letters has
        // not moved; the caret after the B has, by a lot.
        let between = caret_at(&mixed, 1).unwrap().x;
        assert!((between - caret_at(&plain, 1).unwrap().x).abs() < 0.01);
        assert!(caret_at(&mixed, 2).unwrap().x > caret_at(&plain, 2).unwrap().x + 20.0);
        // Both lines of a two-line layout are placed, and the tall B's
        // line is taller than a plain one.
        let mut two = spec("AB\nA");
        two.runs.push(StyleRun {
            size: Some(96.0),
            ..run(1, 2)
        });
        let spans = line_spans(&two);
        assert_eq!(spans.len(), 2);
        assert!(spans[0].height > spans[1].height + 10.0);
        assert!((spans[1].top - spans[0].height).abs() < 0.01);
    }

    #[test]
    fn hit_testing_picks_the_nearest_caret_on_the_nearest_line() {
        let s = spec("ab\ncd");
        let zero = caret_at(&s, 0).unwrap();
        let one = caret_at(&s, 1).unwrap();
        let first_end = caret_at(&s, 2).unwrap();
        let second_start = caret_at(&s, 3).unwrap();
        let four = caret_at(&s, 4).unwrap();

        let first_y = zero.top + zero.height / 2.0;
        let second_y = second_start.top + second_start.height / 2.0;
        assert_eq!(hit_test(&s, zero.x - 100.0, first_y), Some(0));
        assert_eq!(
            hit_test(&s, zero.x + (one.x - zero.x) * 0.75, first_y),
            Some(1)
        );
        assert_eq!(hit_test(&s, first_end.x + 100.0, first_y), Some(2));
        assert_eq!(hit_test(&s, second_start.x - 100.0, second_y), Some(3));
        assert_eq!(
            hit_test(
                &s,
                second_start.x + (four.x - second_start.x) * 0.75,
                second_y + 1_000.0,
            ),
            Some(4),
            "a drag below the text stays on its last line"
        );
    }

    #[test]
    fn old_specs_load_without_runs_and_new_ones_keep_them() {
        let old = r#"{"text":"Hi","family":"X","size":12.0,"align":"Left","line_height":1.0,"tracking":0.0,"wrap_width":null}"#;
        let spec: TextSpec = serde_json::from_str(old).unwrap();
        assert!(spec.runs.is_empty());
        let mut with = spec.clone();
        with.runs.push(StyleRun {
            family: Some("Y".into()),
            bold: Some(true),
            ..run(0, 1)
        });
        let json = serde_json::to_string(&with).unwrap();
        assert!(json.contains("\"runs\""));
        assert!(
            !json.contains("italic\":null"),
            "unset overrides stay out: {json}"
        );
        assert_eq!(serde_json::from_str::<TextSpec>(&json).unwrap(), with);
        // A plain spec writes no runs key at all, so the block is
        // byte-for-byte what it was before runs existed.
        assert!(!serde_json::to_string(&spec).unwrap().contains("runs"));
    }

    #[test]
    fn styling_a_range_splits_the_runs_it_cuts_through() {
        let mut s = spec("hello world");
        s.family = "Base".into();
        s.apply_style(
            0..5,
            &StyleRun {
                bold: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(
            s.runs,
            vec![StyleRun {
                bold: Some(true),
                ..run(0, 5)
            }]
        );
        // A family over "llo wo" cuts the bold run and covers the gap.
        s.apply_style(
            2..8,
            &StyleRun {
                family: Some("Other".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            s.runs,
            vec![
                StyleRun {
                    bold: Some(true),
                    ..run(0, 2)
                },
                StyleRun {
                    bold: Some(true),
                    family: Some("Other".into()),
                    ..run(2, 5)
                },
                StyleRun {
                    family: Some("Other".into()),
                    ..run(5, 8)
                },
            ]
        );
        assert_eq!(s.style_at(3).family, "Other");
        assert!(s.style_at(3).bold);
        assert_eq!(s.style_at(9).family, "Base");
        assert_eq!(s.families(), vec!["Base", "Other"]);
        // The whole text moves the setting onto the layer and lifts it
        // from the runs; the bold stays where it was.
        s.apply_style(
            0..s.text.len(),
            &StyleRun {
                family: Some("All".into()),
                ..Default::default()
            },
        );
        assert_eq!(s.family, "All");
        assert_eq!(
            s.runs,
            vec![StyleRun {
                bold: Some(true),
                ..run(0, 5)
            }]
        );
        assert_eq!(s.families(), vec!["All"]);
    }

    #[test]
    fn editing_the_text_keeps_the_runs_in_step() {
        let mut s = spec("ab cd");
        s.runs.push(StyleRun {
            bold: Some(true),
            ..run(3, 5)
        });
        // Typing after the bold word stays bold.
        s.text.push('e');
        s.splice_runs(5..5, 1);
        assert_eq!(s.runs[0].end, 6);
        // Typing before it goes in plain and pushes it along.
        s.text.insert(3, 'X');
        s.splice_runs(3..3, 1);
        assert_eq!((s.runs[0].start, s.runs[0].end), (4, 7));
        // Typing inside it grows it.
        s.text.insert(5, 'Y');
        s.splice_runs(5..5, 1);
        assert_eq!((s.runs[0].start, s.runs[0].end), (4, 8));
        // Deleting across its start shortens it from the front.
        s.text.replace_range(2..6, "");
        s.splice_runs(2..6, 0);
        assert_eq!((s.runs[0].start, s.runs[0].end), (2, 4));
        // Deleting it entirely drops it.
        s.text.replace_range(2..4, "");
        s.splice_runs(2..4, 0);
        assert!(s.runs.is_empty());
        // Typing over a selection that starts inside a run keeps its style.
        let mut s = spec("abcdef");
        s.runs.push(StyleRun {
            italic: Some(true),
            ..run(2, 4)
        });
        s.text.replace_range(3..5, "XYZ");
        s.splice_runs(3..5, 3);
        assert_eq!((s.runs[0].start, s.runs[0].end), (2, 6));
    }

    #[test]
    fn system_fonts_are_available() {
        assert!(!families().is_empty(), "no system fonts found");
        assert!(!default_family().is_empty());
    }

    #[test]
    fn renders_glyph_coverage() {
        let r = rasterize(&spec("Hi")).expect("font loads");
        assert!(!r.is_empty(), "expected ink");
        assert!(r.bounds.width() > 10, "bounds {:?}", r.bounds);
        assert!(r.bounds.height() > 10);
        assert_eq!(
            r.coverage.len(),
            (r.bounds.width() * r.bounds.height()) as usize
        );
    }

    #[test]
    fn empty_text_is_empty_not_an_error() {
        let r = rasterize(&spec("")).expect("font loads");
        assert!(r.is_empty());
        assert!(r.bounds.is_empty());
    }

    #[test]
    fn whitespace_only_produces_no_ink() {
        let r = rasterize(&spec("   ")).expect("font loads");
        assert!(r.is_empty());
    }

    #[test]
    fn larger_size_makes_larger_output() {
        let small = rasterize(&TextSpec {
            size: 16.0,
            ..spec("Ag")
        })
        .unwrap();
        let large = rasterize(&TextSpec {
            size: 64.0,
            ..spec("Ag")
        })
        .unwrap();
        assert!(
            large.bounds.width() > small.bounds.width() * 2,
            "{} vs {}",
            large.bounds.width(),
            small.bounds.width()
        );
    }

    #[test]
    fn newlines_stack_lines_vertically() {
        let one = rasterize(&spec("A")).unwrap();
        let two = rasterize(&spec("A\nA")).unwrap();
        assert!(
            two.bounds.height() > one.bounds.height() + 10,
            "two lines should be taller: {} vs {}",
            two.bounds.height(),
            one.bounds.height()
        );
        assert!(two.bounds.width() <= one.bounds.width() + 2);
    }

    #[test]
    fn wrapping_narrows_and_heightens() {
        let unwrapped = rasterize(&spec("hello world hello world")).unwrap();
        let wrapped = rasterize(&TextSpec {
            wrap_width: Some(120.0),
            ..spec("hello world hello world")
        })
        .unwrap();
        assert!(wrapped.bounds.width() < unwrapped.bounds.width());
        assert!(wrapped.bounds.height() > unwrapped.bounds.height());
        // Wrapping must not drop glyphs.
        assert!(ink(&wrapped) as f32 > ink(&unwrapped) as f32 * 0.9);
    }

    #[test]
    fn tracking_widens_without_changing_height() {
        let plain = rasterize(&spec("iiii")).unwrap();
        let tracked = rasterize(&TextSpec {
            tracking: 6.0,
            ..spec("iiii")
        })
        .unwrap();
        assert!(tracked.bounds.width() > plain.bounds.width() + 12);
        assert_eq!(tracked.bounds.height(), plain.bounds.height());
    }

    #[test]
    fn alignment_shifts_short_lines() {
        let left = rasterize(&TextSpec {
            align: Align::Left,
            ..spec("mmmmmmm\ni")
        })
        .unwrap();
        let right = rasterize(&TextSpec {
            align: Align::Right,
            ..spec("mmmmmmm\ni")
        })
        .unwrap();
        // Same overall box, but the short line's ink moves to the far side.
        let column_ink = |r: &TextRaster, from: f32, to: f32| {
            let w = r.bounds.width() as usize;
            let x0 = (w as f32 * from) as usize;
            let x1 = (w as f32 * to) as usize;
            let mut n = 0;
            for y in (r.bounds.height() / 2) as usize..r.bounds.height() as usize {
                for x in x0..x1.min(w) {
                    if r.coverage[y * w + x] > 0 {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(column_ink(&left, 0.0, 0.2) > column_ink(&left, 0.8, 1.0));
        assert!(column_ink(&right, 0.8, 1.0) > column_ink(&right, 0.0, 0.2));
    }

    #[test]
    fn unknown_family_falls_back_instead_of_failing() {
        let r = rasterize(&TextSpec {
            family: "No Such Font 12345".into(),
            ..spec("A")
        });
        assert!(r.is_some(), "should fall back to a system sans");
        assert!(!r.unwrap().is_empty());
    }
    #[test]
    fn caret_advances_along_a_line() {
        let s = spec("abc");
        let a = caret_at(&s, 0).unwrap();
        let b = caret_at(&s, 1).unwrap();
        let c = caret_at(&s, 3).unwrap();
        assert!(a.x < b.x && b.x < c.x, "{a:?} {b:?} {c:?}");
        // All on the first line.
        assert_eq!(a.top, 0.0);
        assert_eq!(c.top, 0.0);
    }

    #[test]
    fn caret_steps_down_by_the_real_line_advance() {
        let s = spec("ab\ncd");
        let first = caret_at(&s, 0).unwrap();
        let second = caret_at(&s, 3).unwrap(); // just after the newline
        assert!(second.top > first.top, "second line must sit lower");
        // The step is the engine's own line advance, which is what the
        // old overlay got wrong by assuming size * line_height.
        let spans = line_spans(&s);
        assert_eq!(spans.len(), 2);
        assert!((second.top - first.top - spans[0].height).abs() < 0.01);
    }

    #[test]
    fn caret_after_a_newline_starts_the_next_line() {
        let s = spec("ab\ncd");
        let after_newline = caret_at(&s, 3).unwrap();
        let line_start = line_spans(&s)[1];
        assert!((after_newline.x - line_start.x).abs() < 0.01);
    }

    #[test]
    fn line_spans_cover_the_source_text() {
        let s = spec("ab\ncde\nf");
        let spans = line_spans(&s);
        assert_eq!(spans.len(), 3);
        assert_eq!((spans[0].start, spans[0].end), (0, 2));
        assert_eq!((spans[1].start, spans[1].end), (3, 6));
        assert_eq!((spans[2].start, spans[2].end), (7, 8));
    }

    #[test]
    fn a_trailing_newline_gets_its_own_line() {
        // `str::lines` drops this, which is why the old caret stayed put
        // when you pressed Enter at the end of the text.
        let s = spec("ab\n");
        assert_eq!(line_spans(&s).len(), 2);
        let end = caret_at(&s, 3).unwrap();
        assert!(end.top > 0.0, "caret must move to the new empty line");
    }

    #[test]
    fn an_out_of_range_or_mid_char_offset_does_not_panic() {
        let s = spec("héllo");
        assert!(caret_at(&s, 999).is_some());
        // Byte 2 is inside the two-byte 'é'.
        assert!(caret_at(&s, 2).is_some());
    }
}

// A library call owns these temporary resources. The desktop's mutable font
// database and cache are not linked into the headless distribution.
#[cfg(schist_library)]
fn font_db() -> RwLock<Arc<fontdb::Database>> {
    RwLock::new(Arc::new(scan_fonts()))
}
#[cfg(schist_library)]
fn font_cache() -> std::sync::Mutex<std::collections::HashMap<FaceKey, Option<LoadedFace>>> {
    std::sync::Mutex::new(std::collections::HashMap::new())
}
#[cfg(schist_library)]
pub fn family_names() -> Vec<String> {
    families()
}
#[cfg(schist_library)]
pub fn refresh() {}

#[cfg(test)]
mod color_interchange_tests {
    use super::*;
    #[test]
    fn color_runs_survive_splices_and_do_not_change_plain_font_metrics() {
        let mut spec = TextSpec {
            text: "AV color".into(),
            ..Default::default()
        };
        let width = measure(&spec).unwrap().width;
        spec.apply_style(
            1..spec.text.len(),
            &StyleRun {
                color: Some([210, 25, 15, 128]),
                ..Default::default()
            },
        );
        assert_eq!(measure(&spec).unwrap().width, width);
        assert_eq!(spec.style_at(2).color, Some([210, 25, 15, 128]));
        let raster = rasterize(&spec).unwrap();
        assert!(raster.colors.contains(&Some([210, 25, 15, 128])));
        spec.text.replace_range(2..3, "new");
        spec.splice_runs(2..3, 3);
        assert_eq!(spec.style_at(4).color, Some([210, 25, 15, 128]));
        let decoded: TextSpec =
            serde_json::from_slice(&serde_json::to_vec(&spec).unwrap()).unwrap();
        assert_eq!(decoded, spec);
    }
}
