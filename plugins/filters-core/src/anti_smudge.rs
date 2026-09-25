//! Experimental restoration of lens-contamination streaks and haze.
//! The bundled MFDNet transfer weights expand from XZ on first use.
//! See docs/anti-smudge.md for training and model provenance.

use crate::param;
use schist_i18n::{t, tf};
use schist_plugin_api::{FilterParam, FilterPlugin, FilterValues};

pub struct AntiSmudge;

impl FilterPlugin for AntiSmudge {
    fn id(&self) -> &'static str {
        "filter.neural.anti_smudge"
    }

    fn name(&self) -> &'static str {
        t("filter.neural.anti_smudge.name")
    }

    fn category(&self) -> &'static str {
        t("filter.category.neural")
    }

    fn params(&self) -> Vec<FilterParam> {
        vec![param(
            "strength",
            t("common.strength"),
            0.0,
            100.0,
            60.0,
            "",
        )]
    }

    fn info(&self) -> Option<String> {
        if schist_neural::get("anti-smudge").is_some() {
            return Some(format!("{} · {}", t("common.preview"), t("common.ready")));
        }
        let state = if schist_neural::installed("anti-smudge") {
            t("common.failed").to_owned()
        } else {
            tf!(
                "filter.neural.anti_smudge.msg.no_model",
                model = self.name()
            )
        };
        Some(format!("{state}\n{}", t("filter.neural.msg.get_model")))
    }

    fn apply(&self, px: &mut [f32], width: usize, height: usize, values: &FilterValues) {
        let Some(len) = width.checked_mul(height).and_then(|n| n.checked_mul(4)) else {
            return;
        };
        let strength = values.get("strength") / 100.0;
        if width == 0 || height == 0 || px.len() != len || !strength.is_finite() || strength <= 0.0
        {
            return;
        }
        let Some(model) = schist_neural::get("anti-smudge") else {
            return;
        };
        restore_rgba(&model, px, width, height, strength);
    }
}

fn restore_rgba(
    model: &schist_neural::Model,
    px: &mut [f32],
    width: usize,
    height: usize,
    strength: f32,
) {
    let mut rgb: Vec<f32> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| p[..3].iter().copied())
        .collect();
    if let Err(error) = schist_neural::try_restore(model, &mut rgb, width, height, strength) {
        log::warn!("anti-smudge inference failed: {error:#}");
        return;
    }
    for (rgba, result) in px
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(rgb.as_chunks::<3>().0)
    {
        // Transparent pixels and alpha are preserved, including hidden RGB.
        if rgba[3] > 0.0 {
            rgba[..3].copy_from_slice(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_alpha_hidden_rgb_and_zero_strength() {
        let mut spec = schist_neural::spec("anti-smudge").unwrap().clone();
        spec.input = schist_neural::Input::Tiles {
            size: 384,
            overlap: 96,
            scale: 1,
        };
        let model = schist_neural::Model::from_bytes(
            Box::leak(Box::new(spec)),
            include_bytes!("../../../crates/neural/tests/fixtures/anti-smudge-scale.onnx"),
        )
        .unwrap();
        let original = vec![0.8, 0.4, 0.2, 0.5, 0.7, 0.6, 0.3, 0.0];
        let mut pixels = original.clone();
        restore_rgba(&model, &mut pixels, 2, 1, 0.0);
        assert_eq!(pixels, original);
        restore_rgba(&model, &mut pixels, 2, 1, 1.0);
        assert_eq!(pixels, vec![0.4, 0.2, 0.1, 0.5, 0.7, 0.6, 0.3, 0.0]);
    }
}
