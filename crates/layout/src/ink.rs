//! Inks, and the output-time rules that turn them into plates.
//!
//! An [`Ink`] is a *named colour*, and that is all it is. Two properties
//! matter for print:
//!
//! * whether it is **spot** -- a premixed ink that needs its own plate --
//!   or **process**, built from the CMYK inks every press already has;
//! * what it looks like, which is stored as Lab because that is the only
//!   space in which a Pantone book, a CMYK build and an RGB screen value
//!   can be compared without a profile doing the work.
//!
//! # Why this is not `schist_core::ink`
//!
//! [`schist_core::ink::InkChannel`] is a *painted plate*: a scalar
//! coverage buffer you brush ink into, already registered to the raster,
//! already round-tripping through PSD. That is the right model for
//! retouching a channel, and the wrong one for page layout, where an
//! object simply says "my fill is Pantone 032 C" and the plate is
//! derived from the composition at output time.
//!
//! So the two coexist. This module names inks; the separation engine
//! resolves them and emits [`schist_core::ink::InkChannel`] plates, which
//! inherit the existing registration, undo, CRDT and PSD write for free.

use serde::{Deserialize, Serialize};

/// A named colour, spot or process.
///
/// Lab is the device-independent definition. Authored CMYK builds are
/// retained separately: converting them through RGB would lose black
/// generation. RGB is a screen approximation and a preview-output fallback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ink {
    pub name: String,
    /// L* 0..=100, a* and b* roughly -128..=127.
    pub lab: [f32; 3],
    /// Screen approximation. Explicitly not a colour-managed value.
    #[serde(default)]
    pub preview_rgb: [f32; 3],
    /// Authored device CMYK, when present; values are fractions, not percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_cmyk: Option<[f32; 4]>,
    /// True for a premixed ink needing its own plate.
    pub spot: bool,
}

/// Fractions of the full-strength fill and stroke inks, independent of opacity.
/// Zero still paints opaque paper in knockout mode. Ink identity never changes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaintTints {
    pub fill: f32,
    pub stroke: f32,
}
impl Default for PaintTints {
    fn default() -> Self {
        Self {
            fill: 1.0,
            stroke: 1.0,
        }
    }
}

/// Invalid programmatic values fall back to full strength; authored values are
/// validated by controls/codecs. Rendering always receives a finite fraction.
pub fn bounded_tint(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

impl Ink {
    /// Screen approximation at a fraction of this ink. Native CMYK channels
    /// are scaled before conversion; other colours interpolate toward paper.
    pub fn preview_at_tint(&self, tint: f32) -> [f32; 3] {
        let tint = bounded_tint(tint);
        if let Some(cmyk) = self.source_cmyk.filter(|_| !self.spot) {
            let [c, m, y, k] = cmyk.map(|v| v * tint);
            [
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k),
            ]
        } else {
            self.preview_rgb.map(|v| 1.0 - tint * (1.0 - v))
        }
    }

    /// A process ink, black unless told otherwise.
    pub fn process(name: impl Into<String>, rgb: [f32; 3]) -> Ink {
        Ink {
            name: name.into(),
            lab: rgb_to_lab(rgb),
            preview_rgb: rgb,
            source_cmyk: None,
            spot: false,
        }
    }

    /// A spot ink defined by its Lab value, which is how a spot colour
    /// book is actually specified.
    pub fn spot(name: impl Into<String>, lab: [f32; 3]) -> Ink {
        Ink {
            name: name.into(),
            lab,
            preview_rgb: lab_to_rgb(lab),
            source_cmyk: None,
            spot: true,
        }
    }

    /// Document black. The default text colour, and the one to look for
    /// in preflight: text set in a process mix rather than real black
    /// prints lighter and softer than intended.
    pub fn black() -> Ink {
        Ink::process("Black", [0.0, 0.0, 0.0])
    }

    pub fn white() -> Ink {
        Ink::process("Paper", [1.0, 1.0, 1.0])
    }

    /// Preserve a device CMYK definition without a lossy RGB round trip.
    pub fn cmyk(name: impl Into<String>, cmyk: [f32; 4]) -> Ink {
        let [c, m, y, k] = cmyk;
        let mut ink = Self::process(
            name,
            [
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k),
            ],
        );
        ink.source_cmyk = Some(cmyk);
        ink
    }

    /// The CMYK build of this colour, 0..=1 per channel.
    ///
    /// An authored CMYK definition is returned unchanged. Otherwise this
    /// is the naive conversion on purpose, and it is
    /// **preview-grade**: it sets K to `1 - max(r,g,b)`, so one colour
    /// channel always comes out exactly zero. That is fine for a swatch
    /// preview and for deciding whether a spot must be converted at all.
    ///
    /// It is *not* fine for separation. Under-colour removal withdraws
    /// from the smallest channel, and a build with a zero channel has
    /// nothing to withdraw from, so UCR over this conversion is silently
    /// a no-op. Anything printing a real plate needs an ICC-based build
    /// instead, which is why the separation crate takes its CMYK build
    /// from a caller rather than calling this.
    pub fn to_cmyk(&self) -> [f32; 4] {
        if let Some(cmyk) = self.source_cmyk {
            return cmyk;
        }
        let [r, g, b] = self.preview_rgb;
        let k = 1.0 - r.max(g).max(b);
        if k >= 1.0 {
            return [0.0, 0.0, 0.0, 1.0];
        }
        let inv = 1.0 - k;
        [
            (1.0 - r - k) / inv,
            (1.0 - g - k) / inv,
            (1.0 - b - k) / inv,
            k,
        ]
    }

    /// Luminance 0..=1, for the greyscale proof view.
    pub fn lightness(&self) -> f32 {
        (self.lab[0] / 100.0).clamp(0.0, 1.0)
    }
}

/// Where an ink's plate goes at output, and how it gets there.
///
/// These are output-time decisions, not document content. A prepress
/// provider converting a spot to its process equivalent must not rewrite
/// the document, which is why they live here rather than on the [`Ink`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InkManager {
    /// Ink name -> what to actually do with it.
    pub inks: Vec<(String, InkAlias)>,
    /// Maximum total area coverage as a fraction, 0..=1. `None` means the
    /// press's own limit, which preflight cannot know.
    pub total_area_limit: Option<f32>,
    /// Under-colour removal strength, 0..=1. Trades colour for ink
    /// coverage in the shadows.
    pub ucr: f32,
    /// Black generation: how much K is allowed to form on its own.
    pub black_generation: f32,
    /// The text black a prepress provider would ask for, as CMYK.
    pub text_black: [f32; 4],
}

/// What output should do with one ink.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InkAlias {
    /// Print this ink on its own plate under its own name.
    Separate,
    /// Print this ink on the process plates, converted to CMYK.
    ///
    /// This is what a prepress provider reaches for when a job has more
    /// spot colours than the press can plate.
    ConvertToProcess,
    /// Print this ink's coverage on another ink's plate instead.
    ///
    /// Two Pantones that are really the same ink, or a spot defined in
    /// two ways. The target must itself be a spot.
    Alias(String),
}

impl InkAlias {
    pub fn target(&self) -> Option<&str> {
        match self {
            InkAlias::Alias(name) => Some(name.as_str()),
            _ => None,
        }
    }
}

impl InkManager {
    pub fn with_defaults() -> InkManager {
        InkManager {
            inks: Vec::new(),
            // 300% is a common coated-sheet limit and a safe default
            // that preflight will still flag when exceeded.
            total_area_limit: Some(3.0),
            ucr: 0.0,
            black_generation: 0.0,
            // Text set in 100% K. Any other mix in body copy is worth a
            // preflight warning.
            text_black: [0.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn rule_for(&self, ink: &Ink) -> InkAlias {
        self.inks
            .iter()
            .find(|(name, _)| *name == ink.name)
            .map(|(_, rule)| rule.clone())
            .unwrap_or(if ink.spot {
                InkAlias::Separate
            } else {
                InkAlias::ConvertToProcess
            })
    }

    pub fn set_rule(&mut self, ink: impl Into<String>, rule: InkAlias) {
        let name = ink.into();
        match self.inks.iter_mut().find(|(n, _)| *n == name) {
            Some(existing) => existing.1 = rule,
            None => self.inks.push((name, rule)),
        }
    }

    /// Resolve every alias to the ink whose plate it prints on.
    ///
    /// Chains are followed (`A -> B -> C` gives three inks on C's plate)
    /// and a cycle stops at the point it was detected rather than
    /// looping, because the alternative is a hung preflight.
    pub fn resolve<'a>(&'a self, inks: &'a [Ink]) -> Vec<PlatedInk<'a>> {
        inks.iter()
            .map(|ink| {
                let rule = self.rule_for(ink);
                let (plate_of, separate) = match &rule {
                    InkAlias::Separate => (ink.name.clone(), true),
                    InkAlias::ConvertToProcess => ("Process".to_string(), false),
                    InkAlias::Alias(_) => {
                        let target = self.follow_alias(ink, inks);
                        let separate = inks
                            .iter()
                            .find(|i| i.name == target)
                            .is_none_or(|i| self.rule_for(i) != InkAlias::ConvertToProcess);
                        (target, separate)
                    }
                };
                PlatedInk {
                    ink,
                    plate_of,
                    separate,
                    alias_of: rule.target().map(|s| s.to_string()),
                }
            })
            .collect()
    }

    /// Walk an alias chain to the ink that owns the plate.
    fn follow_alias(&self, ink: &Ink, inks: &[Ink]) -> String {
        let mut current = ink.name.clone();
        // Bounded by the ink count; each step must reach a different ink
        // or we have found a cycle.
        for _ in 0..inks.len() {
            let rule = self.rule_for_by_name(&current);
            let Some(next) = rule.and_then(|r| r.target().map(|s| s.to_string())) else {
                break;
            };
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    fn rule_for_by_name(&self, name: &str) -> Option<InkAlias> {
        self.inks
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, rule)| rule.clone())
    }

    /// Split a CMYK build into process plates plus under-colour removal.
    ///
    /// Given `cmyk`, return `(c, m, y, k)` after UCR and black generation.
    /// The ink limit is applied first, because that is the constraint that
    /// actually gets a job rejected.
    ///
    /// Note that enforcing the limit **changes the artwork**. A caller
    /// that wants to know whether a page is over the limit should measure
    /// it, not clamp it, and should only ask for this to be applied when
    /// the user has asked for the fix. Use
    /// [`InkManager::separating_without_limit`] for the measure case.
    pub fn separate_cmyk(&self, cmyk: [f32; 4]) -> [f32; 4] {
        self.separate_cmyk_with(cmyk)
    }

    /// The same separation with the ink limit and UCR and black
    /// generation all disabled, for measuring a page's own coverage.
    pub fn separating_without_limit(&self) -> InkManager {
        InkManager {
            inks: self.inks.clone(),
            total_area_limit: None,
            ucr: 0.0,
            black_generation: 0.0,
            text_black: self.text_black,
        }
    }

    /// A copy with its separation settings replaced.
    pub fn separating(
        &self,
        ucr: f32,
        black_generation: f32,
        total_area_limit: Option<f32>,
    ) -> InkManager {
        InkManager {
            inks: self.inks.clone(),
            total_area_limit,
            ucr,
            black_generation,
            text_black: self.text_black,
        }
    }

    /// Whether this manager would change anything, so a caller can skip
    /// a full-page pass when it would not.
    pub fn is_identity(&self) -> bool {
        self.ucr == 0.0 && self.black_generation == 0.0 && self.total_area_limit.is_none()
    }

    /// The separation itself, without the convenience wrapper. A copy of
    /// the manager is cheap; a full-page pixel pass is not, so the caller
    /// decides once whether it is needed.
    pub fn separate_cmyk_with(&self, cmyk: [f32; 4]) -> [f32; 4] {
        let [mut c, mut m, mut y, mut k] = cmyk;

        // Total area coverage before anything else.
        let total = c + m + y + k;
        if let Some(limit) = self.total_area_limit {
            if total > limit && total > 0.0 {
                let scale = limit / total;
                c *= scale;
                m *= scale;
                y *= scale;
                k *= scale;
            }
        }

        if self.ucr > 0.0 {
            // Withdraw from the colour inks and give the ink back as K.
            // Only the smallest channel is taken, so a saturated colour
            // keeps its hue instead of collapsing to black. This is a
            // large coverage *reduction*, which is the point of UCR: the
            // shadow detail that would otherwise need 300% ink is
            // carried by K alone.
            let chroma = c.min(m).min(y);
            if chroma > 0.0 {
                let withdraw = chroma * self.ucr;
                c -= withdraw;
                m -= withdraw;
                y -= withdraw;
                k = (k + withdraw).min(1.0);
            }
        }

        if self.black_generation > 0.0 {
            // Pull K up in the darkest areas, taken from colour that is
            // not contributing much to the hue anyway.
            let grey = (c + m + y) / 3.0;
            let add = (1.0 - grey) * self.black_generation;
            if add > 0.0 {
                let available = c.min(m).min(y).max(0.0);
                let take = add.min(available);
                c -= take;
                m -= take;
                y -= take;
                k += take;
            }
        }

        [
            c.clamp(0.0, 1.0),
            m.clamp(0.0, 1.0),
            y.clamp(0.0, 1.0),
            k.clamp(0.0, 1.0),
        ]
    }
}

/// One ink and where its coverage will be printed.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatedInk<'a> {
    pub ink: &'a Ink,
    /// The name of the plate this ink's coverage lands on. Several inks
    /// can share one, which is the point of aliasing.
    pub plate_of: String,
    /// False when this is converted to the process plates.
    pub separate: bool,
    /// The ink this one is an alias of, when it is an alias.
    pub alias_of: Option<String>,
}

impl<'a> PlatedInk<'a> {
    /// The CMYK this ink contributes to the process plates.
    ///
    /// A separated spot contributes nothing: it prints on its own plate,
    /// not smeared across C, M and Y.
    pub fn process_contribution(&self) -> [f32; 4] {
        if self.separate {
            [0.0; 4]
        } else {
            self.ink.to_cmyk()
        }
    }
}

/// Total area coverage of a CMYK build, as a fraction. 1.0 means 100%.
pub fn total_area_coverage(cmyk: [f32; 4]) -> f32 {
    cmyk.iter().sum()
}

/// Lab is stored D50, which is the space ICC uses for ink definitions and
/// the one a spot colour book is specified in.
fn rgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    schist_color::convert::rgb_to_lab_d50(schist_color::Rgba::new(rgb[0], rgb[1], rgb[2], 1.0))
}

fn lab_to_rgb(lab: [f32; 3]) -> [f32; 3] {
    let px = schist_color::convert::lab_d50_to_rgb(lab, 1.0);
    [px.r, px.g, px.b]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inks() -> Vec<Ink> {
        vec![
            Ink::black(),
            Ink::process("Cyan", [0.0, 0.68, 0.94]),
            Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0]),
            Ink::spot("PANTONE 032 C U", [50.0, 60.0, 55.0]),
        ]
    }

    #[test]
    fn a_spot_defaults_to_its_own_plate_and_process_to_the_shared_ones() {
        let mgr = InkManager::with_defaults();
        let inks = inks();
        let plated = mgr.resolve(&inks);
        assert!(plated.iter().all(|p| p.ink.spot || !p.separate));
        let spot = plated
            .iter()
            .find(|p| p.ink.name == "PANTONE 032 C")
            .unwrap();
        assert!(spot.separate);
        assert_eq!(spot.plate_of, "PANTONE 032 C");
    }

    #[test]
    fn a_spot_converted_to_process_contributes_cmyk() {
        let mut mgr = InkManager::with_defaults();
        mgr.set_rule("PANTONE 032 C", InkAlias::ConvertToProcess);
        let inks = inks();
        let plated = mgr.resolve(&inks);
        let spot = plated
            .iter()
            .find(|p| p.ink.name == "PANTONE 032 C")
            .unwrap();
        assert!(!spot.separate);
        assert_eq!(spot.plate_of, "Process");
        // Converted to process, it now has process coverage to give.
        assert!(spot.process_contribution().iter().sum::<f32>() > 0.0);
        // A spot left on its own plate contributes nothing to the process
        // plates -- that is the whole point of separating it.
        let kept = plated
            .iter()
            .find(|p| p.ink.name == "PANTONE 032 C U")
            .unwrap();
        assert_eq!(kept.process_contribution(), [0.0; 4]);
    }

    #[test]
    fn aliasing_merges_two_names_onto_one_plate() {
        let mut mgr = InkManager::with_defaults();
        mgr.set_rule("PANTONE 032 C U", InkAlias::Alias("PANTONE 032 C".into()));
        let inks = inks();
        let plated = mgr.resolve(&inks);
        let a = plated
            .iter()
            .find(|p| p.ink.name == "PANTONE 032 C")
            .unwrap();
        let b = plated
            .iter()
            .find(|p| p.ink.name == "PANTONE 032 C U")
            .unwrap();
        assert_eq!(a.plate_of, b.plate_of);
    }

    #[test]
    fn an_alias_chain_terminates_at_the_last_ink() {
        let mut mgr = InkManager::with_defaults();
        mgr.set_rule("Black", InkAlias::Alias("Cyan".into()));
        mgr.set_rule("Cyan", InkAlias::Alias("PANTONE 032 C".into()));
        let inks = inks();
        let plated = mgr.resolve(&inks);
        let black = plated.iter().find(|p| p.ink.name == "Black").unwrap();
        assert_eq!(black.plate_of, "PANTONE 032 C");
    }

    #[test]
    fn an_alias_cycle_terminates_rather_than_hanging() {
        let mut mgr = InkManager::with_defaults();
        mgr.set_rule("PANTONE 032 C", InkAlias::Alias("PANTONE 032 C U".into()));
        mgr.set_rule("PANTONE 032 C U", InkAlias::Alias("PANTONE 032 C".into()));
        let inks = inks();
        let plated = mgr.resolve(&inks);
        // Whatever it lands on, the point is that it returns.
        let a = plated
            .iter()
            .find(|p| p.ink.name == "PANTONE 032 C")
            .unwrap();
        assert!(!a.plate_of.is_empty());
    }

    #[test]
    fn total_area_limit_is_enforced_by_scaling() {
        let mut mgr = InkManager::with_defaults();
        mgr.total_area_limit = Some(2.0);
        // 100% of everything: 400% coverage.
        let out = mgr.separate_cmyk([1.0, 1.0, 1.0, 1.0]);
        assert!(total_area_coverage(out) <= 2.0 + 1e-5);
        // Scaling preserves the ratios, so the colour is unchanged.
        assert!((out[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn under_colour_removal_reduces_total_coverage_and_adds_black() {
        let mut mgr = InkManager::with_defaults();
        mgr.total_area_limit = None;
        mgr.ucr = 0.0;
        let before = mgr.separate_cmyk([0.5, 0.5, 0.5, 0.2]);
        mgr.ucr = 1.0;
        let after = mgr.separate_cmyk([0.5, 0.5, 0.5, 0.2]);
        // UCR exists to pull 300% shadows back onto one ink, so coverage
        // must come down...
        assert!(total_area_coverage(after) < total_area_coverage(before));
        // ...and the ink that came off the colour plates goes to K.
        assert!(after[3] > before[3]);
    }

    #[test]
    fn under_colour_removal_never_raises_coverage() {
        let source = [0.6, 0.5, 0.4, 0.3];
        let baseline = total_area_coverage(source);
        for ucr in [0.0, 0.25, 0.5, 1.0] {
            let mut mgr = InkManager::with_defaults();
            mgr.total_area_limit = None;
            mgr.ucr = ucr;
            let out = mgr.separate_cmyk(source);
            assert!(
                total_area_coverage(out) <= baseline + 1e-6,
                "ucr {ucr} raised coverage"
            );
        }
    }

    #[test]
    fn a_saturated_colour_keeps_its_hue_under_ucr() {
        // Only the smallest channel may be withdrawn, or pure cyan would
        // come out as neutral grey.
        let mut mgr = InkManager::with_defaults();
        mgr.total_area_limit = None;
        mgr.ucr = 1.0;
        let out = mgr.separate_cmyk([0.0, 1.0, 1.0, 0.0]);
        assert!(out[1] > 0.9 && out[2] > 0.9);
        assert!(out[0] < 0.01);
    }

    #[test]
    fn pure_black_under_ucr_does_not_gain_ink() {
        let mut mgr = InkManager::with_defaults();
        mgr.total_area_limit = None;
        mgr.ucr = 1.0;
        let out = mgr.separate_cmyk([0.0, 0.0, 0.0, 1.0]);
        assert!((out[3] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn text_black_is_100_percent_k() {
        let mgr = InkManager::with_defaults();
        assert_eq!(mgr.text_black, [0.0, 0.0, 0.0, 1.0]);
    }
}
