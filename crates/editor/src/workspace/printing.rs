//! Snapshot, cancellation and save/print handoff for PDF jobs.
use super::*;
#[cfg(not(target_arch = "wasm32"))]
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
        #[cfg(target_arch = "wasm32")]
        let photos = {
            if !photos.is_empty() {
                self.status = t("printing.source_error").into();
                cx.notify();
                return;
            }
            Vec::new()
        };
        #[cfg(not(target_arch = "wasm32"))]
        let photos = photos
            .into_iter()
            .filter(|p| !schist_gallery::is_video(p))
            .map(|path| Item {
                name: schist_gallery::photo_display_name(&path),
                rating: self.library.culling_of(&path).rating,
                path,
            })
            .collect();
        self.open_modal(
            Modal::Printing {
                editor: Editor::new(photos),
            },
            cx,
        );
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
        if editor.running {
            return;
        }
        let count = editor.photos.len().max(1);
        if let Err(error) = editor.options.validate(count) {
            editor.error = Some(error.to_string());
            cx.notify();
            return;
        }
        editor.running = true;
        editor.error = None;
        let editor = editor.clone();
        let snapshot = if editor.photos.is_empty() {
            self.doc.as_ref().map(crate::export_recipes::snapshot)
        } else {
            None
        };
        let codecs = self.registry.shared_codecs();
        let working = self.color.working.clone();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let control = editor.cancel.clone();
            let executor = cx.background_executor().clone();
            let result = cx.background_executor().spawn(async move {
                let mut snapshot = snapshot;
                printing::render_async(&editor.options, count, &working, &editor.cancel, cfg!(target_arch = "wasm32").then_some(&executor), |index| {
                    if editor.photos.is_empty() {
                        let doc = snapshot.take().ok_or_else(|| anyhow::anyhow!("{}", t("printing.no_document")))?;
                        let name = doc.title.clone();
                        return Ok((doc, name));
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        load_photo(&editor.photos[index], &codecs, editor.options.captions)
                    }
                    #[cfg(target_arch = "wasm32")]
                    { let _ = (&codecs, index); anyhow::bail!("{}", t("printing.source_error")); }
                }).await
            }).await;
            this.update_in(cx, |ws, window, cx| {
                if control.load(Ordering::Relaxed) || !matches!(&ws.modal, Some(Modal::Printing { editor }) if Arc::ptr_eq(&editor.cancel, &control)) { return; }
                match result {
                    Ok(bytes) => { ws.close_modal(cx); ws.save_print_pdf(bytes, open, window, cx); }
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
            let rx = self.prompt_for_new_path(&dir, Some("schist-print.pdf"), cx);
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
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load_photo(
    item: &Item,
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
    captions: bool,
) -> anyhow::Result<(Document, String)> {
    let path = print_source(&item.path)?;
    let doc = super::decode_file(codecs, &path)
        .map_err(|_| anyhow::anyhow!("{}", t("printing.source_error")))?;
    if !captions {
        return Ok((doc, String::new()));
    }
    let metadata = schist_gallery::xmp::read(&schist_gallery::capture_original(&item.path))
        .map_err(|_| anyhow::anyhow!("{}", t("printing.source_error")))?;
    let rating = item.rating.min(5) as usize;
    let stars = format!("{}{}", "★".repeat(rating), "☆".repeat(5 - rating));
    let caption = if metadata.caption.is_empty() {
        format!("{}\n{stars}", item.name)
    } else {
        format!("{}\n{stars}\n{}", item.name, metadata.caption)
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
