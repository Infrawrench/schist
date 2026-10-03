//! Rendering frames: compositing the layer stack as each frame shows it.

use schist_core::animation::{self, FrameResult, Refusal, Timeline};
use schist_core::{blit_rgba8, Document, Layer};

/// One rendered frame: straight-alpha RGBA8 at the document's size.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedFrame {
    pub rgba: Vec<u8>,
    pub delay_ms: u32,
}

/// Onion skin tints: earlier frames reddish, later ones bluish, the
/// convention Aseprite and most animation tools share.
pub const ONION_BEFORE: [u8; 3] = [230, 60, 60];
pub const ONION_AFTER: [u8; 3] = [60, 110, 240];

/// A document that composites as frame `index` of `timeline` (which must
/// be synced with `doc`'s layers). Tiles are shared, not copied.
fn frame_document(doc: &Document, timeline: &Timeline, index: usize) -> Document {
    let mut out = Document::new(doc.title.clone(), doc.width, doc.height, doc.depth);
    out.tree = timeline.frame_tree(&doc.tree, index);
    out.mode = doc.mode;
    out.icc_profile = doc.icc_profile.clone();
    out.resolution_dpi = doc.resolution_dpi;
    out
}

/// Frame `index` of `doc`'s animation as RGBA8, the size of the document.
/// `None` when there is no animation or no such frame.
pub fn render_frame(doc: &Document, index: usize) -> Option<Vec<u8>> {
    let timeline = doc.timeline.as_ref()?.synced(&doc.tree);
    (index < timeline.frames.len()).then(|| {
        schist_compositor::composite_region_rgba8(
            &frame_document(doc, &timeline, index),
            doc.canvas_rect(),
        )
    })
}

/// Every frame of `doc`'s animation, in order, with its delay.
pub fn render_frames(doc: &Document) -> Option<Vec<RenderedFrame>> {
    let timeline = doc.timeline.as_ref()?.synced(&doc.tree);
    Some(
        (0..timeline.frames.len())
            .map(|index| RenderedFrame {
                rgba: schist_compositor::composite_region_rgba8(
                    &frame_document(doc, &timeline, index),
                    doc.canvas_rect(),
                ),
                delay_ms: timeline.frames[index].delay_ms,
            })
            .collect(),
    )
}

/// Photoshop's Flatten Frames Into Layers: render every frame into a new
/// layer on top of the stack, each visible only in its own frame. One
/// undoable edit. `layer_name` names the layer for a zero-based frame.
pub fn flatten_frames(
    doc: &mut Document,
    history_name: &str,
    layer_name: impl Fn(usize) -> String,
) -> FrameResult {
    let frames = render_frames(doc).ok_or(Refusal::NoAnimation)?;
    let rect = doc.canvas_rect();
    let layers = frames
        .iter()
        .enumerate()
        .map(|(i, frame)| {
            let mut layer = Layer::new_raster(layer_name(i));
            if let Some(raster) = layer.as_raster_mut() {
                blit_rgba8(&mut raster.tiles, doc.depth, rect, &frame.rgba);
            }
            layer
        })
        .collect();
    animation::add_flattened_layers(doc, layers, history_name)
}

/// Shrink an RGBA8 image to fit `max` pixels on its longer side, averaging
/// each source box with alpha weighting so transparent pixels do not darken
/// the edges. Returns the image unchanged when it already fits.
pub fn downscale(rgba: &[u8], width: u32, height: u32, max: u32) -> (Vec<u8>, u32, u32) {
    let longer = width.max(height);
    if longer <= max || max == 0 {
        return (rgba.to_vec(), width, height);
    }
    let scale = max as f64 / longer as f64;
    let w = ((width as f64 * scale).round() as u32).max(1);
    let h = ((height as f64 * scale).round() as u32).max(1);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let y0 = (y as u64 * height as u64 / h as u64) as u32;
        let y1 = (((y + 1) as u64 * height as u64).div_ceil(h as u64) as u32).max(y0 + 1);
        for x in 0..w {
            let x0 = (x as u64 * width as u64 / w as u64) as u32;
            let x1 = (((x + 1) as u64 * width as u64).div_ceil(w as u64) as u32).max(x0 + 1);
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for sy in y0..y1.min(height) {
                let row = (sy * width) as usize * 4;
                for sx in x0..x1.min(width) {
                    let p = &rgba[row + sx as usize * 4..][..4];
                    let a = p[3] as u64;
                    acc[0] += p[0] as u64 * a;
                    acc[1] += p[1] as u64 * a;
                    acc[2] += p[2] as u64 * a;
                    acc[3] += a;
                    n += 1;
                }
            }
            let o = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                out[o + c] = acc[c].checked_div(acc[3]).unwrap_or(0) as u8;
            }
            out[o + 3] = (acc[3] / n.max(1)) as u8;
        }
    }
    (out, w, h)
}

/// An onion skin: `rgba` washed halfway towards `color`, its alpha scaled
/// by `opacity`.
pub fn tint(rgba: &[u8], color: [u8; 3], opacity: f32) -> Vec<u8> {
    let opacity = opacity.clamp(0.0, 1.0);
    rgba.as_chunks::<4>().0.iter()
        .flat_map(|p| {
            [
                ((p[0] as u16 + color[0] as u16) / 2) as u8,
                ((p[1] as u16 + color[1] as u16) / 2) as u8,
                ((p[2] as u16 + color[2] as u16) / 2) as u8,
                (p[3] as f32 * opacity).round() as u8,
            ]
        })
        .collect()
}

/// Flatten straight-alpha RGBA8 onto an opaque colour.
pub(crate) fn over_matte(rgba: &[u8], matte: [u8; 3]) -> Vec<u8> {
    rgba.as_chunks::<4>().0.iter()
        .flat_map(|p| {
            let a = p[3] as u32;
            let mix = |c: u8, m: u8| ((c as u32 * a + m as u32 * (255 - a) + 127) / 255) as u8;
            [
                mix(p[0], matte[0]),
                mix(p[1], matte[1]),
                mix(p[2], matte[2]),
                255,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_keeps_aspect_and_ignores_transparent_colour() {
        // 4x2: left half opaque red, right half transparent "black".
        let mut rgba = Vec::new();
        for _ in 0..2 {
            rgba.extend_from_slice(&[255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        let (small, w, h) = downscale(&rgba, 4, 2, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(&small[..4], &[255, 0, 0, 255]);
        assert_eq!(small[7], 0);
        let (same, w, h) = downscale(&rgba, 4, 2, 8);
        assert_eq!((w, h, same), (4, 2, rgba));
    }

    #[test]
    fn matte_and_tint() {
        assert_eq!(
            over_matte(&[0, 0, 0, 0], [255, 255, 255]),
            [255, 255, 255, 255]
        );
        assert_eq!(
            over_matte(&[10, 20, 30, 255], [255, 0, 0]),
            [10, 20, 30, 255]
        );
        assert_eq!(tint(&[0, 0, 0, 200], [200, 100, 0], 0.5), [100, 50, 0, 100]);
    }
}
