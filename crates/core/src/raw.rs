//! Non-destructive camera-raw source and development settings.
//!
//! The kernel does not decode camera files. It only carries the immutable
//! capture and the settings used to render a raster layer from it, in the
//! same way [`crate::SmartObject`] carries the source behind rendered smart
//! object pixels. Codecs and the app own the actual development pipeline.

use std::sync::Arc;

/// The editable controls for a camera-raw development.
///
/// Values use the ranges presented by the Camera Raw dialog: exposure is
/// measured in EV, sharpening is 0..=150, and the other controls are
/// generally -100..=100 (noise reduction is 0..=100). Keeping a typed,
/// fixed layout makes the document format stable even if the UI is moved or
/// the filter plug-in is unavailable.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RawSettings {
    pub temperature: f32,
    pub tint: f32,
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub vibrance: f32,
    pub saturation: f32,
    pub sharpening: f32,
    pub noise: f32,
    pub vignette: f32,
    /// Shadows/midtones/highlights/global colour wheels.
    pub grading: ColorGrading,
}

impl RawSettings {
    /// Replace non-finite values with neutral settings and constrain every
    /// control to the range the editor exposes. PSD blocks are untrusted
    /// input, and public callers need the same guarantee as the UI.
    pub fn sanitized(self) -> RawSettings {
        let signed = |value: f32| {
            if value.is_finite() {
                value.clamp(-100.0, 100.0)
            } else {
                0.0
            }
        };
        let unsigned = |value: f32, max: f32| {
            if value.is_finite() {
                value.clamp(0.0, max)
            } else {
                0.0
            }
        };
        RawSettings {
            temperature: signed(self.temperature),
            tint: signed(self.tint),
            exposure: if self.exposure.is_finite() {
                self.exposure.clamp(-5.0, 5.0)
            } else {
                0.0
            },
            contrast: signed(self.contrast),
            highlights: signed(self.highlights),
            shadows: signed(self.shadows),
            whites: signed(self.whites),
            blacks: signed(self.blacks),
            clarity: signed(self.clarity),
            dehaze: signed(self.dehaze),
            vibrance: signed(self.vibrance),
            saturation: signed(self.saturation),
            sharpening: unsigned(self.sharpening, 150.0),
            noise: unsigned(self.noise, 100.0),
            vignette: signed(self.vignette),
            grading: self.grading.sanitized(),
        }
    }
}

/// One colour wheel of [`ColorGrading`]: a tint (hue in degrees, strength
/// 0..=100) and a luminance shift (-100..=100) for one tonal region.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GradeWheel {
    pub hue: f32,
    pub saturation: f32,
    pub luminance: f32,
}

impl GradeWheel {
    pub fn is_neutral(&self) -> bool {
        self.saturation == 0.0 && self.luminance == 0.0
    }

    fn sanitized(self) -> GradeWheel {
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        GradeWheel {
            hue: finite(self.hue).rem_euclid(360.0),
            saturation: finite(self.saturation).clamp(0.0, 100.0),
            luminance: finite(self.luminance).clamp(-100.0, 100.0),
        }
    }
}

/// Colour grading in the manner of Lightroom's Color Grading panel: a
/// wheel each for the shadows, midtones and highlights, one for the whole
/// image, how much the three regions overlap (`blending`, 0..=100) and
/// where the split between shadows and highlights falls (`balance`,
/// -100..=100). The rendering lives with the Camera Raw filter; the
/// kernel only carries the settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorGrading {
    pub shadows: GradeWheel,
    pub midtones: GradeWheel,
    pub highlights: GradeWheel,
    pub global: GradeWheel,
    pub blending: f32,
    pub balance: f32,
}

impl Default for ColorGrading {
    fn default() -> Self {
        ColorGrading {
            shadows: GradeWheel::default(),
            midtones: GradeWheel::default(),
            highlights: GradeWheel::default(),
            global: GradeWheel::default(),
            blending: 50.0,
            balance: 0.0,
        }
    }
}

impl ColorGrading {
    /// True when no wheel tints or shifts anything; blending and balance
    /// only shape the regions, so they cannot change a pixel on their own.
    pub fn is_identity(&self) -> bool {
        self.wheels().iter().all(GradeWheel::is_neutral)
    }

    /// Shadows, midtones, highlights, global.
    pub fn wheels(&self) -> [GradeWheel; 4] {
        [self.shadows, self.midtones, self.highlights, self.global]
    }

    pub fn sanitized(self) -> ColorGrading {
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        ColorGrading {
            shadows: self.shadows.sanitized(),
            midtones: self.midtones.sanitized(),
            highlights: self.highlights.sanitized(),
            global: self.global.sanitized(),
            blending: finite(self.blending, 50.0).clamp(0.0, 100.0),
            balance: finite(self.balance, 0.0).clamp(-100.0, 100.0),
        }
    }
}

/// The original capture behind a rendered RAW layer.
///
/// `Arc` makes history snapshots and live previews cheap: all of them share
/// one immutable copy of a file that may be hundreds of megabytes.
#[derive(Debug, Clone)]
pub struct RawDevelopment {
    pub source: Arc<[u8]>,
    pub settings: RawSettings,
    /// Local adjustments, applied in order over the global development.
    pub masks: Vec<crate::raw_masks::LocalMask>,
}

impl PartialEq for RawDevelopment {
    fn eq(&self, other: &Self) -> bool {
        // Settings almost always differ during an edit, so compare them
        // before considering a potentially enormous byte slice. History
        // snapshots normally share the Arc and take the pointer-fast path.
        self.settings == other.settings
            && self.masks == other.masks
            && (Arc::ptr_eq(&self.source, &other.source) || self.source == other.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_finite_and_bounded_at_the_model_boundary() {
        let settings = RawSettings {
            temperature: f32::NAN,
            exposure: 90.0,
            contrast: -500.0,
            sharpening: 500.0,
            noise: -4.0,
            ..RawSettings::default()
        }
        .sanitized();
        assert_eq!(settings.temperature, 0.0);
        assert_eq!(settings.exposure, 5.0);
        assert_eq!(settings.contrast, -100.0);
        assert_eq!(settings.sharpening, 150.0);
        assert_eq!(settings.noise, 0.0);
    }

    #[test]
    fn grading_is_sanitized_with_the_rest() {
        let mut settings = RawSettings::default();
        assert!(settings.grading.is_identity());
        assert_eq!(settings.grading.blending, 50.0);
        settings.grading.shadows = GradeWheel {
            hue: -30.0,
            saturation: 400.0,
            luminance: f32::INFINITY,
        };
        settings.grading.blending = f32::NAN;
        let clean = settings.sanitized().grading;
        assert_eq!(clean.shadows.hue, 330.0);
        assert_eq!(clean.shadows.saturation, 100.0);
        assert_eq!(clean.shadows.luminance, 0.0);
        assert_eq!(clean.blending, 50.0);
        assert!(!clean.is_identity());
    }
}
