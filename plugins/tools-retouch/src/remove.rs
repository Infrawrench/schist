//! The Remove tool: paint over something in one stroke, let go, and it
//! is gone.
//!
//! What happens on release, in order:
//!
//! 1. **The stroke becomes a mask** ([`removal_mask`]). Every pixel whose
//!    centre falls inside the stroke -- discs at each pointer sample,
//!    joined by tapered capsules, so pressure shows -- is painted. That
//!    is grown a little ([`grow_for`]), because nobody paints exactly to
//!    an object's edge and the soft fringe round it is part of what has
//!    to go, and then feathered over a few more pixels so the result
//!    fades into what was kept rather than ending at a hard line.
//! 2. **A context window is cut round it** ([`context_window`]): the
//!    hole plus half its own size again on every side, which is the
//!    proportion the inpainting network was trained on (holes up to half
//!    its frame) and the one Content-Aware Fill already uses. At an image
//!    edge the window slides inwards rather than shrinking, so a hole at
//!    the border still gets as much context as one in the middle.
//! 3. **The hole is filled.** Thin strokes -- a hair, a sensor spot, a
//!    wire against sky -- use the spot healing brush's ring interpolation
//!    ([`spot_fill`]), which is seamless and exact at that scale; the
//!    threshold is [`SPOT_HEAL_MAX_THICKNESS`] and its doc says how it was
//!    picked. Everything else goes through Content-Aware Fill's pipeline
//!    ([`crate::fill::inpaint_window`]): the network for layout, patch
//!    synthesis for texture, seam relaxation for tone. If the network is
//!    not installed the same pipeline runs without it, and the host is
//!    told so it can offer the download.
//! 4. **The fill is blended back** with the feathered weights, as one
//!    history entry, into the active layer -- or, with Sample All Layers
//!    on and no pixel layer active, into a new empty one.
//!
//! Steps 2-4 are a [`BackgroundEdit`]: the host runs the fill on a worker
//! with a progress bar and a cancel, and painting carries on meanwhile.

use std::sync::Arc;

use schist_color::Rgba;
use schist_core::{Document, IntRect, Layer, LayerId, LayerPath, TileCoord, TILE_SIZE};
use schist_i18n::t;
use schist_plugin_api::{
    BackgroundEdit, BackgroundRun, EditorState, JobControl, OptionValue, Overlay, PointerInput,
    ToolCtx, ToolOption, ToolPlugin,
};

use crate::fill::{inpaint_window, FillControl};

/// One pointer sample of a stroke: a disc in document space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dab {
    pub x: f32,
    pub y: f32,
    pub r: f32,
}

/// Holes no thicker than this many pixels -- the furthest any hole pixel
/// is from a pixel that is kept -- are filled by spot healing's ring
/// interpolation instead of the network and patch synthesis.
///
/// Measured rather than guessed: `examples/remove_eval.rs` punches
/// wandering brush-stroke holes of increasing width into eight Kodak
/// photographs and scores both methods against what was really there,
/// on error and on texture (the fill's mean gradient over the
/// original's; 1.0 keeps the detail, near 0 is a smear). Over 192 holes:
///
/// | thickness | spot err | fill err | spot tex | fill tex |
/// |----------:|---------:|---------:|---------:|---------:|
/// | 3         | 0.057    | 0.064    | 0.28     | 1.69     |
/// | 4         | 0.056    | 0.062    | 0.31     | 1.18     |
/// | 5         | 0.068    | 0.076    | 0.24     | 1.23     |
/// | 6         | 0.065    | 0.068    | 0.26     | 1.14     |
/// | 8         | 0.068    | 0.073    | 0.25     | 0.98     |
/// | 11        | 0.078    | 0.088    | 0.23     | 0.88     |
///
/// Interpolation always has the lower error -- a blur is the
/// least-squares answer -- and always smears. Below a thickness of about
/// 5.5 the fill is worse than a smear: 7x7 patches copied into a hole a
/// few pixels across leave more edges than the photograph had (texture
/// 1.2-1.7) and about 10% more error, while at that width the smear is
/// hard to see. From 6 up the fill's texture settles towards 1 and its
/// error is within 5-10% of the interpolation's, so that is where it
/// takes over. With the growth and feather, 5.5 is a brush up to about
/// 7 px across: a hair, a wire, a sensor spot at 100%. Over strong
/// texture (rock, foliage) a smear that thin can still show; paint those
/// with a wider brush to get the fill.
pub const SPOT_HEAL_MAX_THICKNESS: f32 = 5.5;

/// Pixels the painted stroke is grown by before anything is filled.
///
/// Proportional to the brush, because a wider brush is a less careful
/// one, within limits: one pixel takes a small brush's anti-aliased edge, and past
/// twelve the growth is eating background the user chose not to paint.
pub fn grow_for(radius: f32) -> f32 {
    (radius * 0.1).clamp(1.0, 12.0)
}

/// Pixels over which the fill fades out into the kept image, outside the
/// grown stroke.
pub fn feather_for(radius: f32) -> f32 {
    (radius * 0.08).clamp(1.0, 6.0)
}

/// Context margin bounds for [`context_window`]. The lower one is what a
/// tiny hole needs to give the network and the patch search something
/// to see; the upper one keeps a stroke across a whole photograph from
/// asking for a window the size of the photograph.
const MIN_MARGIN: i32 = 24;
const MAX_MARGIN: i32 = 768;

/// The area a set of strokes removes, with how strongly to replace each
/// pixel of it.
#[derive(Debug, Clone)]
pub struct RemovalMask {
    /// Bounds of `hole`, clipped to the canvas.
    pub rect: IntRect,
    /// True where the fill replaces the picture: the grown stroke plus
    /// its feather.
    pub hole: Vec<bool>,
    /// How much of the fill shows, 0..=1, over `rect`: one over the
    /// grown stroke, falling to nothing across the feather.
    pub weight: Vec<f32>,
    /// The furthest any hole pixel is from a kept one. What decides
    /// between spot healing and the full fill.
    pub thickness: f32,
}

/// Rasterise strokes into the area to remove. `None` if they touch no
/// canvas pixel.
pub fn removal_mask(strokes: &[Vec<Dab>], canvas: IntRect) -> Option<RemovalMask> {
    let dabs = || strokes.iter().flatten();
    let max_r = dabs().map(|d| d.r).fold(0.0f32, f32::max);
    if max_r <= 0.0 || canvas.is_empty() {
        return None;
    }
    let (grow, feather) = (grow_for(max_r), feather_for(max_r));
    let reach = (grow + feather).ceil() as i32 + 1;
    let mut bounds = IntRect::EMPTY;
    for d in dabs() {
        let b = IntRect::new(
            (d.x - d.r).floor() as i32 - reach,
            (d.y - d.r).floor() as i32 - reach,
            (d.x + d.r).ceil() as i32 + reach + 1,
            (d.y + d.r).ceil() as i32 + reach + 1,
        );
        bounds = if bounds.is_empty() {
            b
        } else {
            bounds.union(&b)
        };
    }
    let area = bounds.intersect(&canvas);
    if area.is_empty() {
        return None;
    }
    let (w, h) = (area.width() as usize, area.height() as usize);
    let painted = paint(strokes, area);
    if !painted.iter().any(|&p| p) {
        return None;
    }
    let to_paint = distance_to(&painted, w, h);
    let mut hole = vec![false; w * h];
    let mut weight = vec![0.0f32; w * h];
    for i in 0..w * h {
        let d = to_paint[i];
        if d <= grow + feather {
            hole[i] = true;
            weight[i] = match d <= grow {
                true => 1.0,
                false => smoothstep(1.0 - (d - grow) / feather),
            };
        }
    }
    let kept: Vec<bool> = hole.iter().map(|&g| !g).collect();
    let to_kept = distance_to(&kept, w, h);
    let thickness = hole
        .iter()
        .zip(&to_kept)
        .filter(|(&g, _)| g)
        .map(|(_, &d)| d)
        .fold(0.0f32, f32::max);
    // Trim to the hole itself; the rasterising margin is not part of it.
    let mut rect = IntRect::EMPTY;
    for y in 0..h {
        for x in 0..w {
            if hole[y * w + x] {
                let p = IntRect::new(
                    area.left + x as i32,
                    area.top + y as i32,
                    area.left + x as i32 + 1,
                    area.top + y as i32 + 1,
                );
                rect = if rect.is_empty() { p } else { rect.union(&p) };
            }
        }
    }
    let (rw, rh) = (rect.width() as usize, rect.height() as usize);
    let (ox, oy) = (
        (rect.left - area.left) as usize,
        (rect.top - area.top) as usize,
    );
    fn crop<T: Copy>(v: &[T], w: usize, (ox, oy, rw, rh): (usize, usize, usize, usize)) -> Vec<T> {
        (0..rh)
            .flat_map(|y| v[(oy + y) * w + ox..(oy + y) * w + ox + rw].iter().copied())
            .collect()
    }
    let span = (ox, oy, rw, rh);
    Some(RemovalMask {
        rect,
        hole: crop(&hole, w, span),
        weight: crop(&weight, w, span),
        thickness,
    })
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Which pixels of `area` have their centre inside a stroke. Each pair
/// of consecutive samples is a capsule whose radius tapers from one to
/// the other, so pressure changes along a stroke show in its width.
fn paint(strokes: &[Vec<Dab>], area: IntRect) -> Vec<bool> {
    let (w, h) = (area.width() as usize, area.height() as usize);
    let mut out = vec![false; w * h];
    for stroke in strokes {
        let pairs = stroke
            .windows(2)
            .map(|p| (p[0], p[1]))
            .chain(stroke.first().map(|&d| (d, d)));
        for (a, b) in pairs {
            let r = a.r.max(b.r);
            let span = IntRect::new(
                (a.x.min(b.x) - r).floor() as i32,
                (a.y.min(b.y) - r).floor() as i32,
                (a.x.max(b.x) + r).ceil() as i32 + 1,
                (a.y.max(b.y) + r).ceil() as i32 + 1,
            )
            .intersect(&area);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            for py in span.top..span.bottom {
                for px in span.left..span.right {
                    let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                    let t = match len2 > 1e-9 {
                        true => (((cx - a.x) * dx + (cy - a.y) * dy) / len2).clamp(0.0, 1.0),
                        false => 0.0,
                    };
                    let (qx, qy) = (a.x + dx * t, a.y + dy * t);
                    let radius = a.r + (b.r - a.r) * t;
                    if (cx - qx).hypot(cy - qy) <= radius {
                        out[(py - area.top) as usize * w + (px - area.left) as usize] = true;
                    }
                }
            }
        }
        // A brush smaller than a pixel still removes the pixel it is on.
        for d in stroke {
            let (px, py) = (d.x.floor() as i32, d.y.floor() as i32);
            if area.contains(px, py) {
                out[(py - area.top) as usize * w + (px - area.left) as usize] = true;
            }
        }
    }
    out
}

/// Euclidean distance from every pixel to the nearest `seed`, or
/// infinity if there is none (Felzenszwalb and Huttenlocher's separable
/// transform: exact, and linear in the pixel count).
fn distance_to(seed: &[bool], w: usize, h: usize) -> Vec<f32> {
    // f64 so that "no seed yet" can be a large finite number without
    // swallowing the squared offsets added to it.
    const FAR: f64 = 1e12;
    let mut grid: Vec<f64> = seed.iter().map(|&s| if s { 0.0 } else { FAR }).collect();
    let n = w.max(h);
    let (mut f, mut d, mut v, mut z) = (
        vec![0.0f64; n],
        vec![0.0f64; n],
        vec![0usize; n],
        vec![0.0f64; n + 1],
    );
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        edt_1d(&f[..h], &mut d[..h], &mut v, &mut z);
        for y in 0..h {
            grid[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..y * w + w]);
        edt_1d(&f[..w], &mut d[..w], &mut v, &mut z);
        grid[y * w..y * w + w].copy_from_slice(&d[..w]);
    }
    grid.into_iter()
        .map(|sq| match sq >= FAR * 0.5 {
            true => f32::INFINITY,
            false => sq.sqrt() as f32,
        })
        .collect()
}

/// Squared distance transform of a sampled function, in one dimension:
/// the lower envelope of the parabolas rooted at each sample.
fn edt_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let meet = |q: usize, p: usize| {
        ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64))
    };
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let mut s = meet(q, v[k]);
        while s <= z[k] {
            k -= 1;
            s = meet(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    let mut k = 0usize;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let dq = q as f64 - v[k] as f64;
        *out = dq * dq + f[v[k]];
    }
}

/// The region the fill gets to look at: `hole` plus half its larger side
/// again on every side (within [`MIN_MARGIN`]..=[`MAX_MARGIN`]), moved
/// back inside the canvas where it overhangs, and only cut down where
/// the canvas itself is smaller than the window.
pub fn context_window(hole: IntRect, canvas: IntRect) -> IntRect {
    let extent = hole.width().max(hole.height());
    let margin = (extent / 2).clamp(MIN_MARGIN, MAX_MARGIN);
    let place = |start: i32, len: i32, lo: i32, hi: i32| -> (i32, i32) {
        if len >= hi - lo {
            (lo, hi)
        } else {
            let s = start.clamp(lo, hi - len);
            (s, s + len)
        }
    };
    let (left, right) = place(
        hole.left - margin,
        hole.width() + 2 * margin,
        canvas.left,
        canvas.right,
    );
    let (top, bottom) = place(
        hole.top - margin,
        hole.height() + 2 * margin,
        canvas.top,
        canvas.bottom,
    );
    IntRect::new(left, top, right, bottom)
}

/// Spot healing's fill, for a hole of any shape: every hole pixel is the
/// inverse-square-distance average of the ring of kept pixels just
/// outside the hole (three pixels deep, as the brush uses), looking only
/// as far as the hole is thick so a long scratch fills from its own
/// sides and not from its far end.
pub fn spot_fill(buf: &[Rgba], w: usize, h: usize, hole: &[bool]) -> Vec<Rgba> {
    let mut out = buf.to_vec();
    let to_hole = distance_to(hole, w, h);
    let kept: Vec<bool> = hole.iter().map(|&g| !g).collect();
    let to_kept = distance_to(&kept, w, h);
    let ring: Vec<bool> = (0..w * h)
        .map(|i| !hole[i] && to_hole[i] <= 3.0 && buf[i].a > 0.0)
        .collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !hole[i] {
                continue;
            }
            let reach = if to_kept[i].is_finite() {
                (to_kept[i] * 2.0 + 4.0).ceil() as i32
            } else {
                0
            };
            let (mut acc, mut sum) = ([0f32; 4], 0f32);
            for sy in (y as i32 - reach).max(0)..=(y as i32 + reach).min(h as i32 - 1) {
                for sx in (x as i32 - reach).max(0)..=(x as i32 + reach).min(w as i32 - 1) {
                    let j = sy as usize * w + sx as usize;
                    if !ring[j] {
                        continue;
                    }
                    let d2 = ((sx - x as i32).pow(2) + (sy - y as i32).pow(2)) as f32;
                    let wgt = 1.0 / (d2 + 1.0);
                    let c = buf[j];
                    acc[0] += c.r * wgt;
                    acc[1] += c.g * wgt;
                    acc[2] += c.b * wgt;
                    acc[3] += c.a * wgt;
                    sum += wgt;
                }
            }
            if sum > 0.0 {
                out[i] = Rgba::new(acc[0] / sum, acc[1] / sum, acc[2] / sum, acc[3] / sum);
            }
        }
    }
    out
}

/// Straight-alpha mix of `over` into `under` by `t`, done on
/// premultiplied colour so a fill landing on transparent pixels does not
/// drag black in with it.
fn mix(under: Rgba, over: Rgba, t: f32) -> Rgba {
    let a = under.a + (over.a - under.a) * t;
    if a <= 1e-6 {
        return Rgba::new(over.r, over.g, over.b, 0.0);
    }
    let (ua, oa) = (under.a * (1.0 - t), over.a * t);
    Rgba::new(
        (under.r * ua + over.r * oa) / a,
        (under.g * ua + over.g * oa) / a,
        (under.b * ua + over.b * oa) / a,
        a,
    )
}

/// How a removal fills its hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Ring interpolation, for holes up to [`SPOT_HEAL_MAX_THICKNESS`].
    SpotHeal,
    /// The network (when installed), patch synthesis and seam relaxation.
    Fill,
}

/// Where a removal writes.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    Layer(LayerId),
    /// A new empty layer inserted at this path.
    NewLayer(LayerPath),
}

/// Options a removal is made with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoveOptions {
    /// Read the composite of every visible layer rather than the target
    /// layer alone.
    pub sample_all_layers: bool,
}

/// Everything a removal needs, read off the document on the UI thread.
pub struct Prepared {
    window: IntRect,
    pixels: Vec<Rgba>,
    hole: Vec<bool>,
    weight: Vec<f32>,
    method: Method,
    target: Target,
}

impl Prepared {
    /// The method chosen for this hole.
    pub fn method(&self) -> Method {
        self.method
    }

    /// The window the fill sees.
    pub fn window(&self) -> IntRect {
        self.window
    }
}

/// The layer a removal writes into: the active one if it takes pixels,
/// otherwise (sampling all layers) a new one above it.
fn target(doc: &Document, sample_all: bool) -> Option<Target> {
    let active = doc.active_layer.and_then(|id| doc.tree.find(id));
    if let Some(layer) = active.filter(|l| l.as_raster().is_some() && !l.locked) {
        return Some(Target::Layer(layer.id));
    }
    if !sample_all {
        return None;
    }
    let path = match doc.active_layer.and_then(|a| doc.tree.path_of(a)) {
        Some(mut p) => {
            *p.0.last_mut()? += 1;
            p
        }
        None => LayerPath(vec![doc.tree.layers.len()]),
    };
    Some(Target::NewLayer(path))
}

/// Read what removing `strokes` needs from `doc`. `None` if there is
/// nothing to remove or nowhere to put the result.
pub fn prepare(doc: &Document, strokes: &[Vec<Dab>], options: RemoveOptions) -> Option<Prepared> {
    let canvas = doc.canvas_rect();
    let mask = removal_mask(strokes, canvas)?;
    let target = target(doc, options.sample_all_layers)?;
    let window = context_window(mask.rect, canvas);
    let (w, h) = (window.width() as usize, window.height() as usize);
    let pixels: Vec<Rgba> = match (options.sample_all_layers, &target) {
        (true, _) => schist_compositor::composite_region_f32(doc, window)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| Rgba::new(c[0], c[1], c[2], c[3]))
            .collect(),
        (false, Target::Layer(id)) => {
            let tiles = &doc.tree.find(*id)?.as_raster()?.tiles;
            (window.top..window.bottom)
                .flat_map(|y| (window.left..window.right).map(move |x| tiles.pixel(x, y)))
                .collect()
        }
        (false, Target::NewLayer(_)) => return None,
    };
    let selection = (!doc.selection.is_empty()).then_some(&doc.selection);
    let mut hole = vec![false; w * h];
    let mut weight = vec![0.0f32; w * h];
    let (mw, ox, oy) = (
        mask.rect.width() as usize,
        (mask.rect.left - window.left) as usize,
        (mask.rect.top - window.top) as usize,
    );
    for my in 0..mask.rect.height() as usize {
        for mx in 0..mw {
            let (from, to) = (my * mw + mx, (oy + my) * w + ox + mx);
            hole[to] = mask.hole[from];
            let inside = selection.map_or(1.0, |s| {
                s.coverage(mask.rect.left + mx as i32, mask.rect.top + my as i32) as f32 / 255.0
            });
            weight[to] = mask.weight[from] * inside;
        }
    }
    if !weight.iter().any(|&v| v > 0.0) {
        return None;
    }
    let method = match mask.thickness <= SPOT_HEAL_MAX_THICKNESS {
        true => Method::SpotHeal,
        false => Method::Fill,
    };
    Some(Prepared {
        window,
        pixels,
        hole,
        weight,
        method,
        target,
    })
}

/// The filled window, ready to write back.
pub struct Finished {
    window: IntRect,
    fill: Vec<Rgba>,
    weight: Vec<f32>,
    target: Target,
}

impl Finished {
    /// The fill over the whole window, before blending.
    pub fn fill(&self) -> &[Rgba] {
        &self.fill
    }
}

/// Where a fill gets its network from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InpaintModel {
    /// The catalogue's `inpaint` model, if it is installed.
    Catalog,
    /// Behave as though it were not installed: what a browser that has
    /// not fetched it sees, and how tests reach that path natively,
    /// where the model is built in.
    Missing,
}

/// Fill the hole: the slow part, safe to run on any thread. `None` if
/// cancelled.
pub fn run(prepared: Prepared, control: &JobControl) -> Option<Finished> {
    run_with(prepared, control, InpaintModel::Catalog)
}

/// [`run`], choosing where the network comes from.
pub fn run_with(
    prepared: Prepared,
    control: &JobControl,
    source: InpaintModel,
) -> Option<Finished> {
    let Prepared {
        window,
        pixels,
        hole,
        weight,
        method,
        target,
    } = prepared;
    let (w, h) = (window.width() as usize, window.height() as usize);
    let model = match method {
        Method::SpotHeal => None,
        Method::Fill => match source {
            InpaintModel::Catalog if schist_neural::installed("inpaint") => {
                schist_neural::get("inpaint")
            }
            _ => {
                control.note_missing_model("inpaint");
                None
            }
        },
    };
    if control.is_cancelled() {
        return None;
    }
    let fill = fill_hole(method, pixels, w, h, &hole, model.as_deref(), control)?;
    control.set_progress(1.0);
    (!control.is_cancelled()).then_some(Finished {
        window,
        fill,
        weight,
        target,
    })
}

/// Fill `hole` in a `w`x`h` window with `method`. With [`Method::Fill`]
/// and no `model`, Content-Aware Fill's classical path (diffusion, then
/// patch synthesis) does the whole job. `None` if cancelled.
pub fn fill_hole(
    method: Method,
    pixels: Vec<Rgba>,
    w: usize,
    h: usize,
    hole: &[bool],
    model: Option<&schist_neural::Model>,
    control: &JobControl,
) -> Option<Vec<Rgba>> {
    match method {
        Method::SpotHeal => Some(spot_fill(&pixels, w, h, hole)),
        Method::Fill => {
            let cancelled = || control.is_cancelled();
            let progress = |f: f32| control.set_progress(f);
            inpaint_window(
                pixels,
                w,
                h,
                hole,
                model,
                &FillControl {
                    cancelled: &cancelled,
                    progress: &progress,
                },
            )
        }
    }
}

/// Blend a finished fill into the document as one history entry.
pub fn apply(doc: &mut Document, finished: Finished, name: &str) -> bool {
    let Finished {
        window,
        fill,
        weight,
        target,
    } = finished;
    let w = window.width() as usize;
    if let Target::Layer(id) = &target {
        if !doc
            .tree
            .find(*id)
            .is_some_and(|l| l.as_raster().is_some() && !l.locked)
        {
            return false;
        }
    }
    let mut edit = doc.begin_edit(name.to_string());
    let layer = match target {
        Target::Layer(id) => id,
        Target::NewLayer(path) => {
            edit.insert_layer(path, Layer::new_raster(t("tool.remove.layer_name")))
        }
    };
    for coord in TileCoord::covering(&window) {
        let trect = coord.rect();
        let clip = trect.intersect(&window);
        let touches = (clip.top..clip.bottom).any(|y| {
            (clip.left..clip.right)
                .any(|x| weight[(y - window.top) as usize * w + (x - window.left) as usize] > 0.0)
        });
        if !touches {
            continue;
        }
        let Some(tile) = edit.writable_tile(layer, coord) else {
            break;
        };
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                let i = (y - window.top) as usize * w + (x - window.left) as usize;
                if weight[i] <= 0.0 {
                    continue;
                }
                let ix = ((y - trect.top) * TILE_SIZE + (x - trect.left)) as usize;
                tile.set(ix, mix(tile.get(ix), fill[i], weight[i]));
            }
        }
    }
    let committed = edit.commit();
    if committed {
        doc.active_layer = Some(layer);
    }
    committed
}

/// The smallest a pen's pressure can shrink the brush to, as a fraction
/// of its size; a feather-light touch still removes something.
const MIN_PRESSURE_SCALE: f32 = 0.1;

/// Paint over an unwanted object in one stroke and it is filled from its
/// surroundings when the pointer lifts.
pub struct RemoveTool {
    /// The stroke under way.
    stroke: Vec<Dab>,
    /// Strokes waiting for Enter when not removing after each stroke.
    pending: Vec<Vec<Dab>>,
    cursor: Option<(f32, f32)>,
    sample_all_layers: bool,
    after_each_stroke: bool,
    background: bool,
    queued: Vec<BackgroundEdit>,
}

impl RemoveTool {
    /// A tool with its default settings.
    pub fn default_tool() -> Self {
        Self::new()
    }

    pub(crate) fn new() -> Self {
        RemoveTool {
            stroke: Vec::new(),
            pending: Vec::new(),
            cursor: None,
            sample_all_layers: false,
            after_each_stroke: true,
            background: false,
            queued: Vec::new(),
        }
    }

    /// Strokes painted but not yet removed.
    pub fn pending_strokes(&self) -> usize {
        self.pending.len()
    }

    fn dab(state: &EditorState, input: &PointerInput) -> Dab {
        let scale = state
            .brush_dynamics
            .pressure_size(input.pressure)
            .max(MIN_PRESSURE_SCALE);
        Dab {
            x: input.x,
            y: input.y,
            r: (state.brush_size * 0.5 * scale).max(0.5),
        }
    }

    /// Hand strokes over for removal: to the host's worker if it has one,
    /// otherwise right now.
    fn submit(&mut self, ctx: &mut ToolCtx, strokes: Vec<Vec<Dab>>) {
        if strokes.iter().all(|s| s.is_empty()) {
            return;
        }
        let overlay = strokes
            .iter()
            .map(|s| Overlay::Stroke {
                dabs: s.iter().map(|d| (d.x, d.y, d.r)).collect(),
            })
            .collect();
        let edit = removal_edit(
            Arc::new(strokes),
            RemoveOptions {
                sample_all_layers: self.sample_all_layers,
            },
            overlay,
        );
        if self.background {
            self.queued.push(edit);
        } else {
            edit.run_now(ctx.doc, &JobControl::new());
        }
    }
}

/// A removal as a [`BackgroundEdit`].
pub fn removal_edit(
    strokes: Arc<Vec<Vec<Dab>>>,
    options: RemoveOptions,
    overlay: Vec<Overlay>,
) -> BackgroundEdit {
    let name = t("tool.remove.history.remove");
    BackgroundEdit {
        name,
        overlay,
        prepare: Box::new(move |doc: &Document| -> Option<BackgroundRun> {
            let prepared = prepare(doc, &strokes, options)?;
            Some(Box::new(move |control: &JobControl| {
                let finished = run(prepared, control)?;
                Some(
                    Box::new(move |doc: &mut Document| apply(doc, finished, name))
                        as schist_plugin_api::BackgroundApply,
                )
            }))
        }),
    }
}

impl ToolPlugin for RemoveTool {
    fn set_background_edits(&mut self, enabled: bool) {
        self.background = enabled;
    }

    fn take_background_edit(&mut self) -> Option<BackgroundEdit> {
        (!self.queued.is_empty()).then(|| self.queued.remove(0))
    }

    fn id(&self) -> &'static str {
        "remove"
    }
    fn name(&self) -> &'static str {
        t("tool.remove.name")
    }
    fn description(&self) -> &'static str {
        t("tool.remove.description")
    }
    fn icon(&self) -> &'static str {
        "remove"
    }
    fn group(&self) -> &'static str {
        "heal"
    }

    fn options(&self) -> Vec<ToolOption> {
        vec![
            ToolOption::toggle(
                "remove-sample-all",
                t("tool.remove.option.sample_all_layers"),
                self.sample_all_layers,
            ),
            ToolOption::toggle(
                "remove-each-stroke",
                t("tool.remove.option.remove_after_each_stroke"),
                self.after_each_stroke,
            ),
        ]
    }

    fn set_option(&mut self, key: &str, value: OptionValue) {
        match key {
            "remove-sample-all" => self.sample_all_layers = value.bool(),
            "remove-each-stroke" => self.after_each_stroke = value.bool(),
            _ => {}
        }
    }

    fn on_option_changed(&mut self, ctx: &mut ToolCtx, key: &str) {
        // Turning per-stroke removal back on should not strand strokes
        // that were waiting for Enter.
        if key == "remove-each-stroke" && self.after_each_stroke && !self.pending.is_empty() {
            let strokes = std::mem::take(&mut self.pending);
            self.submit(ctx, strokes);
        }
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.cursor = Some((input.x, input.y));
        self.stroke = vec![Self::dab(ctx.state, &input)];
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.cursor = Some((input.x, input.y));
        if self.stroke.is_empty() {
            return;
        }
        let dab = Self::dab(ctx.state, &input);
        let last = self.stroke[self.stroke.len() - 1];
        // Capsules join the samples, so spacing only has to be fine
        // enough to follow a curve; half a pixel keeps a long stroke's
        // list short without losing one.
        if (dab.x - last.x).hypot(dab.y - last.y) >= 0.5 || (dab.r - last.r).abs() >= 0.5 {
            self.stroke.push(dab);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.stroke.is_empty() {
            return;
        }
        self.on_pointer_move(ctx, input);
        let stroke = std::mem::take(&mut self.stroke);
        if self.after_each_stroke {
            self.submit(ctx, vec![stroke]);
        } else {
            self.pending.push(stroke);
        }
    }

    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        let strokes = std::mem::take(&mut self.pending);
        self.submit(ctx, strokes);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.stroke.clear();
        self.pending.clear();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        // Like a pending transform, strokes waiting for Enter are applied
        // rather than lost when the tool changes.
        self.stroke.clear();
        self.on_commit(ctx);
        self.cursor = None;
    }

    fn overlays(&self, _doc: &Document, state: &EditorState) -> Vec<Overlay> {
        let mut out: Vec<Overlay> = self
            .pending
            .iter()
            .chain(std::iter::once(&self.stroke))
            .filter(|s| !s.is_empty())
            .map(|s| Overlay::Stroke {
                dabs: s.iter().map(|d| (d.x, d.y, d.r)).collect(),
            })
            .collect();
        if let Some((cx, cy)) = self.cursor {
            out.push(Overlay::Circle {
                cx,
                cy,
                r: state.brush_size * 0.5,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Vec<Dab> {
        let n = ((x1 - x0).hypot(y1 - y0) / 2.0).ceil().max(1.0) as usize;
        (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                Dab {
                    x: x0 + (x1 - x0) * t,
                    y: y0 + (y1 - y0) * t,
                    r,
                }
            })
            .collect()
    }

    #[test]
    fn distance_transform_matches_brute_force() {
        let (w, h) = (23, 17);
        let seeds: Vec<bool> = (0..w * h).map(|i| (i * 7919) % 31 == 0).collect();
        let fast = distance_to(&seeds, w, h);
        for y in 0..h {
            for x in 0..w {
                let mut best = f32::INFINITY;
                for sy in 0..h {
                    for sx in 0..w {
                        if seeds[sy * w + sx] {
                            let d = ((sx as f32 - x as f32).powi(2)
                                + (sy as f32 - y as f32).powi(2))
                            .sqrt();
                            best = best.min(d);
                        }
                    }
                }
                assert!((fast[y * w + x] - best).abs() < 1e-3, "({x},{y})");
            }
        }
        assert!(distance_to(&[false; 4], 2, 2)
            .iter()
            .all(|d| d.is_infinite()));
    }

    #[test]
    fn a_stroke_becomes_a_grown_feathered_mask() {
        let canvas = IntRect::new(0, 0, 200, 200);
        let r = 40.0;
        let mask = removal_mask(&[line(50.0, 100.0, 150.0, 100.0, r)], canvas).unwrap();
        let (grow, feather) = (grow_for(r), feather_for(r));
        let w = mask.rect.width() as usize;
        let at = |x: i32, y: i32| {
            let i = (y - mask.rect.top) as usize * w + (x - mask.rect.left) as usize;
            (mask.hole[i], mask.weight[i])
        };
        // The painted line and its growth are fully replaced.
        assert_eq!(at(100, 100), (true, 1.0));
        assert_eq!(at(100, 100 + r as i32 + grow as i32 - 1), (true, 1.0));
        // The feather ramps down outside the growth.
        let (gone, w_mid) = at(100, 100 + (r + grow + feather * 0.5) as i32);
        assert!(gone && w_mid > 0.0 && w_mid < 1.0, "{w_mid}");
        // The rect is the hole and nothing more.
        let reach = r + grow + feather;
        assert!(
            (mask.rect.top - (100.0 - reach) as i32).abs() <= 1,
            "{:?}",
            mask.rect
        );
        assert!(
            (mask.rect.left - (50.0 - reach) as i32).abs() <= 1,
            "{:?}",
            mask.rect
        );
        // Thickness is the half-width of what is filled.
        assert!((mask.thickness - reach).abs() <= 1.5, "{}", mask.thickness);
    }

    #[test]
    fn pressure_tapers_the_stroke() {
        let canvas = IntRect::new(0, 0, 200, 100);
        let stroke = vec![
            Dab {
                x: 20.0,
                y: 50.0,
                r: 2.0,
            },
            Dab {
                x: 180.0,
                y: 50.0,
                r: 20.0,
            },
        ];
        let painted = paint(&[stroke], canvas);
        let width_at = |x: usize| (0..100).filter(|&y| painted[y * 200 + x]).count();
        assert!(
            width_at(30) < width_at(170),
            "{} {}",
            width_at(30),
            width_at(170)
        );
        assert!(width_at(170) >= 36);
    }

    #[test]
    fn strokes_off_the_canvas_remove_nothing() {
        let canvas = IntRect::new(0, 0, 100, 100);
        assert!(removal_mask(&[line(-80.0, -80.0, -40.0, -40.0, 5.0)], canvas).is_none());
        assert!(removal_mask(&[], canvas).is_none());
        // Half on: clipped to the canvas.
        let mask = removal_mask(&[line(-20.0, 50.0, 20.0, 50.0, 5.0)], canvas).unwrap();
        assert_eq!(mask.rect.left, 0);
    }

    #[test]
    fn the_context_window_keeps_its_size_at_image_edges() {
        let canvas = IntRect::new(0, 0, 1000, 800);
        // In the middle: half the hole again on each side.
        let mid = context_window(IntRect::new(400, 300, 500, 360), canvas);
        assert_eq!(mid, IntRect::new(350, 250, 550, 410));
        // Against the top-left corner: slid inwards, same size.
        let corner = context_window(IntRect::new(0, 2, 100, 62), canvas);
        assert_eq!(corner, IntRect::new(0, 0, 200, 160));
        // A small hole against the bottom-right: the minimum margin,
        // slid inwards.
        let far = context_window(IntRect::new(980, 790, 1000, 800), canvas);
        assert_eq!(far.width(), 20 + 2 * MIN_MARGIN);
        assert_eq!(far.right, 1000);
        assert_eq!(far.bottom, 800);
        assert_eq!(far.height(), 10 + 2 * MIN_MARGIN);
        // Bigger than the canvas: the canvas.
        let tiny = IntRect::new(0, 0, 40, 30);
        assert_eq!(context_window(IntRect::new(10, 10, 30, 20), tiny), tiny);
        // A huge hole's margin is capped.
        let big = context_window(
            IntRect::new(2000, 2000, 6000, 6000),
            IntRect::new(0, 0, 10_000, 10_000),
        );
        assert_eq!(big.left, 2000 - MAX_MARGIN);
        for window in [mid, corner, far] {
            assert_eq!(window.intersect(&canvas), window);
        }
    }

    #[test]
    fn spot_fill_interpolates_from_the_ring() {
        // Left half dark, right half light; a thin vertical hole on the
        // boundary fills with a ramp between them, not a flat colour.
        let (w, h) = (40, 20);
        let buf: Vec<Rgba> = (0..w * h)
            .map(|i| match i % w < 20 {
                true => Rgba::new(0.2, 0.2, 0.2, 1.0),
                false => Rgba::new(0.8, 0.8, 0.8, 1.0),
            })
            .collect();
        let hole: Vec<bool> = (0..w * h).map(|i| (17..23).contains(&(i % w))).collect();
        let out = spot_fill(&buf, w, h, &hole);
        let row = 10 * w;
        assert!(out[row + 17].r < out[row + 22].r);
        assert!(out[row + 17].r > 0.2 && out[row + 22].r < 0.8);
        // Kept pixels are untouched.
        assert_eq!(out[row + 5], buf[row + 5]);
    }

    #[test]
    fn mixing_into_transparency_keeps_the_fill_colour() {
        let fill = Rgba::new(0.9, 0.5, 0.1, 1.0);
        let half = mix(Rgba::TRANSPARENT, fill, 0.5);
        assert!((half.a - 0.5).abs() < 1e-6);
        assert!((half.r - 0.9).abs() < 1e-6 && (half.b - 0.1).abs() < 1e-6);
        assert_eq!(mix(Rgba::new(0.1, 0.1, 0.1, 1.0), fill, 1.0), fill);
    }
}
