use super::*;
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
use gpui::StatefulInteractiveElement as _;
use schist_core::model3d::{Model3d, Placement};
use schist_i18n::t;
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
use std::sync::atomic::{AtomicBool, Ordering};

impl Workspace {
    pub(crate) fn place_model3d(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(doc) = &self.doc else {
            self.load_file(path, cx);
            return;
        };
        let (document, width, height, depth) = (doc.id, doc.width, doc.height, doc.depth);
        self.status = t("common.working").into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    #[cfg(not(target_arch = "wasm32"))]
                    let bytes = std::fs::read(&path)?;
                    #[cfg(target_arch = "wasm32")]
                    let bytes = crate::web::read_file(&path)?;
                    let mesh = schist_model3d::import(
                        &bytes,
                        path.extension().and_then(|s| s.to_str()).unwrap_or("glb"),
                    )?;
                    let model = Model3d {
                        mesh,
                        placement: Placement::fitted(width, height),
                    };
                    schist_model3d::layer(
                        &model,
                        depth,
                        IntRect::from_size(width, height),
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(t("tool.model3d.name")),
                    )
                })
                .await;
            let _ = this.update(cx, |ws, cx| match result {
                Ok(layer) => ws.insert_model3d(document, layer, cx),
                Err(error) => {
                    ws.status = error.to_string().into();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn insert_model3d(
        &mut self,
        document: schist_core::DocumentId,
        layer: Layer,
        cx: &mut Context<Self>,
    ) {
        let active = self.doc.as_ref().is_some_and(|doc| doc.id == document);
        let doc = if active {
            self.doc.as_mut()
        } else {
            self.background_tabs
                .iter_mut()
                .find(|tab| tab.doc.id == document)
                .map(|tab| &mut tab.doc)
        };
        let Some(doc) = doc else {
            self.status = t("model3d.error.closed").into();
            cx.notify();
            return;
        };
        let id = layer.id;
        let path = schist_core::LayerPath(vec![doc.tree.layers.len()]);
        let mut edit = doc.begin_edit(t("tool.model3d.name"));
        edit.insert_layer(path, layer);
        edit.commit();
        doc.active_layer = Some(id);
        self.status = t("common.done").into();
        if active {
            self.activate_tool("model3d", cx);
            self.after_change(cx);
        } else {
            cx.notify();
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub(crate) struct Job {
    pub cancel: Arc<AtomicBool>,
    pub processing: bool,
    pub installing: bool,
    pub error: Option<String>,
}
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn model_dir() -> PathBuf {
    schist_neural::model_dir().join("triposr")
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
impl Workspace {
    pub(crate) fn open_image_to_3d(&mut self, cx: &mut Context<Self>) {
        self.open_modal(Modal::ImageTo3d, cx);
    }
    pub(crate) fn start_image_to_3d(&mut self, install: bool, cx: &mut Context<Self>) {
        if self.model3d_job.as_ref().is_some_and(|job| job.processing) {
            return;
        }
        let source = if install {
            None
        } else {
            let Some(doc) = &self.doc else {
                self.status = t("model3d.error.source").into();
                cx.notify();
                return;
            };
            let Some(layer) = doc
                .active_layer
                .and_then(|id| doc.tree.find(id))
                .filter(|l| l.as_raster().is_some() && !l.content_bounds().is_empty())
            else {
                self.status = t("model3d.error.source").into();
                cx.notify();
                return;
            };
            let mut snapshot = Document::new(doc.title.clone(), doc.width, doc.height, doc.depth);
            snapshot.mode = doc.mode;
            snapshot.icc_profile = doc.icc_profile.clone();
            snapshot.selection = doc.selection.clone();
            let mut layer = layer.clone();
            layer.visible = true;
            layer.clipping = false;
            snapshot.push_layer(layer);
            Some((doc.id, snapshot))
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.model3d_job = Some(Job {
            cancel: cancel.clone(),
            processing: true,
            installing: install,
            error: None,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let worker = cancel.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let root = model_dir();
                    if install {
                        schist_model3d::reconstruction::install(&root, &worker)?;
                        return anyhow::Ok(None);
                    }
                    let (id, snapshot) = source.unwrap();
                    let bounds = snapshot.tree.layers[0]
                        .content_bounds()
                        .intersect(&snapshot.canvas_rect());
                    anyhow::ensure!(!bounds.is_empty(), t("model3d.error.source"));
                    anyhow::ensure!(
                        bounds.width() as usize * bounds.height() as usize <= 64 * 1024 * 1024,
                        t("model3d.error.limit")
                    );
                    let mut rgba = schist_compositor::composite_region_rgba8(&snapshot, bounds);
                    for y in bounds.top..bounds.bottom {
                        for x in bounds.left..bounds.right {
                            let i = ((y - bounds.top) as usize * bounds.width() as usize
                                + (x - bounds.left) as usize)
                                * 4
                                + 3;
                            rgba[i] = (rgba[i] as u32 * snapshot.selection.coverage(x, y) as u32
                                / 255) as u8;
                        }
                    }
                    let image = image::RgbaImage::from_raw(
                        bounds.width() as u32,
                        bounds.height() as u32,
                        rgba,
                    )
                    .ok_or_else(|| anyhow::anyhow!(t("model3d.error.source")))?;
                    let mut png = std::io::Cursor::new(Vec::new());
                    image::DynamicImage::ImageRgba8(image)
                        .write_to(&mut png, image::ImageFormat::Png)?;
                    let mesh = schist_model3d::reconstruction::reconstruct(
                        &root,
                        &png.into_inner(),
                        &worker,
                    )?;
                    let model = Model3d {
                        mesh,
                        placement: Placement::fitted(snapshot.width, snapshot.height),
                    };
                    let layer = schist_model3d::layer(
                        &model,
                        snapshot.depth,
                        snapshot.canvas_rect(),
                        t("model3d.reconstruct.title"),
                    )?;
                    Ok(Some((id, layer)))
                })
                .await;
            let _ = this.update(cx, |ws, cx| {
                if !ws
                    .model3d_job
                    .as_ref()
                    .is_some_and(|job| Arc::ptr_eq(&job.cancel, &cancel))
                    || cancel.load(Ordering::Relaxed)
                {
                    return;
                }
                match result {
                    Ok(Some((document, layer))) => {
                        ws.close_modal(cx);
                        ws.insert_model3d(document, layer, cx);
                    }
                    Ok(None) => {
                        ws.model3d_job = None;
                        ws.status = t("model3d.reconstruct.ready").into();
                    }
                    Err(error) => {
                        if let Some(job) = ws.model3d_job.as_mut() {
                            job.processing = false;
                            job.error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub(crate) fn dialog(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let busy = ws.model3d_job.as_ref().is_some_and(|job| job.processing);
    let installed = schist_model3d::reconstruction::installed(&model_dir());
    let mut body = div().flex().flex_col().gap_3().text_size(px(12.0));
    if busy {
        body = body.child(t(if ws.model3d_job.as_ref().unwrap().installing {
            "model3d.reconstruct.installing"
        } else {
            "model3d.reconstruct.running"
        }));
    } else {
        body = body.child(t("model3d.reconstruct.description"));
        if !installed {
            body = body.child(t("model3d.reconstruct.install_info"));
        }
        if let Some(error) = ws.model3d_job.as_ref().and_then(|job| job.error.as_ref()) {
            body = body.child(
                div()
                    .id("model3d-error")
                    .max_h(px(160.0))
                    .overflow_y_scroll()
                    .child(error.clone()),
            );
        }
    }
    let mut buttons = div().flex().gap_2().child(crate::ui::button(
        t("common.cancel"),
        false,
        |ws, _, cx| ws.close_modal(cx),
        cx,
    ));
    if !busy {
        buttons = buttons.child(crate::ui::button(
            t(if installed {
                "model3d.reconstruct.generate"
            } else {
                "model3d.reconstruct.install"
            }),
            true,
            move |ws, _, cx| ws.start_image_to_3d(!installed, cx),
            cx,
        ));
    }
    crate::ui::modal_frame(t("model3d.reconstruct.title"), 520.0, body, buttons).into_any_element()
}
