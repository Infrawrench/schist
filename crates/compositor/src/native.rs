//! Native channel compositing. RGB-only effects and nonseparable blend modes
//! have explicit processing boundaries; source tiles are never rewritten.
use schist_color::{ColorMode, NativePixel, Rgba};
use schist_core::{
    BlendMode, Document, IntRect, Layer, LayerKind, TileCoord, TILE_PIXELS, TILE_SIZE,
};

/// Reference implementation; never dispatches to the active backend.
pub fn composite_native_tile_cpu(doc: &Document, coord: TileCoord) -> Vec<NativePixel> {
    let mut out = vec![NativePixel::transparent(doc.mode); TILE_PIXELS];
    layers(doc, &doc.tree.layers, coord, &mut out);
    out
}

/// Composite authoritative channels through the active backend.
pub fn composite_native_tile(doc: &Document, coord: TileCoord) -> Vec<NativePixel> {
    super::backend().native_tile(doc, coord)
}

pub fn composite_native_region(doc: &Document, rect: IntRect) -> Vec<NativePixel> {
    let mut out =
        vec![NativePixel::transparent(doc.mode); rect.width() as usize * rect.height() as usize];
    let coords: Vec<_> = TileCoord::covering(&rect).collect();
    let tiles = super::backend().native_tiles(doc, &coords);
    for (coord, tile) in coords.into_iter().zip(tiles) {
        let clip = coord.rect().intersect(&rect);
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                out[(y - rect.top) as usize * rect.width() as usize + (x - rect.left) as usize] =
                    tile[(y.rem_euclid(TILE_SIZE) * TILE_SIZE + x.rem_euclid(TILE_SIZE)) as usize];
            }
        }
    }
    out
}

fn rgb_boundary(pixels: &mut [NativePixel], f: impl FnOnce(&mut Vec<f32>)) {
    let mut rgba: Vec<_> = pixels
        .iter()
        .flat_map(|p| {
            let p = p.to_rgba();
            [p.r, p.g, p.b, p.a]
        })
        .collect();
    let before = rgba.clone();
    f(&mut rgba);
    for ((native, p), old) in pixels
        .iter_mut()
        .zip(rgba.as_chunks::<4>().0.iter())
        .zip(before.as_chunks::<4>().0.iter())
    {
        // RGB conversion/LUT arithmetic can move an unchanged colour by
        // a few ULPs, differently on CPU and GPU. Do not re-separate inks
        // or clip out-of-gamut Lab for that numerical noise. Keep this
        // threshold in sync with composite_native.wgsl.
        if p[..3]
            .iter()
            .zip(&old[..3])
            .any(|(a, b)| (a - b).abs() > 1e-6)
        {
            *native = NativePixel::from_rgba(native.mode, Rgba::new(p[0], p[1], p[2], p[3]));
        }
        native.alpha = p[3];
    }
}

fn render(doc: &Document, layer: &Layer, coord: TileCoord) -> Vec<NativePixel> {
    let mut out = vec![NativePixel::transparent(doc.mode); TILE_PIXELS];
    let tiles = layer
        .styled
        .as_ref()
        .map(|s| &s.tiles)
        .or_else(|| layer.as_raster().map(|r| &r.tiles));
    if let Some(tiles) = tiles {
        let rect = coord.rect();
        for (i, p) in out.iter_mut().enumerate() {
            let x = rect.left + i as i32 % TILE_SIZE - layer.render_offset.0;
            let y = rect.top + i as i32 / TILE_SIZE - layer.render_offset.1;
            *p = tiles.native_pixel(x, y).converted(doc.mode);
        }
    } else if let LayerKind::Group(g) = &layer.kind {
        layers(doc, &g.children, coord, &mut out);
    }
    if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
        let rect = coord.rect();
        for (i, p) in out.iter_mut().enumerate() {
            p.alpha *= mask.value(
                rect.left + i as i32 % TILE_SIZE,
                rect.top + i as i32 / TILE_SIZE,
            ) as f32
                / 255.0;
        }
    }
    out
}

fn blend(mode: BlendMode, top: NativePixel, bottom: NativePixel, x: i32, y: i32) -> NativePixel {
    use BlendMode::*;
    if top.alpha <= 0.0 {
        return bottom;
    }
    if matches!(mode, Normal | PassThrough) {
        return top.over(bottom);
    }
    // Hue/saturation/luminosity are RGB operations; Lab's non-Normal modes
    // also use the established RGB definitions. Normal is always native.
    if top.mode == ColorMode::Lab
        || matches!(
            mode,
            Hue | Saturation | Color | Luminosity | DarkerColor | LighterColor
        )
    {
        return NativePixel::from_rgba(
            top.mode,
            super::blend_pixel(mode, top.to_rgba(), bottom.to_rgba(), x, y),
        );
    }
    // Separable blend functions run on ink complements, including K.
    let mut out = top;
    for c in 0..top.mode.channels() {
        let invert = top.mode == ColorMode::Cmyk;
        let t = if invert {
            1.0 - top.color[c]
        } else {
            top.color[c]
        };
        let b = if invert {
            1.0 - bottom.color[c]
        } else {
            bottom.color[c]
        };
        let p = super::blend_pixel(
            mode,
            Rgba::new(t, t, t, top.alpha),
            Rgba::new(b, b, b, bottom.alpha),
            x,
            y,
        );
        out.color[c] = if invert { 1.0 - p.r } else { p.r };
        out.alpha = p.a;
    }
    out
}

fn blend_onto(
    layer: &Layer,
    coord: TileCoord,
    src: &[NativePixel],
    dst: &mut [NativePixel],
    opacity: f32,
) {
    let rect = coord.rect();
    for (i, (s, d)) in src.iter().zip(dst.iter_mut()).enumerate() {
        let mut top = *s;
        top.alpha *= opacity;
        *d = blend(
            layer.blend,
            top,
            *d,
            rect.left + i as i32 % TILE_SIZE,
            rect.top + i as i32 / TILE_SIZE,
        );
    }
}

fn layers(doc: &Document, stack: &[Layer], coord: TileCoord, dst: &mut [NativePixel]) {
    let mut i = 0;
    while i < stack.len() {
        let layer = &stack[i];
        let mut end = i + 1;
        if !layer.clipping {
            while end < stack.len() && stack[end].clipping {
                end += 1;
            }
        }
        if !layer.visible {
            i = end;
            continue;
        }
        if end > i + 1 {
            let mut group = render(doc, layer, coord);
            let alpha: Vec<_> = group.iter().map(|p| p.alpha).collect();
            for clipped in &stack[i + 1..end] {
                if !clipped.visible {
                    continue;
                }
                if let LayerKind::Adjustment(data) = &clipped.kind {
                    rgb_boundary(&mut group, |rgba| {
                        super::apply_adjustment(clipped, data, coord, rgba, Some(&alpha))
                    });
                } else {
                    let mut src = render(doc, clipped, coord);
                    for (p, a) in src.iter_mut().zip(&alpha) {
                        p.alpha *= a;
                    }
                    blend_onto(
                        clipped,
                        coord,
                        &src,
                        &mut group,
                        clipped.opacity * super::content_alpha(clipped),
                    );
                }
            }
            blend_onto(
                layer,
                coord,
                &group,
                dst,
                layer.opacity * super::content_alpha(layer),
            );
        } else if let LayerKind::Adjustment(data) = &layer.kind {
            rgb_boundary(dst, |rgba| {
                super::apply_adjustment(layer, data, coord, rgba, None)
            });
        } else if let LayerKind::Group(g) = &layer.kind {
            if layer.styled.is_none()
                && layer.blend == BlendMode::PassThrough
                && layer.opacity >= 1.0
                && super::content_alpha(layer) >= 1.0
                && layer.mask.is_none()
            {
                layers(doc, &g.children, coord, dst);
            } else {
                let src = render(doc, layer, coord);
                blend_onto(
                    layer,
                    coord,
                    &src,
                    dst,
                    layer.opacity * super::content_alpha(layer),
                );
            }
        } else {
            let src = render(doc, layer, coord);
            blend_onto(
                layer,
                coord,
                &src,
                dst,
                layer.opacity * super::content_alpha(layer),
            );
        }
        i = end;
    }
}

/// Composite editable pixels, retaining native mode and document precision.
/// Used by layer merges; display/export-to-RGB callers use the RGBA APIs.
pub fn composite_region_tiles(doc: &Document, rect: IntRect, opaque: bool) -> schist_core::TileMap {
    let native = matches!(doc.mode, ColorMode::Cmyk | ColorMode::Lab);
    let mut tiles = schist_core::TileMap::new_in_mode(doc.mode);
    let pixels = if native {
        composite_native_region(doc, rect)
    } else {
        super::composite_region_f32(doc, rect)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| NativePixel::from_rgba(ColorMode::Rgb, Rgba::new(p[0], p[1], p[2], p[3])))
            .collect()
    };
    let white = NativePixel::from_rgba(if native { doc.mode } else { ColorMode::Rgb }, Rgba::WHITE);
    for coord in TileCoord::covering(&rect) {
        let clip = coord.rect().intersect(&rect);
        let tile = tiles.get_mut_or_insert(coord, doc.depth);
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                let p = pixels
                    [(y - rect.top) as usize * rect.width() as usize + (x - rect.left) as usize];
                tile.set_native_pixel(
                    (y.rem_euclid(TILE_SIZE) * TILE_SIZE + x.rem_euclid(TILE_SIZE)) as usize,
                    if opaque { p.over(white) } else { p },
                );
            }
        }
    }
    tiles.prune_blank();
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_adjustments::{Levels, Params};
    use schist_color::Depth;
    use schist_core::{AdjustmentData, AdjustmentKind, LayerMask, MaskTileMap};

    #[test]
    fn merge_and_flatten_keep_native_mode_depth_and_key() {
        let mut doc = Document::new("native merge", 1, 1, Depth::ThirtyTwo);
        doc.mode = ColorMode::Cmyk;
        let id = doc.push_layer(Layer::new_raster("ink"));
        let p = NativePixel {
            mode: doc.mode,
            color: [0.125, 0.25, 0.375, 0.5],
            alpha: 0.5,
        };
        let mut edit = doc.begin_edit("seed");
        edit.writable_tile(id, TileCoord::containing(0, 0))
            .unwrap()
            .set_native_pixel(0, p);
        edit.commit();
        let merged = composite_region_tiles(&doc, IntRect::from_size(1, 1), false);
        assert_eq!(merged.native_pixel(0, 0), p);
        assert_eq!(
            merged.get(TileCoord::containing(0, 0)).unwrap().depth(),
            Depth::ThirtyTwo
        );
        let flat = composite_region_tiles(&doc, IntRect::from_size(1, 1), true).native_pixel(0, 0);
        assert_eq!(flat.color, [0.0625, 0.125, 0.1875, 0.25]);
        assert_eq!(flat.alpha, 1.0);
    }

    #[test]
    fn opacity_and_masks_composite_native_inks_without_rgb_roundtrip() {
        let mut doc = Document::new("native", 1, 1, Depth::ThirtyTwo);
        doc.mode = ColorMode::Cmyk;
        let bottom = NativePixel {
            mode: doc.mode,
            color: [0.0, 0.0, 0.0, 0.5],
            alpha: 1.0,
        };
        let top = NativePixel {
            color: [0.5, 0.5, 0.5, 0.0],
            ..bottom
        };
        for (i, p) in [bottom, top].into_iter().enumerate() {
            let mut layer = Layer::new_raster("ink");
            layer
                .as_raster_mut()
                .unwrap()
                .tiles
                .get_mut_or_insert_mode(TileCoord::containing(0, 0), Depth::ThirtyTwo, doc.mode)
                .set_native_pixel(0, p);
            if i == 1 {
                layer.opacity = 0.5;
                layer.mask = Some(LayerMask {
                    tiles: MaskTileMap::new(),
                    default_value: 128,
                    ..LayerMask::new_revealing()
                });
            }
            doc.push_layer(layer);
        }
        let p = composite_native_region(&doc, IntRect::from_size(1, 1))[0];
        let expected = NativePixel {
            alpha: 0.5 * 128.0 / 255.0,
            ..top
        }
        .over(bottom);
        assert_eq!(p, expected);
        assert!(
            p.color[0] > 0.0 && p.color[3] > 0.0,
            "composite retains both process ink and key"
        );
        assert_eq!(
            doc.tree.layers[0]
                .as_raster()
                .unwrap()
                .tiles
                .native_pixel(0, 0),
            bottom
        );
    }

    #[test]
    fn identity_levels_keeps_inks_and_out_of_gamut_lab() {
        for mode in [ColorMode::Cmyk, ColorMode::Lab] {
            let mut doc = Document::new("identity", 256, 1, Depth::ThirtyTwo);
            doc.mode = mode;
            doc.tree.layers.clear();
            let mut layer = Layer::new_raster("native samples");
            let coord = TileCoord::containing(0, 0);
            let tile = layer
                .as_raster_mut()
                .unwrap()
                .tiles
                .get_mut_or_insert_mode(coord, doc.depth, mode);
            for i in 0..256 {
                let sample = |c: usize| ((i * (13 + c * 6) + c * 19) % 239) as f32 / 238.0;
                tile.set_native_pixel(
                    i,
                    NativePixel {
                        mode,
                        color: [
                            sample(0),
                            sample(1),
                            sample(2),
                            if mode == ColorMode::Cmyk {
                                sample(3)
                            } else {
                                0.0
                            },
                        ],
                        alpha: 0.63,
                    },
                );
            }
            doc.push_layer(layer);
            let before = composite_native_tile_cpu(&doc, coord);
            let mut adj = Layer::new_raster("identity Levels");
            adj.kind = LayerKind::Adjustment(AdjustmentData {
                kind: AdjustmentKind::Levels,
                raw: Vec::new(),
                params_json: Some(
                    serde_json::to_string(&Params::Levels(Levels::default())).unwrap(),
                ),
            });
            doc.push_layer(adj);
            assert_eq!(composite_native_tile_cpu(&doc, coord), before);
        }
    }
}
