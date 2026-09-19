//! Editable embedded documents and explicit filesystem-linked smart objects.
use super::*;
use anyhow::{ensure, Context as _};
use schist_core::smart_source::{
    SmartSource, SourceIdentity, MAX_DOCUMENT_BYTES, MAX_SOURCE_PIXELS,
};
use schist_core::{DocumentId, LayerId, SmartObject, TileMap};
use schist_i18n::{t, tf};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub(super) struct SourceEdit {
    parent: DocumentId,
    layer: LayerId,
    original: SmartSource,
    file_digest: Option<Vec<u8>>,
}

fn check_size(width: u32, height: u32) -> anyhow::Result<()> {
    ensure!(
        width > 0
            && height > 0
            && width <= 30_000
            && height <= 30_000
            && u64::from(width) * u64::from(height) <= MAX_SOURCE_PIXELS,
        "{}",
        t("smart.error.too_large")
    );
    Ok(())
}

#[derive(Clone)]
struct SourceColor {
    mode: ColorMode,
    icc: Option<Vec<u8>>,
}
impl From<&Document> for SourceColor {
    fn from(doc: &Document) -> Self {
        Self {
            mode: doc.mode,
            icc: doc.icc_profile.clone(),
        }
    }
}

fn source_pixels(
    doc: &Document,
    target: &SourceColor,
    origin: [i32; 2],
) -> anyhow::Result<TileMap> {
    check_size(doc.width, doc.height)?;
    ensure!(
        origin.iter().all(|v| v.abs_diff(0) <= 1_000_000),
        "{}",
        t("smart.error.invalid")
    );
    if matches!(doc.mode, ColorMode::Cmyk | ColorMode::Lab)
        && doc.mode == target.mode
        && doc.icc_profile == target.icc
    {
        // Preserve ink separations (especially K-only black) in a no-op source
        // edit. RGB round-tripping would change them even with identical ICCs.
        let native = schist_compositor::composite_native_region(doc, doc.canvas_rect());
        return Ok(native_source_tiles(native, target.mode, doc.width, origin));
    }
    let mut floats = schist_compositor::composite_region_f32(doc, doc.canvas_rect());
    // Native documents composite into sRGB; RGB documents retain their working
    // profile. Convert before rendering into the destination's native samples.
    let source_profile = if matches!(doc.mode, ColorMode::Cmyk | ColorMode::Lab) {
        schist_colormgmt::Profile::srgb()
    } else {
        doc.icc_profile
            .as_deref()
            .map(schist_colormgmt::Profile::from_bytes)
            .transpose()?
            .unwrap_or_else(schist_colormgmt::Profile::srgb)
    };
    let target_profile = if matches!(target.mode, ColorMode::Cmyk | ColorMode::Lab) {
        schist_colormgmt::Profile::srgb()
    } else {
        target
            .icc
            .as_deref()
            .map(schist_colormgmt::Profile::from_bytes)
            .transpose()?
            .unwrap_or_else(schist_colormgmt::Profile::srgb)
    };
    schist_colormgmt::ColorTransform::new(
        &source_profile,
        &target_profile,
        schist_colormgmt::Intent::Perceptual,
    )?
    .apply(&mut floats);
    let mut source = TileMap::new_in_mode(target.mode);
    let region = IntRect::from_xywh(origin[0], origin[1], doc.width, doc.height);
    if matches!(target.mode, ColorMode::Cmyk | ColorMode::Lab) {
        let transform = if target.icc.is_some() {
            Some(schist_colormgmt::NativeColorTransform::new(
                target.mode,
                target.icc.as_deref(),
            )?)
        } else {
            None
        };
        let native = schist_colormgmt::rgba_to_native(target.mode, &floats, transform.as_ref());
        source = native_source_tiles(native, target.mode, doc.width, origin);
    } else {
        schist_core::blit_rgba_f32(&mut source, Depth::ThirtyTwo, region, &floats);
    }
    Ok(source)
}

fn native_source_tiles(
    native: Vec<schist_color::NativePixel>,
    mode: ColorMode,
    width: u32,
    origin: [i32; 2],
) -> TileMap {
    let mut source = TileMap::new_in_mode(mode);
    for (i, pixel) in native.into_iter().enumerate() {
        let x = origin[0] + (i % width as usize) as i32;
        let y = origin[1] + (i / width as usize) as i32;
        let coord = TileCoord::containing(x, y);
        let rect = coord.rect();
        source
            .get_mut_or_insert(coord, Depth::ThirtyTwo)
            .set_native_pixel(((y - rect.top) * TILE_SIZE + x - rect.left) as usize, pixel);
    }
    source
}

fn encode_source(doc: &Document, identity: SourceIdentity) -> anyhow::Result<SmartSource> {
    check_size(doc.width, doc.height)?;
    let document = schist_codec_psd::write_psd(doc)?;
    ensure!(
        document.len() <= MAX_DOCUMENT_BYTES,
        "{}",
        t("smart.error.too_large")
    );
    Ok(SmartSource { identity, document })
}

fn file_stamp(path: &std::path::Path) -> Option<[u64; 2]> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let meta = std::fs::metadata(path).ok()?;
        Some([
            meta.len(),
            meta.modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos()
                .try_into()
                .ok()?,
        ])
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

fn read_bounded(path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = {
        use std::io::Read as _;
        let file = std::fs::File::open(path)?;
        ensure!(
            file.metadata()?.len() <= MAX_DOCUMENT_BYTES as u64,
            "{}",
            t("smart.error.too_large")
        );
        let mut bytes = Vec::new();
        file.take(MAX_DOCUMENT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        bytes
    };
    #[cfg(target_arch = "wasm32")]
    let bytes = crate::web::read_file(path)?;
    ensure!(
        bytes.len() <= MAX_DOCUMENT_BYTES,
        "{}",
        t("smart.error.too_large")
    );
    Ok(bytes)
}

fn load_source(
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
    path: &std::path::Path,
) -> anyhow::Result<(Document, Vec<u8>)> {
    let bytes = read_bounded(path)?;
    // Check dimensions before allocating decoded pixels. Unknown image formats
    // are refused here even when a general-purpose import plugin accepts them.
    let (w, h) = if schist_codec_psd::is_psd(&bytes) {
        schist_codec_psd::read_dimensions(&bytes)?
    } else {
        image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()?
            .into_dimensions()?
    };
    check_size(w, h)?;
    let codec = codecs
        .iter()
        .find(|c| c.probe(&bytes))
        .context(t("smart.error.unsupported"))?;
    let mut doc = codec.import(&bytes)?;
    check_size(doc.width, doc.height)?;
    doc.title = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    doc.path = None;
    Ok((doc, Sha256::digest(&bytes).to_vec()))
}

/// Build every new layer before mutating the document: an unavailable filter or
/// corrupt source leaves all instances untouched.
fn replacement_layers(
    registry: &PluginRegistry,
    doc: &Document,
    selected: LayerId,
    previous: &SmartSource,
    replacement: &SmartSource,
    pixels: &TileMap,
    region: IntRect,
) -> anyhow::Result<Vec<Layer>> {
    #[allow(clippy::too_many_arguments)]
    fn visit(
        layers: &[Layer],
        out: &mut Vec<Layer>,
        registry: &PluginRegistry,
        doc: &Document,
        selected: LayerId,
        previous: &SmartSource,
        replacement: &SmartSource,
        pixels: &TileMap,
        region: IntRect,
    ) -> anyhow::Result<()> {
        for layer in layers {
            if let schist_core::LayerKind::Group(g) = &layer.kind {
                visit(
                    &g.children,
                    out,
                    registry,
                    doc,
                    selected,
                    previous,
                    replacement,
                    pixels,
                    region,
                )?;
            }
            let Some(mut smart) = layer.smart.clone() else {
                continue;
            };
            let metadata = SmartSource::identity(layer)?;
            let matches = layer.id == selected
                || metadata.as_ref().is_some_and(|source| {
                    source.id == previous.identity.id
                        || previous
                            .identity
                            .linked_path
                            .as_ref()
                            .is_some_and(|path| source.linked_path.as_ref() == Some(path))
                });
            if !matches {
                continue;
            }
            ensure!(!layer.locked, "{}", t("smart.error.locked"));
            let mut changed = layer.clone();
            let filtered =
                if let Some(mut stack) = schist_core::filter_stack::FilterStack::read(layer)? {
                    stack.region = region;
                    let filtered = schist_plugin_api::filter_stack::render(
                        registry,
                        &stack,
                        pixels,
                        doc.depth,
                        doc.icc_profile.clone(),
                    )?;
                    changed.extras.retain(|b| {
                        b.key != schist_core::filter_stack::SOURCE_KEY && b.key != *b"ScFc"
                    });
                    changed.extras = stack.blocks(&changed, pixels)?;
                    filtered
                } else {
                    ensure!(
                        !schist_core::filter_stack::has_stack(layer),
                        "{}",
                        t("smart.error.invalid")
                    );
                    pixels.clone()
                };
            let old_origin = metadata
                .as_ref()
                .map_or(previous.identity.origin, |s| s.origin);
            let new_origin = replacement.identity.origin;
            smart.transform = smart.transform.then(&schist_core::Affine::translate(
                (old_origin[0] - new_origin[0]) as f32,
                (old_origin[1] - new_origin[1]) as f32,
            ));
            smart.source = filtered;
            smart.source_bounds = smart.source.content_bounds();
            smart.name = layer.name.clone();
            let tiles = smart.render(doc.depth, doc.canvas_rect());
            changed
                .as_raster_mut()
                .context(t("smart.error.select"))?
                .tiles = tiles;
            changed.smart = Some(smart);
            changed.extras = replacement.blocks(&changed)?;
            changed.styled = None;
            out.push(changed);
        }
        Ok(())
    }
    ensure!(
        doc.tree.find(selected).is_some_and(|l| l.smart.is_some()),
        "{}",
        t("smart.error.select")
    );
    let mut out = Vec::new();
    visit(
        &doc.tree.layers,
        &mut out,
        registry,
        doc,
        selected,
        previous,
        replacement,
        pixels,
        region,
    )?;
    Ok(out)
}

fn commit_replacements(doc: &mut Document, layers: Vec<Layer>) {
    let mut edit = doc.begin_edit(t("smart.history.update"));
    for layer in layers {
        edit.replace_layer_tiles(layer.id, layer.as_raster().unwrap().tiles.clone());
        edit.set_smart_object(layer.id, layer.smart.clone());
        edit.set_extras(layer.id, layer.extras);
    }
    edit.commit();
}

fn legacy_source(doc: &Document, layer: &Layer) -> anyhow::Result<SmartSource> {
    let smart = layer.smart.as_ref().context(t("smart.error.select"))?;
    // Normalize legacy source coordinates for the editable canvas and keep
    // the original origin in metadata so instance placement stays unchanged.
    let bounds = schist_core::filter_stack::FilterStack::read(layer)?
        .map(|stack| stack.region)
        .unwrap_or(smart.source_bounds);
    ensure!(
        [bounds.left, bounds.top, bounds.right, bounds.bottom]
            .iter()
            .all(|v| v.abs_diff(0) <= 1_000_000),
        "{}",
        t("smart.error.invalid")
    );
    check_size(bounds.width().max(1) as u32, bounds.height().max(1) as u32)?;
    let mut child = Document::new(
        smart.name.clone(),
        bounds.width().max(1) as u32,
        bounds.height().max(1) as u32,
        doc.depth,
    );
    child.mode = doc.mode;
    child.icc_profile = doc.icc_profile.clone();
    let mut raster = Layer::new_raster(smart.name.clone());
    let original_pixels = if schist_core::filter_stack::has_stack(layer) {
        schist_core::filter_stack::read_source(layer)?
    } else {
        smart.source.clone()
    };
    raster.as_raster_mut().unwrap().tiles = schist_core::resample::transform_tiles(
        &original_pixels,
        &schist_core::Affine::translate(-bounds.left as f32, -bounds.top as f32),
        doc.depth,
        schist_core::Filter::Nearest,
        child.canvas_rect(),
    );
    child.push_layer(raster);
    let identity = SourceIdentity {
        version: 1,
        id: format!(
            "{}-{}-{}",
            doc.id.0,
            layer.id.0,
            web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ),
        linked_path: None,
        origin: [bounds.left, bounds.top],
        linked_stamp: None,
    };
    encode_source(&child, identity)
}
impl Workspace {
    pub(crate) fn smart_source_description(&self) -> Option<String> {
        let doc = self.doc.as_ref()?;
        let layer = doc.tree.find(doc.active_layer?)?;
        layer.smart.as_ref()?;
        match SmartSource::identity(layer) {
            Ok(Some(identity)) => match &identity.linked_path {
                Some(path) => {
                    let status = if cfg!(target_arch = "wasm32") {
                        t("smart.link_browser")
                    } else {
                        match file_stamp(std::path::Path::new(path)) {
                            None => t("smart.link_missing"),
                            Some(stamp) if Some(stamp) != identity.linked_stamp => {
                                t("smart.link_changed")
                            }
                            _ => t("smart.link_current"),
                        }
                    };
                    Some(tf!("smart.link_status", status = status, path = path))
                }
                None => Some(t("smart.embedded_source").into()),
            },
            Ok(None) => Some(t("smart.embedded_source").into()),
            Err(_) => Some(t("smart.error.invalid").into()),
        }
    }

    fn smart_failure(&mut self, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        self.status = tf!("smart.error.failed", error = error).into();
        cx.notify();
    }

    fn active_smart_source(&self) -> anyhow::Result<(LayerId, SmartSource)> {
        let doc = self.doc.as_ref().context(t("common.no_document"))?;
        let layer = doc
            .active_layer
            .and_then(|id| doc.tree.find(id))
            .context(t("smart.error.select"))?;
        ensure!(!layer.locked, "{}", t("smart.error.locked"));
        layer.smart.as_ref().context(t("smart.error.select"))?;
        if let Some(source) = SmartSource::read(layer)? {
            return Ok((layer.id, source));
        }
        Ok((layer.id, legacy_source(doc, layer)?))
    }

    pub fn pick_smart_source(&mut self, linked: bool, replace: bool, cx: &mut Context<Self>) {
        if linked && cfg!(target_arch = "wasm32") {
            self.smart_failure(t("smart.error.browser_link"), cx);
            return;
        }
        self.discard_stack_filter_for_document_change();
        let Some(doc) = self.doc.as_ref() else { return };
        let destination = (doc.id, doc.revision);
        let previous = if replace {
            match self.active_smart_source() {
                Ok(v) => Some(v),
                Err(e) => {
                    self.smart_failure(e, cx);
                    return;
                }
            }
        } else {
            None
        };
        #[cfg(not(target_arch = "wasm32"))]
        let rx = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(t("smart.pick_source").into()),
            },
            cx,
        );
        #[cfg(target_arch = "wasm32")]
        let rx = crate::web::pick_file(".psd,.psb,.png,.jpg,.jpeg,.tif,.tiff,.webp,.bmp,.gif");
        cx.spawn(async move |this, cx| {
            #[cfg(not(target_arch = "wasm32"))]
            let path = match rx.await {
                Ok(Ok(Some(mut paths))) => paths.pop(),
                _ => None,
            };
            #[cfg(target_arch = "wasm32")]
            let path = rx.await.ok().flatten();
            if let Some(path) = path {
                this.update(cx, |ws, cx| {
                    ws.load_smart_source(path, linked, destination, previous, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    fn load_smart_source(
        &mut self,
        path: PathBuf,
        linked: bool,
        destination: (DocumentId, u64),
        previous: Option<(LayerId, SmartSource)>,
        cx: &mut Context<Self>,
    ) {
        if !self
            .doc
            .as_ref()
            .is_some_and(|doc| (doc.id, doc.revision) == destination)
        {
            self.smart_failure(t("smart.error.changed"), cx);
            return;
        }
        let codecs = self.registry.shared_codecs();
        let Some(target) = self.doc.as_ref().map(SourceColor::from) else {
            return;
        };
        self.status = t("smart.loading").into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    #[cfg(not(target_arch = "wasm32"))]
                    let path = if linked {
                        std::fs::canonicalize(path)?
                    } else {
                        path
                    };
                    let (doc, _) = load_source(&codecs, &path)?;
                    let pixels = source_pixels(&doc, &target, [0, 0])?;
                    let identity = SourceIdentity {
                        version: 1,
                        id: previous
                            .as_ref()
                            .map(|(_, p)| p.identity.id.clone())
                            .unwrap_or_else(|| {
                                format!(
                                    "{}-{}",
                                    destination.0 .0,
                                    web_time::SystemTime::now()
                                        .duration_since(web_time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_nanos()
                                )
                            }),
                        linked_path: linked.then(|| path.to_string_lossy().into_owned()),
                        origin: [0, 0],
                        linked_stamp: if linked { file_stamp(&path) } else { None },
                    };
                    let source = encode_source(&doc, identity)?;
                    anyhow::Ok((source, pixels, doc.canvas_rect(), doc.title, previous))
                })
                .await;
            this.update(cx, |ws, cx| {
                let result = result.and_then(|(source, pixels, region, title, previous)| {
                    let doc = ws.doc.as_mut().context(t("common.no_document"))?;
                    ensure!(
                        (doc.id, doc.revision) == destination,
                        "{}",
                        t("smart.error.changed")
                    );
                    if let Some((id, previous)) = previous {
                        let layers = replacement_layers(
                            &ws.registry,
                            doc,
                            id,
                            &previous,
                            &source,
                            &pixels,
                            region,
                        )?;
                        commit_replacements(doc, layers);
                    } else {
                        let mut layer = Layer::new_raster(title.clone());
                        let mut smart = SmartObject::wrap(pixels, title);
                        smart.apply(&schist_core::Affine::translate(
                            (doc.width as f32 - region.width() as f32) / 2.0,
                            (doc.height as f32 - region.height() as f32) / 2.0,
                        ));
                        layer.as_raster_mut().unwrap().tiles =
                            smart.render(doc.depth, doc.canvas_rect());
                        layer.smart = Some(Box::new(smart));
                        layer.extras = source.blocks(&layer)?;
                        let id = layer.id;
                        let at = schist_core::LayerPath(vec![doc.tree.layers.len()]);
                        let mut edit = doc.begin_edit(t("smart.history.place"));
                        edit.insert_layer(at, layer);
                        edit.commit();
                        doc.active_layer = Some(id);
                    }
                    Ok(())
                });
                match result {
                    Ok(()) => {
                        ws.status = t("smart.updated").into();
                        ws.after_change(cx);
                    }
                    Err(e) => ws.smart_failure(e, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn update_linked_smart(&mut self, cx: &mut Context<Self>) {
        if cfg!(target_arch = "wasm32") {
            self.smart_failure(t("smart.error.browser_link"), cx);
            return;
        }
        match self.active_smart_source().and_then(|(id, source)| {
            let path = source
                .identity
                .linked_path
                .clone()
                .context(t("smart.error.no_link"))?;
            Ok((id, source, PathBuf::from(path)))
        }) {
            Ok((id, source, path)) => {
                let doc = self.doc.as_ref().unwrap();
                self.load_smart_source(path, true, (doc.id, doc.revision), Some((id, source)), cx);
            }
            Err(e) => self.smart_failure(e, cx),
        }
    }

    pub fn edit_smart_contents(&mut self, cx: &mut Context<Self>) {
        self.discard_stack_filter_for_document_change();
        let result: anyhow::Result<()> = (|| {
            let (layer, original) = self.active_smart_source()?;
            ensure!(
                self.smart_edit_sessions.len() < 16,
                "{}",
                t("smart.error.nesting")
            );
            let parent = self.doc.as_ref().unwrap().id;
            // Only one editor per source prevents a stale contents tab overwriting
            // a later save. Focus the existing editor when opened again.
            if let Some((&id, _)) = self.smart_edit_sessions.iter().find(|(_, edit)| {
                edit.parent == parent && edit.original.identity.id == original.identity.id
            }) {
                if let Some(index) = self.smart_tab_index(id) {
                    self.select_tab(index, cx);
                    return Ok(());
                }
            }
            let (mut child, file_digest) = if let Some(path) = &original.identity.linked_path {
                ensure!(
                    !cfg!(target_arch = "wasm32"),
                    "{}",
                    t("smart.error.browser_link")
                );
                let (doc, digest) =
                    load_source(&self.registry.shared_codecs(), std::path::Path::new(path))?;
                (doc, Some(digest))
            } else {
                let (w, h) = schist_codec_psd::read_dimensions(&original.document)?;
                check_size(w, h)?;
                (schist_codec_psd::read_psd(&original.document)?, None)
            };
            let parent_doc = self.doc.as_mut().unwrap();
            let parent_layer = parent_doc.tree.find(layer).unwrap();
            child.title = tf!("smart.contents_title", name = parent_layer.name);
            child.path = None;
            child.mark_saved();
            let blocks = original.blocks(parent_layer)?;
            let mut edit = parent_doc.begin_edit(t("smart.history.embed"));
            edit.set_extras(layer, blocks);
            edit.commit();
            self.after_change(cx);
            self.smart_edit_sessions.insert(
                child.id,
                SourceEdit {
                    parent,
                    layer,
                    original,
                    file_digest,
                },
            );
            self.open_in_tab(child, false);
            self.status = t("smart.edit_hint").into();
            cx.notify();
            Ok(())
        })();
        if let Err(e) = result {
            self.smart_failure(e, cx);
        }
    }

    fn smart_tab_index(&self, id: DocumentId) -> Option<usize> {
        if self.doc.as_ref().is_some_and(|d| d.id == id) {
            return Some(self.active_tab);
        }
        self.background_tabs
            .iter()
            .position(|t| t.doc.id == id)
            .map(|i| if i >= self.active_tab { i + 1 } else { i })
    }

    pub(super) fn forget_smart_contents(&mut self, id: DocumentId) {
        // Closing a parent detaches its source tabs: they remain ordinary dirty
        // documents that can be saved independently rather than losing edits.
        self.smart_edit_sessions
            .retain(|child, session| *child != id && session.parent != id);
    }

    pub(super) fn save_smart_contents(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(child_id) = self.doc.as_ref().map(|d| d.id) else {
            return false;
        };
        let Some(session) = self.smart_edit_sessions.get(&child_id).cloned() else {
            return false;
        };
        self.discard_stack_filter_for_document_change();
        let result: anyhow::Result<()> = (|| {
            let child = self.doc.as_ref().unwrap();
            let mut replacement = encode_source(child, session.original.identity.clone())?;
            let parent_index = self
                .smart_tab_index(session.parent)
                .context(t("smart.error.parent_closed"))?;
            let parent = &self
                .background_tabs
                .iter()
                .find(|tab| tab.doc.id == session.parent)
                .context(t("smart.error.parent_closed"))?
                .doc;
            let pixels = source_pixels(
                child,
                &SourceColor::from(parent),
                replacement.identity.origin,
            )?;
            let region = IntRect::from_xywh(
                replacement.identity.origin[0],
                replacement.identity.origin[1],
                child.width,
                child.height,
            );
            let current = parent
                .tree
                .find(session.layer)
                .context(t("smart.error.changed"))?;
            ensure!(
                SmartSource::read(current)?.as_ref() == Some(&session.original),
                "{}",
                t("smart.error.changed")
            );
            let mut layers = replacement_layers(
                &self.registry,
                parent,
                session.layer,
                &session.original,
                &replacement,
                &pixels,
                region,
            )?;
            if let Some(path) = &replacement.identity.linked_path {
                ensure!(
                    !cfg!(target_arch = "wasm32"),
                    "{}",
                    t("smart.error.browser_link")
                );
                let path = std::path::Path::new(path);
                let bytes = read_bounded(path)?;
                ensure!(
                    session.file_digest.as_deref() == Some(Sha256::digest(&bytes).as_slice()),
                    "{}",
                    t("smart.error.file_changed")
                );
                self.write_doc_to(child, path)?;
                replacement.identity.linked_stamp = file_stamp(path);
                for layer in &mut layers {
                    layer.extras = replacement.blocks(layer)?;
                }
            }
            self.doc.as_mut().unwrap().mark_saved();
            // A source save returns to its parent, making the propagated edit
            // visible immediately and letting the normal cloud/recovery hooks run.
            self.select_tab(parent_index, cx);
            commit_replacements(self.doc.as_mut().unwrap(), layers);
            self.smart_edit_sessions.remove(&child_id);
            // Close the clean contents tab; reopening gets the latest shared source.
            if let Some(index) = self.smart_tab_index(child_id) {
                self.close_tab(index, cx);
            }
            self.status = t("smart.updated").into();
            self.after_change(cx);
            Ok(())
        })();
        self.close_after_save = None;
        if let Err(e) = result {
            self.smart_failure(e, cx);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_core::filter_stack::{FilterEffect, FilterStack};
    use schist_plugin_api::{FilterPlugin, FilterValues};

    struct Red;
    impl FilterPlugin for Red {
        fn id(&self) -> &'static str {
            "red"
        }
        fn name(&self) -> &'static str {
            "red"
        }
        fn apply(&self, pixels: &mut [f32], _: usize, _: usize, _: &FilterValues) {
            for p in pixels.chunks_exact_mut(4) {
                if p[3] > 0.0 {
                    p[0] = 1.0;
                }
            }
        }
    }
    fn metadata() -> SmartSource {
        SmartSource {
            identity: SourceIdentity {
                version: 1,
                id: "family".into(),
                linked_path: Some("/missing.png".into()),
                origin: [0, 0],
                linked_stamp: None,
            },
            document: b"8BPSsource".to_vec(),
        }
    }
    fn pixels(color: Rgba) -> TileMap {
        let mut pixels = TileMap::new();
        pixels
            .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::ThirtyTwo)
            .set(0, color);
        pixels
    }
    fn setup() -> (Document, LayerId, PluginRegistry) {
        let mut doc = Document::new("parent", 16, 16, Depth::Sixteen);
        let metadata = metadata();
        let mut registry = PluginRegistry::new();
        registry.register_filter(Box::new(Red));
        let mut selected = LayerId(0);
        for x in [2.0, 7.0] {
            let mut layer = Layer::new_raster("instance");
            let mut smart = SmartObject::wrap(pixels(Rgba::BLACK), "source");
            smart.transform.tx = x;
            layer.as_raster_mut().unwrap().tiles = smart.render(doc.depth, doc.canvas_rect());
            layer.smart = Some(Box::new(smart));
            layer.opacity = 0.6;
            layer.extras = metadata.blocks(&layer).unwrap();
            selected = doc.push_layer(layer);
        }
        (doc, selected, registry)
    }

    #[test]
    fn all_instances_update_preserving_placements_properties_and_undo() {
        let (mut doc, selected, registry) = setup();
        let original = metadata();
        let mut replacement = original.clone();
        replacement.document.push(1);
        let layers = replacement_layers(
            &registry,
            &doc,
            selected,
            &original,
            &replacement,
            &pixels(Rgba::WHITE),
            IntRect::from_size(1, 1),
        )
        .unwrap();
        assert_eq!(layers.len(), 2);
        commit_replacements(&mut doc, layers);
        for (layer, x) in doc.tree.layers.iter().zip([2, 7]) {
            assert_eq!(layer.smart.as_ref().unwrap().transform.tx, x as f32);
            assert_eq!(layer.opacity, 0.6);
            assert_eq!(layer.as_raster().unwrap().tiles.pixel(x, 0), Rgba::WHITE);
            assert_eq!(SmartSource::read(layer).unwrap(), Some(replacement.clone()));
        }
        doc.undo();
        assert_eq!(
            doc.tree.layers[0].as_raster().unwrap().tiles.pixel(2, 0),
            Rgba::BLACK
        );
        assert_eq!(
            SmartSource::read(&doc.tree.layers[0]).unwrap(),
            Some(original)
        );
        doc.redo();
        assert_eq!(
            doc.tree.layers[0].as_raster().unwrap().tiles.pixel(2, 0),
            Rgba::WHITE
        );
    }

    #[test]
    fn replacement_preserves_legacy_source_anchor_when_origin_changes() {
        let (mut doc, selected, registry) = setup();
        let mut previous = metadata();
        previous.identity.origin = [-4, -2];
        let layer = doc.tree.find_mut(selected).unwrap();
        layer.extras = previous.blocks(layer).unwrap();
        let replacement = metadata();
        let layers = replacement_layers(
            &registry,
            &doc,
            selected,
            &previous,
            &replacement,
            &pixels(Rgba::WHITE),
            IntRect::from_size(1, 1),
        )
        .unwrap();
        let updated = layers.iter().find(|layer| layer.id == selected).unwrap();
        let placement = updated.smart.as_ref().unwrap().transform;
        assert_eq!((placement.tx, placement.ty), (3.0, -2.0));
    }

    #[test]
    fn replacement_reruns_filters_from_new_unfiltered_source() {
        let (mut doc, selected, registry) = setup();
        let layer = doc.tree.find_mut(selected).unwrap();
        let mut stack = FilterStack::new(IntRect::from_size(1, 1));
        stack.effects.push(FilterEffect {
            id: "red".into(),
            enabled: true,
            values: Default::default(),
            foreground: [0.0; 4],
            background: [1.0; 4],
        });
        layer.extras = stack.blocks(layer, &pixels(Rgba::BLACK)).unwrap();
        let source = pixels(Rgba::new(0.0, 0.7, 0.0, 1.0));
        let layers = replacement_layers(
            &registry,
            &doc,
            selected,
            &metadata(),
            &metadata(),
            &source,
            IntRect::from_size(2, 2),
        )
        .unwrap();
        commit_replacements(&mut doc, layers);
        let layer = doc.tree.find(selected).unwrap();
        let p = layer.smart.as_ref().unwrap().source.pixel(0, 0);
        assert_eq!(p.r, 1.0);
        assert!((p.g - 0.7).abs() < 0.001);
        assert_eq!(
            schist_core::filter_stack::read_source(layer)
                .unwrap()
                .pixel(0, 0)
                .r,
            0.0
        );
        assert_eq!(
            FilterStack::read(layer).unwrap().unwrap().region,
            IntRect::from_size(2, 2)
        );
    }

    #[test]
    fn unavailable_filters_or_locked_instances_leave_document_unchanged() {
        let (mut doc, selected, registry) = setup();
        doc.tree.layers[0].locked = true;
        assert!(replacement_layers(
            &registry,
            &doc,
            selected,
            &metadata(),
            &metadata(),
            &pixels(Rgba::WHITE),
            IntRect::from_size(1, 1)
        )
        .is_err());
        assert_eq!(
            doc.tree.layers[1].as_raster().unwrap().tiles.pixel(7, 0),
            Rgba::BLACK
        );
        doc.tree.layers[0].locked = false;
        let layer = doc.tree.find_mut(selected).unwrap();
        let mut stack = FilterStack::new(IntRect::from_size(1, 1));
        stack.effects.push(FilterEffect {
            id: "missing".into(),
            enabled: true,
            values: Default::default(),
            foreground: [0.0; 4],
            background: [1.0; 4],
        });
        layer.extras = stack.blocks(layer, &pixels(Rgba::BLACK)).unwrap();
        assert!(replacement_layers(
            &registry,
            &doc,
            selected,
            &metadata(),
            &metadata(),
            &pixels(Rgba::WHITE),
            IntRect::from_size(1, 1)
        )
        .is_err());
        assert_eq!(
            doc.tree.layers[0].as_raster().unwrap().tiles.pixel(2, 0),
            Rgba::BLACK
        );
    }

    #[test]
    fn composite_preserves_precision_converts_profile_and_native_mode_and_origin() {
        let mut child = Document::new("child", 1, 1, Depth::ThirtyTwo);
        let mut layer = Layer::new_raster("deep");
        layer.as_raster_mut().unwrap().tiles = pixels(Rgba::new(0.712345, 0.234567, 0.101234, 1.0));
        child.push_layer(layer);
        let rgb = SourceColor {
            mode: ColorMode::Rgb,
            icc: None,
        };
        let result = source_pixels(&child, &rgb, [-3, -5]).unwrap();
        assert!((result.pixel(-3, -5).r - 0.712345).abs() < 0.00001);
        assert_eq!(result.pixel(0, 0).a, 0.0);
        let p3 = schist_colormgmt::Profile::display_p3();
        let target = SourceColor {
            mode: ColorMode::Rgb,
            icc: p3.icc_bytes().map(|v| v.to_vec()),
        };
        let result = source_pixels(&child, &target, [0, 0]).unwrap();
        assert!((result.pixel(0, 0).r - 0.712345).abs() > 0.001);
        for mode in [ColorMode::Cmyk, ColorMode::Lab] {
            let native = source_pixels(&child, &SourceColor { mode, icc: None }, [0, 0]).unwrap();
            assert_eq!(native.mode(), mode);
            assert_eq!(native.native_pixel(0, 0).mode, mode);
            assert!((native.pixel(0, 0).r - 0.712345).abs() < 0.01);
        }
    }

    #[test]
    fn legacy_edit_normalizes_negative_origin_and_keeps_unfiltered_canvas() {
        let mut doc = Document::new("parent", 8, 8, Depth::ThirtyTwo);
        let region = IntRect::from_xywh(-4, -2, 2, 3);
        let original = schist_core::resample::transform_tiles(
            &pixels(Rgba::WHITE),
            &schist_core::Affine::translate(-4.0, -2.0),
            Depth::ThirtyTwo,
            schist_core::Filter::Nearest,
            region,
        );
        let mut layer = Layer::new_raster("legacy");
        layer.smart = Some(Box::new(SmartObject::wrap(
            TileMap::new(),
            "transparent filtered result",
        )));
        layer.extras = FilterStack::new(region).blocks(&layer, &original).unwrap();
        let source = legacy_source(&doc, &layer).unwrap();
        assert_eq!(source.identity.origin, [-4, -2]);
        let child = schist_codec_psd::read_psd(&source.document).unwrap();
        assert_eq!((child.width, child.height), (2, 3));
        assert_eq!(
            child.tree.layers[0].as_raster().unwrap().tiles.pixel(0, 0),
            Rgba::WHITE
        );
        let restored =
            source_pixels(&child, &SourceColor::from(&doc), source.identity.origin).unwrap();
        assert_eq!(restored.pixel(-4, -2), Rgba::WHITE);
        assert!(source_pixels(&child, &SourceColor::from(&doc), [i32::MAX, 0]).is_err());
        doc.push_layer(layer);
    }

    #[test]
    fn no_op_native_source_edit_preserves_k_only_ink_separations() {
        let mut child = Document::new("inks", 1, 1, Depth::ThirtyTwo);
        child.mode = ColorMode::Cmyk;
        let ink = schist_color::NativePixel {
            mode: ColorMode::Cmyk,
            color: [0.0, 0.0, 0.0, 0.8],
            alpha: 1.0,
        };
        let mut layer = Layer::new_raster("black ink");
        layer.as_raster_mut().unwrap().tiles =
            native_source_tiles(vec![ink], ColorMode::Cmyk, 1, [0, 0]);
        child.push_layer(layer);
        let pixels = source_pixels(&child, &SourceColor::from(&child), [-1, -2]).unwrap();
        assert_eq!(pixels.native_pixel(-1, -2), ink);
    }

    #[test]
    fn bounded_reader_rejects_oversize_and_missing_links_without_mutation() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        temp.as_file()
            .set_len(MAX_DOCUMENT_BYTES as u64 + 1)
            .unwrap();
        assert!(read_bounded(temp.path()).is_err());
        assert!(read_bounded(std::path::Path::new("/nonexistent-schist-source-test")).is_err());
        assert!(check_size(u32::MAX, u32::MAX).is_err());
        assert!(check_size(30_000, 30_000).is_err());
    }
}
