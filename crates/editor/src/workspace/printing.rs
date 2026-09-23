//! Snapshot, cancellation and save/print handoff for PDF jobs.
use super::*;
use crate::printing::Item;
use crate::printing::{self, Editor};
use schist_i18n::t;
#[cfg(not(target_arch = "wasm32"))]
use schist_i18n::tf;
use std::sync::atomic::Ordering;

impl Workspace {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_contact_sheet(&mut self, cx: &mut Context<Self>) {
        if self
            .library
            .selected
            .iter()
            .all(|p| schist_gallery::is_video(p))
        {
            self.status = t("printing.no_document").into();
            cx.notify();
            return;
        }
        self.open_printing(self.library.selected.clone(), cx);
    }
    pub fn open_printing(&mut self, photos: Vec<PathBuf>, cx: &mut Context<Self>) {
        let photos = if photos.is_empty() {
            let Some(doc) = &self.doc else {
                self.status = t("printing.no_document").into();
                cx.notify();
                return;
            };
            vec![Item {
                path: PathBuf::new(),
                name: doc.title.clone(),
                rating: 0,
                snapshot: Some(Arc::new(crate::export_recipes::snapshot(doc))),
                preview: None,
                caption: doc.title.clone(),
                dimensions: Some((doc.width, doc.height, doc.resolution_dpi)),
            }]
        } else {
            self.print_items(photos)
        };
        if photos.is_empty() {
            self.status = t("printing.no_document").into();
            cx.notify();
            return;
        }
        if photos.len() > printing::MAX_ITEMS {
            self.status = t("printing.limit").into();
            cx.notify();
            return;
        }
        self.open_modal(
            Modal::Printing {
                editor: Editor::new(photos),
            },
            cx,
        );
        self.prepare_print_previews(cx);
    }
    fn print_items(&self, paths: Vec<PathBuf>) -> Vec<Item> {
        paths
            .into_iter()
            .filter_map(|path| {
                #[cfg(not(target_arch = "wasm32"))]
                if schist_gallery::is_video(&path) {
                    return None;
                }
                #[cfg(not(target_arch = "wasm32"))]
                let (name, rating) = (
                    schist_gallery::photo_display_name(&path),
                    self.library.culling_of(&path).rating,
                );
                #[cfg(target_arch = "wasm32")]
                let (name, rating) = (
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    0,
                );
                Some(Item {
                    path,
                    name,
                    rating,
                    snapshot: None,
                    preview: None,
                    caption: String::new(),
                    dimensions: None,
                })
            })
            .collect()
    }
    pub(crate) fn add_print_images(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Printing { editor }) = &self.modal else {
            return;
        };
        if editor.running || editor.loading {
            return;
        }
        let control = editor.cancel.clone();
        #[cfg(target_os = "android")]
        let retained = editor.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let picked = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some(t("printing.add_images").into()),
            },
            cx,
        );
        #[cfg(target_arch = "wasm32")]
        let picked = crate::web::pick_file("image/*,.psd,.psb,.tif,.tiff");
        // Android's in-app picker temporarily occupies the modal. Restore the
        // layout on either selection or cancellation, just like export recipes.
        #[cfg(target_os = "android")]
        if matches!(self.modal, Some(Modal::FilePicker)) {
            control.store(false, Ordering::Relaxed);
            self.modal_stack.push(Modal::Printing { editor: retained });
        }
        cx.spawn(async move |this, cx| {
            #[cfg(not(target_arch = "wasm32"))]
            let paths = match picked.await {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            #[cfg(target_arch = "wasm32")]
            let paths = match picked.await {
                Ok(Some(path)) => vec![path],
                _ => return,
            };
            this.update(cx, |ws, cx| {
                let items = ws.print_items(paths);
                let Some(Modal::Printing { editor }) = &mut ws.modal else {
                    return;
                };
                if !Arc::ptr_eq(&editor.cancel, &control) || editor.running {
                    return;
                }
                if editor.photos.len() + items.len() > printing::MAX_ITEMS
                    || editor.options.layout.len() + items.len() > printing::MAX_ITEMS
                {
                    editor.error = Some(t("printing.limit").into());
                    cx.notify();
                    return;
                }
                for item in items {
                    let index = editor.photos.len();
                    editor.photos.push(item);
                    editor.add(index);
                }
                ws.prepare_print_previews(cx);
            })
            .ok();
        })
        .detach();
    }
    fn prepare_print_previews(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Printing { editor }) = &mut self.modal else {
            return;
        };
        editor.loading = true;
        let mut photos = editor.photos.clone();
        let control = editor.cancel.clone();
        let codecs = self.registry.shared_codecs();
        let working = self.color.working.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let cancel = control.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    for item in &mut photos {
                        printing::check(&cancel)?;
                        if item.preview.is_some() {
                            continue;
                        }
                        let (doc, caption) = load_photo(item, &codecs, true)?;
                        item.caption = caption;
                        item.dimensions = Some((doc.width, doc.height, doc.resolution_dpi));
                        let image = printing::preview(
                            &doc,
                            &working,
                            &cancel,
                            cfg!(target_arch = "wasm32").then_some(&executor),
                        )
                        .await?;
                        item.preview = Some(Arc::new(gpui::RenderImage::new(smallvec::smallvec![
                            image::Frame::new(image)
                        ])));
                    }
                    Ok::<_, anyhow::Error>(photos)
                })
                .await;
            this.update(cx, |ws, cx| {
                let Some(Modal::Printing { editor }) = &mut ws.modal else {
                    return;
                };
                if !Arc::ptr_eq(&editor.cancel, &control) {
                    return;
                }
                editor.loading = false;
                match result {
                    Ok(photos) => {
                        editor.photos = photos;
                        editor.error = None;
                    }
                    Err(error) => editor.error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(crate) fn cancel_printing(&self) {
        if let Some(Modal::Printing { editor }) = &self.modal {
            editor.cancel.store(true, Ordering::Relaxed);
        }
    }
    pub(crate) fn run_printing(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Modal::Printing { editor }) = self.modal.as_mut() else {
            return;
        };
        if editor.running || editor.loading {
            return;
        }
        let count = editor.photos.len();
        if let Err(error) = editor.options.validate(count) {
            editor.error = Some(error.to_string());
            cx.notify();
            return;
        }
        editor.running = true;
        editor.error = None;
        editor.notice = None;
        let editor = editor.clone();
        let codecs = self.registry.shared_codecs();
        let working = self.color.working.clone();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let control = editor.cancel.clone();
            let executor = cx.background_executor().clone();
            let result = cx.background_executor().spawn(async move {
                printing::render_async(&editor.options, count, &working, &editor.cancel, cfg!(target_arch = "wasm32").then_some(&executor), |index| {
                    load_photo(&editor.photos[index], &codecs, editor.options.captions)
                }).await
            }).await;
            this.update_in(cx, |ws, window, cx| {
                if control.load(Ordering::Relaxed) || !matches!(&ws.modal, Some(Modal::Printing { editor }) if Arc::ptr_eq(&editor.cancel, &control)) { return; }
                match result {
                    Ok(bytes) => {
                        if let Some(Modal::Printing { editor }) = ws.modal.as_mut() { editor.running = false; }
                        ws.save_print_pdf(bytes, open, window, cx);
                    }
                    Err(error) => {
                        if let Some(Modal::Printing { editor }) = ws.modal.as_mut() { editor.running = false; editor.error = Some(error.to_string()); }
                        cx.notify();
                    }
                }
            }).ok();
        }).detach();
    }
    fn save_print_pdf(
        &mut self,
        bytes: Vec<u8>,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (open, window);
            self.status = match crate::web::download_bytes("schist-print.pdf", &bytes) {
                Ok(_) => t("printing.downloaded").into(),
                Err(_) => t("printing.save_error").into(),
            };
            if let Some(Modal::Printing { editor }) = &mut self.modal {
                editor.notice = Some(self.status.to_string());
            }
            cx.notify();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = self
                .doc
                .as_ref()
                .and_then(|d| d.path.as_ref())
                .and_then(|p| p.parent())
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();
            let Some(Modal::Printing { editor }) = self.modal.as_ref() else {
                return;
            };
            let session = editor.cancel.clone();
            #[cfg(target_os = "android")]
            let retained = editor.clone();
            let rx = self.prompt_for_new_path(&dir, Some("schist-print.pdf"), cx);
            #[cfg(target_os = "android")]
            if matches!(self.modal, Some(Modal::FilePicker)) {
                session.store(false, Ordering::Relaxed);
                self.modal_stack.push(Modal::Printing { editor: retained });
            }
            cx.spawn_in(window, async move |this, cx| {
                let Ok(Ok(Some(mut path))) = rx.await else {
                    return;
                };
                path.set_extension("pdf");
                let destination = path.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { write_pdf(&destination, &bytes) })
                    .await;
                this.update_in(cx, |ws, _, cx| {
                    match result {
                        Ok(path) => {
                            ws.status = tf!("printing.saved", path = path.display()).into();
                            if open {
                                if let Ok(url) = url::Url::from_file_path(&path) {
                                    cx.open_url(url.as_str());
                                }
                            }
                        }
                        Err(error) => {
                            log::warn!("PDF save failed: {error:#}");
                            ws.status = t("printing.save_error").into();
                        }
                    }
                    if let Some(Modal::Printing { editor }) = &mut ws.modal {
                        if Arc::ptr_eq(&editor.cancel, &session) {
                            editor.notice = Some(ws.status.to_string());
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}

fn load_photo(
    item: &Item,
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
    captions: bool,
) -> anyhow::Result<(Document, String)> {
    if let Some(doc) = &item.snapshot {
        return Ok((
            crate::export_recipes::snapshot(doc),
            if captions {
                item.name.clone()
            } else {
                String::new()
            },
        ));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let path = print_source(&item.path)?;
    #[cfg(target_arch = "wasm32")]
    let path = item.path.clone();
    let doc = super::decode_file(codecs, &path)
        .map_err(|_| anyhow::anyhow!("{}", t("printing.source_error")))?;
    if !captions {
        return Ok((doc, String::new()));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let description = schist_gallery::xmp::read(&schist_gallery::capture_original(&item.path))
        .map_err(|_| anyhow::anyhow!("{}", t("printing.source_error")))?
        .caption;
    #[cfg(target_arch = "wasm32")]
    let description = String::new();
    // Numeric ratings work with every caption font, including fonts without
    // the black/white star glyphs. They remain unambiguous in the PDF.
    let rating = item.rating.min(5);
    let caption = if description.is_empty() {
        format!("{}\n{rating}/5", item.name)
    } else {
        format!("{}\n{rating}/5\n{description}", item.name)
    };
    Ok((doc, caption))
}

#[cfg(not(target_arch = "wasm32"))]
fn print_source(path: &std::path::Path) -> anyhow::Result<PathBuf> {
    if let Some(sidecar) = schist_gallery::backing_psd(path) {
        match std::fs::symlink_metadata(&sidecar) {
            Ok(_) => return Ok(sidecar),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => anyhow::bail!("{}", t("printing.source_error")),
        }
    }
    Ok(path.to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn write_pdf(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path)?;
    Ok(std::fs::canonicalize(path)?)
}
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn source_prefers_saved_edits_but_keeps_variant_identity() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("capture.jpg");
        std::fs::write(&original, b"original").unwrap();
        assert_eq!(print_source(&original).unwrap(), original);
        let edit = schist_gallery::backing_psd(&original).unwrap();
        std::fs::create_dir_all(edit.parent().unwrap()).unwrap();
        std::fs::write(&edit, b"edit").unwrap();
        assert_eq!(print_source(&original).unwrap(), edit);
        let variant = dir
            .path()
            .join(".schist/variants/capture.jpg")
            .join(format!("{}.psd", "a".repeat(48)));
        assert_eq!(print_source(&variant).unwrap(), variant);
        assert_eq!(std::fs::read(&original).unwrap(), b"original");
        #[cfg(unix)]
        {
            std::fs::remove_file(&edit).unwrap();
            std::os::unix::fs::symlink(dir.path().join("missing.psd"), &edit).unwrap();
            assert_eq!(
                print_source(&original).unwrap(),
                edit,
                "broken saved edit must fail decoding, never substitute original"
            );
        }
    }
    #[test]
    fn atomic_save_never_overwrites_original_or_previous_print() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quoted ' $file.pdf");
        write_pdf(&path, b"first").unwrap();
        assert!(write_pdf(&path, b"second").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
