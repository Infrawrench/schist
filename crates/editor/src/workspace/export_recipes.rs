//! Workspace entry points and sequential background export jobs.
use super::*;
use crate::export_recipes::{self as recipes, Book, Editor, FinishingCategory, Recipe};
use schist_i18n::{t, tf};

impl Workspace {
    pub(super) fn cloud_export_recipes(&mut self, cx: &mut Context<Self>) {
        let photos: Vec<_> = self
            .cloud
            .assets
            .iter()
            .filter(|a| self.cloud.selected.contains(&a.id))
            .cloned()
            .collect();
        if photos.is_empty() {
            return;
        }
        self.open_export_recipes(Vec::new(), cx);
        if let Some(Modal::ExportRecipes { editor }) = self.modal.as_mut() {
            editor.cloud_assets = photos;
        }
    }

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
    pub fn open_export_finishing(&mut self, category: FinishingCategory, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(Modal::ExportRecipes { mut editor }) = self.modal.clone() else {
            return;
        };
        editor.finishing_category = Some(category);
        editor.error = None;
        let parent = self.modal.take();
        self.open_modal(Modal::ExportRecipes { editor }, cx);
        if let Some(parent) = parent {
            self.modal_stack.push(parent);
        }
    }
    pub fn save_export_finishing(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(mut editor) = self.modal.as_ref().and_then(|modal| {
            let Modal::ExportRecipes { editor } = modal else {
                return None;
            };
            editor.finishing_category.is_some().then(|| editor.clone())
        }) else {
            return;
        };
        editor.finishing_category = None;
        let Some(Modal::ExportRecipes { editor: parent }) = self.modal_stack.last_mut() else {
            return;
        };
        editor.error = parent.error.clone();
        *parent = editor;
        self.close_modal(cx);
    }
    pub fn save_export_recipe(&mut self, cx: &mut Context<Self>) -> bool {
        self.commit_focused_field();
        let Some(Modal::ExportRecipes { editor }) = self.modal.as_mut() else {
            return false;
        };
        match editor.save() {
            Ok(()) => {
                self.status = t("export_recipes.saved").into();
                self.cloud_workflows_changed();
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
            Ok(()) => {
                let assets = std::mem::take(&mut editor.cloud_assets);
                *editor = Editor::new(book, editor.photos.clone());
                editor.cloud_assets = assets;
            }
            Err(error) => editor.error = Some(error.to_string()),
        }
        self.cloud_workflows_changed();
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
    #[cfg(not(target_arch = "wasm32"))]
    pub fn choose_recipe_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(Modal::ExportRecipes { editor }) = self.modal.clone() else {
            return;
        };
        let session = editor.session.clone();
        let index = editor.output;
        let selected = editor.selected;
        let name = editor.draft.name.clone();
        let old = editor.draft.outputs[index].clone();
        let rx = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(t("common.color_profile").into()),
            },
            cx,
        );
        #[cfg(target_os = "android")]
        if matches!(self.modal, Some(Modal::FilePicker)) {
            self.modal_stack.push(Modal::ExportRecipes { editor });
        }
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    use std::io::Read as _;
                    let mut bytes = Vec::new();
                    std::fs::File::open(&path)?
                        .take(4 * 1024 * 1024 + 1)
                        .read_to_end(&mut bytes)?;
                    anyhow::ensure!(bytes.len() <= 4 * 1024 * 1024, "{}", t("metadata.invalid"));
                    let profile = schist_colormgmt::Profile::from_bytes(&bytes)?;
                    profile.validate_mode(schist_color::ColorMode::Rgb)?;
                    Ok::<_, anyhow::Error>((
                        bytes,
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    ))
                })
                .await;
            this.update_in(cx, |ws, _window, cx| {
                let Some(Modal::ExportRecipes { editor }) = ws.modal.as_mut() else {
                    return;
                };
                if !Arc::ptr_eq(&session, &editor.session)
                    || editor.selected != selected
                    || editor.draft.name != name
                    || editor.output != index
                    || editor.draft.outputs.get(index) != Some(&old)
                {
                    return;
                }
                match loaded {
                    Ok((bytes, name)) => {
                        let finishing = &mut editor.draft.outputs[index].finishing;
                        finishing.custom_icc = bytes;
                        finishing.custom_name = name;
                        finishing.profile = recipes::TargetProfile::Custom;
                        editor.error = None;
                    }
                    Err(error) => editor.error = Some(tf!("export_recipes.failed", error = error)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub fn run_export_recipe(&mut self, cx: &mut Context<Self>) {
        if !self.save_export_recipe(cx) {
            return;
        }
        let Some(Modal::ExportRecipes { mut editor }) = self.modal.clone() else {
            return;
        };
        editor.draft.working_icc = self.color.working.icc_bytes().map(<[u8]>::to_vec);
        if !editor.cloud_assets.is_empty() {
            self.cloud_run_recipe(editor.cloud_assets, editor.draft, cx);
            return;
        }
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
            self.doc.as_ref().map(|doc| {
                #[allow(unused_mut)] // Only native gallery documents have a separate capture path.
                let mut snapshot = recipes::snapshot(doc);
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(original) = self.library.edit_backings.get(&doc.id) {
                    snapshot.path = Some(original.clone());
                }
                snapshot
            })
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
                    doc.path = Some(path.clone());
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
    let source_copyright = source_copyright(doc, recipe);
    for (region, rect) in regions {
        for (index, output) in recipe.outputs.iter().enumerate() {
            let result = (|| -> anyhow::Result<()> {
                let copyright =
                    if output.finishing.retain_copyright && output.finishing.copyright.is_empty() {
                        source_copyright
                            .as_ref()
                            .map_err(|error| anyhow::anyhow!(error.to_string()))?
                            .as_str()
                    } else {
                        ""
                    };
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
                let mut flat = recipes::render(doc, rect, output);
                if flat.icc_profile.is_none()
                    && !matches!(
                        doc.mode,
                        schist_color::ColorMode::Cmyk | schist_color::ColorMode::Lab
                    )
                {
                    flat.icc_profile = recipe.working_icc.clone();
                }
                output.finishing.apply(&mut flat)?;
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
                let bytes = output.finishing.metadata(&output.codec, bytes, copyright)?;
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

fn source_copyright(doc: &Document, recipe: &Recipe) -> anyhow::Result<String> {
    if !recipe
        .outputs
        .iter()
        .any(|o| o.finishing.retain_copyright && o.finishing.copyright.is_empty())
    {
        return Ok(String::new());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = doc.path.as_deref() {
        let capture = schist_gallery::capture_original(path);
        let path = capture.as_path();
        // XMP rights explicitly override EXIF, including an explicitly empty value.
        if let Some(copyright) = schist_gallery::xmp::copyright(path)? {
            return Ok(copyright);
        }
        return Ok(schist_gallery::copyright_of(path).unwrap_or_default());
    }
    let _ = doc;
    Ok(String::new())
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
    fn untagged_rgb_uses_captured_working_profile_for_conversion() {
        let doc = document("working", 4, 2);
        let mut recipe = Recipe {
            working_icc: schist_colormgmt::Profile::display_p3()
                .icc_bytes()
                .map(<[u8]>::to_vec),
            outputs: vec![recipes::Output {
                finishing: recipes::Finishing {
                    profile: recipes::TargetProfile::Srgb,
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let codecs: Vec<Arc<dyn CodecPlugin>> = vec![Arc::new(schist_codecs_common::PngCodec)];
        let mut p3 = None;
        let report = export_document(&doc, &recipe, &codecs, |_, _, bytes| {
            p3 = Some(image::load_from_memory(bytes).unwrap().to_rgba8());
            Ok(())
        });
        assert_eq!(report.failed, 0);
        recipe.working_icc = schist_colormgmt::Profile::srgb()
            .icc_bytes()
            .map(<[u8]>::to_vec);
        let mut srgb = None;
        let report = export_document(&doc, &recipe, &codecs, |_, _, bytes| {
            srgb = Some(image::load_from_memory(bytes).unwrap().to_rgba8());
            Ok(())
        });
        assert_eq!(report.failed, 0);
        assert_ne!(p3.unwrap().as_raw(), srgb.unwrap().as_raw());
        assert!(doc.icc_profile.is_none());
        let persisted = serde_json::to_string(&recipe).unwrap();
        assert!(!persisted.contains("working_icc"));
    }
    #[test]
    fn gallery_recipe_retains_xmp_copyright_while_omitting_location() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.png");
        let original = schist_codecs_common::PngCodec
            .export(&document("source", 32, 16))
            .unwrap();
        std::fs::write(&source, &original).unwrap();
        schist_gallery::xmp::write(
            &source,
            &schist_gallery::xmp::Patch {
                copyright: Some("© Rights holder".into()),
                gps: Some(Some((51.5, -0.1))),
                ..Default::default()
            },
        )
        .unwrap();
        let mut recipe = Recipe {
            destination: dir.path().into(),
            outputs: vec![recipes::Output {
                template: "published".into(),
                finishing: recipes::Finishing {
                    retain_copyright: true,
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let codecs: Vec<Arc<dyn CodecPlugin>> = vec![Arc::new(schist_codecs_common::PngCodec)];
        let report = export_source(Source::Photo(source.clone()), &recipe, &codecs);
        assert_eq!(
            (report.written, report.failed),
            (1, 0),
            "{:?}",
            report.error
        );
        // A virtual-copy identity resolves metadata to its shared original capture.
        let mut variant = document("variant", 32, 16);
        variant.path = Some(
            dir.path()
                .join(".schist/variants/source.png")
                .join(format!("{}.psd", "a".repeat(48))),
        );
        assert_eq!(
            source_copyright(&variant, &recipe).unwrap(),
            "© Rights holder"
        );
        let published = dir.path().join("published.png");
        assert_eq!(
            schist_gallery::copyright_of(&published).as_deref(),
            Some("© Rights holder")
        );
        assert!(schist_gallery::exif_of(&published)
            .as_ref()
            .and_then(schist_gallery::gps_from)
            .is_none());
        recipe.outputs[0].finishing.retain_copyright = false;
        let report = export_source(Source::Photo(source.clone()), &recipe, &codecs);
        assert_eq!((report.written, report.failed), (1, 0));
        assert!(schist_gallery::exif_of(&dir.path().join("published-2.png")).is_none());
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(
            schist_gallery::xmp::read(&source).unwrap().gps,
            Some(Some((51.5, -0.1)))
        );
        schist_gallery::xmp::write(
            &source,
            &schist_gallery::xmp::Patch {
                copyright: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            schist_gallery::xmp::copyright(&source).unwrap(),
            Some(String::new())
        );
    }
    #[test]
    fn unreadable_source_rights_fail_only_outputs_that_need_them() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("photo.png");
        std::fs::write(
            &source,
            schist_codecs_common::PngCodec
                .export(&document("photo", 4, 4))
                .unwrap(),
        )
        .unwrap();
        std::fs::write(source.with_extension("xmp"), "not XML").unwrap();
        let preserve = recipes::Output {
            finishing: recipes::Finishing {
                retain_copyright: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let recipe = Recipe {
            destination: dir.path().into(),
            outputs: vec![
                preserve.clone(),
                recipes::Output::default(),
                recipes::Output {
                    finishing: recipes::Finishing {
                        copyright: "Override".into(),
                        ..preserve.finishing
                    },
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let codecs: Vec<Arc<dyn CodecPlugin>> = vec![Arc::new(schist_codecs_common::PngCodec)];
        let report = export_source(Source::Photo(source), &recipe, &codecs);
        assert_eq!((report.written, report.failed), (2, 1));
        assert_eq!(
            schist_gallery::copyright_of(&dir.path().join("photo-canvas-3.png")).as_deref(),
            Some("Override")
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

/// The existing recipe renderer, with bounded memory and no device path writes.
pub(super) fn cloud_outputs(
    doc: &Document,
    recipe: &Recipe,
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let mut outputs = Vec::new();
    let mut bytes = 0usize;
    let report = export_document(doc, recipe, codecs, |stem, extension, data| {
        bytes += data.len();
        anyhow::ensure!(
            bytes <= 256 * 1024 * 1024,
            t("cloud.upload.selection_too_large")
        );
        outputs.push((format!("{stem}.{extension}"), data.to_vec()));
        Ok(())
    });
    if let Some(error) = report.error {
        anyhow::bail!(error);
    }
    Ok(outputs)
}
