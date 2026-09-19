//! Visual browsing of a gallery photo's saved sidecars. All decoding runs
//! off the UI thread; session/selection tokens reject stale completions.

use super::*;
use crate::ui;
use gpui::{StatefulInteractiveElement as _, StyledImage as _, img};
use schist_gallery::versions::{Version, VersionKind};
use schist_i18n::{t, tf};
use schist_ui::Button;
use std::path::Path;

const PAGE_SIZE: usize = 6;

#[derive(Clone)]
struct PreviewImage {
    image: Arc<RenderImage>,
    width: u32,
    height: u32,
}

#[derive(Clone, Default)]
enum PreviewState {
    #[default]
    Loading,
    Ready(PreviewImage),
    Failed,
}

pub(super) struct VersionBrowser {
    session: u64,
    original: PathBuf,
    entries: Vec<Version>,
    thumbnails: Vec<PreviewState>,
    selected: usize,
    selection_revision: u64,
    page: usize,
    scanning: bool,
    restoring: bool,
    original_preview: PreviewState,
    selected_preview: PreviewState,
    error: Option<String>,
    split: f32,
    bounds: Bounds<Pixels>,
}

impl VersionBrowser {
    fn new(original: PathBuf) -> Self {
        static SESSION: AtomicU64 = AtomicU64::new(1);
        Self {
            session: SESSION.fetch_add(1, Ordering::Relaxed),
            original,
            entries: Vec::new(),
            thumbnails: Vec::new(),
            selected: 0,
            selection_revision: 0,
            page: 0,
            scanning: true,
            restoring: false,
            original_preview: PreviewState::Loading,
            selected_preview: PreviewState::Loading,
            error: None,
            split: 0.5,
            bounds: Bounds::default(),
        }
    }

    fn page_range(&self) -> std::ops::Range<usize> {
        let start = self.page * PAGE_SIZE;
        start..(start + PAGE_SIZE).min(self.entries.len())
    }
}

impl Workspace {
    pub(crate) fn version_history_original(&self) -> Option<PathBuf> {
        self.doc
            .as_ref()
            .and_then(|doc| self.library.edit_backings.get(&doc.id))
            .cloned()
    }

    pub(crate) fn open_version_history(&mut self, original: PathBuf, cx: &mut Context<Self>) {
        if schist_gallery::is_video(&original) {
            return;
        }
        self.open_modal(Modal::VersionHistory, cx);
        self.library.versions = Some(VersionBrowser::new(original.clone()));
        let session = self.library.versions.as_ref().unwrap().session;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { schist_gallery::versions::list(&original) })
                .await;
            this.update(cx, |ws, cx| {
                let Some(browser) = ws.version_browser(session) else {
                    return;
                };
                browser.scanning = false;
                match result {
                    Ok(entries) => {
                        browser.thumbnails = vec![PreviewState::Loading; entries.len()];
                        browser.entries = entries;
                        ws.load_version_original(session, cx);
                        ws.select_version(0, cx);
                        ws.load_version_thumbnails(session, cx);
                    }
                    Err(error) => browser.error = Some(tf!("versions.read_failed", error = error)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn version_browser(&mut self, session: u64) -> Option<&mut VersionBrowser> {
        if !matches!(self.modal, Some(Modal::VersionHistory)) {
            return None;
        }
        self.library
            .versions
            .as_mut()
            .filter(|b| b.session == session)
    }

    fn load_version_original(&mut self, session: u64, cx: &mut Context<Self>) {
        let Some(browser) = self.version_browser(session) else {
            return;
        };
        let path = browser.original.clone();
        cx.spawn(async move |this, cx| {
            let preview = cx
                .background_executor()
                .spawn(async move { preview(&path, 1200) })
                .await;
            this.update(cx, |ws, cx| {
                if let Some(browser) = ws.version_browser(session) {
                    browser.original_preview = preview;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn select_version(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(browser) = self.library.versions.as_mut() else {
            return;
        };
        if browser.restoring {
            return;
        }
        let Some(entry) = browser.entries.get(index) else {
            return;
        };
        let path = entry.path.clone();
        browser.selected = index;
        browser.selection_revision += 1;
        browser.selected_preview = PreviewState::Loading;
        browser.error = None;
        let revision = browser.selection_revision;
        let session = browser.session;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let preview = cx
                .background_executor()
                .spawn(async move { preview(&path, 1200) })
                .await;
            this.update(cx, |ws, cx| {
                if let Some(browser) = ws
                    .version_browser(session)
                    .filter(|b| b.selection_revision == revision)
                {
                    browser.selected_preview = preview;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn load_version_thumbnails(&mut self, session: u64, cx: &mut Context<Self>) {
        let Some(browser) = self.version_browser(session) else {
            return;
        };
        let page = browser.page;
        // Keep only the visible page's small images, even for years of edits.
        browser.thumbnails.fill(PreviewState::Loading);
        let entries: Vec<_> = browser
            .page_range()
            .map(|i| (i, browser.entries[i].path.clone()))
            .collect();
        cx.spawn(async move |this, cx| {
            for (index, path) in entries {
                let current = this
                    .update(cx, |ws, _| {
                        ws.version_browser(session).is_some_and(|b| b.page == page)
                    })
                    .unwrap_or(false);
                if !current {
                    break;
                }
                let thumbnail = cx
                    .background_executor()
                    .spawn(async move { preview(&path, 160) })
                    .await;
                let current = this
                    .update(cx, |ws, cx| {
                        let Some(browser) = ws.version_browser(session).filter(|b| b.page == page)
                        else {
                            return false;
                        };
                        browser.thumbnails[index] = thumbnail;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !current {
                    break;
                }
            }
        })
        .detach();
    }

    fn version_page(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(browser) = self.library.versions.as_mut() else {
            return;
        };
        if browser.restoring || browser.entries.is_empty() {
            return;
        }
        let last = (browser.entries.len() - 1) / PAGE_SIZE;
        browser.page = if forward {
            (browser.page + 1).min(last)
        } else {
            browser.page.saturating_sub(1)
        };
        let index = browser.page * PAGE_SIZE;
        let session = browser.session;
        self.select_version(index, cx);
        self.load_version_thumbnails(session, cx);
    }

    fn version_divider(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if let Some(browser) = self.library.versions.as_mut() {
            browser.split = divider_fraction(x, browser.bounds);
            cx.notify();
        }
    }

    fn restore_version_copy(&mut self, cx: &mut Context<Self>) {
        let Some(browser) = self.library.versions.as_mut() else {
            return;
        };
        if browser.restoring {
            return;
        }
        let Some(entry) = browser.entries.get(browser.selected) else {
            return;
        };
        let path = entry.path.clone();
        let original = browser.original.clone();
        let session = browser.session;
        browser.restoring = true;
        browser.error = None;
        let codecs = self.registry.shared_codecs();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { restored_copy(&codecs, &path, &original) })
                .await;
            this.update(cx, |ws, cx| {
                let Some(browser) = ws.version_browser(session) else {
                    return;
                };
                browser.restoring = false;
                match result {
                    Ok(doc) => {
                        ws.close_modal(cx);
                        ws.open_in_tab(doc, false);
                        ws.offer_missing_fonts(cx);
                        ws.status = t("versions.restored").into();
                    }
                    Err(error) => {
                        browser.error = Some(tf!("versions.restore_failed", error = error))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn restored_copy(
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
    source: &Path,
    original: &Path,
) -> anyhow::Result<Document> {
    let mut doc = super::decode_file(codecs, source)?;
    doc.path = None;
    doc.title = tf!(
        "versions.copy_title",
        name = original.file_name().unwrap_or_default().to_string_lossy()
    );
    doc.dirty = true;
    Ok(doc)
}

fn preview(path: &Path, edge: u32) -> PreviewState {
    match schist_preview::render_file(path, edge) {
        Ok(preview) => {
            let (width, height) = (preview.width, preview.height);
            match super::library::rgba_to_render_image(width, height, preview.rgba) {
                Some(image) => PreviewState::Ready(PreviewImage {
                    image,
                    width,
                    height,
                }),
                None => PreviewState::Failed,
            }
        }
        Err(error) => {
            log::warn!("version preview failed for {}: {error:#}", path.display());
            PreviewState::Failed
        }
    }
}

fn label(version: &Version) -> String {
    match version.kind {
        VersionKind::Original => t("versions.original").into(),
        VersionKind::Current => t("versions.current").into(),
        VersionKind::Saved { seconds, sequence } => {
            let date = schist_gallery::taken_from_unix(seconds);
            if sequence == 0 {
                tf!("versions.saved_at", date = date)
            } else {
                tf!(
                    "versions.saved_sequence",
                    date = date,
                    n = sequence.saturating_add(1)
                )
            }
        }
    }
}

fn divider_fraction(x: Pixels, bounds: Bounds<Pixels>) -> f32 {
    let width = f32::from(bounds.size.width);
    if width <= 0.0 {
        return 0.5;
    }
    (f32::from(x - bounds.origin.x) / width).clamp(0.0, 1.0)
}

fn fitted(bounds: Bounds<Pixels>, width: u32, height: u32) -> Bounds<Pixels> {
    let scale = (f32::from(bounds.size.width) / width.max(1) as f32)
        .min(f32::from(bounds.size.height) / height.max(1) as f32);
    let size = size(px(width as f32 * scale), px(height as f32 * scale));
    Bounds {
        origin: point(
            bounds.origin.x + (bounds.size.width - size.width) / 2.0,
            bounds.origin.y + (bounds.size.height - size.height) / 2.0,
        ),
        size,
    }
}

pub(crate) fn dialog(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let Some(browser) = ws.library.versions.as_ref() else {
        return div().into_any_element();
    };
    let session = browser.session;
    let split = browser.split;
    let original = browser.original_preview.clone();
    let selected = browser.selected_preview.clone();
    let selected_label = browser
        .entries
        .get(browser.selected)
        .map(label)
        .unwrap_or_default();
    let name = browser
        .original
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let can_restore = !browser.scanning && !browser.restoring && !browser.entries.is_empty();
    let entity = cx.entity();
    let comparison = div()
        .relative()
        .w_full()
        .h(px(300.0))
        .overflow_hidden()
        .cursor(gpui::CursorStyle::ResizeLeftRight)
        .bg(gpui::rgb(0x252525))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, ev: &MouseDownEvent, _w, cx| ws.version_divider(ev.position.x, cx)),
        )
        .on_mouse_move(cx.listener(|ws, ev: &MouseMoveEvent, _w, cx| {
            if ev.pressed_button == Some(MouseButton::Left) {
                ws.version_divider(ev.position.x, cx);
            }
        }))
        .child(
            canvas(
                move |bounds, _window, cx| {
                    entity.update(cx, |ws, _| {
                        if let Some(browser) = ws.version_browser(session) {
                            browser.bounds = bounds;
                        }
                    });
                },
                move |bounds, _, window, _cx| {
                    if let PreviewState::Ready(preview) = &selected {
                        let rect = fitted(bounds, preview.width, preview.height);
                        let _ = window.paint_image(
                            rect,
                            gpui::Corners::default(),
                            preview.image.clone(),
                            0,
                            false,
                        );
                    }
                    let left = Bounds {
                        origin: bounds.origin,
                        size: size(bounds.size.width * split, bounds.size.height),
                    };
                    window.with_content_mask(Some(gpui::ContentMask { bounds: left }), |window| {
                        window.paint_quad(gpui::fill(bounds, gpui::rgb(0x252525)));
                        if let PreviewState::Ready(preview) = &original {
                            let rect = fitted(bounds, preview.width, preview.height);
                            let _ = window.paint_image(
                                rect,
                                gpui::Corners::default(),
                                preview.image.clone(),
                                0,
                                false,
                            );
                        }
                    });
                    let divider = Bounds {
                        origin: point(
                            bounds.origin.x + bounds.size.width * split - px(1.0),
                            bounds.origin.y,
                        ),
                        size: size(px(2.0), bounds.size.height),
                    };
                    window.paint_quad(gpui::fill(divider, gpui::rgb(0xFFFFFF)));
                },
            )
            .size_full(),
        );
    let mut strip = div()
        .id("versions-thumbnails")
        .flex()
        .flex_row()
        .gap_2()
        .overflow_x_scroll()
        .w_full();
    for index in browser.page_range() {
        let thumbnail = &browser.thumbnails[index];
        let card = div()
            .id(("version-card", index))
            .flex()
            .flex_col()
            .flex_none()
            .w(px(116.0))
            .gap_1()
            .p_1()
            .border_2()
            .rounded_md()
            .border_color(gpui::rgb(if index == browser.selected {
                ui::palette().accent
            } else {
                ui::palette().edge
            }))
            .cursor_pointer()
            .on_click(cx.listener(move |ws, _ev, _w, cx| ws.select_version(index, cx)))
            .child(match thumbnail {
                PreviewState::Ready(preview) => div()
                    .h(px(76.0))
                    .child(
                        img(preview.image.clone())
                            .size_full()
                            .object_fit(gpui::ObjectFit::Contain),
                    )
                    .into_any_element(),
                state => div()
                    .h(px(76.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.0))
                    .child(t(if matches!(state, PreviewState::Loading) {
                        "versions.loading"
                    } else {
                        "versions.preview_failed"
                    }))
                    .into_any_element(),
            })
            .child(
                div()
                    .text_size(px(10.0))
                    .child(label(&browser.entries[index])),
            );
        strip = strip.child(card);
    }
    let page_count = browser.entries.len().div_ceil(PAGE_SIZE).max(1);
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_size(px(12.0)).child(name))
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(px(11.0))
                .child(t("versions.original"))
                .child(selected_label),
        )
        .child(comparison)
        .child(div().text_size(px(11.0)).child(t("versions.drag_hint")));
    let preview_message = if browser.scanning {
        Some("versions.loading")
    } else if matches!(browser.original_preview, PreviewState::Failed)
        || matches!(browser.selected_preview, PreviewState::Failed)
    {
        Some("versions.preview_failed")
    } else if matches!(browser.original_preview, PreviewState::Loading)
        || matches!(browser.selected_preview, PreviewState::Loading)
    {
        Some("versions.loading")
    } else {
        None
    };
    if let Some(key) = preview_message {
        body = body.child(div().text_size(px(11.0)).child(t(key)));
    }
    if !browser.scanning && browser.entries.len() == 1 {
        body = body.child(div().text_size(px(11.0)).child(t("versions.no_saved")));
    }
    body = body.child(strip);
    if page_count > 1 {
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("versions-previous", t("versions.previous"))
                        .disabled(browser.page == 0 || browser.restoring)
                        .on_click(cx.listener(|ws, _ev, _w, cx| ws.version_page(false, cx))),
                )
                .child(div().text_size(px(11.0)).child(tf!(
                    "versions.page",
                    n = browser.page + 1,
                    total = page_count
                )))
                .child(
                    Button::new("versions-next", t("versions.next"))
                        .disabled(browser.page + 1 == page_count || browser.restoring)
                        .on_click(cx.listener(|ws, _ev, _w, cx| ws.version_page(true, cx))),
                ),
        );
    }
    body = body.child(div().text_size(px(11.0)).child(t("versions.copy_hint")));
    if let Some(error) = &browser.error {
        body = body.child(div().text_size(px(11.0)).child(error.clone()));
    }
    let restore = if can_restore {
        ui::button(
            t("versions.restore"),
            true,
            |ws, _w, cx| ws.restore_version_copy(cx),
            cx,
        )
        .into_any_element()
    } else {
        Button::new(
            "versions-restore",
            t(if browser.restoring {
                "versions.restoring"
            } else {
                "versions.restore"
            }),
        )
        .primary()
        .disabled(true)
        .into_any_element()
    };
    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("versions.close"),
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .child(restore);
    ui::modal_frame(t("versions.title"), 820.0, body, actions).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_is_an_unsaved_layered_copy_and_never_writes_sources() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("photo.psd");
        let sidecar = schist_gallery::backing_psd(&original).unwrap();
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        let mut document = Document::new("photo", 3, 2, Depth::Eight);
        let mut first = Layer::new_raster("first");
        blit_rgba8(
            &mut first.as_raster_mut().unwrap().tiles,
            Depth::Eight,
            document.canvas_rect(),
            &[200, 40, 10, 255].repeat(6),
        );
        document.push_layer(first);
        document.push_layer(Layer::new_raster("second"));
        let expected_pixels =
            schist_compositor::composite_region_rgba8(&document, document.canvas_rect());
        let bytes = schist_codec_psd::write_psd(&document).unwrap();
        std::fs::write(&original, &bytes).unwrap();
        std::fs::write(&sidecar, &bytes).unwrap();
        let saved = schist_gallery::versions::keep(&sidecar).unwrap().unwrap();
        let codecs: Vec<Arc<dyn schist_plugin_api::CodecPlugin>> =
            vec![Arc::new(schist_codecs_common::PsdCodec)];
        for source in [&original, &sidecar, &saved] {
            let copy = restored_copy(&codecs, source, &original).unwrap();
            assert!(copy.path.is_none());
            assert!(copy.dirty);
            assert_eq!(copy.tree.len(), 2);
            assert_eq!((copy.width, copy.height), (3, 2));
            assert_eq!(
                schist_compositor::composite_region_rgba8(&copy, copy.canvas_rect()),
                expected_pixels
            );
            assert_eq!(std::fs::read(source).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(original).unwrap(), bytes);
        assert_eq!(std::fs::read(sidecar).unwrap(), bytes);
    }

    #[test]
    fn unreadable_version_does_not_create_a_copy_or_change_the_original() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("photo.jpg");
        let corrupt = directory.path().join("broken.psd");
        std::fs::write(&original, b"untouched original").unwrap();
        std::fs::write(&corrupt, b"broken version").unwrap();
        let codecs: Vec<Arc<dyn schist_plugin_api::CodecPlugin>> =
            vec![Arc::new(schist_codecs_common::PsdCodec)];
        assert!(restored_copy(&codecs, &corrupt, &original).is_err());
        assert_eq!(std::fs::read(original).unwrap(), b"untouched original");
        assert_eq!(std::fs::read(corrupt).unwrap(), b"broken version");
    }

    #[test]
    fn labels_accept_largest_archive_sequence_without_overflow() {
        let version = Version {
            path: PathBuf::from("saved.psd"),
            kind: VersionKind::Saved {
                seconds: 42,
                sequence: u64::MAX,
            },
        };
        let text = label(&version);
        assert!(text.contains(&u64::MAX.to_string()), "unexpected label: {text}");
    }

    #[test]
    fn divider_tracks_pointer_and_clamps_to_comparison_bounds() {
        let bounds = Bounds {
            origin: point(px(100.0), px(50.0)),
            size: size(px(400.0), px(200.0)),
        };
        assert_eq!(divider_fraction(px(300.0), bounds), 0.5);
        assert_eq!(divider_fraction(px(50.0), bounds), 0.0);
        assert_eq!(divider_fraction(px(600.0), bounds), 1.0);
        let fitted = fitted(bounds, 400, 400);
        assert_eq!(fitted.origin, point(px(200.0), px(50.0)));
        assert_eq!(fitted.size, size(px(200.0), px(200.0)));
    }
}
