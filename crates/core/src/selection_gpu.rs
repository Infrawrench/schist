use crate::{IntRect, TileMap};
use schist_fx::{ComputeEntry, ComputeProgram, ComputeShader, ComputeSource as Source};

#[derive(Clone, Copy)]
pub enum ColorMatch {
    FuzzyRgb { color: [f32; 3], tolerance: f32 },
    Rgb { color: [f32; 3], tolerance: f32 },
    Rgba8 { color: [u8; 4], tolerance: u8 },
}

static CLASSIFY: ComputeShader = ComputeShader::new(
    "selection-classify",
    include_str!("selection_classify.wgsl"),
);
static LABELS: ComputeShader =
    ComputeShader::new("selection-labels", include_str!("selection_labels.wgsl"));
static UNION: ComputeShader = ComputeShader {
    name: "selection-union",
    source: include_str!("selection_union.wgsl"),
    entry: ComputeEntry::Atomic,
};

/// An optional seed mask selects only connected components touching its nonzero
/// entries. Connectivity is four-neighbor, including disconnected seed islands.
/// Atomic union is followed by enough pointer jumps to flatten any forest.
pub fn program(
    w: usize,
    h: usize,
    rule: ColorMatch,
    seeds: Option<&[f32]>,
) -> Option<ComputeProgram> {
    let n = w.checked_mul(h)?;
    if n == 0 || n > 16_777_216 || seeds.is_some_and(|s| s.len() != n) {
        return None;
    }
    let mut params = match rule {
        ColorMatch::FuzzyRgb { color, tolerance } => {
            vec![0.0, color[0], color[1], color[2], 0.0, tolerance]
        }
        ColorMatch::Rgb { color, tolerance } => {
            vec![1.0, color[0], color[1], color[2], 0.0, tolerance]
        }
        ColorMatch::Rgba8 { color, tolerance } => vec![
            2.0,
            color[0] as f32,
            color[1] as f32,
            color[2] as f32,
            color[3] as f32,
            tolerance as f32,
        ],
    };
    params.push(u8::from(seeds.is_some()) as f32);
    let shape = [w as u32, h as u32, 1];
    let mut p = ComputeProgram::single(&CLASSIFY, params, n, shape, n.saturating_mul(256));
    if let Some(seeds) = seeds {
        p.buffers.push(seeds.to_vec());
        p.steps[0].auxiliary = Source::Input(1);
        let classified = Source::Step(0);
        let mut labels = p.push(&UNION, classified, classified, vec![0.0], n, shape);
        // Each jump at least halves a path's remaining length. The first pass
        // also turns the union kernel's implicit roots into explicit labels.
        for _ in 0..=n.ilog2() {
            labels = p.push(&LABELS, labels, classified, vec![0.0], n, shape);
        }
        let chosen = p.push(&UNION, labels, Source::Input(1), vec![1.0], n, shape);
        p.result = p.push(&LABELS, labels, chosen, vec![1.0], n, shape);
    }
    Some(p)
}

pub fn available(rect: IntRect, connected: bool) -> bool {
    let pixels = (rect.width().max(0) as usize).saturating_mul(rect.height().max(0) as usize);
    schist_fx::backend().compute_available(pixels.saturating_mul(if connected { 256 } else { 32 }))
}

/// Native tile callers avoid flattening below the backend's cost threshold.
/// The map remains available for an unchanged CPU fallback on any failure.
pub fn classify(
    tiles: &TileMap,
    rect: IntRect,
    rule: ColorMatch,
    seeds: Option<&[f32]>,
) -> Option<Vec<u8>> {
    let (w, h) = (
        usize::try_from(rect.width()).ok()?,
        usize::try_from(rect.height()).ok()?,
    );
    let work = w
        .saturating_mul(h)
        .saturating_mul(if seeds.is_some() { 256 } else { 32 });
    if !schist_fx::backend().compute_available(work) {
        return None;
    }
    let p = program(w, h, rule, seeds)?;
    let mut input = Vec::with_capacity(w * h * 4);
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            let c = tiles.pixel(x, y);
            input.extend([c.r, c.g, c.b, c.a]);
        }
    }
    schist_fx::try_compute(&input, &p)
        .map(|out| out.into_iter().map(|v| (v * 255.0).round() as u8).collect())
}

/// Reference classification for an owned browser request. Seeds are included
/// even when they do not match, as Grow starts from the existing selection.
pub fn classify_pixels(
    input: &[f32],
    width: usize,
    rule: ColorMatch,
    seeds: Option<&[f32]>,
) -> Vec<f32> {
    let mut mask = input
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let (delta, tolerance, fuzzy) = match rule {
                ColorMatch::FuzzyRgb { color, tolerance }
                | ColorMatch::Rgb { color, tolerance } => (
                    (p[0] - color[0])
                        .abs()
                        .max((p[1] - color[1]).abs())
                        .max((p[2] - color[2]).abs()),
                    tolerance,
                    matches!(rule, ColorMatch::FuzzyRgb { .. }),
                ),
                ColorMatch::Rgba8 { color, tolerance } => {
                    let pixel = schist_color::Rgba::new(p[0], p[1], p[2], p[3]).to_u8();
                    (
                        pixel
                            .iter()
                            .zip(color)
                            .map(|(&a, b)| (a as f32 - b as f32).abs())
                            .fold(0.0f32, f32::max),
                        tolerance as f32,
                        false,
                    )
                }
            };
            if fuzzy && tolerance > 0.0 {
                (1.0 - delta / tolerance).clamp(0.0, 1.0)
            } else {
                u8::from(delta <= tolerance) as f32
            }
        })
        .collect::<Vec<_>>();
    if let Some(seeds) = seeds {
        if width == 0 || seeds.len() != mask.len() {
            return vec![0.0; mask.len()];
        }
        let mut selected = vec![0.0; mask.len()];
        let mut stack = Vec::new();
        for (i, &seed) in seeds.iter().enumerate() {
            if seed > 0.0 {
                selected[i] = 1.0;
                stack.push(i);
            }
        }
        while let Some(i) = stack.pop() {
            let neighbors = [
                (i % width > 0).then(|| i - 1),
                (i % width + 1 < width).then_some(i + 1),
                (i >= width).then(|| i - width),
                (i + width < mask.len()).then_some(i + width),
            ];
            for j in neighbors.into_iter().flatten() {
                if selected[j] == 0.0 && mask[j] > 0.0 {
                    selected[j] = 1.0;
                    stack.push(j);
                }
            }
        }
        mask = selected;
    }
    mask
}
