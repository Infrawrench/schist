//! Baking a document's adjustment layers into a 3D lookup table.
//!
//! The identity lattice is laid out as an image, one pixel per lattice
//! point, and composited under copies of the document's colour
//! adjustments by the active compositor: the table is the stack's own
//! output, not a re-implementation of it.
//!
//! Only what applies to every pixel the same way can be stored. Layer
//! masks, clipping and adjustments inside isolated groups depend on where
//! a pixel is or what is under it, so those layers are left out and
//! counted; so is everything that is not an adjustment (pixel layers,
//! effects, filters).

use schist_adjustments::lut::{Lut3d, MAX_3D_SIZE};
use schist_color::Depth;
use schist_core::{BlendMode, Document, Layer, LayerKind, TileCoord, TILE_PIXELS, TILE_SIZE};

/// The lattice sampled through the stack.
#[derive(Debug, Clone)]
pub struct BakedLut {
    pub cube: Lut3d,
    /// Adjustment layers baked into the table.
    pub included: usize,
    /// Visible adjustment layers left out because they are spatial.
    pub skipped: usize,
}

/// The identity lattice for `size`, as straight RGBA with red varying
/// fastest along each row, green across rows of `size`, blue down the
/// image: `size * size` wide and `size` tall.
pub fn identity_lattice(size: usize) -> (Vec<f32>, usize, usize) {
    let cube = Lut3d::identity(size);
    let n = cube.size;
    let mut rgba = Vec::with_capacity(cube.table.len() * 4);
    for v in &cube.table {
        rgba.extend_from_slice(&[v[0], v[1], v[2], 1.0]);
    }
    (rgba, n * n, n)
}

/// Sample `doc`'s global colour adjustments on a `size`³ lattice.
///
/// `before` runs on the lattice image first, which is how a caller folds
/// in a colour operation that lives outside the layer stack (Camera Raw's
/// global settings, say); it receives straight RGBA and the image size.
pub fn bake_adjustments(
    doc: &Document,
    size: usize,
    before: impl FnOnce(&mut [f32], usize, usize),
) -> BakedLut {
    let size = size.clamp(2, MAX_3D_SIZE);
    let (mut rgba, w, h) = identity_lattice(size);
    before(&mut rgba, w, h);

    let mut stack = Vec::new();
    let mut skipped = 0;
    collect(&doc.tree.layers, 1.0, &mut stack, &mut skipped);
    let included = stack.len();

    let mut lattice = Document::new("lut", w as u32, h as u32, Depth::ThirtyTwo);
    let mut base = Layer::new_raster("lattice");
    if let Some(raster) = base.as_raster_mut() {
        let mut tile = vec![0.0f32; TILE_PIXELS * 4];
        for coord in TileCoord::covering(&lattice.canvas_rect()) {
            let rect = coord.rect();
            tile.fill(0.0);
            for y in rect.top.max(0)..rect.bottom.min(h as i32) {
                for x in rect.left.max(0)..rect.right.min(w as i32) {
                    let src = (y as usize * w + x as usize) * 4;
                    let dst = ((y - rect.top) * TILE_SIZE + (x - rect.left)) as usize * 4;
                    tile[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                }
            }
            raster
                .tiles
                .get_mut_or_insert(coord, Depth::ThirtyTwo)
                .encode_f32(&tile);
        }
    }
    lattice.tree.layers.push(base);
    lattice.tree.layers.extend(stack);

    let out = crate::composite_region_f32(&lattice, lattice.canvas_rect());
    let mut cube = Lut3d::identity(size);
    for (i, v) in cube.table.iter_mut().enumerate() {
        *v = [out[i * 4], out[i * 4 + 1], out[i * 4 + 2]];
    }
    BakedLut {
        cube,
        included,
        skipped,
    }
}

/// The visible adjustments that act on everything below them, bottom to
/// top, as standalone layers carrying their enclosing groups' opacity.
fn collect(layers: &[Layer], opacity: f32, out: &mut Vec<Layer>, skipped: &mut usize) {
    for layer in layers.iter().filter(|l| l.visible) {
        match &layer.kind {
            LayerKind::Group(group) => {
                // A pass-through group without a mask is transparent to
                // what is beneath it; any other group composites its
                // children on their own first, so their adjustments only
                // see the group's contents.
                let open = layer.blend == BlendMode::PassThrough
                    && layer.mask.as_ref().is_none_or(|m| !m.enabled);
                if open {
                    collect(&group.children, opacity * layer.opacity, out, skipped);
                } else {
                    *skipped += count_adjustments(&group.children);
                }
            }
            LayerKind::Adjustment(_) => {
                let masked = layer.mask.as_ref().is_some_and(|m| m.enabled);
                if masked || layer.clipping {
                    *skipped += 1;
                    continue;
                }
                let mut copy = layer.clone();
                copy.opacity = (layer.opacity * opacity).clamp(0.0, 1.0);
                copy.mask = None;
                copy.render_offset = (0, 0);
                out.push(copy);
            }
            LayerKind::Raster(_) => {}
        }
    }
}

fn count_adjustments(layers: &[Layer]) -> usize {
    layers
        .iter()
        .filter(|l| l.visible)
        .map(|l| match &l.kind {
            LayerKind::Adjustment(_) => 1,
            LayerKind::Group(g) => count_adjustments(&g.children),
            LayerKind::Raster(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_adjustments::{Curve, Curves, Params};
    use schist_color::Rgba;
    use schist_core::{AdjustmentData, LayerMask};

    fn adjustment(params: &Params) -> Layer {
        let mut layer = Layer::new_raster("adjustment");
        layer.kind = LayerKind::Adjustment(AdjustmentData {
            kind: params.kind(),
            raw: Vec::new(),
            params_json: Some(serde_json::to_string(params).unwrap()),
        });
        layer
    }

    fn stack() -> Vec<Params> {
        vec![
            Params::Curves(Curves {
                rgb: Curve {
                    points: vec![(0.0, 0.05), (0.4, 0.5), (1.0, 0.95)],
                },
                ..Curves::default()
            }),
            Params::HueSaturation {
                hue: 25.0,
                saturation: 30.0,
                lightness: 0.0,
                colorize: false,
                lightness_desaturates: false,
                reciprocal_saturation: false,
                ranges: Vec::new(),
            },
            Params::ColorBalance {
                shadows: [10.0, 0.0, -15.0],
                midtones: [0.0, 5.0, 0.0],
                highlights: [-5.0, 0.0, 10.0],
                preserve_luminosity: true,
            },
        ]
    }

    #[test]
    fn baked_lut_reproduces_the_stack() {
        let mut doc = Document::new("graded", 8, 8, Depth::Eight);
        for params in stack() {
            doc.tree.layers.push(adjustment(&params));
        }
        // Half-strength layers are colour operations too.
        let mut half = adjustment(&Params::Invert);
        half.opacity = 0.25;
        doc.tree.layers.push(half);

        let baked = bake_adjustments(&doc, 33, |_, _, _| {});
        assert_eq!((baked.included, baked.skipped), (4, 0));
        let lookup = Params::ColorLookup(schist_adjustments::ColorLookup {
            name: "baked".into(),
            input: Default::default(),
            table: Some(schist_adjustments::LutTable::from_cube(
                "baked",
                &baked.cube,
            )),
        });
        let mut worst = 0.0f32;
        for i in 0..400 {
            let px = Rgba::new(
                (i * 37 % 101) as f32 / 100.0,
                (i * 53 % 103) as f32 / 102.0,
                (i * 71 % 107) as f32 / 106.0,
                1.0,
            );
            let mut expected = px;
            for params in stack() {
                expected = params.apply(expected);
            }
            let inverted = Params::Invert.apply(expected);
            for (e, inv) in [
                (&mut expected.r, inverted.r),
                (&mut expected.g, inverted.g),
                (&mut expected.b, inverted.b),
            ] {
                *e += (inv - *e) * 0.25;
            }
            let actual = lookup.apply(px);
            for (a, e) in [
                (actual.r, expected.r),
                (actual.g, expected.g),
                (actual.b, expected.b),
            ] {
                worst = worst.max((a - e).abs());
            }
        }
        // Hue/saturation bends sharply at the hue boundaries; a 33-point
        // lattice still lands within a few 8-bit levels.
        assert!(worst < 0.02, "baked LUT drifted {worst}");
    }

    #[test]
    fn spatial_layers_are_left_out_and_counted() {
        let mut doc = Document::new("spatial", 8, 8, Depth::Eight);
        let mut masked = adjustment(&Params::Invert);
        masked.mask = Some(LayerMask::new_revealing());
        doc.tree.layers.push(masked);
        let mut clipped = adjustment(&Params::Invert);
        clipped.clipping = true;
        doc.tree.layers.push(clipped);
        let mut isolated = Layer::new_group("isolated");
        isolated.blend = BlendMode::Normal;
        if let LayerKind::Group(g) = &mut isolated.kind {
            g.children.push(adjustment(&Params::Invert));
        }
        doc.tree.layers.push(isolated);
        let mut hidden = adjustment(&Params::Invert);
        hidden.visible = false;
        doc.tree.layers.push(hidden);

        let baked = bake_adjustments(&doc, 5, |_, _, _| {});
        assert_eq!((baked.included, baked.skipped), (0, 3));
        assert_eq!(baked.cube.table, Lut3d::identity(5).table);
    }

    #[test]
    fn pass_through_groups_contribute_with_their_opacity() {
        let mut doc = Document::new("grouped", 8, 8, Depth::Eight);
        let mut group = Layer::new_group("look");
        group.blend = BlendMode::PassThrough;
        group.opacity = 0.5;
        if let LayerKind::Group(g) = &mut group.kind {
            g.children.push(adjustment(&Params::Invert));
        }
        doc.tree.layers.push(group);
        let baked = bake_adjustments(&doc, 3, |_, _, _| {});
        assert_eq!(baked.included, 1);
        // Half an inversion is mid-grey everywhere.
        for v in &baked.cube.table {
            assert!(v.iter().all(|c| (c - 0.5).abs() < 1e-5), "{v:?}");
        }
    }

    #[test]
    fn the_before_hook_sees_the_lattice() {
        let doc = Document::new("empty", 8, 8, Depth::Eight);
        let baked = bake_adjustments(&doc, 4, |rgba, w, h| {
            assert_eq!((w, h), (16, 4));
            for px in rgba.as_chunks_mut::<4>().0 {
                px[0] = 1.0 - px[0];
            }
        });
        let identity = Lut3d::identity(4);
        for (a, b) in baked.cube.table.iter().zip(&identity.table) {
            assert!((a[0] - (1.0 - b[0])).abs() < 1e-6 && a[1] == b[1]);
        }
    }
}
