//! Similar originals and capture bursts, with reversible review decisions.
use super::*;
use gpui::{img, StatefulInteractiveElement as _, StyledImage as _};
use schist_gallery::similar::{self, Cache, Choice, Decisions, Group, Mode, Photo};
use schist_i18n::{t, tf};
use schist_ui::Button;
use std::sync::atomic::{AtomicBool, AtomicUsize};

pub(super) struct SimilarReview {
    pub open: bool,
    mode: Mode,
    threshold: u32,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
    running: bool,
    total: usize,
    failed: usize,
    cancelled: bool,
    photos: Vec<Photo>,
    groups: Vec<Group>,
    group: usize,
    candidate: usize,
    images: [Option<Arc<RenderImage>>; 2],
    loading: bool,
    preview_cancel: Arc<AtomicBool>,
    revision: u64,
    decisions: Decisions,
    decisions_loaded: bool,
    error: Option<String>,
}
impl Default for SimilarReview {
    fn default() -> Self {
        Self {
            open: false,
            mode: Mode::Visual,
            threshold: 6,
            cancel: Arc::default(),
            progress: Arc::default(),
            running: false,
            total: 0,
            failed: 0,
            cancelled: false,
            photos: vec![],
            groups: vec![],
            group: 0,
            candidate: 1,
            images: [None, None],
            loading: false,
            preview_cancel: Arc::default(),
            revision: 0,
            decisions: Decisions::default(),
            decisions_loaded: false,
            error: None,
        }
    }
}
fn review_io_error(error: std::io::Error) -> String {
    log::warn!("similar review persistence failed: {error}");
    let detail = match error.kind() {
        std::io::ErrorKind::PermissionDenied => t("common.permission_denied"),
        std::io::ErrorKind::NotFound => t("common.file_not_found"),
        std::io::ErrorKind::InvalidData => t("common.unsupported_format"),
        _ => t("common.not_available"),
    };
    tf!("library.ops.save_failed", error = detail)
}
fn decision_path() -> Option<PathBuf> {
    Some(schist_gallery::library_path()?.with_file_name("similar-review.json"))
}

impl Workspace {
    pub(super) fn open_similar_review(&mut self, cx: &mut Context<Self>) {
        self.library.map_view = false;
        self.library.viewer = None;
        self.library.similar.open = true;
        self.library.selected.clear();
        if self.library.similar.photos.is_empty() && !self.library.similar.running {
            self.scan_similar(cx);
        } else if !self.library.similar.running {
            self.load_similar_pair(cx);
        }
        cx.notify();
    }
    pub(super) fn close_similar_review(&mut self) {
        self.library.similar.open = false;
        self.library
            .similar
            .preview_cancel
            .store(true, Ordering::Relaxed);
        self.library.similar.cancel.store(true, Ordering::Relaxed);
        self.library.similar.revision += 1;
        self.library.similar.images = [None, None];
        self.library.similar.loading = false;
    }
    pub(super) fn similar_review_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.library.similar.open || self.library.viewer.is_some() || self.library.search.active
        {
            return false;
        }
        match ev.keystroke.key.as_str() {
            "left" | "right" => {
                let state = &mut self.library.similar;
                if let Some(group) = state.groups.get(state.group) {
                    let next = if ev.keystroke.key == "left" {
                        state.candidate.saturating_sub(1).max(1)
                    } else {
                        (state.candidate + 1).min(group.photos.len() - 1)
                    };
                    if next != state.candidate {
                        state.candidate = next;
                        self.load_similar_pair(cx);
                        cx.notify();
                    }
                }
            }
            "space" | "enter" | "delete" | "backspace" | "up" | "down" => {}
            _ => return false,
        }
        true
    }
    fn scan_similar(&mut self, cx: &mut Context<Self>) {
        if self.library.similar.running {
            return;
        }
        let mut paths = self.gallery_flat_order();
        paths.retain(|p| !schist_gallery::is_video(p));
        paths.sort();
        paths.dedup();
        let state = &mut self.library.similar;
        state.preview_cancel.store(true, Ordering::Relaxed);
        state.total = paths.len();
        state.cancel = Arc::default();
        state.progress = Arc::default();
        state.running = true;
        state.cancelled = false;
        state.error = None;
        state.groups.clear();
        state.photos.clear();
        state.images = [None, None];
        state.revision += 1;
        let (cancel, progress, mode, threshold) = (
            state.cancel.clone(),
            state.progress.clone(),
            state.mode,
            state.threshold,
        );
        let ticker_cancel = cancel.clone();
        // Progress is actual completed source inspections, including cache hits
        // and failures. The timer only repaints; it does not invent progress.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            let active = this
                .update(cx, |ws, cx| {
                    cx.notify();
                    ws.library.similar.running
                        && Arc::ptr_eq(&ws.library.similar.cancel, &ticker_cancel)
                })
                .unwrap_or(false);
            if !active {
                break;
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let cache_path =
                        schist_gallery::state_dir().map(|p| p.join("schist/similar-v1.json"));
                    let mut cache = cache_path.as_deref().map(Cache::load).unwrap_or_default();
                    let photos = cache.scan(
                        &paths,
                        &cancel,
                        &progress,
                        |path| {
                            let preview = schist_preview::render_file(path, 256).ok()?;
                            image::RgbaImage::from_raw(preview.width, preview.height, preview.rgba)
                                .map(|rgba| {
                                    image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
                                        let p = rgba.get_pixel(x, y);
                                        let a = p[3] as u32;
                                        image::Rgb([0, 1, 2].map(|c| {
                                            ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8
                                        }))
                                    })
                                })
                        },
                        |path, stamp| {
                            let cache =
                                schist_gallery::thumb_cache_path(path, stamp.seconds).map(|p| {
                                    p.with_extension(format!("{}-{}.png", stamp.nanos, stamp.bytes))
                                });
                            schist_gallery::photo_meta(&cache, path)
                                .taken
                                .as_deref()
                                .and_then(similar::capture_seconds)
                        },
                    );
                    if let Some(path) = cache_path {
                        if let Err(error) = cache.save(&path) {
                            log::warn!("similar cache write failed: {error}");
                        }
                    }
                    let groups = similar::groups(&photos, mode, threshold, &cancel);
                    let decisions = decision_path()
                        .ok_or_else(|| std::io::Error::other(t("common.not_available")))
                        .and_then(|p| Decisions::load(&p));
                    (photos, groups, cancel.load(Ordering::Relaxed), decisions)
                })
                .await;
            this.update(cx, |ws, cx| {
                let state = &mut ws.library.similar;
                state.running = false;
                state.cancelled = outcome.2;
                state.failed = state
                    .total
                    .saturating_sub(outcome.0.iter().filter(|p| p.signature.is_some()).count());
                if !outcome.2 {
                    state.photos = outcome.0;
                    state.groups = outcome.1;
                }
                state.group = 0;
                state.candidate = 1;
                match outcome.3 {
                    Ok(decisions) => {
                        state.decisions = decisions;
                        state.decisions_loaded = true;
                    }
                    Err(error) => {
                        state.decisions_loaded = false;
                        state.error = Some(review_io_error(error));
                    }
                }
                ws.load_similar_pair(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    fn load_similar_pair(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.library.similar;
        state.revision += 1;
        state.preview_cancel.store(true, Ordering::Relaxed);
        state.preview_cancel = Arc::default();
        let cancel = state.preview_cancel.clone();
        state.images = [None, None];
        let Some(group) = state.groups.get(state.group) else {
            state.loading = false;
            return;
        };
        let indices = [group.photos[0], group.photos[state.candidate]];
        let paths = indices.map(|i| state.photos[i].path.clone());
        let revision = state.revision;
        state.loading = true;
        cx.spawn(async move |this, cx| {
            let previews = cx
                .background_executor()
                .spawn(async move {
                    paths.map(|path| {
                        if cancel.load(Ordering::Relaxed) {
                            None
                        } else {
                            schist_preview::render_file(&path, 1600).ok()
                        }
                    })
                })
                .await;
            this.update(cx, |ws, cx| {
                let state = &mut ws.library.similar;
                if state.revision != revision {
                    return;
                }
                state.images = previews.map(|p| {
                    p.and_then(|p| super::library::rgba_to_render_image(p.width, p.height, p.rgba))
                });
                state.loading = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    fn similar_choice(&mut self, index: usize, choice: Option<Choice>, cx: &mut Context<Self>) {
        let state = &mut self.library.similar;
        if state.running || !state.decisions_loaded {
            return;
        }
        let Some(group) = state.groups.get(state.group) else {
            return;
        };
        let mut next = state.decisions.clone();
        if !next.set(&state.photos, group, index, choice) {
            state.error = Some(t("library.similar.stale").into());
        } else {
            match decision_path()
                .ok_or_else(|| std::io::Error::other(t("common.not_available")))
                .and_then(|p| next.save(&p))
            {
                Ok(()) => {
                    state.decisions = next;
                    state.error = None;
                }
                Err(error) => state.error = Some(review_io_error(error)),
            }
        }
        cx.notify();
    }
}

pub(super) fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let state = &ws.library.similar;
    let running = state.running;
    let mut controls = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            Button::new("similar-close", t("common.back")).on_click(cx.listener(|ws, _, _, cx| {
                ws.close_similar_review();
                cx.notify();
            })),
        )
        .child(t("library.similar.title"));
    for (mode, key) in [
        (Mode::Visual, "library.similar.visual"),
        (Mode::Burst, "library.similar.burst"),
    ] {
        controls = controls.child(
            Button::new(key, t(key))
                .disabled(running || state.mode == mode)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.library.similar.mode = mode;
                    ws.scan_similar(cx);
                })),
        );
    }
    if state.mode == Mode::Visual {
        controls = controls.child(t("common.threshold"));
        for threshold in [2, 6, 10] {
            controls = controls.child(
                Button::new(
                    ("similar-threshold", threshold as usize),
                    format!("{threshold}/64"),
                )
                .disabled(running || state.threshold == threshold)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.library.similar.threshold = threshold;
                    ws.scan_similar(cx);
                })),
            );
        }
    }
    controls = controls.child(
        Button::new("similar-scan", t("common.refresh"))
            .disabled(running)
            .on_click(cx.listener(|ws, _, _, cx| ws.scan_similar(cx))),
    );
    if running {
        controls = controls
            .child(t("common.in_progress"))
            .child(format!(
                "{}/{}",
                state.progress.load(Ordering::Relaxed),
                state.total
            ))
            .child(
                Button::new("similar-cancel", t("common.cancel")).on_click(cx.listener(
                    |ws, _, _, cx| {
                        ws.library.similar.cancel.store(true, Ordering::Relaxed);
                        cx.notify();
                    },
                )),
            );
    }
    let mut content = div()
        .id("similar-review")
        .flex()
        .flex_col()
        .flex_grow()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .overflow_y_scroll()
        .gap_2()
        .p_3()
        .child(controls)
        .child(div().text_size(px(12.0)).child(t("library.similar.help")))
        .children(state.error.clone().map(|error| div().child(error)));
    if state.cancelled {
        content = content.child(t("common.cancelled"));
    }
    if !running {
        content = content.child(
            div()
                .flex()
                .gap_2()
                .child(t("common.group"))
                .child(state.groups.len().to_string())
                .child(t("common.failed"))
                .child(state.failed.to_string()),
        );
    }
    if !running && !state.cancelled {
        content = content.child(t("common.ready"));
        if state.mode == Mode::Burst {
            content = content.child(
                div()
                    .flex()
                    .gap_2()
                    .child(t("library.month.undated"))
                    .child(
                        state
                            .photos
                            .iter()
                            .filter(|p| p.captured.is_none())
                            .count()
                            .to_string(),
                    ),
            );
        }
    }
    let Some(group) = state.groups.get(state.group) else {
        return content.into_any_element();
    };
    let group_len = group.photos.len();
    let group_count = state.groups.len();
    let group_index = state.group;
    let candidate = state.candidate;
    content = content.child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(t("common.group"))
            .child(format!("{}/{}", group_index + 1, group_count))
            .child(
                Button::new("similar-prev-group", t("common.back"))
                    .disabled(group_index == 0)
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.library.similar.group -= 1;
                        ws.library.similar.candidate = 1;
                        ws.load_similar_pair(cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("similar-next-group", t("common.next"))
                    .disabled(group_index + 1 == group_count)
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.library.similar.group += 1;
                        ws.library.similar.candidate = 1;
                        ws.load_similar_pair(cx);
                        cx.notify();
                    })),
            )
            .child(t("common.photo"))
            .child(format!("{}/{}", candidate + 1, group_len))
            .child(
                Button::new("similar-prev-photo", t("common.back"))
                    .disabled(candidate == 1)
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.library.similar.candidate -= 1;
                        ws.load_similar_pair(cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("similar-next-photo", t("common.next"))
                    .disabled(candidate + 1 == group_len)
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.library.similar.candidate += 1;
                        ws.load_similar_pair(cx);
                        cx.notify();
                    })),
            ),
    );
    let left = &state.photos[group.photos[0]];
    let right = &state.photos[group.photos[candidate]];
    let evidence = match state.mode {
        Mode::Visual => format!(
            "{}/64",
            left.signature
                .as_ref()
                .zip(right.signature.as_ref())
                .map(|(a, b)| a.distance(b))
                .unwrap_or(64)
        ),
        Mode::Burst => format!(
            "{} {}",
            left.captured
                .zip(right.captured)
                .map(|(a, b)| b - a)
                .unwrap_or(0),
            t("common.unit.seconds_suffix")
        ),
    };
    content = content.child(
        div()
            .flex()
            .gap_2()
            .child(t("common.distance"))
            .child(evidence),
    );
    let mut pair = div().flex().flex_row().flex_wrap().gap_3();
    for (pane, index) in [group.photos[0], group.photos[candidate]]
        .into_iter()
        .enumerate()
    {
        let photo = &state.photos[index];
        let choice = state.decisions.get(photo);
        let keep = choice == Some(Choice::Keep);
        let reject = choice == Some(Choice::Reject);
        let can_reject = group.photos.iter().any(|other| {
            *other != index && state.decisions.get(&state.photos[*other]) != Some(Choice::Reject)
        });
        let mut card = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(220.0))
            .gap_2()
            .child(photo.path.display().to_string())
            .child(match choice {
                Some(Choice::Keep) => t("library.similar.keep"),
                Some(Choice::Reject) => t("library.similar.reject"),
                None => t("common.none"),
            });
        if let Some(image) = &state.images[pane] {
            card = card.child(
                img(image.clone())
                    .w_full()
                    .h(px(360.0))
                    .object_fit(gpui::ObjectFit::Contain),
            );
        } else {
            card = card.child(if state.loading {
                t("common.loading")
            } else {
                t("library.cell.no_preview")
            });
        }
        let path = photo.path.clone();
        card = card.child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new(("similar-keep", pane), t("library.similar.keep"))
                        .disabled(keep || !state.decisions_loaded)
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.similar_choice(index, Some(Choice::Keep), cx)
                        })),
                )
                .child(
                    Button::new(("similar-reject", pane), t("library.similar.reject"))
                        .disabled(reject || !can_reject || !state.decisions_loaded)
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.similar_choice(index, Some(Choice::Reject), cx)
                        })),
                )
                .child(
                    Button::new(("similar-clear", pane), t("common.reset"))
                        .disabled(choice.is_none() || !state.decisions_loaded)
                        .on_click(
                            cx.listener(move |ws, _, _, cx| ws.similar_choice(index, None, cx)),
                        ),
                )
                .child(
                    Button::new(("similar-open", pane), t("common.edit")).on_click(
                        cx.listener(move |ws, _, _, cx| ws.open_from_gallery(path.clone(), cx)),
                    ),
                ),
        );
        pair = pair.child(card);
    }
    content.child(pair).into_any_element()
}
