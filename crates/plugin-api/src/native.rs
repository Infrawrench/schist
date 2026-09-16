//! Mode-aware destructive filter interchange. RGB-only filters use the
//! default adapter; native filters can override `apply_native_with`.
use schist_color::{ColorMode, NativePixel};
use schist_core::{IntRect, Selection, TileCoord, TileMap, TILE_SIZE};

#[derive(Clone, Debug)]
pub struct NativeFilterBuffer {
    pub mode: ColorMode,
    pub width: usize,
    pub height: usize,
    /// Row-major native pixels; alpha is independent of colour channels.
    pub pixels: Vec<NativePixel>,
    pub icc_profile: Option<Vec<u8>>,
}

impl NativeFilterBuffer {
    pub fn read(
        tiles: &TileMap,
        rect: IntRect,
        mode: ColorMode,
        icc_profile: Option<Vec<u8>>,
    ) -> Self {
        let pixels = (rect.top..rect.bottom)
            .flat_map(|y| {
                (rect.left..rect.right).map(move |x| tiles.native_pixel(x, y).converted(mode))
            })
            .collect();
        Self {
            mode,
            width: rect.width() as usize,
            height: rect.height() as usize,
            pixels,
            icc_profile,
        }
    }

    /// Explicit RGB compatibility boundary. Unchanged samples retain their
    /// original native values, including distinct CMYK separations and Lab
    /// colours outside the RGB gamut. Alpha-only changes never re-separate.
    pub fn process_rgba(&mut self, f: impl FnOnce(&mut [f32], usize, usize)) {
        let transform =
            schist_colormgmt::NativeColorTransform::new(self.mode, self.icc_profile.as_deref())
                .ok();
        let mut rgba = schist_colormgmt::native_to_rgba(&self.pixels, transform.as_ref());
        let before = rgba.clone();
        f(&mut rgba, self.width, self.height);
        let converted = schist_colormgmt::rgba_to_native(self.mode, &rgba, transform.as_ref());
        for (((native, p), old), new) in self
            .pixels
            .iter_mut()
            .zip(rgba.as_chunks::<4>().0.iter())
            .zip(before.as_chunks::<4>().0.iter())
            .zip(converted)
        {
            if p[..3] != old[..3] {
                *native = new;
            }
            native.alpha = p[3];
        }
    }

    /// Blend results into a COW snapshot, through the current selection.
    /// The host installs this map with one native tile history entry.
    pub fn write(
        &self,
        original: &TileMap,
        rect: IntRect,
        depth: schist_color::Depth,
        selection: &Selection,
    ) -> TileMap {
        assert_eq!(self.pixels.len(), self.width * self.height);
        assert_eq!(
            (self.width, self.height),
            (rect.width() as usize, rect.height() as usize)
        );
        let mut out = original.clone();
        for coord in TileCoord::covering(&rect) {
            let clip = coord.rect().intersect(&rect);
            let tile = out.get_mut_or_insert_mode(coord, depth, self.mode);
            for y in clip.top..clip.bottom {
                for x in clip.left..clip.right {
                    let weight = selection.coverage(x, y) as f32 / 255.0;
                    if weight == 0.0 {
                        continue;
                    }
                    let ix =
                        (y.rem_euclid(TILE_SIZE) * TILE_SIZE + x.rem_euclid(TILE_SIZE)) as usize;
                    let src = self.pixels
                        [(y - rect.top) as usize * self.width + (x - rect.left) as usize];
                    assert_eq!(src.mode, self.mode);
                    let mut p = tile.native_pixel(ix);
                    for c in 0..self.mode.channels() {
                        p.color[c] += (src.color[c] - p.color[c]) * weight;
                    }
                    p.alpha += (src.alpha - p.alpha) * weight;
                    tile.set_native_pixel(ix, p);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FilterContext, FilterPlugin, FilterValues};
    use schist_color::Depth;

    struct NativeInk;
    impl FilterPlugin for NativeInk {
        fn id(&self) -> &'static str {
            "native-ink-test"
        }
        fn name(&self) -> &'static str {
            "native ink"
        }
        fn apply(&self, _: &mut [f32], _: usize, _: usize, _: &FilterValues) {
            panic!("native entry point must be used")
        }
        fn apply_native_with(
            &self,
            pixels: &mut NativeFilterBuffer,
            _: &FilterValues,
            _: &FilterContext,
        ) {
            for p in &mut pixels.pixels {
                p.color[3] = 0.875;
            }
        }
    }

    #[test]
    fn rgb_identity_alpha_changes_and_native_plugin_keep_separations() {
        let mode = ColorMode::Cmyk;
        let original = NativePixel {
            mode,
            color: [0.25, 0.5, 0.75, 0.5],
            alpha: 0.5,
        };
        let mut tiles = TileMap::new_in_mode(mode);
        tiles
            .get_mut_or_insert(TileCoord::containing(0, 0), Depth::ThirtyTwo)
            .set_native_pixel(0, original);
        let rect = IntRect::from_size(1, 1);
        let mut buffer = NativeFilterBuffer::read(&tiles, rect, mode, None);
        buffer.process_rgba(|_, _, _| {});
        assert_eq!(buffer.pixels, [original]);
        buffer.process_rgba(|rgba, _, _| rgba[3] = 0.25);
        assert_eq!(buffer.pixels[0].color, original.color);
        assert_eq!(buffer.pixels[0].alpha, 0.25);
        NativeInk.apply_native_with(
            &mut buffer,
            &FilterValues::default(),
            &FilterContext::default(),
        );
        let edited = buffer.write(&tiles, rect, Depth::ThirtyTwo, &Selection::new());
        let p = edited.native_pixel(0, 0);
        assert_eq!(p.color, [0.25, 0.5, 0.75, 0.875]);
        assert_eq!(p.alpha, 0.25);
        assert_eq!(tiles.native_pixel(0, 0), original);
    }

    #[test]
    fn out_of_rgb_gamut_lab_survives_identity_filter() {
        let p = NativePixel {
            mode: ColorMode::Lab,
            color: [0.5, 1.0, 0.0, 0.0],
            alpha: 0.75,
        };
        let mut b = NativeFilterBuffer {
            mode: p.mode,
            width: 1,
            height: 1,
            pixels: vec![p],
            icc_profile: None,
        };
        b.process_rgba(|_, _, _| {});
        assert_eq!(b.pixels, [p]);
    }
}
