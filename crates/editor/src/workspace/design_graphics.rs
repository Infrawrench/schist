//! File access for Design placement. Raster decoding runs off the UI thread;
//! a document snapshot prevents delayed work from editing a different target.
use super::*;
use crate::design::graphics::{resolve_path, Graphic, Graphics};
use schist_i18n::t;
use schist_layout::{graphics, LayoutObject, Link, ObjectId, Rect};

#[derive(Clone)]
pub enum Destination {
    Page(usize),
    Relink(ObjectId),
    Pages,
}

fn decode_graphic(
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
    path: &std::path::Path,
) -> anyhow::Result<Arc<Graphic>> {
    let doc = decode_file(codecs, path)?;
    #[cfg(not(target_arch = "wasm32"))]
    let modified = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    #[cfg(target_arch = "wasm32")]
    let modified = None;
    graphic_from_document(doc, modified)
}

fn graphic_from_document(doc: Document, modified: Option<u64>) -> anyhow::Result<Arc<Graphic>> {
    let rect = doc.canvas_rect();
    let rgba = schist_compositor::composite_region_rgba8(&doc, rect);
    let pixels = image::RgbaImage::from_raw(rect.width() as u32, rect.height() as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!(t("design.graphic_invalid")))?;
    let mut graphic = Graphic::new(pixels, doc.resolution_dpi, modified);
    if doc.mode == schist_color::ColorMode::Cmyk {
        graphic.cmyk = Some(
            schist_compositor::composite_native_region(&doc, rect)
                .into_iter()
                .map(|p| p.color)
                .collect(),
        );
    }
    Ok(Arc::new(graphic))
}

impl Workspace {
    pub fn pick_design_graphic(&mut self, destination: Destination, cx: &mut Context<Self>) {
        if !self.design_mode() || self.design.graphics_busy {
            return;
        }
        self.commit_focused_field();
        let session = self.design.session.clone();
        let document = self.design.document.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let picker = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: matches!(destination, Destination::Pages),
                prompt: Some(t("design.place_graphic").into()),
            },
            cx,
        );
        #[cfg(target_arch = "wasm32")]
        let picker = crate::web::pick_file(".psd,.psb,.png,.jpg,.jpeg,.tif,.tiff,.webp,.bmp,.gif");
        cx.spawn(async move |this, cx| {
            #[cfg(not(target_arch = "wasm32"))]
            let paths = picker
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .unwrap_or_default();
            #[cfg(target_arch = "wasm32")]
            let paths: Vec<_> = picker.await.ok().flatten().into_iter().collect();
            if !paths.is_empty() {
                this.update(cx, |ws, cx| {
                    if Arc::ptr_eq(&session, &ws.design.session) && document == ws.design.document {
                        ws.load_design_graphics(paths, destination, cx);
                    } else {
                        ws.status = t("design.graphic_target_changed").into();
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    pub(super) fn load_design_graphics(
        &mut self,
        paths: Vec<PathBuf>,
        destination: Destination,
        cx: &mut Context<Self>,
    ) {
        if self.design.graphics_busy {
            return;
        }
        let session = self.design.session.clone();
        let document = self.design.document.clone();
        let codecs = self.registry.shared_codecs();
        self.design.graphics_busy = true;
        self.status = t("design.graphics_loading").into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| decode_graphic(&codecs, &path).map(|source| (path, source)))
                        .collect::<anyhow::Result<Vec<_>>>()
                })
                .await;
            this.update(cx, |ws, cx| {
                if !Arc::ptr_eq(&session, &ws.design.session) {
                    return;
                }
                ws.design.graphics_busy = false;
                if document != ws.design.document {
                    ws.status = t("design.graphic_target_changed").into();
                    cx.notify();
                    return;
                }
                let sources = match result {
                    Ok(sources) => sources,
                    Err(error) => {
                        ws.status =
                            schist_i18n::tf!("design.graphic_load_failed", error = error).into();
                        cx.notify();
                        return;
                    }
                };
                let link = |path: &PathBuf, source: &Graphic| Link {
                    path: path.to_string_lossy().into_owned(),
                    modified: source.modified,
                    present: true,
                    info: Some(schist_layout::GraphicInfo {
                        width: source.pixels.width(),
                        height: source.pixels.height(),
                        dpi: source.dpi,
                    }),
                };
                let changed = match destination {
                    Destination::Page(page) => {
                        let Some((path, source)) = sources.first() else {
                            return;
                        };
                        let Some(paper) = ws.design.document.pages.get(page) else {
                            return;
                        };
                        let (w, h) = source.size_points();
                        let factor = (paper.width / w).min(paper.height / h).min(1.0);
                        let frame = Rect::new(
                            (paper.width - w * factor) / 2.0,
                            (paper.height - h * factor) / 2.0,
                            w * factor,
                            h * factor,
                        );
                        let id = schist_layout::authoring::graphic_frame_with_link(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            page,
                            frame,
                            link(path, source),
                            false,
                        );
                        if let Some(id) = id {
                            ws.design.selection = vec![id];
                        }
                        id.is_some()
                    }
                    Destination::Relink(id) => {
                        let Some((path, source)) = sources.first() else {
                            return;
                        };
                        graphics::relink(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            id,
                            link(path, source),
                        )
                    }
                    Destination::Pages => {
                        let first_page = ws.design.document.pages.len();
                        let pages: Vec<_> = sources
                            .iter()
                            .map(|(path, source)| {
                                let (width, height) = source.size_points();
                                graphics::GraphicPage {
                                    source: link(path, source),
                                    name: path
                                        .file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned(),
                                    width,
                                    height,
                                }
                            })
                            .collect();
                        let changed = graphics::import_pages(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            &pages,
                        );
                        if changed {
                            ws.design.page = Some(first_page);
                            ws.design.view.page = Some(first_page);
                            ws.design.selection.clear();
                            ws.refit_design = true;
                        }
                        changed
                    }
                };
                if changed {
                    let cache = Arc::make_mut(&mut ws.design.graphics);
                    for (path, source) in sources {
                        cache
                            .sources
                            .insert(path.to_string_lossy().into_owned(), Ok(source));
                    }
                    ws.design.preflight = Default::default();
                    ws.status = t("design.graphics_ready").into();
                } else {
                    ws.status = t("design.graphic_target_changed").into();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Explicit Refresh and document-open both recheck every distinct source.
    pub fn refresh_design_graphics(&mut self, cx: &mut Context<Self>) {
        if !self.design_mode() || self.design.graphics_busy {
            return;
        }
        let mut paths: Vec<_> = self
            .design
            .document
            .objects
            .iter()
            .chain(
                self.design
                    .document
                    .parents
                    .iter()
                    .flat_map(|p| p.objects.iter().map(|o| &o.object)),
            )
            .filter_map(|object| {
                if let LayoutObject::GraphicFrame { link, .. } = &object.object {
                    Some(link.path.clone())
                } else {
                    None
                }
            })
            .collect();
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            return;
        }
        let session = self.design.session.clone();
        let base = self.design_path.clone();
        let assets = self.design.document.assets.clone();
        let codecs = self.registry.shared_codecs();
        self.design.graphics_busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let sources = cx
                .background_executor()
                .spawn(async move {
                    let mut cache = Graphics::default();
                    for path in paths {
                        let result = if let Some(bytes) = assets.get(&path) {
                            decode_bytes(&codecs, std::path::Path::new(&path), bytes)
                                .and_then(|doc| graphic_from_document(doc, None))
                        } else {
                            resolve_path(&path, base.as_deref())
                                .ok_or_else(|| anyhow::anyhow!(t("design.graphic_unresolved")))
                                .and_then(|resolved| decode_graphic(&codecs, &resolved))
                        }
                        .map_err(|error| error.to_string());
                        cache.sources.insert(path, result);
                    }
                    cache
                })
                .await;
            this.update(cx, |ws, cx| {
                if !Arc::ptr_eq(&session, &ws.design.session) {
                    return;
                }
                ws.design.graphics_busy = false;
                ws.design.graphics = Arc::new(sources);
                ws.design.preflight = Default::default();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_plugin_api::{PluginManifest, PluginRegistry};

    #[test]
    fn cmyk_graphics_keep_every_native_channel_through_placement() {
        use schist_color::{ColorMode, Depth, NativePixel};
        use schist_core::{Layer, TileCoord};
        use schist_separation::GraphicSource;
        for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
            let mut doc = Document::new("process", 3, 1, depth);
            doc.mode = ColorMode::Cmyk;
            let id = doc.push_layer(Layer::new_raster("inks"));
            let mut edit = doc.begin_edit("seed");
            let tile = edit.writable_tile(id, TileCoord::containing(0, 0)).unwrap();
            for (i, color) in [
                [0.0, 0.0, 0.0, 0.75],
                [0.6, 0.4, 0.3, 0.8],
                [0.2, 0.2, 0.2, 0.2],
            ]
            .into_iter()
            .enumerate()
            {
                tile.set_native_pixel(
                    i,
                    NativePixel {
                        mode: ColorMode::Cmyk,
                        color,
                        alpha: 1.0,
                    },
                );
            }
            edit.commit();
            let expected = schist_compositor::composite_native_region(&doc, doc.canvas_rect());
            let graphic = graphic_from_document(doc, None).unwrap();
            assert_eq!(
                graphic.cmyk.as_ref().unwrap(),
                &expected.iter().map(|p| p.color).collect::<Vec<_>>()
            );
            let mut graphics = Graphics::default();
            graphics.sources.insert("native".into(), Ok(graphic));
            for scale in [1, 2, 5] {
                let placed = graphics
                    .sample(
                        &Link::new("native"),
                        &schist_separation::GraphicPlacement {
                            dest: schist_core::IntRect::from_xywh(0, 0, 3 * scale, scale),
                            crop: None,
                            fit: schist_layout::GraphicFit::Stretch,
                            scale: 1.0,
                            image_transform: Default::default(),
                            dpi: 72.0 * scale as f32,
                        },
                    )
                    .unwrap();
                for y in 0..scale {
                    for x in 0..3 * scale {
                        assert_eq!(
                            placed.cmyk[(y * 3 * scale + x) as usize],
                            expected[(x / scale) as usize].color
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn linked_and_embedded_images_use_the_same_decoder_and_keep_alpha() {
        let mut registry = PluginRegistry::new();
        schist_codecs_common::CommonCodecsPlugin.register(&mut registry);
        let codecs = registry.shared_codecs();
        let pixels = image::RgbaImage::from_fn(7, 5, |x, y| {
            image::Rgba([255, 0, 0, (x * 20 + y * 5) as u8])
        });
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels.clone())
            .write_to(&mut buffer, image::ImageFormat::Png)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("placed.png");
        std::fs::write(&path, buffer.get_ref()).unwrap();
        let linked = decode_graphic(&codecs, &path).unwrap();
        let embedded = graphic_from_document(
            decode_bytes(&codecs, &path, buffer.get_ref()).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(linked.pixels, embedded.pixels);
        assert_eq!(linked.pixels.dimensions(), pixels.dimensions());
        for (decoded, source) in linked.pixels.pixels().zip(pixels.pixels()) {
            assert_eq!(decoded[3], source[3]);
            if decoded[3] > 0 {
                assert_eq!(&decoded.0[..3], &source.0[..3]);
            }
        }
    }
}
