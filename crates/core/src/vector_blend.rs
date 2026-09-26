//! Editable vector blends. The private layer block travels with native/PSD saves
//! and ordinary layer-extras history; the raster is only a portable preview.
use crate::curves::{add, length, lerp, mul, sub, ArcPath, Cubic};
use crate::{Anchor, Layer, RawBlock, SubPath, VectorPath, VectorShape};
use schist_color::Rgba;
use serde::{Deserialize, Serialize};

pub const BLOCK: [u8; 4] = *b"scBl";
pub const MAX_STEPS: usize = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorBlend {
    pub start: VectorShape,
    pub end: VectorShape,
    /// Number of intermediate objects; the two endpoints are additional.
    pub steps: usize,
    /// 0.5 is uniform; lower values concentrate objects near the start.
    pub bias: f32,
    pub easing: Easing,
    pub spine: Option<VectorPath>,
    /// A second rail controls the transverse width and orientation.
    pub rail: Option<VectorPath>,
    pub orient: bool,
    /// Per-subpath correspondence: start anchor i maps to end anchor mapping[i].
    /// Empty entries use automatic winding and cyclic-start alignment.
    pub mapping: Vec<Vec<usize>>,
}

impl VectorBlend {
    pub fn new(start: VectorShape, end: VectorShape) -> Self {
        Self {
            start,
            end,
            steps: 12,
            bias: 0.5,
            easing: Easing::Linear,
            spine: None,
            rail: None,
            orient: false,
            mapping: Vec::new(),
        }
    }
    pub fn from_layer(layer: &Layer) -> Option<Self> {
        Self::from_blocks(&layer.extras)
    }
    pub(crate) fn from_blocks(blocks: &[RawBlock]) -> Option<Self> {
        let b = blocks.iter().find(|b| b.key == BLOCK)?;
        let blend: Self = serde_json::from_slice(&b.data).ok()?;
        blend.valid().then_some(blend)
    }
    pub fn valid(&self) -> bool {
        self.steps <= MAX_STEPS
            && self.bias.is_finite()
            && (0.01..=0.99).contains(&self.bias)
            && [&self.start.path, &self.end.path]
                .into_iter()
                .chain(self.spine.iter())
                .chain(self.rail.iter())
                .all(|p| {
                    p.anchors().count() <= 4096
                        && p.anchors().all(|(_, _, a)| {
                            [a.point, a.handle_in, a.handle_out].iter().all(|p| {
                                p.0.is_finite()
                                    && p.1.is_finite()
                                    && p.0.abs() < 1e7
                                    && p.1.abs() < 1e7
                            })
                        })
                })
    }
    pub fn blocks(&self, layer: &Layer) -> Vec<RawBlock> {
        self.replace_blocks(&layer.extras)
    }
    pub(crate) fn replace_blocks(&self, blocks: &[RawBlock]) -> Vec<RawBlock> {
        let mut blocks = blocks.to_vec();
        blocks.retain(|b| b.key != BLOCK);
        blocks.push(RawBlock {
            key: BLOCK,
            data: serde_json::to_vec(self).expect("finite vector blend"),
        });
        blocks
    }
    pub fn position(&self, t: f32) -> f32 {
        let t = match self.easing {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t).powi(2),
            Easing::EaseInOut => t * t * (3.0 - 2.0 * t),
        };
        // A power bias leaves both endpoints exact and remains monotonic.
        t.powf(self.bias.clamp(0.01, 0.99).ln() / 0.5f32.ln())
    }
    pub fn shapes(&self) -> Vec<VectorShape> {
        if !self.valid() {
            return Vec::new();
        }
        let pairs = paired_paths(&self.start.path, &self.end.path, &self.mapping);
        let spine = self
            .spine
            .as_ref()
            .and_then(|p| p.subpaths.first())
            .map(ArcPath::new);
        let rail = self
            .rail
            .as_ref()
            .and_then(|p| p.subpaths.first())
            .map(ArcPath::new);
        let center = |p: &VectorPath| {
            let b = p.bounds();
            (
                (b.left + b.right) as f32 * 0.5,
                (b.top + b.bottom) as f32 * 0.5,
            )
        };
        let a = center(&self.start.path);
        let b = center(&self.end.path);
        let reference_width = self.start.path.bounds().height().max(1) as f32;
        (0..self.steps + 2)
            .map(|i| {
                let t = self.position(i as f32 / (self.steps + 1) as f32);
                let mut path = VectorPath::new(self.start.path.name.clone());
                for (sa, sb) in &pairs {
                    path.subpaths.push(SubPath {
                        closed: sa.closed,
                        anchors: sa
                            .anchors
                            .iter()
                            .zip(&sb.anchors)
                            .map(|(a, b)| Anchor {
                                point: lerp(a.point, b.point, t),
                                handle_in: lerp(a.handle_in, b.handle_in, t),
                                handle_out: lerp(a.handle_out, b.handle_out, t),
                            })
                            .collect(),
                    });
                }
                if let Some(spine) = &spine {
                    let (mut p, tangent) = spine.sample(t);
                    let mut xaxis = if self.orient { tangent } else { (1.0, 0.0) };
                    let mut yaxis = (-xaxis.1, xaxis.0);
                    if let Some(rail) = &rail {
                        let q = rail.sample(t).0;
                        yaxis = mul(sub(q, p), 1.0 / reference_width);
                        let span = length(yaxis);
                        if span > 1e-6 {
                            xaxis = (yaxis.1 / span, -yaxis.0 / span);
                        }
                        p = lerp(p, q, 0.5);
                    }
                    let c = lerp(a, b, t);
                    let transform = |v: (f32, f32)| add(mul(xaxis, v.0), mul(yaxis, v.1));
                    for contour in &mut path.subpaths {
                        for an in &mut contour.anchors {
                            an.point = add(p, transform(sub(an.point, c)));
                            an.handle_in = transform(an.handle_in);
                            an.handle_out = transform(an.handle_out);
                        }
                    }
                }
                let stroke = match (self.start.stroke, self.end.stroke) {
                    (None, None) => None,
                    (a, b) => {
                        let (ac, aw) = a.unwrap_or((Rgba::TRANSPARENT, 0.0));
                        let (bc, bw) = b.unwrap_or((Rgba::TRANSPARENT, 0.0));
                        Some((color(ac, bc, t), aw + (bw - aw) * t))
                    }
                };
                VectorShape {
                    path,
                    fill: color(self.start.fill, self.end.fill, t),
                    stroke,
                    even_odd: self.start.even_odd,
                }
            })
            .collect()
    }
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.start.path.translate(dx, dy);
        self.end.path.translate(dx, dy);
        if let Some(p) = &mut self.spine {
            p.translate(dx, dy);
        }
        if let Some(p) = &mut self.rail {
            p.translate(dx, dy);
        }
    }
}

fn color(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Insert anchors by exact de Casteljau subdivision, preserving the source
/// outline. This allows unequal node counts without flattening curved shapes.
fn subdivide(mut path: SubPath, n: usize) -> SubPath {
    while path.anchors.len() < n && path.anchors.len() > 1 {
        let segments = if path.closed {
            path.anchors.len()
        } else {
            path.anchors.len() - 1
        };
        let i = (0..segments)
            .max_by(|&i, &j| {
                let span = |k: usize| {
                    let c =
                        Cubic::between(path.anchors[k], path.anchors[(k + 1) % path.anchors.len()]);
                    c.0.windows(2).map(|w| length(sub(w[1], w[0]))).sum::<f32>()
                };
                span(i).total_cmp(&span(j))
            })
            .unwrap();
        let next = (i + 1) % path.anchors.len();
        let (a, b) = Cubic::between(path.anchors[i], path.anchors[next]).split(0.5);
        path.anchors[i].handle_out = sub(a.0[1], a.0[0]);
        path.anchors[next].handle_in = sub(b.0[2], b.0[3]);
        path.anchors.insert(
            i + 1,
            Anchor {
                point: a.0[3],
                handle_in: sub(a.0[2], a.0[3]),
                handle_out: sub(b.0[1], b.0[0]),
            },
        );
    }
    if path.anchors.len() == 1 {
        path.anchors.resize(n, path.anchors[0]);
    }
    path
}

pub fn paired_paths(
    a: &VectorPath,
    b: &VectorPath,
    mapping: &[Vec<usize>],
) -> Vec<(SubPath, SubPath)> {
    let mut pairs = Vec::new();
    for s in 0..a.subpaths.len().max(b.subpaths.len()) {
        let left = a.subpaths.get(s);
        let right = b.subpaths.get(s);
        let collapse = |p: &SubPath| {
            let n = p.anchors.len().max(1) as f32;
            let c = p
                .anchors
                .iter()
                .fold((0.0, 0.0), |c, a| add(c, mul(a.point, 1.0 / n)));
            SubPath {
                anchors: vec![Anchor::corner(c.0, c.1); p.anchors.len()],
                closed: p.closed,
            }
        };
        let (Some(mut left), Some(mut right)) = (
            left.cloned().or_else(|| right.map(collapse)),
            right.cloned().or_else(|| left.map(collapse)),
        ) else {
            continue;
        };
        if left.anchors.is_empty() || right.anchors.is_empty() {
            continue;
        }
        let n = left.anchors.len().max(right.anchors.len());
        left = subdivide(left, n);
        right = subdivide(right, n);
        if let Some(map) = mapping.get(s).filter(|m| {
            m.len() == n && {
                let mut m = m.to_vec();
                m.sort_unstable();
                m == (0..n).collect::<Vec<_>>()
            }
        }) {
            right.anchors = map.iter().map(|&i| right.anchors[i]).collect();
        } else if left.closed && right.closed {
            // Compare normalized positions, so location and scale do not bias
            // the automatic point correspondence. Test both windings.
            let normalize = |p: &SubPath| {
                let c = p
                    .anchors
                    .iter()
                    .fold((0.0, 0.0), |c, a| add(c, mul(a.point, 1.0 / n as f32)));
                let scale = p
                    .anchors
                    .iter()
                    .map(|a| length(sub(a.point, c)))
                    .fold(1e-6, f32::max);
                p.anchors
                    .iter()
                    .map(|a| mul(sub(a.point, c), 1.0 / scale))
                    .collect::<Vec<_>>()
            };
            let l = normalize(&left);
            let r = normalize(&right);
            let mut best = (f32::INFINITY, 0, false);
            for reverse in [false, true] {
                for offset in 0..n {
                    let score = (0..n)
                        .map(|i| {
                            let j = if reverse {
                                (offset + n - i) % n
                            } else {
                                (offset + i) % n
                            };
                            let d = sub(l[i], r[j]);
                            d.0 * d.0 + d.1 * d.1
                        })
                        .sum();
                    if score < best.0 {
                        best = (score, offset, reverse);
                    }
                }
            }
            right.anchors = (0..n)
                .map(|i| {
                    let j = if best.2 {
                        (best.1 + n - i) % n
                    } else {
                        (best.1 + i) % n
                    };
                    let mut a = right.anchors[j];
                    if best.2 {
                        std::mem::swap(&mut a.handle_in, &mut a.handle_out);
                    }
                    a
                })
                .collect();
        }
        pairs.push((left, right));
    }
    pairs
}
