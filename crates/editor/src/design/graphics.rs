//! Decoded artwork is session data. LayoutDocument contains only links and
//! placement, while the raster codecs and compositor supply these pixels.
use schist_layout::{
    graphics::{image_rect, ImageMapping},
    Link, Point, Rect,
};
use schist_separation::{GraphicPlacement, GraphicSource, PlacedGraphic};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// GPUI uploads BGRA bytes even though its frame carrier is `RgbaImage`.
/// Convert only the owned preview image; source samples remain straight RGBA
/// for transforms and separation, and native CMYK channels stay independent.
pub(super) fn render_image(mut pixels: image::RgbaImage) -> Arc<gpui::RenderImage> {
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(gpui::RenderImage::new(vec![image::Frame::new(pixels)]))
}

pub struct Graphic {
    pub pixels: image::RgbaImage,
    /// Authored process channels from a native CMYK composite. Preview
    /// pixels never become the source for separating these again.
    pub cmyk: Option<Vec<[f32; 4]>>,
    pub image: Arc<gpui::RenderImage>,
    pub dpi: f32,
    pub modified: Option<u64>,
    opacity_images: std::sync::Mutex<std::collections::VecDeque<(u8, Arc<gpui::RenderImage>)>>,
}

impl Graphic {
    pub fn new(pixels: image::RgbaImage, dpi: f32, modified: Option<u64>) -> Self {
        let image = render_image(pixels.clone());
        Self {
            pixels,
            cmyk: None,
            image,
            dpi: if dpi.is_finite() && dpi > 0.0 {
                dpi
            } else {
                72.0
            },
            modified,
            opacity_images: Default::default(),
        }
    }
    /// Two recent opacity variants bound the CPU/atlas memory cost. The
    /// decoded source stays unchanged and is reused by separation.
    pub fn image_with_opacity(&self, opacity: f32) -> Arc<gpui::RenderImage> {
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        if alpha == 255 {
            return self.image.clone();
        }
        let mut cache = self
            .opacity_images
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(index) = cache.iter().position(|(a, _)| *a == alpha) {
            let entry = cache.remove(index).unwrap();
            let image = entry.1.clone();
            cache.push_back(entry);
            return image;
        }
        let mut pixels = self.pixels.clone();
        for pixel in pixels.pixels_mut() {
            pixel.0[3] = ((u16::from(pixel.0[3]) * u16::from(alpha) + 127) / 255) as u8;
        }
        let image = render_image(pixels);
        if cache.len() == 2 {
            cache.pop_front();
        }
        cache.push_back((alpha, image.clone()));
        image
    }
    pub fn size_points(&self) -> (f32, f32) {
        (
            self.pixels.width() as f32 * 72.0 / self.dpi,
            self.pixels.height() as f32 * 72.0 / self.dpi,
        )
    }
}

#[derive(Default, Clone)]
pub struct Graphics {
    pub sources: HashMap<String, Result<Arc<Graphic>, String>>,
}
impl Graphics {
    pub fn get(&self, path: &str) -> Option<&Arc<Graphic>> {
        self.sources.get(path)?.as_ref().ok()
    }
}

/// Relative links are based on the layout file, never the process's cwd.
pub fn resolve_path(path: &str, document: Option<&Path>) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    if path.starts_with("file:") {
        #[cfg(not(target_family = "wasm"))]
        return url::Url::parse(path).ok()?.to_file_path().ok();
        // Browsers cannot open a native file URL as a local filesystem
        // path. Such links remain unavailable until the user relinks them.
        #[cfg(target_family = "wasm")]
        return None;
    }
    let path = Path::new(path);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(document?.parent()?.join(path))
    }
}

impl GraphicSource for Graphics {
    fn info(&self, link: &Link) -> Option<schist_layout::GraphicInfo> {
        let source = self.get(&link.path)?;
        Some(schist_layout::GraphicInfo {
            width: source.pixels.width(),
            height: source.pixels.height(),
            dpi: source.dpi,
        })
    }
    fn sample(&self, link: &Link, placement: &GraphicPlacement) -> Option<PlacedGraphic> {
        self.sample_managed(link, placement, None)
    }
}
impl Graphics {
    pub fn sample_managed(
        &self,
        link: &Link,
        placement: &GraphicPlacement,
        transform: Option<&schist_colormgmt::NativeColorTransform>,
    ) -> Option<PlacedGraphic> {
        let source = self.get(&link.path)?;
        let dest = placement.dest;
        let output_scale = placement.dpi / 72.0;
        if !output_scale.is_finite() || output_scale <= 0.0 {
            return None;
        }
        let frame = Rect::new(
            0.0,
            0.0,
            dest.width() as f32 / output_scale,
            dest.height() as f32 / output_scale,
        );
        let mapped = image_rect(
            frame,
            source.pixels.dimensions(),
            source.dpi,
            placement.crop,
            placement.fit,
            placement.scale,
        )?;
        let mapping = ImageMapping::new(frame, mapped, placement.image_transform)?;
        let area = (dest.width().max(0) as usize).checked_mul(dest.height().max(0) as usize)?;
        // Bound a single request before allocating its five channels.
        if area > 64 * 1024 * 1024 {
            return None;
        }
        let mut graphic = PlacedGraphic {
            rect: dest,
            coverage: vec![0; area],
            cmyk: vec![[0.0; 4]; area],
        };
        for y in 0..dest.height() {
            for x in 0..dest.width() {
                let Some(at) = mapping.source_at(Point::new(
                    (x as f32 + 0.5) / output_scale,
                    (y as f32 + 0.5) / output_scale,
                )) else {
                    continue;
                };
                let u = (at.x - mapped.x) / mapped.width;
                let v = (at.y - mapped.y) / mapped.height;
                let sx = ((u * source.pixels.width() as f32) as u32).min(source.pixels.width() - 1);
                let sy =
                    ((v * source.pixels.height() as f32) as u32).min(source.pixels.height() - 1);
                let pixel = source.pixels.get_pixel(sx, sy).0;
                let index = (y * dest.width() + x) as usize;
                graphic.coverage[index] = pixel[3];
                let rgb = [
                    pixel[0] as f32 / 255.0,
                    pixel[1] as f32 / 255.0,
                    pixel[2] as f32 / 255.0,
                ];
                graphic.cmyk[index] = if let Some(cmyk) = &source.cmyk {
                    *cmyk.get(sy as usize * source.pixels.width() as usize + sx as usize)?
                } else if transform.is_some() {
                    [rgb[0], rgb[1], rgb[2], 0.0]
                } else {
                    schist_color::convert::rgb_to_cmyk(schist_color::Rgba::new(
                        rgb[0], rgb[1], rgb[2], 1.0,
                    ))
                };
            }
        }
        if let Some(transform) = transform.filter(|_| source.cmyk.is_none()) {
            for chunk in graphic.cmyk.chunks_mut(65_536) {
                let rgb: Vec<_> = chunk.iter().map(|p| [p[0], p[1], p[2]]).collect();
                let converted = transform.rgb_to_cmyk_checked(&rgb).ok()?;
                chunk.copy_from_slice(&converted);
            }
        }
        Some(graphic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_uploads_keep_channel_order_and_straight_alpha_without_changing_print_sources() {
        let colors = [[255, 0, 0], [0, 0, 255], [17, 63, 129], [0, 255, 0]];
        let pixels = image::RgbaImage::from_fn(4, 256, |x, y| {
            let [r, g, b] = colors[x as usize];
            image::Rgba([r, g, b, y as u8])
        });
        let source = Graphic::new(pixels.clone(), 72.0, None);
        for opacity in [1.0, 0.5, 0.0, 0.2, 0.5, 1.0] {
            let gpu = source.image_with_opacity(opacity);
            let alpha = (opacity * 255.0).round() as u16;
            for (original, uploaded) in pixels
                .as_raw()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(gpu.as_bytes(0).unwrap().as_chunks::<4>().0.iter())
            {
                assert_eq!(
                    *uploaded,
                    [
                        original[2],
                        original[1],
                        original[0],
                        ((u16::from(original[3]) * alpha + 127) / 255) as u8
                    ]
                );
            }
            assert_eq!(source.pixels, pixels);
            assert!(source.opacity_images.lock().unwrap().len() <= 2);
        }
    }

    #[test]
    fn inner_image_affines_sample_the_original_cmyk_grid_and_clip_to_the_frame() {
        use schist_layout::affine::Affine;
        let mut source = Graphic::new(
            image::RgbaImage::from_fn(3, 2, |x, y| {
                image::Rgba([0, 0, 0, [32, 64, 96, 128, 192, 255][(y * 3 + x) as usize]])
            }),
            72.0,
            None,
        );
        let colors: Vec<_> = (0..6)
            .map(|i| [i as f32 / 10.0, 0.17, 0.42, 0.63])
            .collect();
        source.cmyk = Some(colors.clone());
        let mut sources = Graphics::default();
        sources
            .sources
            .insert("native".into(), Ok(Arc::new(source)));
        for matrix in [
            Affine::IDENTITY,
            Affine {
                a: 0.0,
                b: 1.0,
                c: -1.0,
                d: 0.0,
                tx: 1.0,
                ty: 0.0,
            },
            Affine {
                a: -1.0,
                tx: 1.0,
                ..Affine::IDENTITY
            },
            Affine {
                c: 0.5,
                tx: -0.25,
                ..Affine::IDENTITY
            },
            Affine::scale(0.5, 0.5).around(0.5, 0.5),
        ] {
            for scale in [1, 2, 5] {
                let image = sources
                    .sample(
                        &Link::new("native"),
                        &GraphicPlacement {
                            dest: schist_core::IntRect::from_xywh(-7, 13, 12 * scale, 8 * scale),
                            crop: None,
                            fit: schist_layout::GraphicFit::Stretch,
                            scale: 1.0,
                            dpi: 72.0 * scale as f32,
                            image_transform: matrix,
                        },
                    )
                    .unwrap();
                for y in 0..image.rect.height() {
                    for x in 0..image.rect.width() {
                        // Invert the normalized matrix independently of ImageMapping.
                        let qx = (x as f32 + 0.5) / (12 * scale) as f32 - matrix.tx;
                        let qy = (y as f32 + 0.5) / (8 * scale) as f32 - matrix.ty;
                        let det = matrix.a * matrix.d - matrix.b * matrix.c;
                        let u = (matrix.d * qx - matrix.c * qy) / det;
                        let v = (matrix.a * qy - matrix.b * qx) / det;
                        let index = (y * image.rect.width() + x) as usize;
                        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                            assert_eq!(image.coverage[index], 0);
                        } else {
                            let source = (v * 2.0) as usize * 3 + (u * 3.0) as usize;
                            assert_eq!(image.coverage[index], [32, 64, 96, 128, 192, 255][source]);
                            assert_eq!(image.cmyk[index], colors[source]);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn links_resolve_against_the_layout_and_file_uris_decode_once() {
        // File URLs must name a native absolute path. A Unix root without
        // a drive or UNC share correctly cannot resolve on Windows.
        #[cfg(windows)]
        let roots = [
            ("C:/work", "file:///C:/work"),
            ("D:/work", "file://localhost/D:/work"),
            (r"\\server\share\work", "file://server/share/work"),
        ];
        #[cfg(not(windows))]
        let roots = [
            ("/work", "file:///work"),
            ("/work", "file://localhost/work"),
        ];
        for (root, uri) in roots {
            let root = Path::new(root);
            let base = root.join("layout/book.idml");
            assert!(base.is_absolute());
            assert_eq!(
                resolve_path("Links/photo.psd", Some(&base)),
                Some(root.join("layout/Links/photo.psd"))
            );
            let absolute = root.join("other/photo.psd");
            for document in [None, Some(base.as_path())] {
                assert_eq!(
                    resolve_path(absolute.to_str().unwrap(), document),
                    Some(absolute.clone())
                );
                for (encoded, name) in [
                    ("photo.psd", "photo.psd"),
                    ("a%20b.psd", "a b.psd"),
                    ("a%2520b.psd", "a%20b.psd"),
                    ("caf%C3%A9%23%25.psd", "café#%.psd"),
                ] {
                    assert_eq!(
                        resolve_path(&format!("{uri}/{encoded}"), document),
                        Some(root.join(name))
                    );
                }
            }
        }
        assert_eq!(resolve_path("relative.psd", None), None);
    }
    #[test]
    fn original_size_and_alpha_are_independent_of_output_resolution() {
        let mut sources = Graphics::default();
        sources.sources.insert(
            "photo".into(),
            Ok(Arc::new(Graphic::new(
                image::RgbaImage::from_pixel(2, 1, image::Rgba([255, 0, 0, 128])),
                72.0,
                None,
            ))),
        );
        for scale in [1, 2, 4, 8] {
            let image = sources
                .sample(
                    &Link::new("photo"),
                    &GraphicPlacement {
                        dest: schist_core::IntRect::from_xywh(0, 0, 4 * scale, 3 * scale),
                        crop: None,
                        fit: schist_layout::GraphicFit::Original,
                        scale: 1.0,
                        image_transform: Default::default(),
                        dpi: 72.0 * scale as f32,
                    },
                )
                .unwrap();
            assert_eq!(
                image.coverage.iter().filter(|v| **v == 128).count(),
                (2 * scale * scale) as usize
            );
            assert!(image.coverage.iter().all(|v| *v == 0 || *v == 128));
        }
    }
}
