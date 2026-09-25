//! Salient-object segmentation for Object Selection and background removal.
//! BiRefNet foreground removal decodes logits with sigmoid and uses the
//! original sRGB range. Object Selection keeps U2Net's probability decoder.
//!
//! U^2-Net answers one question: which pixels belong to the thing the
//! picture is *of*. That is not the same question Photoshop's Object
//! Selection asks -- it wants the object under the box you drew, and
//! there may be several in a picture -- but it is the same question once
//! the box is the picture, which is why the tool runs this on a crop
//! around the box rather than on the layer.
//!
//! The network is a stack of nested U-nets and emits seven maps, one per
//! depth, so it can be supervised at every scale. Only the first is the
//! answer; the rest are scaffolding from training, and they are ignored.
//!
//! One detail of its preprocessing matters and is easy to miss: the
//! picture is divided by its own maximum channel value before the
//! ImageNet normalisation, which is what it was trained with.
//!
//! What comes back is left as the probability the network emitted, and
//! deliberately *not* stretched over its own range the way the reference
//! implementation stretches it. Stretching is right when the map is going
//! to be an alpha channel and something is definitely there. It is wrong
//! here, because a selection tool has to be able to come back empty: hand
//! this a close-up of a brick wall and the honest answer is a map of
//! nothing, and a stretch turns that into a map of noise.

use anyhow::{bail, Context as _, Result};
use std::borrow::Cow;

use crate::{frame, Model};

/// How much of the object is in each pixel, 1.0 for certainly.
///
/// `rgb` is interleaved RGB in 0..=1; the map comes back at the image's
/// size.
pub fn segment(model: &Model, rgb: &[f32], width: usize, height: usize) -> Result<Vec<f32>> {
    probability_map(model, rgb, width, height, true, false)
}

/// BiRefNet foreground alpha: ImageNet channel normalization, no per-image
/// peak division, sigmoid before interpolation, and no min/max stretching.
pub fn foreground(model: &Model, rgb: &[f32], width: usize, height: usize) -> Result<Vec<f32>> {
    probability_map(model, rgb, width, height, false, true)
}

fn probability_map(
    model: &Model,
    rgb: &[f32],
    width: usize,
    height: usize,
    normalize_peak: bool,
    logits: bool,
) -> Result<Vec<f32>> {
    let count = width.checked_mul(height).and_then(|n| n.checked_mul(3));
    if width == 0 || height == 0 || count.is_none_or(|n| rgb.len() < n) {
        bail!("image is {width}x{height} but has {} floats", rgb.len());
    }
    // Divided by its own brightest channel, which is what the reference
    // preprocessing does before normalising. On an ordinary photograph
    // that is a no-op; on a dark one it is the difference between a
    // subject and a shrug.
    let source = &rgb[..width * height * 3];
    let peak = if normalize_peak {
        source.iter().copied().fold(0.0f32, f32::max)
    } else {
        1.0
    };
    let scaled: Cow<'_, [f32]> = match normalize_peak && peak > 1e-4 && (peak - 1.0).abs() > 1e-3 {
        true => Cow::Owned(source.iter().map(|v| v / peak).collect()),
        false => Cow::Borrowed(source),
    };

    let (input, framing) = if logits {
        let (fw, fh) = model.spec.input.dims();
        (
            crate::resample::rgb_triangle(&scaled, width, height, fw, fh),
            crate::Framing {
                scale: (fw as f32 / width as f32, fh as f32 / height as f32),
                offset: (0.0, 0.0),
            },
        )
    } else {
        frame(model.spec, &scaled, width, height)
    };
    let out = model.run(&input)?;
    let view = out[0].to_plain_array_view::<f32>()?;
    let shape: Vec<usize> = view.shape().iter().copied().filter(|d| *d != 1).collect();
    let [fh, fw] = shape[..] else {
        bail!("unexpected segmentation output shape {:?}", view.shape());
    };
    let flat = view.as_slice().context("non-contiguous output")?;
    let framing = framing.against(model.spec.input.dims(), (fw, fh));

    let probabilities = probabilities(flat, logits)?;

    let (sx, sy) = framing.scale;
    let (ox, oy) = framing.offset;
    let mut map = vec![0.0f32; width * height];
    for y in 0..height {
        let fy = ((y as f32 + 0.5) * sy + oy - 0.5).clamp(0.0, fh as f32 - 1.0);
        let (y0, ty) = (fy.floor() as usize, fy - fy.floor());
        let y1 = (y0 + 1).min(fh - 1);
        for x in 0..width {
            let fx = ((x as f32 + 0.5) * sx + ox - 0.5).clamp(0.0, fw as f32 - 1.0);
            let (x0, tx) = (fx.floor() as usize, fx - fx.floor());
            let x1 = (x0 + 1).min(fw - 1);
            let at = |x: usize, y: usize| probabilities[y * fw + x];
            let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
            let bot = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
            map[y * width + x] = top * (1.0 - ty) + bot * ty;
        }
    }
    Ok(map)
}

fn probabilities(raw: &[f32], logits: bool) -> Result<Vec<f32>> {
    if raw.iter().any(|v| !v.is_finite()) {
        bail!("non-finite segmentation output");
    }
    Ok(raw
        .iter()
        .map(|&v| {
            if logits {
                1.0 / (1.0 + (-v.clamp(-80.0, 80.0)).exp())
            } else {
                v.clamp(0.0, 1.0)
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logits_are_decoded_and_invalid_predictions_are_rejected() {
        let values = probabilities(&[-90.0, 0.0, 90.0], true).unwrap();
        assert!(values[0] < 1e-30);
        assert_eq!(values[1], 0.5);
        assert_eq!(values[2], 1.0);
        assert_eq!(probabilities(&[0.2, 0.7], false).unwrap(), [0.2, 0.7]);
        assert!(probabilities(&[0.0, f32::NAN], true).is_err());
    }
}
