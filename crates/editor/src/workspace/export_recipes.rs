//! Workspace entry points and sequential background export jobs.
use super::*;
use crate::export_recipes::{self as recipes, Book, Editor, Recipe};
use schist_i18n::{t, tf};

impl Workspace {
    pub fn open_export_recipes(&mut self, photos: Vec<PathBuf>, cx: &mut Context<Self>) {
        match Book::load() {
            Ok(book) => self.open_modal(
                Modal::ExportRecipes {
                    editor: Editor::new(book, photos),
                },
                cx,
            ),
            Err(error) => {
                self.status = tf!("export_recipes.failed", error = error).into();
                cx.notify();
            }
        }
    }
    pub fn save_export_recipe(&mut self, cx: &mut Context<Self>) -> bool {
        self.commit_focused_field();
        let Some(Modal::ExportRecipes { editor }) = self.modal.as_mut() else {
            return false;
        };
        match editor.save() {
            Ok(()) => {
                self.status = t("export_recipes.saved").into();
                cx.notify();
                true
            }
            Err(error) => {
                editor.error = Some(error.to_string());
                cx.notify();
                false
            }
        }
    }
    pub fn delete_export_recipe(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::ExportRecipes { editor }) = self.modal.as_mut() else {
            return;
        };
        let Some(index) = editor.selected else { return };
        let mut book = editor.book.clone();
        book.recipes.remove(index);
        book.selected = index.min(book.recipes.len().saturating_sub(1));
        match book.save() {
            Ok(()) => *editor = Editor::new(book, editor.photos.clone()),
            Err(error) => editor.error = Some(error.to_string()),
        }
        self.focused_field = None;
        self.field_buffer.clear();
        cx.notify();
    }
    pub fn choose_recipe_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (window, cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.commit_focused_field();
            let Some(Modal::ExportRecipes { editor }) = self.modal.clone() else {
                return;
            };
            let session = editor.session.clone();
            let rx = self.prompt_for_paths(
                gpui::PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some(t("export_recipes.choose_folder").into()),
                },
                cx,
            );
            // Android's picker occupies the modal. Its normal close restores the
            // parent; opening any unrelated modal clears this stack instead.
            #[cfg(target_os = "android")]
            if matches!(self.modal, Some(Modal::FilePicker)) {
                self.modal_stack.push(Modal::ExportRecipes { editor });
            }
            cx.spawn_in(window, async move |this, cx| {
                let result = rx.await;
                this.update_in(cx, |ws, _window, cx| {
                    let Some(Modal::ExportRecipes { editor }) = ws.modal.as_mut() else {
                        return;
                    };
                    if !Arc::ptr_eq(&session, &editor.session) {
                        return;
                    }
                    if let Ok(Ok(Some(paths))) = result {
                        if let Some(path) = paths.into_iter().next() {
                            editor.draft.destination = path;
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
    pub fn run_export_recipe(&mut self, cx: &mut Context<Self>) {
        if !self.save_export_recipe(cx) {
            return;
        }
        let Some(Modal::ExportRecipes { editor }) = self.modal.clone() else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        if !editor.draft.destination.is_dir() {
            self.update_modal(|modal| {
                if let Modal::ExportRecipes { editor } = modal {
                    editor.error = Some(t("export_recipes.folder_required").into());
                }
            });
            cx.notify();
            return;
        }
        let codecs = self.registry.shared_codecs();
        for output in &editor.draft.outputs {
            if !codecs
                .iter()
                .any(|codec| codec.id() == output.codec && codec.can_export())
            {
                self.update_modal(|modal| {
                    if let Modal::ExportRecipes { editor } = modal {
                        editor.error = Some(tf!(
                            "export_recipes.unsupported_codec",
                            codec = output.codec
                        ));
                    }
                });
                cx.notify();
                return;
            }
        }
        let document = if editor.photos.is_empty() {
            self.doc.as_ref().map(recipes::snapshot)
        } else {
            None
        };
        if editor.photos.is_empty() && document.is_none() {
            return;
        }
        self.close_modal(cx);
        self.status = t("export_recipes.running").into();
        cx.notify();
        let recipe = editor.draft;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut sources: Vec<Source> = editor.photos.into_iter().map(Source::Photo).collect();
            if let Some(doc) = document {
                sources.push(Source::Document(Box::new(doc)));
            }
            let total = sources.len();
            cx.spawn(async move |this, cx| {
                let mut written = 0;
                let mut failed = 0;
                let mut first_error = None;
                for (index, source) in sources.into_iter().enumerate() {
                    let codecs = codecs.clone();
                    let recipe = recipe.clone();
                    let report = cx
                        .background_executor()
                        .spawn(async move { export_source(source, &recipe, &codecs) })
                        .await;
                    written += report.written;
                    failed += report.failed;
                    if first_error.is_none() {
                        first_error = report.error;
                    }
                    if this
                        .update(cx, |ws, cx| {
                            ws.status = tf!(
                                "export_recipes.progress",
                                done = index + 1,
                                total = total,
                                written = written,
                                failed = failed
                            )
                            .into();
                            cx.notify();
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                this.update(cx, |ws, cx| {
                    ws.status = match first_error {
                        Some(error) => tf!(
                            "export_recipes.partial",
                            written = written,
                            failed = failed,
                            error = error
                        )
                        .into(),
                        None => tf!("export_recipes.done", written = written).into(),
                    };
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        #[cfg(target_arch = "wasm32")]
        {
            let Some(mut document) = document else { return };
            self.ensure_browser_gpu(cx);
            let gpu = self.browser_gpu.context.clone();
            cx.spawn(async move |this, cx| {
                if let Some(gpu) = gpu.filter(|_| {
                    !matches!(
                        document.mode,
                        schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
                    )
                }) {
                    if let Some(tiles) = gpu.flatten_async(&document).await {
                        let mut layer = Layer::new_raster(t("common.background_layer"));
                        layer.as_raster_mut().unwrap().tiles = tiles;
                        document.tree.layers = vec![layer];
                    }
                }
                let mut names = std::collections::HashSet::new();
                let report =
                    export_document(&document, &recipe, &codecs, |stem, extension, bytes| {
                        let mut name = format!("{stem}.{extension}");
                        let mut index = 2;
                        while !names.insert(name.clone()) {
                            name = format!("{stem}-{index}.{extension}");
                            index += 1;
                        }
                        crate::web::download_bytes(&name, bytes)
                    });
                this.update(cx, |ws, cx| {
                    ws.status = match report.error {
                        Some(error) => tf!(
                            "export_recipes.partial",
                            written = report.written,
                            failed = report.failed,
                            error = error
                        )
                        .into(),
                        None => tf!("export_recipes.done", written = report.written).into(),
                    };
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}

#[derive(Default)]
struct Report {
    written: usize,
    failed: usize,
    error: Option<String>,
}
impl Report {
    fn fail(&mut self, error: anyhow::Error) {
        self.failed += 1;
        if self.error.is_none() {
            self.error = Some(error.to_string());
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
enum Source {
    Document(Box<Document>),
    Photo(PathBuf),
}
#[cfg(not(target_arch = "wasm32"))]
fn export_source(
    source: Source,
    recipe: &Recipe,
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
) -> Report {
    let doc = match source {
        Source::Document(doc) => Ok(*doc),
        Source::Photo(path) => {
            let source = schist_gallery::backing_psd(&path)
                .filter(|p| p.exists())
                .unwrap_or_else(|| path.clone());
            super::decode_file(codecs, &source)
                .map(|mut doc| {
                    // The original's name remains the recipe's naming base even when decoding an edit.
                    doc.title = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    doc
                })
                .map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))
        }
    };
    match doc {
        Ok(doc) => export_document(&doc, recipe, codecs, |stem, extension, bytes| {
            recipes::write_copy(&recipe.destination, stem, extension, bytes).map(|_| ())
        }),
        Err(error) => {
            let mut report = Report::default();
            report.fail(error);
            report
        }
    }
}
fn export_document(
    doc: &Document,
    recipe: &Recipe,
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
    mut write: impl FnMut(&str, &str, &[u8]) -> anyhow::Result<()>,
) -> Report {
    let mut report = Report::default();
    let regions = match recipes::regions(doc, recipe.scope) {
        Ok(regions) => regions,
        Err(error) => {
            report.fail(error);
            return report;
        }
    };
    let name = std::path::Path::new(&doc.title)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    for (region, rect) in regions {
        for (index, output) in recipe.outputs.iter().enumerate() {
            let result = (|| -> anyhow::Result<()> {
                let codec = codecs
                    .iter()
                    .find(|codec| codec.id() == output.codec && codec.can_export())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "{}",
                            tf!("export_recipes.unsupported_codec", codec = output.codec)
                        )
                    })?;
                let extension =
                    codec.extensions().first().copied().ok_or_else(|| {
                        anyhow::anyhow!("{}", t("export_recipes.invalid_extension"))
                    })?;
                let flat = recipes::render(doc, rect, output);
                let stem = output.filename(&name, &region, flat.width, flat.height, index + 1)?;
                let bytes = codec.export_with(
                    &flat,
                    &schist_plugin_api::ExportOptions {
                        quality: output.quality,
                        bit_depth: if matches!(output.codec.as_str(), "codec.png" | "codec.tiff")
                            && flat.depth != Depth::Eight
                        {
                            16
                        } else {
                            8
                        },
                        ..Default::default()
                    },
                )?;
                write(&stem, extension, &bytes)
            })();
            match result {
                Ok(()) => report.written += 1,
                Err(error) => report.fail(error),
            }
        }
    }
    report
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use schist_plugin_api::CodecPlugin;
    fn document(name: &str, width: u32, height: u32) -> Document {
        let mut doc = Document::new(name, width, height, Depth::Eight);
        let mut layer = Layer::new_raster("pixels");
        schist_core::blit_rgba8(
            &mut layer.as_raster_mut().unwrap().tiles,
            doc.depth,
            doc.canvas_rect(),
            &[255, 100, 50, 255].repeat((width * height) as usize),
        );
        doc.push_layer(layer);
        doc
    }
    #[test]
    fn multiple_outputs_continue_after_failure_and_use_real_codec_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let mut recipe = Recipe {
            destination: dir.path().into(),
            ..Default::default()
        };
        recipe.outputs[0].template = "{name}.jpeg".into();
        let codecs: Vec<Arc<dyn CodecPlugin>> = vec![
            Arc::new(schist_codecs_common::PngCodec),
            Arc::new(schist_codecs_common::WebPCodec),
        ];
        let report = export_source(
            Source::Document(Box::new(document("photo", 20, 10))),
            &recipe,
            &codecs,
        );
        assert_eq!((report.written, report.failed), (2, 1));
        let bytes = std::fs::read(dir.path().join("photo.jpeg.png")).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Png
        );
    }
    #[test]
    fn gallery_source_uses_saved_sidecar_and_preserves_every_input() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original.png");
        let original = schist_codecs_common::PngCodec
            .export(&document("original", 20, 10))
            .unwrap();
        std::fs::write(&source, &original).unwrap();
        let sidecar = schist_gallery::backing_psd(&source).unwrap();
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        let edited = schist_codecs_common::PsdCodec
            .export(&document("edit", 8, 4))
            .unwrap();
        std::fs::write(&sidecar, &edited).unwrap();
        let recipe = Recipe {
            destination: dir.path().into(),
            outputs: vec![recipes::Output {
                template: "{name}".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let codecs: Vec<Arc<dyn CodecPlugin>> = vec![
            Arc::new(schist_codecs_common::PngCodec),
            Arc::new(schist_codecs_common::PsdCodec),
        ];
        let report = export_source(Source::Photo(source.clone()), &recipe, &codecs);
        assert_eq!(
            (report.written, report.failed),
            (1, 0),
            "{:?}",
            report.error
        );
        let copy = image::open(dir.path().join("original-2.png")).unwrap();
        assert_eq!((copy.width(), copy.height()), (8, 4));
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(std::fs::read(&sidecar).unwrap(), edited);
    }
}
