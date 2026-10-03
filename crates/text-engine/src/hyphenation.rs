//! Transient policy for generated hyphens in the single-line composer.
//! Manual source discretionary characters remain explicit break requests.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HyphenationPolicy {
    /// Maximum selected hyphens on consecutive lines. Zero means unlimited.
    pub consecutive_limit: usize,
    /// Selected hyphens preceding this continuation of the same paragraph.
    pub preceding_hyphens: usize,
    /// Permitted end whitespace before generating a hyphen, in TextSpec units.
    /// The caller passes zero for justified paragraphs.
    pub zone: f32,
    /// 0 favors spacing; 100 favors a whole-word break whenever one fits.
    /// A single-line squared-raggedness penalty, not native paragraph-composer
    /// parity. Values above 100 are clamped defensively.
    pub weight: u8,
}

#[derive(Default)]
pub(crate) struct BreakPolicy<'a> {
    pub generated: &'a [usize],
    pub settings: HyphenationPolicy,
}

impl BreakPolicy<'_> {
    pub fn permits(
        &self,
        end: usize,
        consecutive: usize,
        width: f32,
        visible: f32,
        plain: Option<f32>,
    ) -> bool {
        if self.generated.binary_search(&end).is_err() {
            return true;
        }
        let settings = self.settings;
        if settings.consecutive_limit != 0 && consecutive >= settings.consecutive_limit {
            return false;
        }
        let Some(plain) = plain else { return true };
        let gap = (width - plain).max(0.0);
        let zone = if settings.zone.is_finite() {
            settings.zone.max(0.0)
        } else {
            0.0
        };
        if gap <= zone {
            return false;
        }
        let hyphen_gap = (width - visible).max(0.0);
        let penalty = f32::from(settings.weight.min(100)) / 100.0 * width.max(0.0).powi(2);
        hyphen_gap.powi(2) + penalty < gap.powi(2) + 0.000001
    }
}
