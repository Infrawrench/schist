//! Resampling and affine transforms over tile maps.
//!
//! All interpolation happens in **premultiplied** alpha and is converted
//! back to straight alpha afterwards: interpolating straight-alpha colour
//! across a transparent edge would drag the (meaningless) colour of fully
//! transparent pixels into the result and fringe the edge.

use crate::geom::IntRect;
use crate::tile::{TileBuf, TileCoord, TileMap, TILE_SIZE};
use rayon::prelude::*;
use schist_color::{Depth, Rgba};

/// Reconstruction filter used when sampling between pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Filter {
    /// Nearest neighbour — preserves hard pixel edges (pixel art).
    Nearest,
    /// Bilinear — cheap and smooth; good for previews and downscales < 2x.
    Bilinear,
    /// Catmull-Rom bicubic — sharper than bilinear, mild overshoot.
    Bicubic,
}

impl Filter {
    pub fn display_name(self) -> &'static str {
        match self {
            Filter::Nearest => "Nearest Neighbor",
            Filter::Bilinear => "Bilinear",
            Filter::Bicubic => "Bicubic",
        }
    }
}

/// 2x3 affine matrix mapping source pixels to destination pixels:
/// `x' = a*x + c*y + tx`, `y' = b*x + d*y + ty`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for Affine {
    fn default() -> Self {
        Affine::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub fn translate(tx: f32, ty: f32) -> Affine {
        Affine {
            tx,
            ty,
            ..Affine::IDENTITY
        }
    }

    pub fn scale(sx: f32, sy: f32) -> Affine {
        Affine {
            a: sx,
            d: sy,
            ..Affine::IDENTITY
        }
    }

    pub fn rotate(radians: f32) -> Affine {
        let (s, c) = radians.sin_cos();
        Affine {
            a: c,
            b: s,
            c: -s,
            d: c,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Shear: `kx` slants x by y, `ky` slants y by x.
    pub fn skew(kx: f32, ky: f32) -> Affine {
        Affine {
            a: 1.0,
            b: ky,
            c: kx,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// `self` applied after `rhs` (i.e. `self ∘ rhs`).
    pub fn then(&self, rhs: &Affine) -> Affine {
        Affine {
            a: rhs.a * self.a + rhs.b * self.c,
            b: rhs.a * self.b + rhs.b * self.d,
            c: rhs.c * self.a + rhs.d * self.c,
            d: rhs.c * self.b + rhs.d * self.d,
            tx: rhs.tx * self.a + rhs.ty * self.c + self.tx,
            ty: rhs.tx * self.b + rhs.ty * self.d + self.ty,
        }
    }

    /// Rotate/scale/skew about a pivot instead of the origin.
    pub fn around(&self, px: f32, py: f32) -> Affine {
        Affine::translate(px, py).then(&self.then(&Affine::translate(-px, -py)))
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    pub fn determinant(&self) -> f32 {
        self.a * self.d - self.b * self.c
    }

    /// Inverse, or `None` when the matrix is degenerate (zero scale).
    pub fn invert(&self) -> Option<Affine> {
        let det = self.determinant();
        if det.abs() < 1e-9 {
            return None;
        }
        let inv = 1.0 / det;
        Some(Affine {
            a: self.d * inv,
            b: -self.b * inv,
            c: -self.c * inv,
            d: self.a * inv,
            tx: (self.c * self.ty - self.d * self.tx) * inv,
            ty: (self.b * self.tx - self.a * self.ty) * inv,
        })
    }

    /// Axis-aligned bounds of a transformed rect (corners transformed, then
    /// bounded — exact for affine maps).
    pub fn transform_bounds(&self, r: IntRect) -> IntRect {
        if r.is_empty() {
            return IntRect::EMPTY;
        }
        let corners = [
            self.apply(r.left as f32, r.top as f32),
            self.apply(r.right as f32, r.top as f32),
            self.apply(r.left as f32, r.bottom as f32),
            self.apply(r.right as f32, r.bottom as f32),
        ];
        let (mut x0, mut y0) = (f32::MAX, f32::MAX);
        let (mut x1, mut y1) = (f32::MIN, f32::MIN);
        for (x, y) in corners {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        IntRect::new(
            x0.floor() as i32,
            y0.floor() as i32,
            x1.ceil() as i32,
            y1.ceil() as i32,
        )
    }
}

#[inline]
fn premul(p: Rgba) -> [f32; 4] {
    [p.r * p.a, p.g * p.a, p.b * p.a, p.a]
}

#[inline]
fn unpremul(v: [f32; 4]) -> Rgba {
    if v[3] <= 1e-6 {
        Rgba::TRANSPARENT
    } else {
        Rgba::new(v[0] / v[3], v[1] / v[3], v[2] / v[3], v[3].clamp(0.0, 1.0))
    }
}

/// Catmull-Rom weights for a fractional offset in [0,1).
#[inline]
fn catmull_rom(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        0.5 * (-t3 + 2.0 * t2 - t),
        0.5 * (3.0 * t3 - 5.0 * t2 + 2.0),
        0.5 * (-3.0 * t3 + 4.0 * t2 + t),
        0.5 * (t3 - t2),
    ]
}

/// Sample a tile map at fractional pixel coordinates.
///
/// Coordinates address pixel *centres*: `(0.0, 0.0)` is the centre of the
/// pixel at (0, 0), which is what inverse-mapping a destination pixel
/// centre produces.
pub fn sample(tiles: &TileMap, x: f32, y: f32, filter: Filter) -> Rgba {
    sample_with(|x, y| tiles.pixel(x, y), x, y, filter)
}

/// Sampling with an arbitrary fetch function, so callers can clamp lookups
/// to the artwork's edge (see `transform_tiles`).
fn sample_with<F: Fn(i32, i32) -> Rgba>(fetch: F, x: f32, y: f32, filter: Filter) -> Rgba {
    match filter {
        Filter::Nearest => fetch(x.round() as i32, y.round() as i32),
        Filter::Bilinear => {
            let x0 = x.floor();
            let y0 = y.floor();
            let fx = x - x0;
            let fy = y - y0;
            let (x0, y0) = (x0 as i32, y0 as i32);
            let mut acc = [0.0f32; 4];
            for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
                if wy == 0.0 {
                    continue;
                }
                for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
                    let w = wx * wy;
                    if w == 0.0 {
                        continue;
                    }
                    let p = premul(fetch(x0 + dx, y0 + dy));
                    for i in 0..4 {
                        acc[i] += p[i] * w;
                    }
                }
            }
            unpremul(acc)
        }
        Filter::Bicubic => {
            let x0 = x.floor();
            let y0 = y.floor();
            let wx = catmull_rom(x - x0);
            let wy = catmull_rom(y - y0);
            let (x0, y0) = (x0 as i32 - 1, y0 as i32 - 1);
            let mut acc = [0.0f32; 4];
            for (j, wyj) in wy.iter().enumerate() {
                for (i, wxi) in wx.iter().enumerate() {
                    let w = wxi * wyj;
                    if w == 0.0 {
                        continue;
                    }
                    let p = premul(fetch(x0 + i as i32, y0 + j as i32));
                    for k in 0..4 {
                        acc[k] += p[k] * w;
                    }
                }
            }
            // Catmull-Rom overshoots; clamp back into range.
            let a = acc[3].clamp(0.0, 1.0);
            unpremul([
                acc[0].clamp(0.0, a),
                acc[1].clamp(0.0, a),
                acc[2].clamp(0.0, a),
                a,
            ])
        }
    }
}

/// Box-average `src` over a `scale`-sized footprint, for downscales where
/// point sampling would alias badly.
fn sample_box<F: Fn(i32, i32) -> Rgba>(fetch: F, x: f32, y: f32, sx: f32, sy: f32) -> Rgba {
    let hx = (sx * 0.5).max(0.5);
    let hy = (sy * 0.5).max(0.5);
    let x0 = (x - hx).round() as i32;
    let x1 = (x + hx).round() as i32;
    let y0 = (y - hy).round() as i32;
    let y1 = (y + hy).round() as i32;
    let mut acc = [0.0f32; 4];
    let mut n = 0.0f32;
    for yy in y0..y1.max(y0 + 1) {
        for xx in x0..x1.max(x0 + 1) {
            let p = premul(fetch(xx, yy));
            for i in 0..4 {
                acc[i] += p[i];
            }
            n += 1.0;
        }
    }
    if n == 0.0 {
        return Rgba::TRANSPARENT;
    }
    unpremul([acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n])
}

/// Transform a tile map by an affine matrix, resampling with `filter`.
///
/// `clip` bounds the destination (pass the canvas rect, inflated if you want
/// to keep off-canvas content). Downscales beyond 2x automatically switch to
/// box averaging so minification doesn't alias.
pub fn transform_tiles(
    src: &TileMap,
    m: &Affine,
    depth: Depth,
    filter: Filter,
    clip: IntRect,
) -> TileMap {
    let mut out = TileMap::new_in_mode(src.mode());
    let Some(inv) = m.invert() else { return out };
    // Clamp lookups to the artwork's pixel-tight edge: without it an
    // upscale samples "past" the last row/column, pulling in transparency
    // and fading the outer edge of every transformed layer.
    let src_bounds = src.content_bounds();
    if src_bounds.is_empty() {
        return out;
    }
    let fetch = |x: i32, y: i32| {
        src.pixel(
            x.clamp(src_bounds.left, src_bounds.right - 1),
            y.clamp(src_bounds.top, src_bounds.bottom - 1),
        )
    };
    // Clamping alone would give a transformed layer hard, aliased edges, so
    // the *colour* comes from clamped sampling while the *alpha* is scaled
    // by how much of the destination pixel actually falls inside the source
    // rectangle (4x4 supersampled). Upscales stay solid to the edge;
    // rotations and skews get antialiased boundaries.
    let coverage = |x: i32, y: i32| -> f32 {
        let mut hits = 0u32;
        for sy in 0..4 {
            for sx in 0..4 {
                let (u, v) = inv.apply(
                    x as f32 + (sx as f32 + 0.5) / 4.0,
                    y as f32 + (sy as f32 + 0.5) / 4.0,
                );
                if u >= src_bounds.left as f32
                    && u < src_bounds.right as f32
                    && v >= src_bounds.top as f32
                    && v < src_bounds.bottom as f32
                {
                    hits += 1;
                }
            }
        }
        hits as f32 / 16.0
    };
    let dst_bounds = m.transform_bounds(src_bounds).intersect(&clip);
    if dst_bounds.is_empty() {
        return out;
    }
    // Source footprint of one destination pixel; > 1 means minification.
    let fx = (inv.a * inv.a + inv.b * inv.b).sqrt();
    let fy = (inv.c * inv.c + inv.d * inv.d).sqrt();
    let boxed = fx > 2.0 || fy > 2.0;

    if let Some(result) = transform_gpu(src, &inv, depth, filter, src_bounds, dst_bounds) {
        return result;
    }

    let coords: Vec<TileCoord> = TileCoord::covering(&dst_bounds).collect();
    let tiles: Vec<(TileCoord, TileBuf)> = coords
        .into_par_iter()
        .filter_map(|coord| {
            let trect = coord.rect();
            let clip_rect = trect.intersect(&dst_bounds);
            if clip_rect.is_empty() {
                return None;
            }
            let mut buf = TileBuf::new_in_mode(depth, src.mode());
            let mut any = false;
            for y in clip_rect.top..clip_rect.bottom {
                for x in clip_rect.left..clip_rect.right {
                    let cov = coverage(x, y);
                    if cov <= 0.0 {
                        continue;
                    }
                    let (sx, sy) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
                    // Sample coordinates address pixel centres.
                    let (sx, sy) = (sx - 0.5, sy - 0.5);
                    if matches!(
                        src.mode(),
                        schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
                    ) {
                        let fetch_native = |x: i32, y: i32| {
                            src.native_pixel(
                                x.clamp(src_bounds.left, src_bounds.right - 1),
                                y.clamp(src_bounds.top, src_bounds.bottom - 1),
                            )
                        };
                        let mut p = schist_color::NativePixel::transparent(src.mode());
                        for start in [0, 3] {
                            if start == 3 && src.mode() != schist_color::ColorMode::Cmyk {
                                break;
                            }
                            let fetch = |x, y| {
                                let n = fetch_native(x, y);
                                if start == 0 {
                                    Rgba::new(n.color[0], n.color[1], n.color[2], n.alpha)
                                } else {
                                    Rgba::new(n.color[3], 0.0, 0.0, n.alpha)
                                }
                            };
                            let q = if boxed {
                                sample_box(fetch, sx, sy, fx, fy)
                            } else {
                                sample_with(fetch, sx, sy, filter)
                            };
                            if start == 0 {
                                p.color[..3].copy_from_slice(&[q.r, q.g, q.b]);
                            } else {
                                p.color[3] = q.r;
                            }
                            p.alpha = q.a * cov;
                        }
                        if p.alpha > 0.0 {
                            buf.set_native_pixel(
                                ((y - trect.top) * TILE_SIZE + (x - trect.left)) as usize,
                                p,
                            );
                            any = true;
                        }
                        continue;
                    }
                    let mut px = if boxed {
                        sample_box(fetch, sx, sy, fx, fy)
                    } else {
                        sample_with(fetch, sx, sy, filter)
                    };
                    px.a *= cov;
                    if px.a > 0.0 {
                        let ix = ((y - trect.top) * TILE_SIZE + (x - trect.left)) as usize;
                        buf.set(ix, px);
                        any = true;
                    }
                }
            }
            any.then_some((coord, buf))
        })
        .collect();
    for (coord, buf) in tiles {
        out.insert(coord, std::sync::Arc::new(buf));
    }
    out
}

pub(crate) static AFFINE_SHADER: schist_fx::ComputeShader =
    schist_fx::ComputeShader::new("affine-resample", include_str!("affine.wgsl"));

pub(crate) fn affine_work(inv: &Affine, dst: IntRect, channels: usize) -> usize {
    let fx = (inv.a * inv.a + inv.b * inv.b).sqrt();
    let fy = (inv.c * inv.c + inv.d * inv.d).sqrt();
    (dst.width() as usize)
        .saturating_mul(dst.height() as usize)
        .saturating_mul(32 + (fx * fy).min(1e6) as usize)
        .saturating_mul(channels)
}

pub(crate) fn affine_program(
    inv: &Affine,
    filter: Filter,
    src: IntRect,
    dst: IntRect,
    channels: usize,
) -> schist_fx::ComputeProgram {
    let fx = (inv.a * inv.a + inv.b * inv.b).sqrt();
    let fy = (inv.c * inv.c + inv.d * inv.d).sqrt();
    let n = dst.width() as usize * dst.height() as usize;
    let cost = affine_work(inv, dst, channels);
    let mut p = schist_fx::ComputeProgram::single(
        &AFFINE_SHADER,
        vec![
            inv.a,
            inv.b,
            inv.c,
            inv.d,
            inv.tx,
            inv.ty,
            src.left as f32,
            src.top as f32,
            src.width() as f32,
            src.height() as f32,
            dst.left as f32,
            dst.top as f32,
            match filter {
                Filter::Nearest => 0.0,
                Filter::Bilinear => 1.0,
                Filter::Bicubic => 2.0,
            },
            fx,
            fy,
        ],
        n * channels,
        [dst.width() as u32, dst.height() as u32, channels as u32],
        cost,
    );
    for (start, len, u, v) in [
        (dst.left, dst.width(), inv.a, inv.b),
        (dst.top, dst.height(), inv.c, inv.d),
    ] {
        for i in start..start + len {
            for offset in [0.5, 0.125, 0.375, 0.625, 0.875] {
                let x = i as f32 + offset;
                p.steps[0].params.extend_from_slice(&[u * x, v * x]);
            }
        }
    }
    p.steps[0].invocations = n;
    p
}

/// Keep the immutable source snapshot and its flattened plane across drag
/// previews. Holding the tile Arcs makes subsequent edits copy on write; exact
/// Arc identity comparisons invalidate the cache without a collision-prone hash.
struct AffineSource {
    tiles: TileMap,
    bounds: IntRect,
    pixels: std::sync::Arc<[f32]>,
}

fn affine_source(
    src: &TileMap,
    bounds: IntRect,
    channels: usize,
    native: bool,
) -> std::sync::Arc<[f32]> {
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<Option<AffineSource>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.bounds == bounds
                && cached.tiles.mode() == src.mode()
                && cached.tiles.len() == src.len()
                && src.iter().all(|(coord, tile)| {
                    cached
                        .tiles
                        .get(*coord)
                        .is_some_and(|other| Arc::ptr_eq(tile, other))
                })
            {
                return cached.pixels.clone();
            }
        }
    }
    let mut pixels =
        Vec::with_capacity(bounds.width() as usize * bounds.height() as usize * channels);
    for y in bounds.top..bounds.bottom {
        for x in bounds.left..bounds.right {
            if native {
                let p = src.native_pixel(x, y);
                pixels.extend_from_slice(&p.color[..channels - 1]);
                pixels.push(p.alpha);
            } else {
                let p = src.pixel(x, y);
                pixels.extend_from_slice(&[p.r, p.g, p.b, p.a]);
            }
        }
    }
    let pixels: Arc<[f32]> = pixels.into();
    // Large one-off images still work, without retaining an unbounded snapshot.
    if let Ok(mut guard) = cache.lock() {
        let retained = src
            .iter()
            .fold(std::mem::size_of_val(pixels.as_ref()), |n, (_, tile)| {
                n.saturating_add(tile.byte_len())
            });
        *guard = (retained <= 64 << 20).then(|| AffineSource {
            tiles: src.clone(),
            bounds,
            pixels: pixels.clone(),
        });
    }
    pixels
}

fn transform_gpu(
    src: &TileMap,
    inv: &Affine,
    depth: Depth,
    filter: Filter,
    bounds: IntRect,
    dst: IntRect,
) -> Option<TileMap> {
    let native = matches!(
        src.mode(),
        schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
    );
    let channels = if src.mode() == schist_color::ColorMode::Cmyk {
        5
    } else {
        4
    };
    if !schist_fx::backend().compute_available(affine_work(inv, dst, channels)) {
        return None;
    }
    let len = (bounds.width() as usize)
        .checked_mul(bounds.height() as usize)?
        .checked_mul(channels)?;
    let output_len = (dst.width() as usize)
        .checked_mul(dst.height() as usize)?
        .checked_mul(channels)?;
    if len > 32 << 20 || output_len > 32 << 20 {
        return None;
    }
    let program = affine_program(inv, filter, bounds, dst, channels);
    let pixels = affine_source(src, bounds, channels, native);
    let result = schist_fx::try_compute(&pixels, &program)?;
    Some(
        AffineOutput {
            rect: dst,
            depth,
            mode: src.mode(),
            channels,
        }
        .decode(&result),
    )
}

/// Output geometry retained independently of a resident affine program.
#[derive(Clone, Copy)]
pub struct AffineOutput {
    rect: IntRect,
    depth: Depth,
    mode: schist_color::ColorMode,
    channels: usize,
}

impl AffineOutput {
    /// Pack a CPU fallback into the same native channel layout as the GPU.
    pub fn encode(&self, tiles: &TileMap) -> Vec<f32> {
        affine_source(
            tiles,
            self.rect,
            self.channels,
            matches!(
                self.mode,
                schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
            ),
        )
        .to_vec()
    }

    pub fn decode(&self, result: &[f32]) -> TileMap {
        if result.len() != self.rect.width() as usize * self.rect.height() as usize * self.channels
        {
            return TileMap::new_in_mode(self.mode);
        }
        let mode = self.mode;
        let native = matches!(
            mode,
            schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
        );
        let (dst, depth, channels) = (self.rect, self.depth, self.channels);
        let mut out = TileMap::new_in_mode(mode);
        for coord in TileCoord::covering(&dst) {
            let rect = coord.rect();
            let clip = rect.intersect(&dst);
            let mut any = false;
            let mut tile = TileBuf::new_in_mode(depth, mode);
            for y in clip.top..clip.bottom {
                for x in clip.left..clip.right {
                    let i = ((y - dst.top) as usize * dst.width() as usize
                        + (x - dst.left) as usize)
                        * channels;
                    if result[i + channels - 1] <= 0.0 {
                        continue;
                    }
                    let j = ((y - rect.top) * TILE_SIZE + x - rect.left) as usize;
                    if native {
                        let mut p = schist_color::NativePixel::transparent(mode);
                        p.color[..channels - 1].copy_from_slice(&result[i..i + channels - 1]);
                        p.alpha = result[i + channels - 1];
                        tile.set_native_pixel(j, p);
                    } else {
                        tile.set(
                            j,
                            Rgba::new(result[i], result[i + 1], result[i + 2], result[i + 3]),
                        );
                    }
                    any = true;
                }
            }
            if any {
                out.insert(coord, std::sync::Arc::new(tile));
            }
        }
        out
    }
}

/// Capture a checked affine dispatch without invoking a synchronous backend.
pub fn affine_operation(
    src: &TileMap,
    m: &Affine,
    depth: Depth,
    filter: Filter,
    clip: IntRect,
) -> Option<(
    std::sync::Arc<[f32]>,
    schist_fx::ComputeProgram,
    AffineOutput,
)> {
    let inv = m.invert()?;
    let bounds = src.content_bounds();
    let dst = m.transform_bounds(bounds).intersect(&clip);
    if bounds.is_empty() || dst.is_empty() {
        return None;
    }
    let channels = if src.mode() == schist_color::ColorMode::Cmyk {
        5
    } else {
        4
    };
    let len = (bounds.width() as usize)
        .checked_mul(bounds.height() as usize)?
        .checked_mul(channels)?;
    let output_len = (dst.width() as usize)
        .checked_mul(dst.height() as usize)?
        .checked_mul(channels)?;
    if len > 32 << 20 || output_len > 32 << 20 {
        return None;
    }
    let native = matches!(
        src.mode(),
        schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
    );
    Some((
        affine_source(src, bounds, channels, native),
        affine_program(&inv, filter, bounds, dst, channels),
        AffineOutput {
            rect: dst,
            depth,
            channels,
            mode: src.mode(),
        },
    ))
}

/// Awaitable transform for browser hosts, preserving native CMYK/Lab channels.
pub async fn transform_tiles_async(
    src: &TileMap,
    m: &Affine,
    depth: Depth,
    filter: Filter,
    clip: IntRect,
    backend: &impl schist_fx::AsyncCompute,
) -> TileMap {
    if let Some((input, program, output)) = affine_operation(src, m, depth, filter, clip) {
        if let Some(result) = backend
            .compute_async(schist_fx::ComputeJob {
                input: &input,
                program: &program,
            })
            .await
        {
            return output.decode(&result);
        }
    }
    transform_tiles(src, m, depth, filter, clip)
}

/// Rescale a tile map from `from` to `to` (used by Image Size).
pub fn resize_tiles(
    src: &TileMap,
    from: (u32, u32),
    to: (u32, u32),
    depth: Depth,
    filter: Filter,
) -> TileMap {
    if from.0 == 0 || from.1 == 0 {
        return TileMap::new();
    }
    let m = Affine::scale(to.0 as f32 / from.0 as f32, to.1 as f32 / from.1 as f32);
    transform_tiles(src, &m, depth, filter, IntRect::from_size(to.0, to.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::blit_rgba8;

    #[test]
    fn affine_source_cache_reuses_clones_and_invalidates_edited_tiles() {
        let mut tiles = checker(7, 5);
        let bounds = IntRect::from_size(7, 5);
        let first = affine_source(&tiles, bounds, 4, false);
        let cloned = tiles.clone();
        let second = affine_source(&cloned, bounds, 4, false);
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        tiles
            .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::Eight)
            .set(0, Rgba::new(0.2, 0.3, 0.4, 1.0));
        let changed = affine_source(&tiles, bounds, 4, false);
        assert!(!std::sync::Arc::ptr_eq(&first, &changed));
        assert_ne!(&first[..4], &changed[..4]);
        assert_eq!(cloned.pixel(0, 0), Rgba::WHITE);
    }

    fn checker(w: u32, h: u32) -> TileMap {
        let mut tiles = TileMap::new();
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                buf[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        blit_rgba8(&mut tiles, Depth::Eight, IntRect::from_size(w, h), &buf);
        tiles
    }

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> TileMap {
        let mut tiles = TileMap::new();
        let buf: Vec<u8> = rgba
            .iter()
            .cycle()
            .take((w * h * 4) as usize)
            .copied()
            .collect();
        blit_rgba8(&mut tiles, Depth::Eight, IntRect::from_size(w, h), &buf);
        tiles
    }

    #[test]
    fn affine_inverse_round_trips_points() {
        let m = Affine::rotate(0.7)
            .then(&Affine::scale(2.0, 3.0))
            .then(&Affine::translate(5.0, -2.0));
        let inv = m.invert().unwrap();
        let (x, y) = m.apply(3.0, 4.0);
        let (bx, by) = inv.apply(x, y);
        assert!((bx - 3.0).abs() < 1e-3, "{bx}");
        assert!((by - 4.0).abs() < 1e-3, "{by}");
    }

    #[test]
    fn degenerate_matrix_has_no_inverse() {
        assert!(Affine::scale(0.0, 1.0).invert().is_none());
    }

    #[test]
    fn rotation_around_pivot_keeps_pivot_fixed() {
        let m = Affine::rotate(std::f32::consts::FRAC_PI_2).around(10.0, 10.0);
        let (x, y) = m.apply(10.0, 10.0);
        assert!(
            (x - 10.0).abs() < 1e-3 && (y - 10.0).abs() < 1e-3,
            "{x},{y}"
        );
    }

    #[test]
    fn identity_transform_preserves_pixels() {
        let src = checker(16, 16);
        let out = transform_tiles(
            &src,
            &Affine::IDENTITY,
            Depth::Eight,
            Filter::Bilinear,
            IntRect::from_size(16, 16),
        );
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(
                    out.pixel(x, y).to_u8(),
                    src.pixel(x, y).to_u8(),
                    "pixel {x},{y}"
                );
            }
        }
    }

    #[test]
    fn translation_moves_content_exactly() {
        let src = solid(8, 8, [255, 0, 0, 255]);
        let out = transform_tiles(
            &src,
            &Affine::translate(20.0, 10.0),
            Depth::Eight,
            Filter::Nearest,
            IntRect::from_size(64, 64),
        );
        assert_eq!(out.pixel(24, 14).to_u8(), [255, 0, 0, 255]);
        assert_eq!(out.pixel(4, 4).to_u8()[3], 0);
    }

    #[test]
    fn quarter_turn_maps_corners() {
        // A 16x16 square rotated 90° about its centre stays in place.
        let src = solid(16, 16, [0, 128, 255, 255]);
        let m = Affine::rotate(std::f32::consts::FRAC_PI_2).around(8.0, 8.0);
        let out = transform_tiles(
            &src,
            &m,
            Depth::Eight,
            Filter::Nearest,
            IntRect::from_size(32, 32),
        );
        assert_eq!(out.pixel(8, 8).to_u8(), [0, 128, 255, 255]);
        assert_eq!(out.pixel(1, 1).to_u8(), [0, 128, 255, 255]);
        assert_eq!(out.pixel(20, 20).to_u8()[3], 0);
    }

    #[test]
    fn upscale_doubles_extent() {
        let src = solid(8, 8, [10, 20, 30, 255]);
        let out = resize_tiles(&src, (8, 8), (16, 16), Depth::Eight, Filter::Bilinear);
        assert_eq!(out.pixel(15, 15).to_u8(), [10, 20, 30, 255]);
        assert_eq!(out.pixel(16, 16).to_u8()[3], 0);
    }

    #[test]
    fn downscale_of_checker_averages_to_mid_gray() {
        // Box filtering a 1px checkerboard by 8x must land near 50% gray;
        // point sampling would give pure black or white (aliasing).
        let src = checker(64, 64);
        let out = resize_tiles(&src, (64, 64), (8, 8), Depth::Eight, Filter::Bilinear);
        let v = out.pixel(4, 4).to_u8()[0];
        assert!((100..=155).contains(&v), "expected ~128, got {v}");
    }

    #[test]
    fn transparent_edges_do_not_fringe() {
        // Interpolating straight alpha would pull black from the empty
        // neighbours into the edge; premultiplied sampling must not.
        let src = solid(4, 4, [255, 255, 255, 255]);
        let out = transform_tiles(
            &src,
            &Affine::translate(0.5, 0.5),
            Depth::Eight,
            Filter::Bilinear,
            IntRect::from_size(16, 16),
        );
        let edge = out.pixel(0, 0).to_u8();
        assert!(edge[3] > 0 && edge[3] < 255, "edge is partial: {edge:?}");
        assert_eq!(
            [edge[0], edge[1], edge[2]],
            [255, 255, 255],
            "colour stays white, only alpha falls off"
        );
    }

    #[test]
    fn empty_source_yields_empty_output() {
        let out = transform_tiles(
            &TileMap::new(),
            &Affine::scale(2.0, 2.0),
            Depth::Eight,
            Filter::Bilinear,
            IntRect::from_size(16, 16),
        );
        assert!(out.is_empty());
    }
}
