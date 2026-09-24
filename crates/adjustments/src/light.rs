//! Photoshop's modern Light adjustment (`brit` + a `CgEd` descriptor).
//!
//! The native six controls are preserved exactly. Rendering uses a bounded
//! tonal-curve approximation; Photoshop's adaptive highlights/shadows are
//! not a documented per-pixel operation. See docs/psd-interchange.md.

use crate::{descriptor, ParamSpec};
use schist_i18n::t;

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Light {
    /// Exposure in stops. The remaining controls are -100..100.
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
}

impl Light {
    /// `CgEd` also accompanies legacy Brightness/Contrast and other kinds.
    /// Accept only a complete, finite Light descriptor of the known version.
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.get(..4)? != 16u32.to_be_bytes() {
            return None;
        }
        let d = descriptor::parse(&raw[4..])?;
        if !matches!(d.get("Vrsn"), Some(descriptor::Value::Integer(1)))
            || !matches!(d.get("brightnessMode"), Some(descriptor::Value::Enum(ty, mode))
                if ty == "brightnessModeType" && mode == "brightnessModeLight")
        {
            return None;
        }
        let number = |key| {
            let value = match d.get(key)? {
                descriptor::Value::Double(v) => *v as f32,
                descriptor::Value::Integer(v) => *v as f32,
                _ => return None,
            };
            value.is_finite().then_some(value)
        };
        Some(Self {
            exposure: number("brightnessExposure")?,
            contrast: number("Cntr")?,
            highlights: number("brightnessHighlights")?,
            shadows: number("brightnessShadows")?,
            whites: number("brightnessWhites")?,
            blacks: number("brightnessBlacks")?,
        })
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut d = descriptor::Builder::new("null");
        d.integer("Vrsn", 1)
            .double("brightnessExposure", self.exposure as f64)
            .integer("Brgh", 0)
            .integer("Cntr", self.contrast.round() as i32)
            .integer("brightnessWhites", self.whites.round() as i32)
            .integer("brightnessBlacks", self.blacks.round() as i32)
            .integer("brightnessHighlights", self.highlights.round() as i32)
            .integer("brightnessShadows", self.shadows.round() as i32)
            .integer("means", 127)
            .bool("Lab ", false)
            .enumerated(
                "brightnessMode",
                "brightnessModeType",
                "brightnessModeLight",
            )
            .bool("useLegacy", false)
            .bool("Auto", false);
        let mut out = 16u32.to_be_bytes().to_vec();
        out.extend(d.finish());
        out
    }

    pub(crate) fn apply(&self, value: f32) -> f32 {
        let amount = |v: f32| v.clamp(-100.0, 100.0) / 100.0;
        // A gain with a white-preserving shoulder avoids clipping exposure
        // before subsequent controls can recover the bright end of the range.
        let gain = 2.0f32.powf(self.exposure.clamp(-20.0, 20.0));
        let x = value.clamp(0.0, 1.0);
        let x = gain * x / (1.0 + (gain - 1.0) * x);
        // Bending the middle keeps the endpoints fixed, unlike legacy
        // contrast, whose -100 would turn the entire image middle grey.
        let x = x + amount(self.contrast) * (4.0 / 3.0) * x * (1.0 - x) * (x - 0.5);
        let x = x
            + amount(self.shadows) * x * (1.0 - x).powi(2)
            + amount(self.highlights) * x * x * (1.0 - x);
        let black = -amount(self.blacks) * 0.4;
        let white = 1.0 - amount(self.whites) * 0.125;
        ((x - black) / (white - black)).clamp(0.0, 1.0)
    }

    pub(crate) fn param_specs(&self) -> Vec<ParamSpec> {
        [
            ("exposure", "common.exposure", self.exposure),
            ("contrast", "common.contrast", self.contrast),
            (
                "highlights",
                "filter.camera_raw.param.highlights",
                self.highlights,
            ),
            ("shadows", "filter.camera_raw.param.shadows", self.shadows),
            ("whites", "filter.camera_raw.param.whites", self.whites),
            ("blacks", "filter.camera_raw.param.blacks", self.blacks),
        ]
        .into_iter()
        .map(|(key, label, value)| ParamSpec {
            key,
            label: t(label),
            min: if key == "exposure" { -5.0 } else { -100.0 },
            max: if key == "exposure" { 5.0 } else { 100.0 },
            value,
            suffix: "",
        })
        .collect()
    }

    pub(crate) fn set_param(&mut self, key: &str, value: f32) {
        if !value.is_finite() {
            return;
        }
        let slot = match key {
            "exposure" => {
                self.exposure = value.clamp(-5.0, 5.0);
                return;
            }
            "contrast" => &mut self.contrast,
            "highlights" => &mut self.highlights,
            "shadows" => &mut self.shadows,
            "whites" => &mut self.whites,
            "blacks" => &mut self.blacks,
            _ => return,
        };
        *slot = value.clamp(-100.0, 100.0).round();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Params;
    use schist_color::Rgba;

    #[test]
    fn default_is_identity_and_every_control_is_live() {
        let neutral = Params::Light(Light::default());
        let px = Rgba::new(0.17, 0.49, 0.83, 0.37);
        assert_eq!(neutral.apply(px), px);
        for spec in neutral.param_specs() {
            let mut p = neutral.clone();
            p.set_param(spec.key, spec.max);
            let out = p.apply(px);
            assert_ne!(out, px, "{} is ignored", spec.key);
            assert_eq!(out.a, px.a);
            assert_eq!(
                serde_json::from_str::<Params>(&serde_json::to_string(&p).unwrap()).unwrap(),
                p
            );
            p.set_param(spec.key, f32::NAN);
            assert_eq!(p.apply(px), out, "invalid slider value was accepted");
        }
    }

    #[test]
    fn extremes_are_bounded_and_monotone() {
        // All combinations of the six controls must avoid reversals, NaNs
        // and range excursions, including coincident extreme settings.
        for bits in 0..64 {
            let mut p = Params::Light(Light::default());
            for (i, spec) in p.param_specs().iter().enumerate() {
                p.set_param(
                    spec.key,
                    if bits & (1 << i) == 0 {
                        spec.min
                    } else {
                        spec.max
                    },
                );
            }
            let mut last = 0.0;
            for i in 0..=1024 {
                let v = i as f32 / 1024.0;
                let out = p.apply(Rgba::new(v, v, v, 0.7));
                assert!((0.0..=1.0).contains(&out.r), "{p:?}: {out:?}");
                assert!(out.r >= last - 1e-6, "tone reversal in {p:?}");
                assert_eq!(out.a, 0.7);
                last = out.r;
            }
        }
    }

    #[test]
    fn negative_contrast_retains_endpoints_and_tonal_detail() {
        let p = Light {
            contrast: -100.0,
            ..Light::default()
        };
        assert_eq!(p.apply(0.0), 0.0);
        assert_eq!(p.apply(1.0), 1.0);
        assert!(p.apply(0.25) < p.apply(0.5));
        assert!(p.apply(0.5) < p.apply(0.75));
    }
}
