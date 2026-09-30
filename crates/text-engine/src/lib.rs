//! Text layout and rasterization for text layers.
//!
//! Scope: system font discovery, one colour per layer with the family,
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

#[cfg(test)]
mod directions_tests;
mod shaping;
mod text_path;
pub use text_path::TextPath;

/// A layer-wide OpenType feature override. Tags are four ASCII bytes,
/// e.g. `liga`, `kern`, `smcp`, or `ss01`; zero disables a feature.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OpenTypeFeature {
    pub tag: String,
    pub value: u32,
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
    pub text: String,
    pub family: String,
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
    /// Extra spacing between characters, in pixels.
    pub tracking: f32,
    /// Extra advance for each ordinary word space, independent of tracking.
    #[serde(default)]
    pub word_spacing: f32,
    /// Inline wrap length in pixels (column length in vertical writing); `None` means never wrap.
    pub wrap_width: Option<f32>,
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
            text: String::new(),
            family: default_family(),
            bold: false,
            italic: false,
            size: 48.0,
            align: Align::Left,
            direction: ParagraphDirection::Auto,
            writing_mode: WritingMode::Horizontal,
            line_height: 1.0,
            tracking: 0.0,
            word_spacing: 0.0,
            wrap_width: None,
            runs: Vec::new(),
            features: Vec::new(),
            path: None,
        }
    }
}

/// A range of characters set in something other than the layer's own
/// font: each field that is `Some` overrides the layer's, the rest
/// inherit. Byte offsets into `TextSpec::text`.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct StyleRun {
    pub start: usize,
    pub end: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
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
    /// Cross-axis offset in pixels: positive raises horizontal text and moves
    /// vertical text right. It does not change line advance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_shift: Option<f32>,
    /// Character fill; None inherits the text layer fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
}

impl StyleRun {
    /// True when this run changes nothing, so it can be dropped.
    pub fn is_plain(&self) -> bool {
        self.family.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.size.is_none()
            && self.metric_size.is_none()
            && self.color.is_none()
            && self.tracking.is_none()
            && self.leading.is_none()
            && self.underline.is_none()
            && self.strikethrough.is_none()
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
        if over.family.is_some() {
            self.family = over.family.clone();
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
        if over.tracking.is_some() {
            self.tracking = over.tracking;
        }
        if over.leading.is_some() {
            self.leading = over.leading;
        }
        if over.baseline_shift.is_some() {
            self.baseline_shift = over.baseline_shift;
        }
        if over.strikethrough.is_some() {
            self.strikethrough = over.strikethrough;
        }
        if over.underline.is_some() {
            self.underline = over.underline;
        }
        if over.color.is_some() {
            self.color = over.color;
        }
    }

    /// Whether the two runs would set a character the same way.
    fn same_style(&self, other: &StyleRun) -> bool {
        self.family == other.family
            && self.bold == other.bold
            && self.italic == other.italic
            && self.size == other.size
            && self.metric_size == other.metric_size
            && self.color == other.color
            && self.tracking == other.tracking
            && self.leading == other.leading
            && self.underline == other.underline
            && self.strikethrough == other.strikethrough
            && self.baseline_shift == other.baseline_shift
    }
}

/// The font one character is set in, once the layer's own settings and
/// any run covering it have been reconciled.
#[derive(Debug, Clone, PartialEq)]
pub struct CharStyle {
    pub tracking: f32,
    pub leading: Option<f32>,
    pub underline: bool,
    pub strikethrough: bool,
    pub baseline_shift: f32,
    pub color: Option<[u8; 4]>,
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub size: f32,
    pub metric_size: Option<f32>,
}

impl CharStyle {
    /// Decorations do not split shaping runs: toggling a line through a word
    /// must not change its ligatures, kerning, wrapping or caret positions.
    fn shapes_like(&self, other: &Self) -> bool {
        self.family == other.family
            && self.bold == other.bold
            && self.italic == other.italic
            && self.size == other.size
            && self.tracking == other.tracking
            && self.leading == other.leading
            && self.baseline_shift == other.baseline_shift
            && self.color == other.color
    }

    /// The style as an override that would reproduce it in full.
    pub fn as_run(&self) -> StyleRun {
        StyleRun {
            start: 0,
            end: 0,
            family: Some(self.family.clone()),
            bold: Some(self.bold),
            italic: Some(self.italic),
            size: Some(self.size),
            metric_size: self.metric_size,
            color: self.color,
            tracking: Some(self.tracking),
            leading: self.leading,
            underline: Some(self.underline),
            strikethrough: Some(self.strikethrough),
            baseline_shift: Some(self.baseline_shift),
        }
    }
}

impl TextSpec {
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
            tracking: self.tracking,
            leading: None,
            underline: false,
            strikethrough: false,
            baseline_shift: 0.0,
            color: None,
            family: self.family.clone(),
            bold: self.bold,
            italic: self.italic,
            size: self.size,
            metric_size: None,
        }
    }

    /// The font the character at `byte` is set in.
    pub fn style_at(&self, byte: usize) -> CharStyle {
        let mut style = self.base_style();
        if let Some(run) = self.runs.iter().find(|r| r.start <= byte && byte < r.end) {
            if let Some(f) = &run.family {
                style.family = f.clone();
            }
            if let Some(b) = run.bold {
                style.bold = b;
            }
            if let Some(i) = run.italic {
                style.italic = i;
            }
            if let Some(s) = run.size {
                style.size = s;
            }
            style.metric_size = run.metric_size.filter(|v| v.is_finite() && *v > 0.0);
            style.color = run.color;
            style.tracking = run.tracking.unwrap_or(style.tracking);
            style.leading = run.leading;
            style.underline = run.underline.unwrap_or(false);
            style.strikethrough = run.strikethrough.unwrap_or(false);
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
    /// cuts through. A range spanning the whole text moves the setting
    /// onto the layer itself and lifts it from every run, so text set
    /// in one font stays described as such.
    pub fn apply_style(&mut self, range: std::ops::Range<usize>, over: &StyleRun) {
        let len = self.text.len();
        let range = range.start.min(len)..range.end.min(len);
        if over.is_plain() {
            return;
        }
        if range.start == 0
            && range.end == len
            && over.color.is_none()
            && over.metric_size.is_none()
            && over.tracking.is_none()
            && over.leading.is_none()
            && over.underline.is_none()
            && over.strikethrough.is_none()
            && over.baseline_shift.is_none()
        {
            if let Some(f) = &over.family {
                self.family = f.clone();
                self.runs.iter_mut().for_each(|r| r.family = None);
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
    /// Per-pixel run fill overrides, empty when all glyphs inherit the layer fill.
    pub colors: Vec<Option<[u8; 4]>>,
    /// Populated only by `rasterize_with_paints`.
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
type FaceKey = (String, bool, bool);

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

/// Load and cache a parsed font by family name.
fn load_font(family: &str, bold: bool, italic: bool) -> Option<LoadedFace> {
    let cache = font_cache();
    let key = (family.to_string(), bold, italic);
    if let Some(hit) = cache.lock().ok()?.get(&key) {
        return hit.clone();
    }

    // Asked-for family first, then its metric equivalents, then the
    // generic sans as a last resort.
    let mut families = vec![fontdb::Family::Name(family)];
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
    let font = db().query(&query).and_then(|id| {
        db().with_face_data(id, |data, index| {
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
#[derive(Debug, Clone, Copy)]
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
struct Layout {
    glyphs: Vec<PlacedGlyph>,
    first_baseline: f32,
    line_advance: f32,
    layout_width: f32,
    lines: Vec<LineSpan>,
    chars: Vec<CharPos>,
}

/// The faces a spec sets its text in, one per distinct family, style
/// and size, and which of them each byte of the text uses. Index 0 is
/// the layer's own font.
struct Faces {
    faces: Vec<(LoadedFace, f32)>,
    by_byte: Vec<usize>,
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
            let mut style = spec.style_at(s);
            // These properties do not change the font face or kerning.
            style.color = None;
            style.tracking = base_style.tracking;
            style.leading = base_style.leading;
            style.underline = base_style.underline;
            style.strikethrough = base_style.strikethrough;
            style.baseline_shift = base_style.baseline_shift;
            style.metric_size = base_style.metric_size;
            if style == base_style {
                continue;
            }
            let ix = match styles.iter().position(|k| *k == style) {
                Some(ix) => ix,
                None => {
                    // A family that cannot be loaded falls back to the
                    // layer's own face, at the run's size.
                    let face = load_font(&style.family, style.bold, style.italic)
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
        Faces { faces, by_byte }
    }

    fn at(&self, byte: usize) -> usize {
        self.by_byte.get(byte).copied().unwrap_or(0)
    }

    fn line_metrics_at(&self, spec: &TextSpec, byte: usize) -> (f32, f32) {
        let ix = self.at(byte);
        let Some(size) = spec.style_at(byte).metric_size else {
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

/// Absolute run leading participates in the same maximum as inherited font
/// metrics, so a large run cannot overlap the following line accidentally.
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
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or_else(|| {
                    faces.line_metrics_at(spec, start + i).1 * spec.line_height.max(0.1)
                })
        })
        .reduce(f32::max)
        .unwrap_or(fallback * spec.line_height.max(0.1))
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
    /// Logical line-box advance. Baseline spacing also depends on font ascent.
    pub height: f32,
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
    if shaping::required(spec) {
        return shaping::layout(spec, base, widths);
    }
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
            let wraps = wrap_width_at(spec, widths, lines.len()).is_some_and(|w| {
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
    let mut top = 0.0f32;
    for (i, line) in lines.iter().enumerate() {
        // Nominal script sizes keep line geometry independent of glyph scaling.
        let (ascent, line_gap) = line
            .text
            .char_indices()
            .map(|(byte, _)| faces.line_metrics_at(spec, line.start + byte))
            .reduce(|(a, g), (fa, fg)| (a.max(fa), g.max(fg)))
            .unwrap_or_else(|| faces.line_metrics(0));
        let line_advance = run_line_advance(spec, &faces, line.start, line.end, line_gap);
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
            height: line_advance,
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
        top += line_advance;
    }
    Layout {
        glyphs: placed,
        first_baseline,
        line_advance: first_advance,
        layout_width: max_width,
        lines: spans,
        chars,
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
    if widths.iter().any(|w| !w.is_finite() || *w <= 0.0) {
        return Vec::new();
    }
    let Some(face) = load_font(&spec.family, spec.bold, spec.italic) else {
        return Vec::new();
    };
    layout_with_widths(spec, &face, widths).lines
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
    let face = load_font(&spec.family, spec.bold, spec.italic)?;
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
    let Some(face) = load_font(&spec.family, spec.bold, spec.italic) else {
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
    let total = laid.lines.last().map_or(0.0, |l| l.top + l.height);
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
    let Some(face) = load_font(&spec.family, spec.bold, spec.italic) else {
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
    let total = spans.last().map_or(0.0, |l| l.top + l.height);
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
    let Some(face) = load_font(&spec.family, spec.bold, spec.italic) else {
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
    /// vertical writing and text on a path. No coverage bitmap is allocated.
    pub ink_bounds: Option<[f32; 4]>,
}

pub fn measure(spec: &TextSpec) -> Option<TextMetrics> {
    let face = load_font(&spec.family, spec.bold, spec.italic)?;
    let laid = layout(spec, &face);
    let ink_bounds = if spec.writing_mode == WritingMode::Horizontal && spec.path.is_none() {
        let faces = Faces::resolve(spec, &face);
        laid.glyphs
            .iter()
            .filter_map(|glyph| {
                let (face, size) = &faces.faces[glyph.face];
                let b = face.font.metrics_indexed(glyph.glyph, *size).bounds;
                (b.width > 0.0 && b.height > 0.0).then_some([
                    glyph.x + b.xmin,
                    glyph.baseline - b.ymin - b.height,
                    glyph.x + b.xmin + b.width,
                    glyph.baseline - b.ymin,
                ])
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

/// A glyph or decoration fragment, retaining the owning source cluster.
struct ColoredRaster {
    rect: IntRect,
    bitmap: Vec<u8>,
    byte: usize,
}

/// Solid decorations follow character advances, including spaces and RTL.
/// Horizontal offsets/thickness use OpenType post/OS/2 metrics when available.
/// Vertical underlines follow the column's outside edge; strikes cross its
/// center. Path decorations and custom line styles are not supported yet.
fn decoration_rasters(spec: &TextSpec, faces: &Faces, laid: &Layout) -> Vec<ColoredRaster> {
    let mut out = Vec::new();
    if spec.path.is_some()
        || !spec
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
    let total_height: f32 = laid.lines.iter().map(|line| line.height).sum();
    for line in &laid.lines {
        for ch in laid
            .chars
            .iter()
            .filter(|ch| line.start <= ch.byte && ch.byte < line.end)
        {
            let style = spec.style_at(ch.byte);
            let start = ch.x.min(ch.end_x).floor() as i32;
            let length = (ch.x.max(ch.end_x).ceil() as i32 - start).max(0) as u32;
            for (strike, enabled) in [(false, style.underline), (true, style.strikethrough)] {
                if !enabled {
                    continue;
                }
                let (position, thickness) = metrics[faces.at(ch.byte)][usize::from(strike)];
                let thickness = thickness.round().max(1.0) as u32;
                let rect = if spec.writing_mode.is_vertical() {
                    let center = if spec.writing_mode == WritingMode::VerticalRl {
                        total_height - line.top - line.height / 2.0
                    } else {
                        line.top + line.height / 2.0
                    };
                    let cross = if strike {
                        center - thickness as f32 / 2.0
                    } else if spec.writing_mode == WritingMode::VerticalRl {
                        center + style.size * 0.55
                    } else {
                        center - style.size * 0.55 - thickness as f32
                    };
                    IntRect::from_xywh(
                        (cross + style.baseline_shift).round() as i32,
                        start,
                        thickness,
                        length,
                    )
                } else {
                    IntRect::from_xywh(
                        start,
                        (line.baseline - style.baseline_shift - position).round() as i32,
                        length,
                        thickness,
                    )
                };
                if !rect.is_empty() {
                    out.push(ColoredRaster {
                        rect,
                        bitmap: vec![255; rect.width() as usize * rect.height() as usize],
                        byte: ch.byte,
                    });
                }
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
    let face = load_font(&spec.family, spec.bold, spec.italic)?;
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
    let laid = layout(spec, &face);
    let guide = path_guide(spec, &laid);
    let decorations = decoration_rasters(spec, &faces, &laid);
    // Associate decorations with the same consecutive visual paint as their
    // glyphs. Appending all decoration masks after the glyphs repaints earlier
    // translucent runs and lets their lines cross later differently colored ink.
    let paint_groups = (!decorations.is_empty()).then(|| {
        let mut colors = Vec::new();
        let mut groups = std::collections::HashMap::new();
        for ch in &laid.chars {
            let color = spec.style_at(ch.byte).color;
            if colors.last() != Some(&color) {
                colors.push(color);
            }
            groups.insert(ch.byte, colors.len() - 1);
        }
        groups
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
            first_baseline: 0.0,
            line_advance: 0.0,
            layout_width: 0.0,
            cap_height: face.cap_ratio.map(|r| r * spec.size),
        });
    }

    // Rasterize once to find the union of glyph boxes...
    let mut rasterized = Vec::with_capacity(placed.len());
    let mut bounds = IntRect::EMPTY;
    for g in &placed {
        let (font, size) = &faces.faces[g.face];
        let (metrics, bitmap) = font.font.rasterize_indexed(g.glyph, *size);
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        let (rect, bitmap) = if let Some(guide) = &guide {
            text_path::glyph_bitmap(guide, g, first_baseline, &metrics, bitmap)
        } else if g.sideways {
            let left = (g.x + metrics.ymin as f32).floor() as i32;
            let top = (g.baseline + metrics.xmin as f32).floor() as i32;
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
            let left = (g.x + metrics.xmin as f32).floor() as i32;
            let top = (g.baseline - metrics.height as f32 - metrics.ymin as f32).floor() as i32;
            (
                IntRect::from_xywh(left, top, metrics.width as u32, metrics.height as u32),
                bitmap,
            )
        };
        bounds = bounds.union(&rect);
        rasterized.push(ColoredRaster {
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
    if let Some(groups) = paint_groups {
        rasterized.sort_by_key(|fragment| groups.get(&fragment.byte).copied().unwrap_or(0));
    }
    if bounds.is_empty() {
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

    // ...then blit them into one mask, taking the max where glyphs overlap.
    let w = bounds.width() as usize;
    let h = bounds.height() as usize;
    let mut coverage = vec![0u8; w * h];
    let mut colors = if spec.runs.iter().any(|r| r.color.is_some()) {
        vec![None; w * h]
    } else {
        Vec::new()
    };
    let mut paints: Vec<TextPaint> = Vec::new();
    for ColoredRaster { rect, bitmap, byte } in rasterized {
        let color = spec.style_at(byte).color;
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
        font_cache()
            .lock()
            .unwrap()
            .insert((spec.family.clone(), false, false), Some(face.clone()));
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
            assert_eq!(after.lines[0].height, 120.0);
            assert_eq!(after.lines[1].height, before.lines[1].height);
            assert_eq!(after.lines[1].top, 120.0);
            let saved = serde_json::to_string(&s).unwrap();
            assert_eq!(serde_json::from_str::<TextSpec>(&saved).unwrap(), s);
        }
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
