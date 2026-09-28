//! Semantic support for people and animals; BiRefNet still supplies fine edges.

use crate::Model;
use anyhow::{bail, Result};

const SIDE: usize = 520;
const PIXELS: usize = SIDE * SIDE;

pub fn guide_foreground(
    model: &Model,
    rgb: &[f32],
    coarse: &[f32],
    width: usize,
    height: usize,
) -> Result<Vec<f32>> {
    guide_foreground_with_reference(model, rgb, coarse, None, width, height)
}

pub fn guide_foreground_with_reference(
    model: &Model,
    rgb: &[f32],
    coarse: &[f32],
    reference: Option<&[f32]>,
    width: usize,
    height: usize,
) -> Result<Vec<f32>> {
    let Some(n) = width.checked_mul(height) else {
        bail!("invalid image dimensions")
    };
    if n == 0 || n.checked_mul(3).is_none_or(|count| rgb.len() < count) || coarse.len() != n {
        bail!("invalid foreground guide input");
    }
    let input = crate::resample::rgb_triangle(rgb, width, height, SIDE, SIDE);
    let output = model.run(&input)?;
    let array = output[0].to_plain_array_view::<f32>()?;
    let probability: Vec<f32> = array.iter().copied().collect();
    if let Some(reference) = reference {
        let coarse = constrain_detail(coarse, reference, &probability, width, height)?;
        guide(&coarse, &probability, width, height)
    } else {
        guide(coarse, &probability, width, height)
    }
}

fn constrain_detail(
    coarse: &[f32],
    reference: &[f32],
    probability: &[f32],
    width: usize,
    height: usize,
) -> Result<Vec<f32>> {
    if reference.len() != coarse.len()
        || reference.len() != width * height
        || probability.len() != PIXELS
        || reference
            .iter()
            .chain(coarse)
            .chain(probability)
            .any(|v| !v.is_finite())
    {
        bail!("invalid detector agreement input");
    }
    let seeds: Vec<f32> = probability
        .iter()
        .map(|&p| if p > 0.8 { 1.0 } else { 0.0 })
        .collect();
    if seeds.iter().filter(|&&s| s > 0.0).count() as f64 / PIXELS as f64 <= 0.003 {
        return Ok(reference.to_vec());
    }
    let general = resize(reference, width, height, SIDE, SIDE);
    let agreement = extremum(
        &general
            .iter()
            .map(|&a| if a > 0.1 { 1.0 } else { 0.0 })
            .collect::<Vec<_>>(),
        3,
        true,
    );
    let subject = extremum(&seeds, 9, true);
    let gate = soften(
        &agreement
            .iter()
            .zip(subject)
            .map(|(&a, s)| a.max(s))
            .collect::<Vec<_>>(),
    );
    let gate = resize(&gate, SIDE, SIDE, width, height);
    Ok(coarse.iter().zip(gate).map(|(&a, g)| a * g).collect())
}

fn components(mask: &[bool], seeds: &[bool], minimum: usize) -> Vec<f32> {
    let mut labels = vec![0usize; PIXELS];
    let mut accepted = vec![false];
    let mut pending = Vec::new();
    for start in 0..PIXELS {
        if !mask[start] || labels[start] != 0 {
            continue;
        }
        let id = accepted.len();
        labels[start] = id;
        pending.push(start);
        let mut hits = 0;
        while let Some(i) = pending.pop() {
            hits += usize::from(seeds[i]);
            let x = i % SIDE;
            let y = i / SIDE;
            for (valid, next) in [
                (x > 0, i.wrapping_sub(1)),
                (x + 1 < SIDE, i + 1),
                (y > 0, i.wrapping_sub(SIDE)),
                (y + 1 < SIDE, i + SIDE),
            ] {
                if valid && mask[next] && labels[next] == 0 {
                    labels[next] = id;
                    pending.push(next);
                }
            }
        }
        accepted.push(hits >= minimum);
    }
    labels
        .iter()
        .map(|&id| if accepted[id] { 1.0 } else { 0.0 })
        .collect()
}

fn extremum(input: &[f32], radius: usize, maximum: bool) -> Vec<f32> {
    let combine = |a: f32, b: f32| if maximum { a.max(b) } else { a.min(b) };
    let initial = if maximum { 0.0 } else { 1.0 };
    let mut horizontal = vec![initial; PIXELS];
    let mut output = vec![initial; PIXELS];
    for y in 0..SIDE {
        for x in 0..SIDE {
            for ix in x.saturating_sub(radius)..=(x + radius).min(SIDE - 1) {
                horizontal[y * SIDE + x] = combine(horizontal[y * SIDE + x], input[y * SIDE + ix]);
            }
        }
    }
    for y in 0..SIDE {
        for x in 0..SIDE {
            for iy in y.saturating_sub(radius)..=(y + radius).min(SIDE - 1) {
                output[y * SIDE + x] = combine(output[y * SIDE + x], horizontal[iy * SIDE + x]);
            }
        }
    }
    output
}

fn soften(input: &[f32]) -> Vec<f32> {
    let mut weights = [0.0f64; 7];
    for (i, value) in weights.iter_mut().enumerate() {
        *value = (-0.5 * (i as f64 - 3.0).powi(2)).exp();
    }
    let sum: f64 = weights.iter().sum();
    for value in &mut weights {
        *value /= sum;
    }
    let mut temporary = vec![0.0f32; PIXELS];
    let mut output = vec![0.0f32; PIXELS];
    // scipy.ndimage visits axis 0, then axis 1, storing float32 each time.
    for y in 0..SIDE {
        for x in 0..SIDE {
            temporary[y * SIDE + x] = weights
                .iter()
                .enumerate()
                .map(|(j, &w)| {
                    let iy = (y as isize + j as isize - 3).clamp(0, SIDE as isize - 1) as usize;
                    input[iy * SIDE + x] as f64 * w
                })
                .sum::<f64>() as f32;
        }
    }
    for y in 0..SIDE {
        for x in 0..SIDE {
            output[y * SIDE + x] = weights
                .iter()
                .enumerate()
                .map(|(j, &w)| {
                    let ix = (x as isize + j as isize - 3).clamp(0, SIDE as isize - 1) as usize;
                    temporary[y * SIDE + ix] as f64 * w
                })
                .sum::<f64>() as f32;
        }
    }
    output
}

fn resize(
    input: &[f32],
    width: usize,
    height: usize,
    out_width: usize,
    out_height: usize,
) -> Vec<f32> {
    let mut output = vec![0.0; out_width * out_height];
    for y in 0..out_height {
        let fy = ((y as f32 + 0.5) * (height as f32 / out_height as f32) - 0.5)
            .clamp(0.0, height as f32 - 1.0);
        let y0 = fy as usize;
        let y1 = (y0 + 1).min(height - 1);
        let ty = fy - y0 as f32;
        for x in 0..out_width {
            let fx = ((x as f32 + 0.5) * (width as f32 / out_width as f32) - 0.5)
                .clamp(0.0, width as f32 - 1.0);
            let x0 = fx as usize;
            let x1 = (x0 + 1).min(width - 1);
            let tx = fx - x0 as f32;
            let top = input[y0 * width + x0] * (1.0 - tx) + input[y0 * width + x1] * tx;
            let bottom = input[y1 * width + x0] * (1.0 - tx) + input[y1 * width + x1] * tx;
            output[y * out_width + x] = top * (1.0 - ty) + bottom * ty;
        }
    }
    output
}

fn guide(coarse: &[f32], probability: &[f32], width: usize, height: usize) -> Result<Vec<f32>> {
    if probability.len() != PIXELS || probability.iter().chain(coarse).any(|p| !p.is_finite()) {
        bail!("invalid semantic guide probabilities");
    }
    let seeds: Vec<bool> = probability.iter().map(|&p| p > 0.8).collect();
    if seeds.iter().filter(|&&s| s).count() as f64 / PIXELS as f64 <= 0.003 {
        return Ok(coarse.to_vec());
    }
    let keep = components(
        &probability.iter().map(|&p| p > 0.15).collect::<Vec<_>>(),
        &seeds,
        25,
    );
    if !keep.iter().any(|&v| v > 0.0) {
        return Ok(coarse.to_vec());
    }
    let support = extremum(&keep, 9, true);
    let small = resize(coarse, width, height, SIDE, SIDE);
    let valid = components(
        &small
            .iter()
            .zip(&support)
            .map(|(&a, &s)| a * s > 0.1)
            .collect::<Vec<_>>(),
        &seeds,
        25,
    );
    let mut gate = soften(&extremum(&valid, 3, true));
    for i in 0..PIXELS {
        gate[i] *= support[i];
    }
    // Protect cropped foreground extensions at the frame edge. The extension
    // must connect to a recognized subject in the detector's own mask.
    let full = components(
        &small.iter().map(|&a| a > 0.1).collect::<Vec<_>>(),
        &seeds
            .iter()
            .zip(&keep)
            .map(|(&s, &k)| s && k > 0.0)
            .collect::<Vec<_>>(),
        25,
    );
    let extension = components(
        &full
            .iter()
            .zip(&support)
            .map(|(&a, &s)| a > 0.0 && s == 0.0)
            .collect::<Vec<_>>(),
        &(0..PIXELS)
            .map(|i| !(SIDE..PIXELS - SIDE).contains(&i) || i % SIDE == 0 || i % SIDE == SIDE - 1)
            .collect::<Vec<_>>(),
        1,
    );
    let extension = soften(&extremum(&extension, 3, true));
    for (g, extension) in gate.iter_mut().zip(extension) {
        *g = g.max(extension);
    }
    let gate = resize(&gate, SIDE, SIDE, width, height);
    // Semantic segmentation cannot resolve hair gaps or holes between limbs.
    // Restrict its role to removing unsupported alpha; never make pixels opaque.
    Ok(coarse
        .iter()
        .zip(gate)
        .map(|(&a, g)| (a * g).clamp(0.0, 1.0))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detector_agreement_recovers_supported_clothing_and_rejects_new_background() {
        let mut reference = vec![0.0; PIXELS];
        let mut coarse = reference.clone();
        let mut probability = reference.clone();
        for y in 100..450 {
            for x in 180..280 {
                reference[y * SIDE + x] = if y < 300 { 1.0 } else { 0.0 };
                coarse[y * SIDE + x] = 1.0;
                probability[y * SIDE + x] = 0.99;
            }
        }
        for y in 100..300 {
            for x in 280..400 {
                coarse[y * SIDE + x] = 1.0;
                probability[y * SIDE + x] = 0.3;
            }
        }
        coarse[200 * SIDE + 235] = 0.0;
        let result = constrain_detail(&coarse, &reference, &probability, SIDE, SIDE).unwrap();
        assert_eq!(result[380 * SIDE + 230], 1.0);
        assert_eq!(result[200 * SIDE + 350], 0.0);
        assert_eq!(result[200 * SIDE + 235], 0.0);
        assert_eq!(
            constrain_detail(&coarse, &reference, &vec![0.0; PIXELS], SIDE, SIDE).unwrap(),
            reference
        );
    }
    #[test]
    fn cropped_subject_survives_without_restoring_detached_border_objects() {
        let mut alpha = vec![0.0; PIXELS];
        let mut probability = alpha.clone();
        for y in 0..260 {
            for x in 200..280 {
                alpha[y * SIDE + x] = 1.0;
                if y >= 100 {
                    probability[y * SIDE + x] = 0.99;
                }
            }
        }
        for y in 0..50 {
            for x in 400..440 {
                alpha[y * SIDE + x] = 1.0;
            }
        }
        for y in 150..180 {
            for x in 280..350 {
                alpha[y * SIDE + x] = 1.0;
            }
        }
        let result = guide(&alpha, &probability, SIDE, SIDE).unwrap();
        assert_eq!(result[20 * SIDE + 240], 1.0);
        assert_eq!(result[20 * SIDE + 420], 0.0);
        assert_eq!(result[165 * SIDE + 330], 0.0);
        assert!(result.iter().zip(&alpha).all(|(&a, &b)| a <= b));
    }

    #[test]
    fn confident_semantics_cannot_fill_hair_gaps_or_thicken_transparent_edges() {
        let mut alpha = vec![0.0; PIXELS];
        let mut probability = alpha.clone();
        for y in 100..400 {
            for x in 100..400 {
                probability[y * SIDE + x] = 0.999;
                alpha[y * SIDE + x] = if (240..260).contains(&x) { 0.0 } else { 0.6 };
            }
        }
        let result = guide(&alpha, &probability, SIDE, SIDE).unwrap();
        assert_eq!(result[250 * SIDE + 250], 0.0);
        assert_eq!(result[250 * SIDE + 200], 0.6);
        assert!(result.iter().zip(&alpha).all(|(&a, &b)| a <= b));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn shipped_subject_guide_executes() {
        let model = Model::from_bytes(
            crate::spec("subject-guide").unwrap(),
            crate::SUBJECT_GUIDE_ONNX_XZ,
        )
        .unwrap();
        let probabilities = model.run_scores(&vec![0.5; PIXELS * 3]).unwrap();
        assert_eq!(probabilities.len(), PIXELS);
        assert!(probabilities
            .iter()
            .all(|p| p.is_finite() && (0.0..=1.0).contains(p)));
    }

    #[test]
    fn multiple_subjects_survive_while_an_unsupported_object_is_removed() {
        let mut alpha = vec![0.0; PIXELS];
        let mut probability = alpha.clone();
        for (x, supported) in [(40, true), (230, true), (420, false)] {
            for y in 100..160 {
                for xx in x..x + 50 {
                    alpha[y * SIDE + xx] = 1.0;
                    if supported {
                        probability[y * SIDE + xx] = 0.99;
                    }
                }
            }
        }
        let result = guide(&alpha, &probability, SIDE, SIDE).unwrap();
        assert_eq!(result[130 * SIDE + 65], 1.0);
        assert_eq!(result[130 * SIDE + 255], 1.0);
        assert_eq!(result[130 * SIDE + 445], 0.0);
        assert_eq!(
            guide(&alpha, &vec![0.0; PIXELS], SIDE, SIDE).unwrap(),
            alpha
        );
        assert!(guide(&alpha, &[f32::NAN; 4], SIDE, SIDE).is_err());
    }
}
