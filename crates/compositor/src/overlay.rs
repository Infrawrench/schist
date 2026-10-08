//! Viewer overlays: focus peaking and highlight/shadow clipping warnings.
//!
//! Both are display aids, so they work on the *displayed* image — the
//! resampled, colour-managed BGRA frame the canvas or a gallery viewer
//! paints — never on the document. That keeps them cheap (a few
//! megapixels at most, whatever the document's size) and makes them
//! answer the question the user is asking: what is sharp, and what is
//! clipped, in what I am looking at.
//!
//! * **Clipping** is judged per channel. A pixel with every channel at
//!   255 is a blown highlight and is painted solid in [`HIGHLIGHT`]; one
//!   where only some channels hit 255 (a saturated sky, a red flower)
//!   gets a half-strength wash of the same colour. Crushed shadows are
//!   the same at 0, in [`SHADOW`].
//! * **Focus peaking** marks edge energy: the Sobel gradient of the
//!   frame's luma, normalised so a hard step of `threshold` grey levels
//!   just triggers it, is painted in the chosen colour. It is drawn over
//!   the clipping marks.

use rayon::prelude::*;

/// Lightroom's convention: red for clipped highlights…
pub const HIGHLIGHT: [u8; 3] = [255, 0, 0];
/// …and blue for crushed shadows.
pub const SHADOW: [u8; 3] = [0, 72, 255];

/// Focus peaking's colour and how strong an edge must be to light up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Peaking {
    /// RGB.
    pub color: [u8; 3],
    /// The luma step, in 8-bit grey levels, that counts as an edge.
    pub threshold: u8,
}

impl Peaking {
    /// Thresholds for the Low, Medium and High sensitivity settings.
    pub const SENSITIVITY: [u8; 3] = [56, 36, 22];
}

/// Which overlays are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Overlays {
    pub clipping: bool,
    pub peaking: Option<Peaking>,
}

impl Overlays {
    pub fn is_empty(&self) -> bool {
        !self.clipping && self.peaking.is_none()
    }

    /// A compact identity for cache keys: zero when nothing is on.
    pub fn key(&self) -> u64 {
        let peaking = self.peaking.map_or(0, |p| {
            1 | (p.threshold as u64) << 1
                | (p.color[0] as u64) << 9
                | (p.color[1] as u64) << 17
                | (p.color[2] as u64) << 25
        });
        self.clipping as u64 | peaking << 1
    }
}

/// What, if anything, one pixel is marked as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mark {
    None = 0,
    Peak,
    /// Every channel at 255.
    Highlight,
    /// Some channels at 255.
    HighlightPartial,
    /// Every channel at 0.
    Shadow,
    /// Some channels at 0.
    ShadowPartial,
}

/// Which pixels belong to the picture. The canvas passes the document's
/// footprint so the surround and the canvas edge are never marked; a
/// gallery photo is all picture.
pub type Inside<'a> = Option<&'a (dyn Fn(usize, usize) -> bool + Sync)>;

/// Mark every pixel of a `width * height` BGRA frame.
pub fn marks(
    bgra: &[u8],
    width: usize,
    height: usize,
    overlays: &Overlays,
    inside: Inside<'_>,
) -> Vec<Mark> {
    let mut out = vec![Mark::None; width * height];
    if overlays.is_empty() || width == 0 || height == 0 || bgra.len() < width * height * 4 {
        return out;
    }
    // Luma once, with -1 for anything outside the picture so the
    // gradient never straddles the canvas edge.
    let luma: Vec<i16> = match overlays.peaking {
        Some(_) => {
            let mut luma = vec![0i16; width * height];
            luma.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
                for (x, out) in row.iter_mut().enumerate() {
                    let at = (y * width + x) * 4;
                    let (b, g, r) = (bgra[at] as i32, bgra[at + 1] as i32, bgra[at + 2] as i32);
                    *out = if inside.is_none_or(|f| f(x, y)) {
                        // Rec. 709 weights in 8-bit fixed point.
                        ((54 * r + 183 * g + 19 * b + 128) >> 8) as i16
                    } else {
                        -1
                    };
                }
            });
            luma
        }
        None => Vec::new(),
    };
    out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        for (x, mark) in row.iter_mut().enumerate() {
            if let Some(peaking) = overlays.peaking {
                if peak(&luma, width, height, x, y, peaking.threshold) {
                    *mark = Mark::Peak;
                    continue;
                }
            }
            if !overlays.clipping || inside.is_some_and(|f| !f(x, y)) {
                continue;
            }
            let at = (y * width + x) * 4;
            let px = &bgra[at..at + 3];
            let high = px.iter().filter(|&&c| c == 255).count();
            let low = px.iter().filter(|&&c| c == 0).count();
            *mark = match (high, low) {
                (3, _) => Mark::Highlight,
                (_, 3) => Mark::Shadow,
                (h, _) if h > 0 => Mark::HighlightPartial,
                (_, l) if l > 0 => Mark::ShadowPartial,
                _ => Mark::None,
            };
        }
    });
    out
}

/// Sobel edge energy at one pixel, against the threshold. Border pixels
/// and pixels next to the picture's edge are never edges.
fn peak(luma: &[i16], width: usize, height: usize, x: usize, y: usize, threshold: u8) -> bool {
    if x == 0 || y == 0 || x + 1 >= width || y + 1 >= height {
        return false;
    }
    let at = |dx: usize, dy: usize| luma[(y + dy - 1) * width + x + dx - 1] as i32;
    let n = [
        at(0, 0),
        at(1, 0),
        at(2, 0),
        at(0, 1),
        at(2, 1),
        at(0, 2),
        at(1, 2),
        at(2, 2),
    ];
    if n.iter().any(|&v| v < 0) || at(1, 1) < 0 {
        return false;
    }
    let gx = (n[2] + 2 * n[4] + n[7]) - (n[0] + 2 * n[3] + n[5]);
    let gy = (n[5] + 2 * n[6] + n[7]) - (n[0] + 2 * n[1] + n[2]);
    // A step of s grey levels gives |g| = 4s; compare squared.
    let limit = 4 * threshold.max(1) as i32;
    gx * gx + gy * gy >= limit * limit
}

fn color_of(mark: Mark, overlays: &Overlays) -> Option<([u8; 3], bool)> {
    match mark {
        Mark::None => None,
        Mark::Peak => overlays.peaking.map(|p| (p.color, true)),
        Mark::Highlight => Some((HIGHLIGHT, true)),
        Mark::HighlightPartial => Some((HIGHLIGHT, false)),
        Mark::Shadow => Some((SHADOW, true)),
        Mark::ShadowPartial => Some((SHADOW, false)),
    }
}

/// Paint the overlays into an opaque BGRA frame in place.
pub fn apply_bgra(
    bgra: &mut [u8],
    width: usize,
    height: usize,
    overlays: &Overlays,
    inside: Inside<'_>,
) {
    if overlays.is_empty() {
        return;
    }
    let marks = marks(bgra, width, height, overlays, inside);
    bgra.par_chunks_mut(4)
        .zip(marks.par_iter())
        .for_each(|(px, &mark)| {
            if let Some(([r, g, b], solid)) = color_of(mark, overlays) {
                if solid {
                    px[..3].copy_from_slice(&[b, g, r]);
                } else {
                    px[0] = ((px[0] as u16 + b as u16) / 2) as u8;
                    px[1] = ((px[1] as u16 + g as u16) / 2) as u8;
                    px[2] = ((px[2] as u16 + r as u16) / 2) as u8;
                }
            }
        });
}

/// The overlays alone, as a straight-alpha BGRA layer to draw over the
/// picture: transparent where nothing is marked, half-transparent for
/// partial clipping.
pub fn layer_bgra(bgra: &[u8], width: usize, height: usize, overlays: &Overlays) -> Vec<u8> {
    let marks = marks(bgra, width, height, overlays, None);
    let mut out = vec![0u8; width * height * 4];
    out.par_chunks_mut(4)
        .zip(marks.par_iter())
        .for_each(|(px, &mark)| {
            if let Some(([r, g, b], solid)) = color_of(mark, overlays) {
                px.copy_from_slice(&[b, g, r, if solid { 255 } else { 128 }]);
            }
        });
    out
}

/// Box-filter a BGRA image down to fit `max_edge` on its longer side,
/// returning it with its new size. Images already that small are copied.
pub fn downsample_bgra(
    bgra: &[u8],
    width: usize,
    height: usize,
    max_edge: usize,
) -> (Vec<u8>, usize, usize) {
    let edge = width.max(height);
    if edge <= max_edge.max(1) {
        return (bgra.to_vec(), width, height);
    }
    let scale = max_edge.max(1) as f64 / edge as f64;
    let (ow, oh) = (
        ((width as f64 * scale).round() as usize).max(1),
        ((height as f64 * scale).round() as usize).max(1),
    );
    let mut out = vec![0u8; ow * oh * 4];
    out.par_chunks_mut(ow * 4)
        .enumerate()
        .for_each(|(oy, row)| {
            let y0 = oy * height / oh;
            let y1 = ((oy + 1) * height / oh).max(y0 + 1);
            for ox in 0..ow {
                let x0 = ox * width / ow;
                let x1 = ((ox + 1) * width / ow).max(x0 + 1);
                let mut acc = [0u32; 4];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let at = (y * width + x) * 4;
                        for (a, &v) in acc.iter_mut().zip(&bgra[at..at + 4]) {
                            *a += v as u32;
                        }
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as u32;
                for (o, a) in row[ox * 4..ox * 4 + 4].iter_mut().zip(acc) {
                    *o = ((a + n / 2) / n) as u8;
                }
            }
        });
    (out, ow, oh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A BGRA frame from RGB triples.
    fn frame(pixels: &[[u8; 3]]) -> Vec<u8> {
        pixels
            .iter()
            .flat_map(|&[r, g, b]| [b, g, r, 255])
            .collect()
    }

    const PEAK: Overlays = Overlays {
        clipping: false,
        peaking: Some(Peaking {
            color: [0, 255, 0],
            threshold: 36,
        }),
    };

    #[test]
    fn clipping_is_judged_per_channel() {
        let bgra = frame(&[
            [255, 255, 255],
            [255, 128, 40],
            [0, 0, 0],
            [10, 0, 90],
            [128, 128, 128],
        ]);
        let overlays = Overlays {
            clipping: true,
            peaking: None,
        };
        assert_eq!(
            marks(&bgra, 5, 1, &overlays, None),
            [
                Mark::Highlight,
                Mark::HighlightPartial,
                Mark::Shadow,
                Mark::ShadowPartial,
                Mark::None,
            ]
        );
        // Off means nothing is marked, even pure white.
        assert!(marks(&bgra, 5, 1, &Overlays::default(), None)
            .iter()
            .all(|&m| m == Mark::None));
    }

    #[test]
    fn clipping_paints_solid_and_partial_warnings() {
        let mut bgra = frame(&[[255, 255, 255], [255, 128, 40], [0, 0, 0], [100, 100, 100]]);
        let overlays = Overlays {
            clipping: true,
            peaking: None,
        };
        apply_bgra(&mut bgra, 4, 1, &overlays, None);
        assert_eq!(&bgra[0..4], &[0, 0, 255, 255], "blown white turns red");
        assert_eq!(&bgra[4..8], &[20, 64, 255, 255], "partial clip is a wash");
        assert_eq!(&bgra[8..12], &[255, 72, 0, 255], "crushed black turns blue");
        assert_eq!(&bgra[12..16], &[100, 100, 100, 255], "midtones untouched");
    }

    /// A 9x9 image, dark on the left and light from column `edge` on.
    fn step(dark: u8, light: u8, edge: usize) -> Vec<u8> {
        let mut px = Vec::new();
        for _ in 0..9 {
            for x in 0..9 {
                let v = if x < edge { dark } else { light };
                px.push([v, v, v]);
            }
        }
        frame(&px)
    }

    #[test]
    fn peaking_marks_strong_edges_only() {
        let sharp = step(60, 160, 4);
        let m = marks(&sharp, 9, 9, &PEAK, None);
        // Both columns either side of the step carry the gradient.
        for y in 1..8 {
            assert_eq!(m[y * 9 + 3], Mark::Peak);
            assert_eq!(m[y * 9 + 4], Mark::Peak);
            assert_eq!(m[y * 9 + 1], Mark::None, "flat areas stay clear");
        }
        // A step weaker than the threshold is not an edge…
        let faint = step(100, 130, 4);
        assert!(marks(&faint, 9, 9, &PEAK, None)
            .iter()
            .all(|&m| m == Mark::None));
        // …until sensitivity rises.
        let sensitive = Overlays {
            peaking: Some(Peaking {
                threshold: 22,
                ..PEAK.peaking.unwrap()
            }),
            ..PEAK
        };
        assert!(marks(&faint, 9, 9, &sensitive, None).contains(&Mark::Peak));
    }

    #[test]
    fn peaking_ignores_the_picture_edge() {
        // A bright photo on a dark surround: the boundary is a strong
        // step, but it is the canvas edge, not detail.
        let bgra = step(30, 220, 4);
        let inside = |x: usize, _y: usize| x >= 4;
        let m = marks(&bgra, 9, 9, &PEAK, Some(&inside));
        assert!(m.iter().all(|&m| m == Mark::None), "{m:?}");
    }

    #[test]
    fn peaking_draws_over_clipping() {
        let mut bgra = step(0, 255, 4);
        let both = Overlays {
            clipping: true,
            ..PEAK
        };
        apply_bgra(&mut bgra, 9, 9, &both, None);
        let at = |x: usize, y: usize| &bgra[(y * 9 + x) * 4..(y * 9 + x) * 4 + 4];
        assert_eq!(at(4, 4), &[0, 255, 0, 255], "edge in peaking green");
        assert_eq!(at(7, 4), &[0, 0, 255, 255], "white beyond it in red");
        assert_eq!(at(1, 4), &[255, 72, 0, 255], "black before it in blue");
    }

    #[test]
    fn layer_is_transparent_where_unmarked() {
        let bgra = frame(&[[255, 255, 255], [255, 0, 90], [128, 128, 128]]);
        let overlays = Overlays {
            clipping: true,
            peaking: None,
        };
        let layer = layer_bgra(&bgra, 3, 1, &overlays);
        assert_eq!(&layer[0..4], &[0, 0, 255, 255]);
        assert_eq!(layer[7], 128);
        assert_eq!(&layer[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn downsampling_averages_blocks() {
        let bgra = frame(&[[0, 0, 0], [200, 100, 50], [0, 0, 0], [200, 100, 50]]);
        let (out, w, h) = downsample_bgra(&bgra, 4, 1, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(&out[0..4], &[25, 50, 100, 255]);
        let (same, w, h) = downsample_bgra(&bgra, 4, 1, 8);
        assert_eq!((w, h, same), (4, 1, bgra));
    }

    #[test]
    fn keys_distinguish_settings() {
        let off = Overlays::default();
        let clip = Overlays {
            clipping: true,
            peaking: None,
        };
        assert_eq!(off.key(), 0);
        assert_ne!(clip.key(), PEAK.key());
        let red = Overlays {
            peaking: Some(Peaking {
                color: [255, 0, 0],
                threshold: 36,
            }),
            ..PEAK
        };
        assert_ne!(red.key(), PEAK.key());
    }
}
